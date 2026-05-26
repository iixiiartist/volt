#!/usr/bin/env python3
"""
Context Kind Ablation Study for Volt.

Runs the BFCL benchmark across multiple context-kind configurations
and reports accuracy, latency, and token usage per configuration.

Usage:
    # Requires GROQ_API_KEY (or other LLM provider keys) in environment
    python context_ablation.py --category simple_python --limit 20 --model llama-3.1-8b-instant

Output:
    Writes results/volt_ablation_<timestamp>.json with per-configuration results.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

# Add volt-bfcl to path for imports
sys.path.insert(0, str(Path(__file__).parent))
from benchmark import load_test_cases  # noqa: E402
from volt_bench import _normalize_params  # noqa: E402
from stats_utils import binomial_proportion_ci, format_accuracy  # noqa: E402

RESULTS_DIR = Path(__file__).parent / "results"
VOLT_BENCH = Path(__file__).parent / "volt_bench.py"

ABLATION_CONFIGS = [
    {"name": "tool_only", "kinds": "tool"},
    {"name": "tool_skill", "kinds": "tool,skill"},
    {"name": "tool_skill_memory", "kinds": "tool,skill,memory"},
    {"name": "tool_skill_conversation", "kinds": "tool,skill,conversation"},
    {"name": "tool_skill_memory_conversation", "kinds": "tool,skill,memory,conversation"},
    {"name": "tool_skill_memory_conversation_artifact", "kinds": "tool,skill,memory,conversation,artifact"},
    {"name": "all", "kinds": "tool,skill,memory,conversation,agent_run,artifact,system_prompt,few_shot,policy,permission,security,mcp_config"},
]


def _load_env(use_embed: bool = False):
    """Load .env and set benchmark-optimized env vars."""
    env = os.environ.copy()
    env_file = Path(__file__).parent.parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            if "=" in line:
                k, v = line.split("=", 1)
                env[k.strip()] = v.strip()
    # Benchmark optimizations (skip embedding disable if using RAG)
    if not use_embed:
        env["EMBEDDING_PROVIDER"] = "none"
    env["VOLT_MINIMAL_TOOLS"] = "1"
    return env


def run_ablation_config(config: dict, args) -> dict:
    """Run a single ablation configuration."""
    name = config["name"]
    kinds = config["kinds"]
    embed_label = " (RAG enabled)" if args.embed else ""
    print(f"\n{'='*60}")
    print(f"Configuration: {name}{embed_label}")
    print(f"Context kinds: {kinds}")
    if args.runs > 1:
        print(f"Running {args.runs} passes per case")
    print(f"{'='*60}")

    binary = str(Path(__file__).parent.parent / "target" / "release" / "volt")
    if os.name == "nt":
        binary += ".exe"

    cases = load_test_cases(args.category)
    cases = cases[:args.limit] if args.limit > 0 else cases

    passed = 0
    failed = 0
    total_latency = 0.0
    total_prompt_tokens = 0
    total_completion_tokens = 0
    details = []

    env = _load_env(use_embed=args.embed)

    for i, case in enumerate(cases):
        question = _get_question(case)
        functions = case.get("function", [])

        # Write tools to temp JSONL (reused across runs for same case)
        import tempfile
        tools_file = tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False)
        for f in functions:
            fn_name = f.get("name", f.get("function", {}).get("name", ""))
            desc = f.get("description", f.get("function", {}).get("description", ""))
            params = f.get("parameters", f.get("function", {}).get("parameters", {"type": "object", "properties": {}}))
            params = _normalize_params(params)
            tools_file.write(json.dumps({"name": fn_name, "description": desc, "parameters": params}) + "\n")
        tools_path = tools_file.name
        tools_file.close()

        case_pass = 0
        case_lat = 0.0
        from volt_bench import evaluate_case, _extract_token_usage

        for run_i in range(args.runs):
            label = f" [run {run_i+1}/{args.runs}]" if args.runs > 1 else ""
            t0 = time.time()
            try:
                timeout = 300 if args.embed else 120
                result = subprocess.run(
                    [binary, "agent-run", "--model", args.model, "-a",
                     "--input", question,
                     "--load-tools", tools_path,
                     "--context-kinds", kinds],
                    capture_output=True, timeout=timeout,
                    env=env,
                )
                output = result.stdout.decode("utf-8", errors="replace") + "\n" + result.stderr.decode("utf-8", errors="replace")
                elapsed = time.time() - t0

                ok, reason, eval_details = evaluate_case(case, output)
                p_tok, c_tok = _extract_token_usage(output)

                case_pass += 1 if ok else 0
                case_lat += elapsed
                total_latency += elapsed
                total_prompt_tokens += p_tok
                total_completion_tokens += c_tok

                status = "PASS" if ok else "FAIL"
                print(f"  [{status}]{label} {question[:60]}... ({elapsed:.1f}s)")

                if run_i == args.runs - 1:  # last run: record details
                    details.append({
                        "case_idx": i,
                        "question": question[:100],
                        "passed": ok,
                        "run_passes": f"{case_pass}/{args.runs}" if args.runs > 1 else None,
                        "reason": reason,
                        "latency_ms": round(case_lat / args.runs * 1000, 1),
                        "prompt_tokens": p_tok,
                        "completion_tokens": c_tok,
                    })

            except Exception as e:
                case_lat = case_lat  # no change
                if run_i == args.runs - 1:
                    failed += 1
                    details.append({
                        "case_idx": i,
                        "question": question[:100],
                        "passed": False,
                        "reason": str(e),
                        "latency_ms": 0,
                        "prompt_tokens": 0,
                        "completion_tokens": 0,
                    })
                    print(f"  [ERR] {question[:60]}... - {e}")

        case_ok = case_pass >= args.runs / 2  # majority vote
        if case_ok:
            passed += 1
        else:
            failed += 1

        os.unlink(tools_path)

    # Recompute total/accuracy using majority vote
    total = passed + failed
    acc_pct = (passed / total * 100) if total > 0 else 0

    # Wilson CI
    try:
        ci = binomial_proportion_ci(passed, total)
    except Exception:
        ci = {"accuracy_pct": round(acc_pct, 2), "ci_lower": 0, "ci_upper": 100, "margin": 0}

    avg_latency = (total_latency / max(total, 1))
    # Per-run average latency (across all runs)
    n_runs = total * args.runs
    per_run_avg = (total_latency / n_runs) if n_runs > 0 else 0

    print(f"\n  Results: {passed}/{total} = {format_accuracy(ci['accuracy_pct'], ci['ci_lower'], ci['ci_upper'])}")
    print(f"  Avg latency: {per_run_avg:.1f}s per run  |  {avg_latency:.1f}s per case (majority)")

    return {
        "config": name,
        "kinds": kinds,
        "passed": passed,
        "failed": failed,
        "total": total,
        "accuracy": ci["accuracy_pct"],
        "ci_lower": ci["ci_lower"],
        "ci_upper": ci["ci_upper"],
        "avg_latency_sec": round(avg_latency, 2),
        "per_run_latency_sec": round(per_run_avg, 2),
        "runs_per_case": args.runs,
        "embed_enabled": args.embed,
        "total_prompt_tokens": total_prompt_tokens,
        "total_completion_tokens": total_completion_tokens,
        "details": details,
    }


def _get_question(case: dict) -> str:
    questions = case.get("question", [])
    if not questions:
        return "unknown"
    q = questions[0]
    if isinstance(q, str):
        return q
    if isinstance(q, dict):
        return q.get("body", q.get("text", q.get("content", str(q))))
    if isinstance(q, list):
        for item in q:
            if isinstance(item, dict):
                return item.get("content", item.get("body", str(item)))
            if isinstance(item, str):
                return item
        return str(q[0]) if q else "unknown"
    return str(q)


def main():
    parser = argparse.ArgumentParser(description="Volt Context Kind Ablation Study")
    parser.add_argument("--category", default="simple_python")
    parser.add_argument("--limit", type=int, default=50, help="Cases per config")
    parser.add_argument("--runs", type=int, default=1, help="Passes per case (majority vote)")
    parser.add_argument("--model", default="llama-3.1-8b-instant")
    parser.add_argument("--embed", action="store_true", help="Enable actual RAG embeddings (not PROVIDER=none)")
    parser.add_argument("--configs", default=None, help="Comma-separated config names to run (default: all)")
    parser.add_argument("--output", default=None, help="Output JSON path")
    args = parser.parse_args()

    configs = ABLATION_CONFIGS
    if args.configs:
        names = set(args.configs.split(","))
        configs = [c for c in configs if c["name"] in names]

    if not configs:
        print("No matching configurations.")
        sys.exit(1)

    print(f"Ablation Study: {len(configs)} configurations × {args.limit} cases")
    print(f"Model: {args.model} | Category: {args.category}")
    print("-" * 60)

    results = []
    for config in configs:
        result = run_ablation_config(config, args)
        results.append(result)

    summary = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "model": args.model,
        "category": args.category,
        "cases_per_config": args.limit,
        "configurations": results,
    }

    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    out_path = Path(args.output) if args.output else RESULTS_DIR / f"volt_ablation_{datetime.now(timezone.utc).strftime('%Y%m%d_%H%M%S')}.json"
    with open(out_path, "w") as f:
        json.dump(summary, f, indent=2)
    print(f"\nResults written to: {out_path}")

    # Print summary table
    print("\n" + "="*90)
    print(f"{'Configuration':<42} {'Acc':>12} {'CI':>16} {'Latency':>10}")
    print("="*90)
    for r in results:
        acc_str = f"{r['accuracy']:.1f}%"
        ci_str = f"[{r.get('ci_lower', 0):.1f}–{r.get('ci_upper', 100):.1f}]"
        lat_str = f"{r.get('avg_latency_sec', 0):.1f}s"
        print(f"{r['config']:<42} {acc_str:>12} {ci_str:>16} {lat_str:>10}")
    print("="*90)


if __name__ == "__main__":
    main()
