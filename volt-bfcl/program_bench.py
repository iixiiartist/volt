#!/usr/bin/env python3
"""Volt ProgramBench — measures agent accuracy on programming puzzles.

Usage:
    python program_bench.py --limit 5     # run 5 puzzles
    python program_bench.py --mode all     # run all 25 puzzles

Cost (Groq): ~$0.07 for full suite
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
import traceback
from pathlib import Path

RESULTS_DIR = Path(__file__).parent / "results"
RESULTS_DIR.mkdir(parents=True, exist_ok=True)
PROBLEMS_DIR = RESULTS_DIR / "problems"
PROBLEMS_DIR.mkdir(parents=True, exist_ok=True)

DEFAULT_MODEL = "llama-3.1-8b-instant"

# -- Programming Problems ----------------------------------------------

PROBLEMS = [
    {
        "id": "two_sum",
        "description": "Write a function two_sum(nums, target) that returns indices of two numbers that add up to target.",
        "test_code": "print(two_sum([2,7,11,15], 9))",
        "expected_output": "[0, 1]\n",
        "check_fn": "exact",
    },
    {
        "id": "reverse_string",
        "description": "Write a function reverse_string(s) that reverses a string in-place as a list of characters.",
        "test_code": "s = ['h','e','l','l','o']\nreverse_string(s)\nprint(''.join(s))",
        "expected_output": "olleh\n",
        "check_fn": "exact",
    },
    {
        "id": "valid_parentheses",
        "description": "Write a function is_valid(s) that returns True if parentheses/brackets/braces are properly nested.",
        "test_code": "print(is_valid('()[]{}'))\nprint(is_valid('(]'))\nprint(is_valid('([)]'))",
        "expected_output": "True\nFalse\nFalse\n",
        "check_fn": "exact",
    },
    {
        "id": "merge_sorted_arrays",
        "description": "Write a function merge(nums1, m, nums2, n) that merges two sorted arrays into nums1.",
        "test_code": "nums1 = [1,2,3,0,0,0]\nmerge(nums1, 3, [2,5,6], 3)\nprint(nums1)",
        "expected_output": "[1, 2, 2, 3, 5, 6]\n",
        "check_fn": "exact",
    },
    {
        "id": "max_subarray",
        "description": "Write a function max_subarray(nums) that finds the contiguous subarray with the largest sum.",
        "test_code": "print(max_subarray([-2,1,-3,4,-1,2,1,-5,4]))\nprint(max_subarray([1]))\nprint(max_subarray([5,4,-1,7,8]))",
        "expected_output": "6\n1\n23\n",
        "check_fn": "exact",
    },
    {
        "id": "climbing_stairs",
        "description": "Write a function climb_stairs(n) that returns the number of ways to climb n stairs (1 or 2 steps at a time).",
        "test_code": "print(climb_stairs(2))\nprint(climb_stairs(3))\nprint(climb_stairs(5))",
        "expected_output": "2\n3\n8\n",
        "check_fn": "exact",
    },
    {
        "id": "binary_search",
        "description": "Write a function binary_search(nums, target) that returns the index of target in sorted nums, or -1 if not found.",
        "test_code": "print(binary_search([-1,0,3,5,9,12], 9))\nprint(binary_search([-1,0,3,5,9,12], 2))",
        "expected_output": "4\n-1\n",
        "check_fn": "exact",
    },
    {
        "id": "invert_binary_tree",
        "description": "Write a function invert_tree(root) that inverts a binary tree. TreeNode has val, left, right attributes.",
        "test_code": "class TreeNode:\n    def __init__(self, val=0, left=None, right=None):\n        self.val = val\n        self.left = left\n        self.right = right\nroot = TreeNode(4, TreeNode(2, TreeNode(1), TreeNode(3)), TreeNode(7, TreeNode(6), TreeNode(9)))\nresult = invert_tree(root)\ndef inorder(n):\n    return (inorder(n.left) + [n.val] + inorder(n.right)) if n else []\nprint(inorder(result))",
        "expected_output": "[9, 7, 6, 4, 3, 2, 1]\n",
        "check_fn": "exact",
    },
    {
        "id": "linked_list_cycle",
        "description": "Write a function has_cycle(head) that returns True if a linked list has a cycle. ListNode has val, next attributes.",
        "test_code": "class ListNode:\n    def __init__(self, x):\n        self.val = x\n        self.next = None\nhead = ListNode(3)\nhead.next = ListNode(2)\nhead.next.next = ListNode(0)\nhead.next.next.next = ListNode(-4)\nhead.next.next.next.next = head.next\nprint(has_cycle(head))\nhead2 = ListNode(1)\nhead2.next = ListNode(2)\nhead2.next.next = head2\nprint(has_cycle(head2))\nhead3 = ListNode(1)\nprint(has_cycle(head3))",
        "expected_output": "True\nTrue\nFalse\n",
        "check_fn": "exact",
    },
    {
        "id": "longest_common_prefix",
        "description": "Write a function longest_common_prefix(strs) that returns the longest common prefix string among an array of strings.",
        "test_code": "print(longest_common_prefix(['flower','flow','flight']))\nprint(longest_common_prefix(['dog','racecar','car']))\nprint(longest_common_prefix(['']))",
        "expected_output": "fl\n\n\n",
        "check_fn": "exact",
    },
    {
        "id": "fizzbuzz",
        "description": "Write a function fizzbuzz(n) that returns a list of strings from 1 to n. Multiples of 3 -> 'Fizz', 5 -> 'Buzz', both -> 'FizzBuzz'.",
        "test_code": "print(fizzbuzz(15))",
        "expected_output": "['1', '2', 'Fizz', '4', 'Buzz', 'Fizz', '7', '8', 'Fizz', 'Buzz', '11', 'Fizz', '13', '14', 'FizzBuzz']\n",
        "check_fn": "exact",
    },
    {
        "id": "palindrome_number",
        "description": "Write a function is_palindrome(x) that returns True if x is a palindrome integer.",
        "test_code": "print(is_palindrome(121))\nprint(is_palindrome(-121))\nprint(is_palindrome(10))",
        "expected_output": "True\nFalse\nFalse\n",
        "check_fn": "exact",
    },
    {
        "id": "roman_to_int",
        "description": "Write a function roman_to_int(s) that converts a Roman numeral to an integer.",
        "test_code": "print(roman_to_int('III'))\nprint(roman_to_int('LVIII'))\nprint(roman_to_int('MCMXCIV'))",
        "expected_output": "3\n58\n1994\n",
        "check_fn": "exact",
    },
    {
        "id": "first_unique_char",
        "description": "Write a function first_unique_char(s) that returns the index of the first non-repeating character, or -1 if none.",
        "test_code": "print(first_unique_char('leetcode'))\nprint(first_unique_char('loveleetcode'))\nprint(first_unique_char('aabb'))",
        "expected_output": "0\n2\n-1\n",
        "check_fn": "exact",
    },
    {
        "id": "plus_one",
        "description": "Write a function plus_one(digits) that increments a large integer represented as an array of digits by one.",
        "test_code": "print(plus_one([1,2,3]))\nprint(plus_one([4,3,2,1]))\nprint(plus_one([9]))",
        "expected_output": "[1, 2, 4]\n[4, 3, 2, 2]\n[1, 0]\n",
        "check_fn": "exact",
    },
    {
        "id": "single_number",
        "description": "Write a function single_number(nums) that finds the element that appears only once (others appear twice).",
        "test_code": "print(single_number([2,2,1]))\nprint(single_number([4,1,2,1,2]))\nprint(single_number([1]))",
        "expected_output": "1\n4\n1\n",
        "check_fn": "exact",
    },
    {
        "id": "contains_duplicate",
        "description": "Write a function contains_duplicate(nums) that returns True if any value appears at least twice.",
        "test_code": "print(contains_duplicate([1,2,3,1]))\nprint(contains_duplicate([1,2,3,4]))\nprint(contains_duplicate([1,1,1,3,3,4,3,2,4,2]))",
        "expected_output": "True\nFalse\nTrue\n",
        "check_fn": "exact",
    },
    {
        "id": "missing_number",
        "description": "Write a function missing_number(nums) that returns the missing number in the range [0, n].",
        "test_code": "print(missing_number([3,0,1]))\nprint(missing_number([0,1]))\nprint(missing_number([9,6,4,2,3,5,7,0,1]))",
        "expected_output": "2\n2\n8\n",
        "check_fn": "exact",
    },
    {
        "id": "move_zeroes",
        "description": "Write a function move_zeroes(nums) that moves all zeroes to the end while maintaining relative order of non-zero elements.",
        "test_code": "nums = [0,1,0,3,12]\nmove_zeroes(nums)\nprint(nums)\nnums2 = [0]\nmove_zeroes(nums2)\nprint(nums2)",
        "expected_output": "[1, 3, 12, 0, 0]\n[0]\n",
        "check_fn": "exact",
    },
    {
        "id": "intersection_two_arrays",
        "description": "Write a function intersect(nums1, nums2) that returns the intersection of two arrays.",
        "test_code": "print(sorted(intersect([1,2,2,1], [2,2])))\nprint(sorted(intersect([4,9,5], [9,4,9,8,4])))",
        "expected_output": "[2, 2]\n[4, 9]\n",
        "check_fn": "exact",
    },
    {
        "id": "valid_anagram",
        "description": "Write a function is_anagram(s, t) that returns True if t is an anagram of s.",
        "test_code": "print(is_anagram('anagram', 'nagaram'))\nprint(is_anagram('rat', 'car'))",
        "expected_output": "True\nFalse\n",
        "check_fn": "exact",
    },
    {
        "id": "excel_column_title",
        "description": "Write a function convert_to_title(columnNumber) that returns the Excel column title (1 -> 'A', 28 -> 'AB').",
        "test_code": "print(convert_to_title(1))\nprint(convert_to_title(28))\nprint(convert_to_title(701))",
        "expected_output": "A\nAB\nZY\n",
        "check_fn": "exact",
    },
    {
        "id": "majority_element",
        "description": "Write a function majority_element(nums) that returns the element appearing more than n/2 times.",
        "test_code": "print(majority_element([3,2,3]))\nprint(majority_element([2,2,1,1,1,2,2]))",
        "expected_output": "3\n2\n",
        "check_fn": "exact",
    },
    {
        "id": "happy_number",
        "description": "Write a function is_happy(n) that returns True if n is a happy number.",
        "test_code": "print(is_happy(19))\nprint(is_happy(2))",
        "expected_output": "True\nFalse\n",
        "check_fn": "exact",
    },
    {
        "id": "remove_duplicates",
        "description": "Write a function remove_duplicates(nums) that removes duplicates in-place from sorted array and returns the count.",
        "test_code": "nums = [1,1,2]\nk = remove_duplicates(nums)\nprint(k, nums[:k])",
        "expected_output": "2 [1, 2]\n",
        "check_fn": "exact",
    },
]

TOOL_DESCRIPTION = """You solve programming puzzles by writing Python code.
Write ONLY a Python code block containing the required function(s).
Do NOT include any explanatory text, test code, or print statements outside the function.
Do NOT include markdown outside the code block.
The code will be extracted and run with the test harness.

Example:
```python
def two_sum(nums, target):
    seen = {}
    for i, n in enumerate(nums):
        diff = target - n
        if diff in seen:
            return [seen[diff], i]
        seen[n] = i
```

Just the code block, nothing else."""


# -- LLM Call ----------------------------------------------------------

def call_llm(messages: list[dict], model: str = DEFAULT_MODEL) -> dict:
    from openai import OpenAI

    api_key = (os.getenv("OPENAI_API_KEY") or os.getenv("GROQ_API_KEY")
               or os.getenv("LLM_API_KEY") or "")
    base_url = os.getenv("OPENAI_BASE_URL") or os.getenv("LLM_BASE_URL") or "https://api.openai.com/v1"

    client = OpenAI(api_key=api_key, base_url=base_url)

    start = time.time()
    resp = client.chat.completions.create(
        model=model, messages=messages, temperature=0.0
    )
    elapsed = time.time() - start

    choice = resp.choices[0]
    return {
        "content": choice.message.content or "",
        "input_tokens": resp.usage.prompt_tokens if resp.usage else 0,
        "output_tokens": resp.usage.completion_tokens if resp.usage else 0,
        "latency_ms": round(elapsed * 1000),
    }


# -- Code Extraction & Execution ---------------------------------------

def extract_code(response: str) -> str | None:
    """Extract Python code from the model response."""
    blocks = re.findall(r'```(?:python)?\n(.*?)```', response, re.DOTALL)
    if blocks:
        return blocks[0].strip()

    lines = response.strip().split("\n")
    code_lines = []
    in_code = False
    for line in lines:
        if line.startswith("def ") or line.startswith("class ") or line.startswith("import ") or line.startswith("from "):
            in_code = True
        if in_code:
            code_lines.append(line)
    if code_lines:
        return "\n".join(code_lines)
    return None


def run_code(code: str, test_code: str, problem_id: str) -> str:
    """Write code to a file and execute it, returning output."""
    file_path = PROBLEMS_DIR / f"{problem_id}.py"
    full_code = code + "\n\n" + test_code
    file_path.write_text(full_code)
    try:
        result = subprocess.run(
            [sys.executable, str(file_path)],
            capture_output=True, text=True, timeout=30,
            cwd=str(PROBLEMS_DIR)
        )
        output = result.stdout
        if result.stderr:
            stderr = result.stderr.strip()
            if stderr:
                output += f"\n[ERROR] {stderr[:2000]}"
        return output
    except subprocess.TimeoutExpired:
        return "TIMEOUT: Execution exceeded 30 seconds"
    except Exception as e:
        return f"EXECUTION ERROR: {e}"


def check_output(actual: str, expected: str) -> bool:
    """Compare actual output against expected."""
    return actual.strip() == expected.strip()


# -- Main Benchmark ----------------------------------------------------

SEP = "=" * 60
DASH = "-" * 60


def run_benchmark(model: str, limit: int = 0):
    problems = PROBLEMS[:limit] if limit > 0 else PROBLEMS

    print(f"\n{SEP}")
    print(f"ProgramBench | Model: {model} | Problems: {len(problems)}")
    print(f"{SEP}\n")

    results = []
    correct = 0
    total_tokens_in = 0
    total_tokens_out = 0
    total_latency = 0

    for i, prob in enumerate(problems):
        pid = prob["id"]
        desc = prob["description"]

        print(f"  [{i+1}/{len(problems)}] {pid}: {desc[:60]}...")

        messages = [
            {"role": "system", "content": TOOL_DESCRIPTION},
            {"role": "user", "content": desc + "\n\nWrite ONLY the function definition in a Python code block. NO test code, NO print statements, NO extra output. Just the function."},
        ]

        response = call_llm(messages, model)
        total_tokens_in += response["input_tokens"]
        total_tokens_out += response["output_tokens"]
        total_latency += response["latency_ms"]

        code = extract_code(response["content"])

        if not code:
            print(f"    FAIL - No code generated")
            results.append({
                "id": pid, "correct": False, "error": "No code generated",
                "input_tokens": response["input_tokens"],
                "output_tokens": response["output_tokens"],
            })
            continue

        output = run_code(code, prob["test_code"], pid)
        passed = check_output(output, prob["expected_output"])

        if passed:
            correct += 1
            status = "PASS"
        else:
            status = "FAIL"

        print(f"    {status} | tokens: {response['input_tokens']}P+{response['output_tokens']}C | {response['latency_ms']}ms")
        if not passed:
            actual_summary = output.strip()[:80].replace("\n", "\\n")
            expected_summary = prob["expected_output"].strip()[:80].replace("\n", "\\n")
            print(f"      Expected: {expected_summary}")
            print(f"      Got:      {actual_summary}")

        results.append({
            "id": pid,
            "correct": passed,
            "input_tokens": response["input_tokens"],
            "output_tokens": response["output_tokens"],
            "latency_ms": response["latency_ms"],
        })

    accuracy = correct / len(results) * 100 if results else 0
    avg_tokens_in = total_tokens_in / len(results) if results else 0
    avg_tokens_out = total_tokens_out / len(results) if results else 0
    avg_latency = total_latency / len(results) if results else 0

    print(f"\n{DASH}")
    print(f"RESULTS - ProgramBench | {model}")
    print(DASH)
    print(f"  Accuracy:      {correct}/{len(results)} = {accuracy:.1f}%")
    print(f"  Total tokens:  {total_tokens_in}P + {total_tokens_out}C = {total_tokens_in + total_tokens_out}")
    print(f"  Avg tokens:    {avg_tokens_in:.0f}P + {avg_tokens_out:.0f}C per case")
    print(f"  Avg latency:   {avg_latency:.0f}ms per case")
    print(DASH)

    out_path = RESULTS_DIR / "program_bench_results.json"
    summary = {
        "model": model,
        "total_cases": len(results),
        "correct": correct,
        "accuracy_pct": round(accuracy, 1),
        "total_tokens_in": total_tokens_in,
        "total_tokens_out": total_tokens_out,
    }
    with open(out_path, "w") as f:
        json.dump({"results": results, "summary": summary}, f, indent=2)
    print(f"  Results saved to {out_path}")

    return results, summary


def _load_env():
    """Load .env from Volt project root."""
    env_path = Path(__file__).parent.parent / ".env"
    if env_path.exists():
        with open(env_path) as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    k, v = line.split("=", 1)
                    os.environ.setdefault(k.strip(), v.strip())
    if not os.getenv("OPENAI_API_KEY") and os.getenv("GROQ_API_KEY"):
        os.environ["OPENAI_API_KEY"] = os.environ["GROQ_API_KEY"]
    if not os.getenv("OPENAI_BASE_URL") and os.getenv("LLM_BASE_URL"):
        os.environ["OPENAI_BASE_URL"] = os.environ["LLM_BASE_URL"]


if __name__ == "__main__":
    _load_env()
    parser = argparse.ArgumentParser(description="Volt ProgramBench")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--limit", type=int, default=0, help="Run only N problems")
    args = parser.parse_args()

    run_benchmark(args.model, args.limit)
