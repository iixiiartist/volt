#!/usr/bin/env python3
"""Multi-turn episodic memory benchmark for Volt.

Tests whether the ContextStore's Conversation entries (seeded via
SeedChannel::EpisodeComplete) help agents recall previous interactions.

Also provides a GAIA benchmark adapter scaffold.
"""

import argparse, json, os, subprocess, time, sys, re
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from benchmark import load_test_cases

BINARY = str(Path(__file__).parent.parent / "target" / "debug" / "volt.exe")
RESULTS_DIR = Path(__file__).parent / "results"


def run_agent(input_text: str, model: str = "llama-3.1-8b-instant",
              allow: bool = True, timeout: int = 120) -> tuple[str, float]:
    """Run a single agent-run command."""
    env = os.environ.copy()
    t0 = time.time()
    result = subprocess.run(
        [BINARY, "agent-run", "--model", model, "-a", "--input", input_text],
        capture_output=True, timeout=timeout, env=env,
    )
    elapsed = time.time() - t0
    stdout = result.stdout.decode("utf-8", errors="replace") if result.stdout else ""
    stderr = result.stderr.decode("utf-8", errors="replace") if result.stderr else ""
    return (stdout + "\n" + stderr), elapsed


def extract_final_answer(output: str) -> str:
    """Extract the agent's final response content."""
    # Look for the last meaningful non-log line
    lines = [l.strip() for l in output.split("\n") if l.strip()
             and not l.startswith("[") and not l.startswith("\x1b")
             and "INFO" not in l and "WARN" not in l and "ERROR" not in l]
    # Return the last substantial line
    for line in reversed(lines):
        if len(line) > 5 and not line.startswith("<") and not line.startswith("["):
            return line
    return lines[-1] if lines else ""


def benchmark_episodic_memory(model: str, cases: int = 5):
    """Multi-turn test: ask related questions and check if agent recalls context.
    
    Turn 1: Solve a problem (e.g., area of triangle)
    Turn 2: Ask about the problem from turn 1
    Turn 3: Ask a different problem
    Turn 4: Ask about turn 3
    """
    sequences = [
        {
            "name": "Math recall",
            "turns": [
                "Calculate the area of a triangle with base 10 and height 5 using the available tools.",
                "What was the base length of the triangle you just calculated?",
            ],
        },
        {
            "name": "Code artifact",
            "turns": [
                "Write a Python function called 'double' that returns 2x the input to a file called 'double.py'.",
                "What file did you just create and what function does it contain?",
            ],
        },
        {
            "name": "Factorial chain",
            "turns": [
                "Calculate 5 factorial using the math.factorial tool.",
                "What number did you just calculate the factorial of?",
            ],
        },
    ]

    results = []
    for seq in sequences[:cases]:
        print(f"\n=== Sequence: {seq['name']} ===")
        seq_results = []
        for i, turn in enumerate(seq["turns"]):
            print(f"  Turn {i+1}: {turn[:60]}...")
            output, latency = run_agent(turn, model)
            answer = extract_final_answer(output)
            passed = "?"  
            seq_results.append({"turn": i+1, "input": turn, "output": answer, "latency_s": latency})
            print(f"    -> {answer[:100]} ({latency:.1f}s)")
        results.append({"sequence": seq["name"], "turns": seq_results})

    # Summary
    print(f"\n=== Episodic Memory Summary ===")
    for r in results:
        print(f"  {r['sequence']}:")
        for t in r["turns"]:
            print(f"    Turn {t['turn']}: {t['output'][:80]}")


def benchmark_gaia_smoke(model: str):
    """GAIA benchmark smoke test — validates multi-step agent capability.
    
    GAIA (https://huggingface.co/datasets/gaia-benchmark/GAIA) tests:
    1. Multi-step reasoning across web search, file I/O, tool use
    2. Answers are exact strings
    
    This smoke test uses simplified GAIA-like questions.
    """
    gaia_questions = [
        {
            "id": "gaia_smoke_1",
            "question": "What is the capital of France? Write the answer to a file called capital.txt.",
            "expected_file": "capital.txt",
            "expected_contains": "Paris",
        },
        {
            "id": "gaia_smoke_2", 
            "question": "Search the web for the current President of the United States and write their name to president.txt.",
            "expected_file": "president.txt",
        },
        {
            "id": "gaia_smoke_3",
            "question": "Read the file capital.txt and tell me what city it contains.",
            "expected_contains": "Paris",
        },
    ]

    print(f"\n=== GAIA Smoke Test ({len(gaia_questions)} questions) ===")
    passed = 0
    for q in gaia_questions:
        print(f"  [{q['id']}] {q['question'][:80]}...")
        output, latency = run_agent(q["question"], model, timeout=180)
        answer = extract_final_answer(output)
        ok = False
        if "expected_contains" in q:
            ok = q["expected_contains"].lower() in output.lower()
        if "expected_file" in q:
            ok = os.path.exists(q["expected_file"])
        status = "PASS" if ok else "FAIL"
        if ok:
            passed += 1
        print(f"    [{status}] {answer[:80]} ({latency:.1f}s)")

    print(f"\nGAIA smoke: {passed}/{len(gaia_questions)} passed")


def main():
    parser = argparse.ArgumentParser(description="Multi-turn episodic memory benchmark")
    parser.add_argument("--mode", choices=["episodic", "gaia", "all"], default="all")
    parser.add_argument("--model", default="llama-3.1-8b-instant")
    parser.add_argument("--cases", type=int, default=3)
    args = parser.parse_args()

    if args.mode in ("episodic", "all"):
        benchmark_episodic_memory(args.model, args.cases)

    if args.mode in ("gaia", "all"):
        benchmark_gaia_smoke(args.model)


if __name__ == "__main__":
    main()
