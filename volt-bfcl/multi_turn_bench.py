#!/usr/bin/env python3
"""Multi-turn episodic memory benchmark for Volt.

Tests whether the ContextStore's Conversation entries (seeded via
SeedChannel::EpisodeComplete) help agents recall previous interactions.
"""

import argparse, json, os, subprocess, time, sys, re, uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from benchmark import load_test_cases

BINARY = str(Path(__file__).parent.parent / "target" / "release" / "volt.exe")
RESULTS_DIR = Path(__file__).parent / "results"


def run_agent(input_text: str, model: str = "llama-3.1-8b-instant",
              allow: bool = True, timeout: int = 600,
              session_id: str | None = None) -> tuple[str, float]:
    """Run a single agent-run command."""
    env = os.environ.copy()
    env_file = Path(__file__).parent.parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            if line.startswith("GROQ_API_KEY="):
                env["GROQ_API_KEY"] = line.split("=", 1)[1].strip()
    if "GROQ_API_KEY" not in env:
        env["GROQ_API_KEY"] = ""
    env["EMBEDDING_PROVIDER"] = "none"
    env["VOLT_MINIMAL_TOOLS"] = "1"
    cmd = [BINARY, "agent-run", "--model", model, "-a", "--input", input_text]
    if session_id:
        cmd.extend(["--session-id", session_id])
    t0 = time.time()
    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
    )
    try:
        stdout_b, stderr_b = proc.communicate(timeout=timeout)
        elapsed = time.time() - t0
    except subprocess.TimeoutExpired:
        proc.kill()
        stdout_b, stderr_b = proc.communicate()
        elapsed = time.time() - t0
    stdout = stdout_b.decode("utf-8", errors="replace") if stdout_b else ""
    stderr = stderr_b.decode("utf-8", errors="replace") if stderr_b else ""
    return (stdout + "\n" + stderr), elapsed


def extract_final_answer(output: str) -> str:
    """Extract the agent's final response content."""
    lines = [l.strip() for l in output.split("\n") if l.strip()
             and not l.startswith("[") and not l.startswith("\x1b")
             and "INFO" not in l and "WARN" not in l and "ERROR" not in l]
    for line in reversed(lines):
        if len(line) > 5 and not line.startswith("<") and not line.startswith("["):
            return line
    return lines[-1] if lines else ""


def benchmark_episodic_memory(model: str, cases: int = 5):
    """Multi-turn test: ask related questions and check if agent recalls context."""
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
        session_id = str(uuid.uuid4())
        for i, turn in enumerate(seq["turns"]):
            print(f"  Turn {i+1}: {turn[:60]}...")
            output, latency = run_agent(turn, model, session_id=session_id)
            answer = extract_final_answer(output)
            passed = "?"
            seq_results.append({"turn": i+1, "input": turn, "output": answer, "latency_s": latency})
            print(f"    -> {answer[:100]} ({latency:.1f}s)")
        results.append({"sequence": seq["name"], "turns": seq_results})

    print(f"\n=== Episodic Memory Summary ===")
    for r in results:
        print(f"  {r['sequence']}:")
        for t in r["turns"]:
            print(f"    Turn {t['turn']}: {t['output'][:80]}")


def main():
    parser = argparse.ArgumentParser(description="Multi-turn episodic memory benchmark")
    parser.add_argument("--model", default="llama-3.1-8b-instant")
    parser.add_argument("--cases", type=int, default=3)
    args = parser.parse_args()

    benchmark_episodic_memory(args.model, args.cases)


if __name__ == "__main__":
    main()
