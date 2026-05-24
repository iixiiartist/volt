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


def _validate_args(args: dict, schema: dict) -> list[str]:
    """Validate arguments against JSON Schema. Returns list of issues."""
    issues = []
    props = schema.get("properties", {})
    required = schema.get("required", [])

    # Check required fields present
    for req in required:
        if isinstance(req, str) and req not in args:
            issues.append(f"missing required param '{req}'")
        elif isinstance(req, list):
            for r in req:
                if isinstance(r, str) and r not in args:
                    issues.append(f"missing required param '{r}'")

    # Check argument types
    for key, val in args.items():
        if key in props:
            prop_schema = props[key]
            expected_type = prop_schema.get("type", "string")
            type_ok = _check_type(val, expected_type)
            if not type_ok:
                issues.append(f"param '{key}' type mismatch: got {type(val).__name__}, expected {expected_type}")

    # Check for hallucinated params (not in schema)
    for key in args:
        if key not in props and key not in ("unit", "units"):
            issues.append(f"hallucinated param '{key}'")

    return issues


def _check_type(val, expected: str) -> bool:
    """Check if value matches expected JSON Schema type."""
    type_map = {
        "string": str,
        "integer": int,
        "number": (int, float),
        "boolean": bool,
        "array": list,
        "object": dict,
    }
    if expected in type_map:
        expected_types = type_map[expected]
        return isinstance(val, expected_types) if not isinstance(expected_types, tuple) else isinstance(val, expected_types)
    return True  # unknown types pass


def evaluate_case(case: dict, output: str) -> tuple[bool, str, dict]:
    """Check if Volt called the expected function with valid arguments.
    Returns (passed, reason, details_dict)."""
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

    # Check name match
    matched_names = expected_funcs.keys() & called_names
    if matched_names:
        # Check arguments for matched functions
        all_issues = []
        for name in matched_names:
            schema = expected_funcs[name]
            matching_calls = [c for c in calls if c[0] == name]
            for _, args in matching_calls:
                issues = _validate_args(args, schema)
                if issues:
                    all_issues.extend(f"{name}: {i}" for i in issues)
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


def run_volt(input_text: str, functions: list[dict], model: str = "llama-3.1-8b-instant") -> tuple[str, float]:
    """Run Volt binary with the given input and available tools."""
    import tempfile

    binary = str(Path(__file__).parent.parent / "target" / "debug" / "volt.exe")
    env = os.environ.copy()

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
    result = subprocess.run(
        [binary, "agent-run", "--model", model, "-a", "--input", input_text,
         "--load-tools", tools_path],
        capture_output=True, text=True, timeout=300,
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
    parser.add_argument("--limit", type=int, default=5)
    parser.add_argument("--distractors", type=int, default=0,
                        help="Add N distractor tools per case to simulate large registries")
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
    print(f"Loaded {len(cases)} cases from {args.category}{distractor_label}")

    results = []
    passed = 0
    total_latency = 0
    total_prompt_tokens = 0
    total_completion_tokens = 0

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

        output, latency = run_volt(prompt, functions, args.model)
        ok, reason, details = evaluate_case(case, output)
        total_latency += latency

        # Track token usage from agent output
        p_tok, c_tok = _extract_token_usage(output)
        total_prompt_tokens += p_tok
        total_completion_tokens += c_tok

        status = "PASS" if ok else "FAIL"
        extra = ""
        if p_tok + c_tok > 0:
            extra = f" | {p_tok}+{c_tok} tok"
        if details.get("arg_issues"):
            extra += f" | args: {details['arg_issues'][:1]}"
        print(f"  [{status}] {reason} ({latency:.1f}s{extra})")

        if ok:
            passed += 1

        results.append({
            "id": case.get("id", f"case_{i}"),
            "query": query,
            "expected": list(_get_expected_functions(case).keys()),
            "passed": ok,
            "reason": reason,
            "arg_issues": details.get("arg_issues", []),
            "calls": details.get("called", []),
            "latency_s": latency,
            "output": output[:500],
        })

    accuracy = passed / len(cases) * 100 if cases else 0
    print(f"\nAccuracy: {passed}/{len(cases)} = {accuracy:.1f}%")
    print(f"Total latency: {total_latency:.1f}s")
    if total_prompt_tokens + total_completion_tokens > 0:
        total_tok = total_prompt_tokens + total_completion_tokens
        print(f"Token usage: {total_prompt_tokens} prompt + {total_completion_tokens} completion = {total_tok} total")
        # Compare to static injection (all tools in every prompt)
        tool_tokens_per_case = sum(
            len(json.dumps({"name": f.get("name", f.get("function", {}).get("name", "")),
                            "description": f.get("description", f.get("function", {}).get("description", "")),
                            "parameters": f.get("parameters", f.get("function", {}).get("parameters", {}))}))
            for f in (load_test_cases(args.category)[:1] or [{"function": []}]).get("function", [])
        ) // 3  # rough: chars/3 = tokens
        est_static_tokens = total_prompt_tokens + (tool_tokens_per_case * len(cases) * (1 + args.distractors))
        if total_tok > 0:
            savings = (1 - total_tok / max(est_static_tokens, 1)) * 100
            print(f"Est. token savings vs static injection: {savings:.0f}%")

    if args.output:
        RESULTS_DIR.mkdir(parents=True, exist_ok=True)
        with open(RESULTS_DIR / args.output, "w") as f:
            json.dump({"results": results, "accuracy": accuracy, "category": args.category}, f, indent=2)
        print(f"Saved to {RESULTS_DIR / args.output}")


if __name__ == "__main__":
    main()
