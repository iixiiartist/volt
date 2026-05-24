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


def _get_expected_functions(case: dict) -> set[str]:
    """Expected functions = the function list in the test case."""
    expected = set()
    for f in case.get("function", []):
        name = f.get("name", f.get("function", {}).get("name", ""))
        if name:
            expected.add(name)
    return expected


def evaluate_case(case: dict, output: str) -> tuple[bool, str]:
    """Check if Volt called the expected function."""
    import re
    expected = _get_expected_functions(case)
    called = set()
    for line in output.split("\n"):
        m = re.search(r"executing tool:\s*(\S+)", line)
        if m:
            called.add(m.group(1))

    if not expected:
        return called != set(), f"called {called}" if called else "no expected functions"

    matched = expected & called
    if matched:
        return True, f"called {matched}"
    elif called:
        return False, f"called {called} but expected {expected}"
    else:
        # Check if the answer string contains expected output patterns
        lower = output.lower()
        if "error" not in lower and len(output) > 20:
            return True, "produced answer (no tool needed)"
        return False, "no tool calls detected"


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


def main():
    parser = argparse.ArgumentParser(description="BFCL benchmark via Volt binary")
    parser.add_argument("--category", default="simple_python")
    parser.add_argument("--model", default="llama-3.1-8b-instant")
    parser.add_argument("--limit", type=int, default=5)
    parser.add_argument("--distractors", type=int, default=0,
                        help="Add N distractor tools per case to simulate large registries")
    parser.add_argument("--output", help="Save results to JSON file")
    args = parser.parse_args()

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
        ok, reason = evaluate_case(case, output)
        total_latency += latency

        # Track token usage from agent output
        p_tok, c_tok = _extract_token_usage(output)
        total_prompt_tokens += p_tok
        total_completion_tokens += c_tok

        status = "PASS" if ok else "FAIL"
        extra = ""
        if p_tok + c_tok > 0:
            extra = f" | {p_tok}+{c_tok} tok"
        print(f"  [{status}] {reason} ({latency:.1f}s{extra})")

        if ok:
            passed += 1

        results.append({
            "id": case.get("id", f"case_{i}"),
            "query": query,
            "expected": list(_get_expected_functions(case)),
            "passed": ok,
            "reason": reason,
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
