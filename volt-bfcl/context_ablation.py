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


def run_ablation_config(config: dict, args) -> dict:
    """Run a single ablation configuration."""
    name = config["name"]
    kinds = config["kinds"]
    print(f"\n{'='*60}")
    print(f"Configuration: {name}")
    print(f"Context kinds: {kinds}")
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

    for i, case in enumerate(cases):
        question = _get_question(case)
        functions = case.get("function", [])

        # Write tools to temp JSONL
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

        env = os.environ.copy()
        t0 = time.time()
        try:
            result = subprocess.run(
                [binary, "agent-run", "--model", args.model, "-a",
                 "--input", question,
                 "--load-tools", tools_path,
                 "--context-kinds", kinds],
                capture_output=True, text=True, timeout=300,
                env=env,
            )
            output = result.stdout + "\n" + result.stderr
            elapsed = time.time() - t0

            # Reuse volt_bench evaluator
            from volt_bench import evaluate_case, _extract_token_usage
            ok, reason, eval_details = evaluate_case(case, output)
            p_tok, c_tok = _extract_token_usage(output)

            total_latency += elapsed
            total_prompt_tokens += p_tok
            total_completion_tokens += c_tok

            if ok:
                passed += 1
            else:
                failed += 1

            details.append({
                "case_idx": i,
                "question": question[:100],
                "passed": ok,
                "reason": reason,
                "latency_ms": round(elapsed * 1000, 1),
                "prompt_tokens": p_tok,
                "completion_tokens": c_tok,
            })

            status = "PASS" if ok else "FAIL"
            print(f"  [{status}] {question[:60]}... ({elapsed:.1f}s)")

        except Exception as e:
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
            print(f"  [ERR] {question[:60]}... — {e}")
        finally:
            os.unlink(tools_path)

    total = passed + failed
    accuracy = (passed / total * 100) if total > 0 else 0
    avg_latency = (total_latency / total) if total > 0 else 0

    print(f"\n  Results: {passed}/{total} ({accuracy:.1f}%) | Avg latency: {avg_latency:.1f}s")

    return {
        "config": name,
        "kinds": kinds,
        "passed": passed,
        "failed": failed,
        "accuracy": round(accuracy, 2),
        "avg_latency_sec": round(avg_latency, 2),
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
    parser.add_argument("--limit", type=int, default=20, help="Cases per config")
    parser.add_argument("--model", default="llama-3.1-8b-instant")
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
    print("\n" + "="*80)
    print(f"{'Configuration':<40} {'Acc':>6} {'Latency':>10} {'Prompt':>10} {'Complete':>10}")
    print("="*80)
    for r in results:
        print(f"{r['config']:<40} {r['accuracy']:>5.1f}% {r['avg_latency_sec']:>8.1f}s {r['total_prompt_tokens']:>10} {r['total_completion_tokens']:>10}")
    print("="*80)


if __name__ == "__main__":
    main()
