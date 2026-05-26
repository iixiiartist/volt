#!/usr/bin/env python3
"""Volt GAIA Benchmark — measures agent accuracy on the GAIA dataset.

Requires:
    pip install datasets requests openai
    export HF_TOKEN=your_hf_token  (for gated dataset access)

Usage:
    python gaia_benchmark.py --limit 5          # run 5 dev questions
    python gaia_benchmark.py --mode dev          # full dev set (165)
    python gaia_benchmark.py --mode test         # test set (301)
    python gaia_benchmark.py --mode both         # dev + test

Cost (Groq): ~$1.22 for full suite
"""

import argparse
import json
import os
import re
import sys
import time
import urllib.request
from pathlib import Path

RESULTS_DIR = Path(__file__).parent / "results"
RESULTS_DIR.mkdir(parents=True, exist_ok=True)

GAIA_DEV_URL = "https://huggingface.co/datasets/gaia-benchmark/GAIA/resolve/main/2023/dev.jsonl?download=1"
GAIA_TEST_URL = "https://huggingface.co/datasets/gaia-benchmark/GAIA/resolve/main/2023/test.jsonl?download=1"
ATTACHMENTS_BASE = "https://huggingface.co/datasets/gaia-benchmark/GAIA/resolve/main/2023/"

DEFAULT_MODEL = "llama-3.1-8b-instant"


# -- Data Loading ------------------------------------------------------

def download_jsonl(mode: str, path: Path) -> list[dict]:
    if path.exists():
        print(f"  Using cached {path}")
        cases = []
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line:
                    cases.append(json.loads(line))
        return cases

    print(f"  Downloading GAIA {mode} set via datasets library...")
    import datasets
    ds_name = "gaia-benchmark/GAIA"
    try:
        ds = datasets.load_dataset(ds_name, split=mode)
    except Exception as e:
        print(f"  Error: {e}")
        print(f"\n  The GAIA dataset is gated. To access it:")
        print(f"  1. Accept terms at https://huggingface.co/datasets/gaia-benchmark/GAIA")
        print(f"  2. Create a token at https://huggingface.co/settings/tokens")
        print(f"  3. Login: huggingface-cli login")
        print(f"     Or set: $env:HF_TOKEN = 'your_token_here'")
        sys.exit(1)
    cases = list(ds)
    with open(path, "w") as f:
        for c in cases:
            f.write(json.dumps(c, default=str) + "\n")
    print(f"  Saved {len(cases)} cases to {path}")
    return cases


def download_attachment(task_id: str, file_name: str) -> Path | None:
    """Download a GAIA attachment and return its local path."""
    if not file_name:
        return None
    attach_dir = RESULTS_DIR / "attachments"
    attach_dir.mkdir(parents=True, exist_ok=True)
    safe_name = file_name.replace("/", "_").replace("\\", "_")
    local_path = attach_dir / f"{task_id}_{safe_name}"
    if local_path.exists():
        return local_path

    try:
        from huggingface_hub import hf_hub_download
        path = hf_hub_download(
            repo_id="gaia-benchmark/GAIA",
            filename=f"2023/{file_name}",
            repo_type="dataset",
        )
        import shutil
        shutil.copy2(path, local_path)
        return local_path
    except Exception as e:
        print(f"    [warn] Failed to download attachment {file_name}: {e}")
        return None


# -- LLM Call ----------------------------------------------------------

def _estimate_tokens(text: str) -> int:
    """Rough token estimate: 1 token ≈ 4 chars."""
    return len(text) // 4


def compress_messages(messages: list[dict], max_tokens: int = 6000) -> list[dict]:
    """Drop oldest tool results when total tokens exceed max_tokens."""
    total = sum(_estimate_tokens(m.get("content", "") or "") for m in messages)
    if total <= max_tokens:
        return messages
    # Keep system + user + last few assistant/tool exchanges
    kept = []
    tool_count = 0
    for m in reversed(messages):
        kept.insert(0, m)
        if m["role"] in ("assistant", "tool"):
            tool_count += 1
            if tool_count >= 6:  # keep last 3 tool turns
                break
    # Always include system message
    if kept[0]["role"] != "system" and messages[0]["role"] == "system":
        kept.insert(0, messages[0])
    return kept


def call_llm(messages: list[dict], tools: list[dict] | None = None, model: str = DEFAULT_MODEL) -> dict:
    from openai import OpenAI

    api_key = (os.getenv("OPENAI_API_KEY") or os.getenv("GROQ_API_KEY")
               or os.getenv("LLM_API_KEY") or "")
    base_url = os.getenv("OPENAI_BASE_URL") or os.getenv("LLM_BASE_URL") or "https://api.openai.com/v1"

    client = OpenAI(api_key=api_key, base_url=base_url)

    kwargs = dict(model=model, messages=messages, temperature=0.0)
    if tools:
        kwargs["tools"] = tools
        kwargs["tool_choice"] = "auto"

    start = time.time()
    resp = client.chat.completions.create(**kwargs)
    elapsed = time.time() - start

    choice = resp.choices[0]
    content = choice.message.content or ""

    tool_calls = []
    if choice.finish_reason == "tool_calls" and choice.message.tool_calls:
        for tc in choice.message.tool_calls:
            tool_calls.append({
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.function.name,
                    "arguments": tc.function.arguments,
                },
            })

    return {
        "content": content,
        "tool_calls": tool_calls,
        "input_tokens": resp.usage.prompt_tokens if resp.usage else 0,
        "output_tokens": resp.usage.completion_tokens if resp.usage else 0,
        "latency_ms": round(elapsed * 1000),
        "finish_reason": choice.finish_reason,
    }


# -- Answer Extraction -------------------------------------------------

def extract_answer(response: dict) -> str:
    """Extract the final answer from the model's response."""
    content = response.get("content", "")
    if not content:
        return ""

    lines = content.strip().split("\n")
    for line in reversed(lines):
        line = line.strip()
        if line and not line.startswith("#") and not line.startswith("//"):
            lower = line.lower()
            if any(line.startswith(p) for p in ["answer:", "final answer:", "the answer is", "result:", "answer ="]):
                for p in ["answer:", "final answer:", "the answer is", "result:", "answer ="]:
                    idx = lower.find(p)
                    if idx >= 0:
                        val = line[idx + len(p):].strip().rstrip(".")
                        if val:
                            return val
                return line.split(":")[-1].strip()

    for line in reversed(lines):
        if line.strip():
            return line.strip().strip(".").strip()
    return ""


def normalize_answer(ans: str) -> str:
    """Normalize an answer for comparison (lowercase, strip punctuation, collapse whitespace)."""
    if not ans:
        return ""
    ans = ans.lower().strip()
    ans = re.sub(r'[^\w\s\-.,%$€£¥]', '', ans)
    ans = re.sub(r'\s+', ' ', ans).strip()
    ans = ans.strip(".,;:!?")
    return ans


def evaluate_answer(predicted: str, expected: str) -> bool:
    """Check if predicted answer matches the expected GAIA answer."""
    pred_norm = normalize_answer(predicted)
    exp_norm = normalize_answer(expected)
    if not pred_norm or not exp_norm:
        return False

    if pred_norm == exp_norm:
        return True

    if exp_norm in pred_norm or pred_norm in exp_norm:
        return True

    try:
        pred_f = float(pred_norm.replace(",", "").replace("$", "").replace("€", "").replace("%", ""))
        exp_f = float(exp_norm.replace(",", "").replace("$", "").replace("€", "").replace("%", ""))
        if abs(pred_f - exp_f) < 0.01 * abs(exp_f) + 0.001:
            return True
    except ValueError:
        pass

    return False


# -- Agent Tools -------------------------------------------------------

GAIA_TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "web_fetch",
            "description": "Fetch content from a URL and return the text.",
            "parameters": {
                "type": "object",
                "properties": {"url": {"type": "string", "description": "The URL to fetch"}},
                "required": ["url"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web for information and return a summary of results.",
            "parameters": {
                "type": "object",
                "properties": {"query": {"type": "string", "description": "The search query"}},
                "required": ["query"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "python_repl",
            "description": "Execute Python code and return the output.",
            "parameters": {
                "type": "object",
                "properties": {"code": {"type": "string", "description": "Python code to execute"}},
                "required": ["code"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read the contents of a file at the given path.",
            "parameters": {
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Path to the file"}},
                "required": ["path"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "final_answer",
            "description": "Provide the final answer for this GAIA question. Call this when you are ready to submit.",
            "parameters": {
                "type": "object",
                "properties": {"answer": {"type": "string", "description": "Your final answer"}},
                "required": ["answer"],
            },
        },
    },
]


def execute_tool(name: str, args: dict, attachments: list[Path]) -> str:
    """Execute a tool call. Simulates available tools for the agent."""
    if name == "web_fetch":
        url = args.get("url", "")
        try:
            import urllib.request
            with urllib.request.urlopen(url, timeout=15) as resp:
                text = resp.read().decode("utf-8", errors="replace")
                return text[:8000]
        except Exception as e:
            return f"Error fetching {url}: {e}"

    elif name == "web_search":
        query = args.get("query", "")
        try:
            import urllib.parse, urllib.request, json
            encoded = urllib.parse.quote(query)
            url = f"https://api.duckduckgo.com/?q={encoded}&format=json&no_html=1"
            with urllib.request.urlopen(url, timeout=10) as resp:
                data = json.loads(resp.read().decode())
                abstract = data.get("AbstractText", "")
                source = data.get("AbstractSource", "")
                answer_type = data.get("Type", "")
                if answer_type == "A" and data.get("Answer"):
                    return data["Answer"]
                if abstract:
                    return f"{abstract}\nSource: {source}"
                results = []
                if data.get("RelatedTopics"):
                    for r in data["RelatedTopics"][:5]:
                        if isinstance(r, dict):
                            results.append(r.get("Text", ""))
                if results:
                    return "\n".join(results)
                return f"No results found for: {query}"
        except Exception as e:
            return f"web_search unavailable: {e}"

    elif name == "python_repl":
        code = args.get("code", "")
        try:
            import subprocess
            result = subprocess.run(
                [sys.executable, "-c", code],
                capture_output=True, text=True, timeout=30, cwd=RESULTS_DIR
            )
            output = result.stdout or ""
            if result.stderr:
                output += f"\n[stderr]\n{result.stderr[:2000]}"
            return output[:5000] or "(no output)"
        except subprocess.TimeoutExpired:
            return "Execution timed out after 30s"
        except Exception as e:
            return f"Execution error: {e}"

    elif name == "read_file":
        path = args.get("path", "")
        p = Path(path)
        if not p.exists():
            for a in attachments:
                if a.name == p.name or str(a) == str(p):
                    p = a
                    break
        if p.exists():
            try:
                text = p.read_text(encoding="utf-8", errors="replace")
                return text[:8000]
            except Exception as e:
                return f"Error reading {p}: {e}"
        return f"File not found: {path}"

    return f"Unknown tool: {name}"


# -- Main Benchmark ----------------------------------------------------

SEP = "=" * 60
DASH = "-" * 60


def run_benchmark(mode: str, model: str, limit: int = 0):
    cases_file = RESULTS_DIR / f"gaia_{mode}.jsonl"
    cases = download_jsonl(mode, cases_file)

    run_cases = cases[:limit] if limit > 0 else cases

    print(f"\n{SEP}")
    print(f"GAIA {mode.upper()} | Model: {model} | Cases: {len(run_cases)}")
    print(SEP)

    results = []
    correct = 0
    total_tokens_in = 0
    total_tokens_out = 0
    total_steps = 0

    for i, case in enumerate(run_cases):
        task_id = case.get("task_id", f"case_{i}")
        question = case.get("Question", case.get("question", ""))
        expected = case.get("Final answer", case.get("final_answer", ""))
        file_name = case.get("file_name", case.get("file_name", ""))
        steps = case.get("Annotator Metadata", {}).get("Steps", "?")

        print(f"\n  [{i+1}/{len(run_cases)}] Task: {task_id} | Steps: {steps}")

        # Download attachment if present
        attachments = []
        if file_name:
            attach_path = download_attachment(task_id, file_name)
            if attach_path:
                attachments.append(attach_path)
                print(f"    Attachment: {attach_path.name} ({attach_path.stat().st_size} bytes)")

        # Build system prompt
        system_msg = {
            "role": "system",
            "content": (
                "You are an AI assistant solving a GAIA benchmark question. "
                "You have access to tools: web_fetch, web_search, python_repl, read_file. "
                "Use them to research and compute the answer. "
                "When you have the final answer, call final_answer with your answer. "
                "Be precise — answers are often specific numbers, names, or short phrases."
            ),
        }

        messages = [system_msg, {"role": "user", "content": question}]
        final_answer = ""
        step_count = 0
        case_tokens_in = 0
        case_tokens_out = 0

        for turn in range(15):
            step_count += 1
            try:
                response = call_llm(messages, GAIA_TOOLS, model)
            except Exception as e:
                # Handle API errors (tool_use_failed, rate limits, etc.)
                print(f"    [warn] API error on turn {turn}: {e}")
                if not final_answer:
                    final_answer = "(api_error)"
                break
            case_tokens_in += response["input_tokens"]
            case_tokens_out += response["output_tokens"]

            content = response.get("content", "")
            tool_calls = response.get("tool_calls", [])

            if not tool_calls and not content:
                final_answer = "(empty response)"
                break

            if content:
                messages.append({"role": "assistant", "content": content})

            if response.get("finish_reason") == "stop" and not tool_calls:
                final_answer = content
                break

            if tool_calls:
                assistant_msg = {"role": "assistant", "content": content or None, "tool_calls": tool_calls}
                messages.append(assistant_msg)

                has_final = False
                for tc in tool_calls:
                    tc_name = tc["function"]["name"]
                    tc_args = json.loads(tc["function"].get("arguments", "{}"))

                    if tc_name == "final_answer":
                        final_answer = tc_args.get("answer", "")
                        has_final = True
                        break

                    result = execute_tool(tc_name, tc_args, attachments)
                    messages.append({
                        "role": "tool",
                        "tool_call_id": tc["id"],
                        "content": result,
                    })
                # Compress if context getting long
                messages = compress_messages(messages)

                if has_final:
                    break

        else:
            if not final_answer:
                final_answer = "(max turns reached)"

        passed = evaluate_answer(final_answer, expected)

        if passed:
            correct += 1

        status = "PASS" if passed else "FAIL"
        print(f"    {status} | Predicted: {final_answer[:80]}")
        print(f"    Expected: {expected[:80]} | Steps: {step_count} | Tokens: {case_tokens_in}P+{case_tokens_out}C")

        total_tokens_in += case_tokens_in
        total_tokens_out += case_tokens_out
        total_steps += step_count

        results.append({
            "task_id": task_id,
            "correct": passed,
            "predicted": final_answer,
            "expected": expected,
            "steps": step_count,
            "input_tokens": case_tokens_in,
            "output_tokens": case_tokens_out,
        })

    accuracy = correct / len(results) * 100 if results else 0
    avg_steps = total_steps / len(results) if results else 0
    avg_tokens_in = total_tokens_in / len(results) if results else 0
    avg_tokens_out = total_tokens_out / len(results) if results else 0

    print(f"\n{DASH}")
    print(f"RESULTS - GAIA {mode.upper()} | {model}")
    print(DASH)
    print(f"  Accuracy:      {correct}/{len(results)} = {accuracy:.1f}%")
    print(f"  Avg steps:     {avg_steps:.1f}")
    print(f"  Total tokens:  {total_tokens_in}P + {total_tokens_out}C = {total_tokens_in + total_tokens_out}")
    print(f"  Avg tokens:    {avg_tokens_in:.0f}P + {avg_tokens_out:.0f}C per case")
    print(DASH)

    # Save results
    out_path = RESULTS_DIR / f"gaia_{mode}_results.json"
    summary = {
        "mode": mode,
        "model": model,
        "total_cases": len(results),
        "correct": correct,
        "accuracy_pct": round(accuracy, 1),
        "avg_steps": round(avg_steps, 1),
        "total_tokens_in": total_tokens_in,
        "total_tokens_out": total_tokens_out,
    }
    with open(out_path, "w") as f:
        json.dump({"results": results, "summary": summary}, f, indent=2)
    print(f"  Results saved to {out_path}")

    return results, summary


def _load_env():
    """Load .env from Volt project root (overrides stale env vars)."""
    env_path = Path(__file__).parent.parent / ".env"
    if env_path.exists():
        with open(env_path) as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    k, v = line.split("=", 1)
                    os.environ[k.strip()] = v.strip()
    if os.getenv("GROQ_API_KEY"):
        os.environ.setdefault("OPENAI_API_KEY", os.environ["GROQ_API_KEY"])
        os.environ.setdefault("OPENAI_BASE_URL", "https://api.groq.com/openai/v1")
    if os.getenv("LLM_BASE_URL"):
        os.environ.setdefault("OPENAI_BASE_URL", os.environ["LLM_BASE_URL"])
    if os.getenv("LLM_API_KEY"):
        os.environ.setdefault("OPENAI_API_KEY", os.environ["LLM_API_KEY"])


if __name__ == "__main__":
    _load_env()
    parser = argparse.ArgumentParser(description="Volt GAIA Benchmark")
    parser.add_argument("--mode", choices=["dev", "test", "both"], default="dev")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--limit", type=int, default=0, help="Run only N questions")
    args = parser.parse_args()

    for mode in (["dev", "test"] if args.mode == "both" else [args.mode]):
        run_benchmark(mode, args.model, args.limit)
