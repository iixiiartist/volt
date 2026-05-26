#!/usr/bin/env python3
"""Volt BFCL benchmark — runs test cases through the actual Volt binary."""

import argparse, json, os, subprocess, time, sys
from pathlib import Path

# Reuse existing BFCL data loader
sys.path.insert(0, str(Path(__file__).parent))
from benchmark import load_test_cases, BFCL_DATA_FILES as _BFCL, _add_distractors, DISTRACTOR_FUNCTIONS  # noqa: E402

RESULTS_DIR = Path(__file__).parent / "results"


def _extract_token_usage(output: str) -> tuple[int, int]:
    """Extract prompt/completion tokens from Volt agent output."""
    import re
    p_tok = 0
    c_tok = 0
    for line in output.split("\n"):
        if "total_prompt_tokens" in line or "prompt_tokens" in line:
            m = re.search(r'"prompt_tokens":\s*(\d+)', line)
            if m:
                p_tok = max(p_tok, int(m.group(1)))
        if "total_completion_tokens" in line or "completion_tokens" in line:
            m = re.search(r'"completion_tokens":\s*(\d+)', line)
            if m:
                c_tok = max(c_tok, int(m.group(1)))
    return p_tok, c_tok


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
        # BFCL format: [[{'role': 'user', 'content': '...'}]]
        for item in q:
            if isinstance(item, dict):
                return item.get("content", item.get("body", str(item)))
            if isinstance(item, str):
                return item
        return str(q[0]) if q else "unknown"
    return str(q)


def _get_expected_functions(case: dict) -> dict[str, dict]:
    """Expected functions with their schemas from the test case."""
    expected = {}
    for f in case.get("function", []):
        name = f.get("name", f.get("function", {}).get("name", ""))
        params = f.get("parameters", f.get("function", {}).get("parameters", {}))
        if name:
            expected[name] = _normalize_params(params)
    return expected


def _extract_calls(output: str) -> list[tuple[str, dict]]:
    """Extract tool name and arguments from Volt agent output."""
    import re
    calls = []
    for line in output.split("\n"):
        m = re.search(r"executing tool:\s*(\S+)\s+with\s+(.+)", line)
        if m:
            name = m.group(1)
            try:
                args = json.loads(m.group(2).replace("'", '"'))
            except json.JSONDecodeError:
                args = {}
            calls.append((name, args))
    return calls


def evaluate_case(case: dict, output: str) -> tuple[bool, str, dict]:
    """Check if Volt called the expected function with valid arguments.
    Uses full JSON Schema validation (incl. enum, pattern, format, min/max, nested objects).
    Returns (passed, reason, details_dict)."""
    from arg_validator import validate_function_call
    expected_funcs = _get_expected_functions(case)
    calls = _extract_calls(output)
    called_names = set(name for name, _ in calls)

    details = {
        "expected": list(expected_funcs.keys()),
        "called": [{"name": n, "args": a} for n, a in calls],
        "arg_issues": [],
    }

    if not expected_funcs:
        ok = len(calls) > 0
        return ok, f"called {called_names}" if ok else "no expected functions", details

    # Check name match with full argument validation
    matched_names = expected_funcs.keys() & called_names
    if matched_names:
        all_issues = []
        for name in matched_names:
            schema = expected_funcs[name]
            matching_calls = [c for c in calls if c[0] == name]
            for _, args in matching_calls:
                issues = validate_function_call(args, schema)
                if issues:
                    all_issues.extend(str(i) for i in issues)
        details["arg_issues"] = all_issues

        if all_issues:
            return False, f"name match ok but arg issues: {'; '.join(all_issues[:3])}", details
        return True, f"called {matched_names} with valid args", details

    elif called_names:
        return False, f"called {called_names} but expected {set(expected_funcs.keys())}", details
    else:
        lower = output.lower()
        if "error" not in lower and len(output) > 20:
            return True, "produced answer (no tool needed)", details
        return False, "no tool calls detected", details


def run_volt(input_text: str, functions: list[dict], model: str = "llama-3.1-8b-instant", use_embed: bool = False) -> tuple[str, float]:
    """Run Volt binary with the given input and available tools."""
    import tempfile

    binary = str(Path(__file__).parent.parent / "target" / "release" / "volt.exe")
    env = os.environ.copy()
    # Load API key from .env
    env_file = Path(__file__).parent.parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            if line.startswith("GROQ_API_KEY="):
                env["GROQ_API_KEY"] = line.split("=", 1)[1].strip()
                break
    # Benchmark optimizations (skip for RAG mode)
    if not use_embed:
        env["EMBEDDING_PROVIDER"] = "none"
    env["VOLT_MINIMAL_TOOLS"] = "1"

    # Write BFCL functions as a JSONL stub file with normalized schemas
    tools_file = tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False)
    for f in functions:
        name = f.get("name", f.get("function", {}).get("name", ""))
        desc = f.get("description", f.get("function", {}).get("description", ""))
        params = f.get("parameters", f.get("function", {}).get("parameters", {"type": "object", "properties": {}}))
        # Normalize BFCL-specific types (dict->object, list->array, etc.)
        params = _normalize_params(params)
        tools_file.write(json.dumps({"name": name, "description": desc, "parameters": params}) + "\n")
    tools_path = tools_file.name
    tools_file.close()

    t0 = time.time()
    timeout = 300 if use_embed else 120  # RAG mode needs more time for embedding computation
    result = subprocess.run(
        [binary, "agent-run", "--model", model, "-a", "--input", input_text,
         "--load-tools", tools_path],
        capture_output=True, text=True, timeout=timeout,
        env=env,
    )
    elapsed = time.time() - t0
    os.unlink(tools_path)
    output = result.stdout + "\n" + result.stderr
    return output, elapsed


TYPE_NORMALIZE = {
    "String": "string", "string": "string",
    "Boolean": "boolean", "boolean": "boolean",
    "Integer": "integer", "integer": "integer",
    "Number": "number", "number": "number",
    "Object": "object", "object": "object",
    "Array": "array", "array": "array",
    "Dict": "object", "dict": "object",
    "Dictionary": "object", "dictionary": "object",
    "List": "array", "list": "array",
    "float": "number", "double": "number", "int": "integer",
}

def _normalize_params(params: dict) -> dict:
    """Normalize BFCL parameter schemas to valid JSON Schema types."""
    result = dict(params)
    raw_type = result.get("type", None)
    if isinstance(raw_type, str):
        result["type"] = TYPE_NORMALIZE.get(raw_type, raw_type)
    if "properties" in result:
        props = result["properties"]
        if isinstance(props, dict):
            result["properties"] = {k: _normalize_params(v) if isinstance(v, dict) else v for k, v in props.items()}
    if "items" in result:
        items = result["items"]
        if isinstance(items, dict):
            result["items"] = _normalize_params(items)
    if "required" in result:
        req = result["required"]
        if not isinstance(req, list):
            # Some BFCL schemas have 'required' as an object key index, fix it
            pass
    return result


def run_sweep(args):
    """Run tool-count scaling ablation: sweep distractor counts."""
    counts = [0, 10, 50, 100, 200]
    results = []
    cases = load_test_cases(args.category)
    cases = cases[:args.limit] if args.limit > 0 else cases

    print(f"=== Tool-Count Scaling Sweep ===")
    print(f"Category: {args.category} | Model: {args.model} | Cases: {len(cases)}")
    print(f"{'Distractors':<12} {'Acc':<8} {'Latency':<10} {'Pass/Fail':<10}")
    print("-" * 50)

    for dist in counts:
        passed = 0
        total_latency = 0
        for i, case in enumerate(cases):
            query = _get_question(case)
            functions = case.get("function", [])
            if dist > 0:
                functions = _add_distractors(functions, dist, case.get("id", str(i)))
            prompt = f"Use the available tools to answer this question. You MUST call the appropriate function.\n\nQuestion: {query}"
            output, latency = run_volt(prompt, functions, args.model)
            ok, reason, _details = evaluate_case(case, output)
            total_latency += latency
            if ok:
                passed += 1

        acc = passed / len(cases) * 100 if cases else 0
        avg_lat = total_latency / len(cases) if cases else 0
        results.append({"distractors": dist, "accuracy": acc, "avg_latency_s": avg_lat, "passed": passed, "total": len(cases)})
        print(f"{dist:<12} {acc:<8.1f}% {avg_lat:<10.1f}s {passed}/{len(cases)}")

    print(f"\n--- Scaling Curve ---")
    for r in results:
        print(f"  {r['distractors']:>4} tools -> {r['accuracy']:.0f}%")
    if args.output:
        full = {"sweep": results, "category": args.category, "model": args.model}
        RESULTS_DIR.mkdir(parents=True, exist_ok=True)
        with open(RESULTS_DIR / args.output, "w") as f:
            json.dump(full, f, indent=2)
        print(f"Saved to {RESULTS_DIR / args.output}")


def main():
    parser = argparse.ArgumentParser(description="BFCL benchmark via Volt binary")
    parser.add_argument("--category", default="simple_python")
    parser.add_argument("--model", default="llama-3.1-8b-instant")
    parser.add_argument("--limit", type=int, default=50)
    parser.add_argument("--runs", type=int, default=1, help="Run each case N times for statistical averaging")
    parser.add_argument("--distractors", type=int, default=0,
                        help="Add N distractor tools per case to simulate large registries")
    parser.add_argument("--embed", action="store_true",
                        help="Enable actual embeddings (not PROVIDER=none fallback)")
    parser.add_argument("--output", help="Save results to JSON file")
    parser.add_argument("--sweep", action="store_true",
                        help="Run tool-count scaling sweep at [0,10,50,100,200] distractors")
    args = parser.parse_args()

    if args.sweep:
        run_sweep(args)
        return

    cases = load_test_cases(args.category)
    cases = cases[:args.limit] if args.limit > 0 else cases
    distractor_label = f" +{args.distractors} distractors" if args.distractors else ""
    embed_label = " (RAG enabled)" if args.embed else ""
    print(f"Loaded {len(cases)} cases from {args.category}{distractor_label}{embed_label}")
    if args.runs > 1:
        print(f"Running {args.runs} passes per case ({len(cases) * args.runs} total runs)")

    results = []
    passed = 0
    total_latency = 0
    total_prompt_tokens = 0
    total_completion_tokens = 0
    per_case_runs = {}  # case_idx -> list of (passed, latency)

    for i, case in enumerate(cases):
        query = _get_question(case)
        print(f"\n[{i+1}/{len(cases)}] {query[:80]}...")

        functions = case.get("function", [])
        if args.distractors:
            functions = _add_distractors(functions, args.distractors, case.get("id", str(i)))

        prompt = (
            f"Use the available tools to answer this question. You MUST call the appropriate function.\n\n"
            f"Question: {query}"
        )

        case_pass, case_lat = 0, 0.0
        for run_i in range(args.runs):
            label = f" [run {run_i+1}/{args.runs}]" if args.runs > 1 else ""
            output, latency = run_volt(prompt, functions, args.model, use_embed=args.embed)
            ok, reason, details = evaluate_case(case, output)
            case_pass += 1 if ok else 0
            case_lat += latency

            p_tok, c_tok = _extract_token_usage(output)
            total_prompt_tokens += p_tok
            total_completion_tokens += c_tok

            status = "PASS" if ok else "FAIL"
            arg_info = ""
            if details.get("arg_issues"):
                arg_info = f" | args: {details['arg_issues'][:1]}"
            print(f"  [{status}]{label} {reason} ({latency:.1f}s{arg_info})")

        avg_lat = case_lat / args.runs
        total_latency += avg_lat
        case_ok = case_pass >= args.runs / 2  # majority vote
        if case_ok:
            passed += 1

        if args.runs > 1:
            per_case_runs[i] = {"passes": case_pass, "total": args.runs, "avg_latency": avg_lat}

        results.append({
            "id": case.get("id", f"case_{i}"),
            "query": query,
            "expected": list(_get_expected_functions(case).keys()),
            "passed": case_ok,
            "run_passes": f"{case_pass}/{args.runs}" if args.runs > 1 else None,
            "reason": reason,
            "arg_issues": details.get("arg_issues", []),
            "calls": details.get("called", []),
            "latency_s": avg_lat,
            "output": output[:500],
        })

    # -- Statistical reporting --
    n_effective = len(cases) * args.runs  # total individual runs
    total_passed_runs = sum(r["passes"] for r in per_case_runs.values()) if args.runs > 1 else passed

    from stats_utils import binomial_proportion_ci, format_accuracy
    ci = binomial_proportion_ci(passed, len(cases))

    print(f"\n{'='*70}")
    print(f"RESULTS | {args.category} | {args.model}")
    print(f"{'='*70}")
    print(f"  Accuracy (majority): {passed}/{len(cases)} = {format_accuracy(ci['accuracy_pct'], ci['ci_lower'], ci['ci_upper'])}")
    if args.runs > 1:
        print(f"  Per-run accuracy:    {total_passed_runs}/{n_effective} = {total_passed_runs/n_effective*100:.1f}%")
    print(f"  Avg latency:         {total_latency/len(cases):.1f}s per case")
    if total_prompt_tokens + total_completion_tokens > 0:
        total_tok = total_prompt_tokens + total_completion_tokens
        print(f"  Token usage:         {total_prompt_tokens} prompt + {total_completion_tokens} completion = {total_tok} total")
    print(f"{'='*70}")

    if args.output:
        RESULTS_DIR.mkdir(parents=True, exist_ok=True)
        summary = {
            "category": args.category,
            "model": args.model,
            "cases": len(cases),
            "runs_per_case": args.runs,
            "passed": passed,
            "accuracy_pct": ci["accuracy_pct"],
            "ci_lower": ci["ci_lower"],
            "ci_upper": ci["ci_upper"],
            "avg_latency_s": round(total_latency / len(cases), 2),
            "total_prompt_tokens": total_prompt_tokens,
            "total_completion_tokens": total_completion_tokens,
            "embed_enabled": args.embed,
            "results": results,
        }
        with open(RESULTS_DIR / args.output, "w") as f:
            json.dump(summary, f, indent=2)
        print(f"Saved to {RESULTS_DIR / args.output}")


if __name__ == "__main__":
    main()
