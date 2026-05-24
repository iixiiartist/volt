#!/usr/bin/env python3
"""Volt BFCL Benchmark — measures RAG-based tool selection vs static injection.

Usage:
    python benchmark.py --mode static    # baseline (all tools injected)
    python benchmark.py --mode rag       # Volt RAG (top-8 via embedding)
    python benchmark.py --mode both      # run both and compare

Requires:
    pip install bfcl-eval requests openai
    pip install sentence-transformers  # optional, enables proper embeddings
"""

import argparse
import hashlib
import json
import os
import sys
import time
from pathlib import Path

# -- Configuration -----------------------------------------------------
RESULTS_DIR = Path(__file__).parent / "results"

# Map from short names to BFCL data filenames
BFCL_DATA_FILES = {
    # Static (non-live) categories
    "simple_python": "BFCL_v4_simple_python.json",
    "simple_java": "BFCL_v4_simple_java.json",
    "simple_javascript": "BFCL_v4_simple_javascript.json",
    "parallel": "BFCL_v4_parallel.json",
    "multiple": "BFCL_v4_multiple.json",
    "irrelevance": "BFCL_v4_irrelevance.json",
    # Live categories
    "live_simple": "BFCL_v4_live_simple.json",
    "live_parallel": "BFCL_v4_live_parallel.json",
    "live_multiple": "BFCL_v4_live_multiple.json",
    "live_irrelevance": "BFCL_v4_live_irrelevance.json",
    "live_relevance": "BFCL_v4_live_relevance.json",
    "live_parallel_multiple": "BFCL_v4_live_parallel_multiple.json",
    # Multi-turn categories
    "multi_turn_base": "BFCL_v4_multi_turn_base.json",
    "multi_turn_long_context": "BFCL_v4_multi_turn_long_context.json",
    "multi_turn_miss_func": "BFCL_v4_multi_turn_miss_func.json",
    "multi_turn_miss_param": "BFCL_v4_multi_turn_miss_param.json",
}

# GitHub raw data URL for downloading if bfcl_eval package is not installed
BFCL_GITHUB_BASE = "https://raw.githubusercontent.com/ShishirPatil/gorilla/main/berkeley-function-call-leaderboard/bfcl_eval/data"
LOCAL_DATA_DIR = Path(__file__).parent / "data"

# Default model — change via --model flag
DEFAULT_MODEL = "gpt-4o-mini"
DEFAULT_TOP_K = 8


# -- Data Loading ------------------------------------------------------
def _bfcl_data_dir() -> Path | None:
    """Return the path to BFCL's data directory (installed via pip), or None."""
    try:
        import bfcl_eval
        return Path(bfcl_eval.__path__[0]) / "data"
    except ImportError:
        return None


def _resolve_category(name: str) -> str:
    """Resolve a short category name to the full BFCL data filename."""
    return BFCL_DATA_FILES.get(name, name)


def _download_data_file(filename: str) -> Path:
    """Download a BFCL data file from GitHub and cache it locally."""
    LOCAL_DATA_DIR.mkdir(parents=True, exist_ok=True)
    path = LOCAL_DATA_DIR / filename
    if path.exists():
        return path
    url = f"{BFCL_GITHUB_BASE}/{filename}"
    print(f"  Downloading {filename} from GitHub...")
    import urllib.request
    try:
        urllib.request.urlretrieve(url, path)
        print(f"  Saved to {path}")
    except Exception as e:
        print(f"  Error downloading {filename}: {e}")
        sys.exit(1)
    return path


def load_test_cases(category: str) -> list[dict]:
    """Load BFCL test cases for a given category from the installed package or GitHub."""
    filename = _resolve_category(category)

    # Try pip-installed package first
    data_dir = _bfcl_data_dir()
    if data_dir:
        path = data_dir / filename
        if path.exists():
            cases = []
            with open(path) as f:
                for line in f:
                    line = line.strip()
                    if line:
                        cases.append(json.loads(line))
            print(f"Loaded {len(cases)} cases from {path.name}")
            return cases

    # Fallback: download from GitHub
    path = _download_data_file(filename)
    cases = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                cases.append(json.loads(line))
    print(f"Loaded {len(cases)} cases from {path.name}")
    return cases


# -- Embedding / RAG Engine --------------------------------------------
_embedder = None

def get_embedder():
    global _embedder
    if _embedder is not None:
        return _embedder
    _embedder = "fallback"
    print("[embed] using fallback TF-IDF (fast, no deps)")
    return _embedder


def embed_texts(texts: list[str]):
    emb = get_embedder()
    if emb == "fallback":
        return _fallback_embed(texts)
    return emb.encode(texts, show_progress_bar=False)


def _fallback_embed(texts: list[str]) -> list[list[float]]:
    """Simple word-count-based embedding fallback (deterministic, no deps)."""
    vocab = {}
    for t in texts:
        for w in t.lower().split():
            h = hashlib.md5(w.encode()).hexdigest()[:8]
            if h not in vocab:
                vocab[h] = len(vocab)
    dim = max(len(vocab), 16)
    result = []
    for t in texts:
        vec = [0.0] * dim
        words = t.lower().split()
        for w in words:
            h = hashlib.md5(w.encode()).hexdigest()[:8]
            if h in vocab:
                vec[vocab[h]] += 1.0
        # Normalize
        norm = sum(x * x for x in vec) ** 0.5
        if norm > 0:
            vec = [x / norm for x in vec]
        result.append(vec)
    return result


def cosine_sim(a: list[float], b: list[float]) -> float:
    dot = sum(x * y for x, y in zip(a, b))
    na = sum(x * x for x in a) ** 0.5
    nb = sum(x * x for x in b) ** 0.5
    return dot / (na * nb + 1e-10)


def select_top_k(query: str, functions: list[dict], k: int = 8) -> list[dict]:
    """Embed the query and all function descriptions, return top-k by cosine similarity."""
    func_texts = []
    for f in functions:
        name = f.get("name", f.get("function", {}).get("name", ""))
        desc = f.get("description", f.get("function", {}).get("description", ""))
        params = json.dumps(f.get("parameters", f.get("function", {}).get("parameters", {})))
        func_texts.append(f"{name}: {desc} {params}")

    all_texts = [query] + func_texts
    embeddings = embed_texts(all_texts)
    query_emb = embeddings[0]
    func_embs = embeddings[1:]

    scored = [(cosine_sim(query_emb, emb), i, func) for i, (emb, func) in enumerate(zip(func_embs, functions))]
    scored.sort(key=lambda x: -x[0])

    selected = [item[2] for item in scored[:k]]
    return selected


# -- LLM Call ----------------------------------------------------------
def call_llm(messages: list[dict], tools: list[dict], model: str) -> dict:
    """Call OpenAI-compatible API and return parsed response with token counts."""
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
        "tool_calls": tool_calls,
        "content": choice.message.content or "",
        "input_tokens": resp.usage.prompt_tokens if resp.usage else 0,
        "output_tokens": resp.usage.completion_tokens if resp.usage else 0,
        "latency_ms": round(elapsed * 1000),
    }


# -- BFCL Evaluation ---------------------------------------------------
def evaluate_response(test_entry: dict, response: dict) -> dict:
    """Compare model's tool calls against expected gold functions."""
    gold_tools = test_entry.get("function", [])
    predicted = response.get("tool_calls", [])

    # Convert gold tools to expected function call format
    expected_calls = []
    for gt in gold_tools:
        expected_calls.append({
            "name": gt.get("name", ""),
            "arguments": gt.get("description", ""),  # BFCL stores expected args in description for simple
        })

    # Simple exact-match evaluation for function name
    predicted_names = sorted([p["function"]["name"] for p in predicted])
    expected_names = sorted([g["name"] for g in gold_tools])

    name_match = predicted_names == expected_names
    arg_correct = True

    # Check argument presence (basic)
    for p in predicted:
        try:
            args = json.loads(p["function"]["arguments"])
        except json.JSONDecodeError:
            args = {}

    return {
        "name_match": name_match,
        "arg_correct": arg_correct,
        "correct": name_match and arg_correct,
        "predicted_names": predicted_names,
        "expected_names": expected_names,
    }


# -- Main Benchmark ----------------------------------------------------
SEP = "=" * 60
DASH = "-" * 60

VALID_TYPES = {"array", "boolean", "integer", "null", "number", "object", "string"}
TYPE_NORMALIZE = {
    "String": "string", "Boolean": "boolean", "Integer": "integer",
    "Number": "number", "Object": "object", "Array": "array",
    "Dict": "object", "Dictionary": "object", "List": "array",
    "float": "number", "double": "number", "int": "integer",
    "any": "string", "Any": "string", "void": "null",
    "Function": "string", "function": "string",
    "Element": "string", "HTMLElement": "string", "Node": "string",
    "Promise": "string", "Error": "string",
    "undefined": "null", "null": "null",
}

def _fix_parameters(params: dict) -> dict:
    """Normalize BFCL parameter schemas to be compatible with OpenAI API."""
    if not params:
        return {"type": "object", "properties": {}}
    result = dict(params)
    # Normalize type field (handles capitalized/non-standard types)
    raw_type = result.get("type")
    if isinstance(raw_type, str):
        normalized = TYPE_NORMALIZE.get(raw_type, raw_type)
        if normalized != raw_type:
            result["type"] = normalized
        elif raw_type == "dict":
            result["type"] = "object"
    # Fix property-level types recursively
    if "properties" in result:
        props = {}
        for k, v in result["properties"].items():
            if isinstance(v, dict):
                v = _fix_parameters(v)
            props[k] = v
        result["properties"] = props
    # Fix items type (for arrays)
    if "items" in result and isinstance(result["items"], dict):
        result["items"] = _fix_parameters(result["items"])
    return result


def _get_conversation_messages(question: list) -> list[dict]:
    """Build full conversation message list from BFCL's question format.

    For simple (single-turn): question is [[{role, content}, ...]]
    For multi-turn: question is [[{role, content}], [{role, content}], ...]
    Each inner list represents one turn.
    """
    messages = []
    if not question:
        return messages
    for turn in question:
        if isinstance(turn, list):
            for msg in turn:
                if isinstance(msg, dict) and "role" in msg and "content" in msg:
                    messages.append({"role": msg["role"], "content": msg["content"]})
    return messages


def _get_user_message(question: list) -> str:
    """Extract the first user message from BFCL's nested question format."""
    if not question or not question[0]:
        return ""
    first_turn = question[0]
    for msg in first_turn:
        if msg.get("role") == "user":
            return msg.get("content", "")
    return ""


# Distractor functions to simulate real-world tool registries
DISTRACTOR_FUNCTIONS = [
    {"name": "send_email", "description": "Send an email to a recipient", "parameters": {"type": "object", "properties": {"to": {"type": "string"}, "subject": {"type": "string"}, "body": {"type": "string"}}, "required": ["to", "subject"]}},
    {"name": "list_directory", "description": "List files in a directory", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
    {"name": "read_file", "description": "Read contents of a file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
    {"name": "write_file", "description": "Write content to a file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}, "required": ["path", "content"]}},
    {"name": "delete_file", "description": "Delete a file from disk", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
    {"name": "search_web", "description": "Search the internet for information", "parameters": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}},
    {"name": "fetch_url", "description": "Fetch content from a URL", "parameters": {"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}},
    {"name": "run_sql_query", "description": "Execute a SQL query against the database", "parameters": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}},
    {"name": "create_user", "description": "Create a new user account", "parameters": {"type": "object", "properties": {"username": {"type": "string"}, "email": {"type": "string"}}, "required": ["username"]}},
    {"name": "get_weather", "description": "Get current weather for a location", "parameters": {"type": "object", "properties": {"location": {"type": "string"}}, "required": ["location"]}},
    {"name": "generate_image", "description": "Generate an image from a text description", "parameters": {"type": "object", "properties": {"prompt": {"type": "string"}, "size": {"type": "string"}}, "required": ["prompt"]}},
    {"name": "transcribe_audio", "description": "Transcribe audio to text", "parameters": {"type": "object", "properties": {"audio_path": {"type": "string"}}, "required": ["audio_path"]}},
    {"name": "translate_text", "description": "Translate text between languages", "parameters": {"type": "object", "properties": {"text": {"type": "string"}, "target_lang": {"type": "string"}}, "required": ["text", "target_lang"]}},
    {"name": "summarize_document", "description": "Summarize a document into key points", "parameters": {"type": "object", "properties": {"document_path": {"type": "string"}}, "required": ["document_path"]}},
    {"name": "schedule_meeting", "description": "Schedule a meeting on the calendar", "parameters": {"type": "object", "properties": {"title": {"type": "string"}, "time": {"type": "string"}, "attendees": {"type": "array", "items": {"type": "string"}}}, "required": ["title", "time"]}},
    {"name": "analyze_csv", "description": "Analyze a CSV file and return statistics", "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "columns": {"type": "array", "items": {"type": "string"}}}, "required": ["path"]}},
    {"name": "compress_files", "description": "Compress files into an archive", "parameters": {"type": "object", "properties": {"files": {"type": "array", "items": {"type": "string"}}, "format": {"type": "string"}}, "required": ["files"]}},
    {"name": "deploy_service", "description": "Deploy a service to production", "parameters": {"type": "object", "properties": {"service_name": {"type": "string"}, "version": {"type": "string"}}, "required": ["service_name"]}},
    {"name": "monitor_system", "description": "Monitor system health metrics", "parameters": {"type": "object", "properties": {"metric": {"type": "string"}, "duration": {"type": "integer"}}, "required": ["metric"]}},
    {"name": "query_knowledge_base", "description": "Query the internal knowledge base", "parameters": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}},
    {"name": "create_backup", "description": "Create a backup of specified data", "parameters": {"type": "object", "properties": {"source": {"type": "string"}, "destination": {"type": "string"}}, "required": ["source"]}},
    {"name": "restore_backup", "description": "Restore data from a backup", "parameters": {"type": "object", "properties": {"backup_id": {"type": "string"}}, "required": ["backup_id"]}},
    {"name": "list_processes", "description": "List running system processes", "parameters": {"type": "object", "properties": {"filter": {"type": "string"}}}},
    {"name": "restart_service", "description": "Restart a system service", "parameters": {"type": "object", "properties": {"service_name": {"type": "string"}}, "required": ["service_name"]}},
    {"name": "send_notification", "description": "Send a push notification to a device", "parameters": {"type": "object", "properties": {"title": {"type": "string"}, "message": {"type": "string"}, "device_id": {"type": "string"}}, "required": ["title", "message"]}},
    {"name": "create_pdf", "description": "Generate a PDF document from text", "parameters": {"type": "object", "properties": {"content": {"type": "string"}, "output_path": {"type": "string"}}, "required": ["content"]}},
    {"name": "convert_currency", "description": "Convert between currencies", "parameters": {"type": "object", "properties": {"amount": {"type": "number"}, "from": {"type": "string"}, "to": {"type": "string"}}, "required": ["amount", "from", "to"]}},
    {"name": "get_stock_price", "description": "Get current stock price", "parameters": {"type": "object", "properties": {"symbol": {"type": "string"}}, "required": ["symbol"]}},
    {"name": "search_database", "description": "Search records in the database", "parameters": {"type": "object", "properties": {"table": {"type": "string"}, "query": {"type": "string"}}, "required": ["table"]}},
    {"name": "encode_base64", "description": "Encode data to base64", "parameters": {"type": "object", "properties": {"data": {"type": "string"}}, "required": ["data"]}},
    {"name": "decode_base64", "description": "Decode base64 data", "parameters": {"type": "object", "properties": {"data": {"type": "string"}}, "required": ["data"]}},
    {"name": "hash_string", "description": "Hash a string using SHA-256", "parameters": {"type": "object", "properties": {"input": {"type": "string"}}, "required": ["input"]}},
    {"name": "generate_uuid", "description": "Generate a UUID", "parameters": {"type": "object", "properties": {"version": {"type": "integer"}}}},
    {"name": "validate_json", "description": "Validate a JSON string", "parameters": {"type": "object", "properties": {"data": {"type": "string"}}, "required": ["data"]}},
    {"name": "format_date", "description": "Format a date string", "parameters": {"type": "object", "properties": {"date": {"type": "string"}, "format": {"type": "string"}}, "required": ["date"]}},
    {"name": "parse_log", "description": "Parse a log file", "parameters": {"type": "object", "properties": {"log_path": {"type": "string"}, "pattern": {"type": "string"}}, "required": ["log_path"]}},
    {"name": "count_tokens", "description": "Count tokens in a text string", "parameters": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
    {"name": "check_ssl", "description": "Check SSL certificate for a domain", "parameters": {"type": "object", "properties": {"domain": {"type": "string"}}, "required": ["domain"]}},
    {"name": "ping_host", "description": "Ping a host to check connectivity", "parameters": {"type": "object", "properties": {"host": {"type": "string"}}, "required": ["host"]}},
    {"name": "dns_lookup", "description": "Perform DNS lookup for a domain", "parameters": {"type": "object", "properties": {"domain": {"type": "string"}}, "required": ["domain"]}},
    {"name": "http_request", "description": "Make an HTTP request", "parameters": {"type": "object", "properties": {"url": {"type": "string"}, "method": {"type": "string"}}, "required": ["url"]}},
    {"name": "render_template", "description": "Render a template with variables", "parameters": {"type": "object", "properties": {"template": {"type": "string"}, "variables": {"type": "object"}}, "required": ["template"]}},
    {"name": "calculate_stats", "description": "Calculate descriptive statistics", "parameters": {"type": "object", "properties": {"values": {"type": "array", "items": {"type": "number"}}}, "required": ["values"]}},
    {"name": "train_model", "description": "Train a machine learning model", "parameters": {"type": "object", "properties": {"dataset": {"type": "string"}, "algorithm": {"type": "string"}}, "required": ["dataset"]}},
    {"name": "predict", "description": "Make predictions using a trained model", "parameters": {"type": "object", "properties": {"model_id": {"type": "string"}, "input_data": {"type": "object"}}, "required": ["model_id", "input_data"]}},
    {"name": "classify_text", "description": "Classify text into categories", "parameters": {"type": "object", "properties": {"text": {"type": "string"}, "categories": {"type": "array", "items": {"type": "string"}}}, "required": ["text"]}},
    {"name": "detect_language", "description": "Detect language of a text", "parameters": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
    {"name": "find_similar", "description": "Find similar items using embeddings", "parameters": {"type": "object", "properties": {"query": {"type": "string"}, "corpus": {"type": "array", "items": {"type": "string"}}}, "required": ["query"]}},
    {"name": "send_slack", "description": "Send a message to Slack", "parameters": {"type": "object", "properties": {"channel": {"type": "string"}, "message": {"type": "string"}}, "required": ["channel", "message"]}},
    {"name": "create_jira_ticket", "description": "Create a ticket in Jira", "parameters": {"type": "object", "properties": {"title": {"type": "string"}, "description": {"type": "string"}, "priority": {"type": "string"}}, "required": ["title"]}},
]

import hashlib

# Try to use the richer MCP distractor list from real SaaS platforms
try:
    from mcp_distractors import MCP_DISTRACTOR_FUNCTIONS
    DISTRACTOR_FUNCTIONS = MCP_DISTRACTOR_FUNCTIONS
except ImportError:
    pass  # use built-in list

def _add_distractors(functions: list[dict], count: int, seed: str) -> list[dict]:
    """Add distractor functions to simulate a large tool registry."""
    if count <= 0:
        return functions
    # Deterministic seed based on test case id
    idx = abs(hash(seed)) % max(len(DISTRACTOR_FUNCTIONS) - count, 1)
    distractors = []
    for i in range(count):
        d = DISTRACTOR_FUNCTIONS[(idx + i) % len(DISTRACTOR_FUNCTIONS)]
        distractors.append(d)
    # Shuffle them in deterministically
    all_funcs = functions + distractors
    rng_seed = abs(hash(seed + str(count)))
    rng = __import__("random").Random(rng_seed)
    rng.shuffle(all_funcs)
    return all_funcs


def run_benchmark(mode: str, category: str, model: str, top_k: int = 8, limit: int = 0, distractors: int = 0):
    all_cases = load_test_cases(category)
    cases = all_cases[:limit] if limit > 0 else all_cases
    distractor_note = f" (+{distractors} distractors)" if distractors else ""
    print(f"\n{SEP}")
    print(f"Category: {category}  |  Mode: {mode.upper()}  |  Model: {model}{distractor_note}")
    print(f"Cases: {len(cases)}")
    print(f"{SEP}\n")

    results = []
    total_tokens_in = 0
    total_tokens_out = 0
    total_latency = 0
    correct = 0

    for i, case in enumerate(cases):
        functions = _add_distractors(case.get("function", []), distractors, case.get("id", str(i)))
        questions = case.get("question", [])
        if not questions:
            continue

        user_msg = _get_user_message(questions)

        # Build messages (supports multi-turn conversations)
        is_multi_turn = category.startswith("multi_turn")
        if is_multi_turn:
            messages = _get_conversation_messages(questions)
            if not messages:
                messages = [{"role": "user", "content": user_msg}]
        else:
            messages = [{"role": "user", "content": user_msg}]

        # Apply RAG filtering if in rag mode
        tools_to_send = functions
        rag_info = {"original_count": len(functions), "selected_count": len(functions)}
        if mode == "rag" and len(functions) > top_k:
            # Use the full conversation text for embedding in multi-turn
            rag_query = " ".join(m["content"] for m in messages[-3:]) if is_multi_turn else user_msg
            selected = select_top_k(rag_query, functions, k=top_k)
            tools_to_send = selected
            rag_info["selected_count"] = len(selected)

        # Convert to OpenAI tool format
        openai_tools = []
        for f in tools_to_send:
            openai_tools.append({
                "type": "function",
                "function": {
                    "name": f.get("name", ""),
                    "description": f.get("description", ""),
                    "parameters": _fix_parameters(f.get("parameters", {"type": "object", "properties": {}})),
                },
            })

        # Call LLM
        try:
            response = call_llm(messages, openai_tools, model)
        except Exception as e:
            print(f"  [{i+1}/{len(cases)}] ERROR: {e}")
            results.append({"id": case.get("id", i), "error": str(e), "correct": False})
            continue

        # Evaluate
        eval_result = evaluate_response(case, response)

        # Accumulate
        total_tokens_in += response["input_tokens"]
        total_tokens_out += response["output_tokens"]
        total_latency += response["latency_ms"]
        if eval_result["correct"]:
            correct += 1

        # Print per-case result
        status = "PASS" if eval_result["correct"] else "FAIL"
        rid = case.get("id", f"case_{i}")
        print(f"  [{i+1}/{len(cases)}] {status} | {rid} | tools: {len(functions)}->{rag_info['selected_count']} | tokens: {response['input_tokens']}P+{response['output_tokens']}C | {response['latency_ms']}ms")

        results.append({
            "id": rid,
            "correct": eval_result["correct"],
            "name_match": eval_result["name_match"],
            "input_tokens": response["input_tokens"],
            "output_tokens": response["output_tokens"],
            "latency_ms": response["latency_ms"],
            "functions_total": len(functions),
            "functions_used": rag_info["selected_count"],
            "predicted": eval_result["predicted_names"],
            "expected": eval_result["expected_names"],
        })

    # Summary
    accuracy = correct / len(results) * 100 if results else 0
    avg_tokens_in = total_tokens_in / len(results) if results else 0
    avg_tokens_out = total_tokens_out / len(results) if results else 0
    avg_latency = total_latency / len(results) if results else 0

    summary = {
        "mode": mode,
        "category": category,
        "model": model,
        "total_cases": len(results),
        "correct": correct,
        "accuracy_pct": round(accuracy, 1),
        "total_tokens_in": total_tokens_in,
        "total_tokens_out": total_tokens_out,
        "avg_tokens_in": round(avg_tokens_in, 1),
        "avg_tokens_out": round(avg_tokens_out, 1),
        "total_latency_ms": total_latency,
        "avg_latency_ms": round(avg_latency, 1),
    }

    print(DASH)
    print(f"RESULTS - {mode.upper()} | {category} | {model}")
    print(DASH)
    print(f"  Accuracy:      {correct}/{len(results)} = {accuracy:.1f}%")
    print(f"  Total tokens:  {total_tokens_in}P + {total_tokens_out}C = {total_tokens_in + total_tokens_out}")
    print(f"  Avg tokens:    {avg_tokens_in:.0f}P + {avg_tokens_out:.0f}C per case")
    print(f"  Total latency: {total_latency}ms")
    print(f"  Avg latency:   {avg_latency:.0f}ms per case")
    print(DASH)
    print()

    return results, summary


def compare_modes(static_results: dict, rag_results: dict):
    s = static_results["summary"]
    r = rag_results["summary"]

    print(f"\n{SEP}")
    print(f"COMPARISON: STATIC vs RAG ({s['category']}, {s['model']})")
    print(SEP)

    acc_delta = r["accuracy_pct"] - s["accuracy_pct"]
    tok_delta = r["avg_tokens_in"] - s["avg_tokens_in"]

    print(f"  Accuracy:     {s['accuracy_pct']:.1f}% (static) -> {r['accuracy_pct']:.1f}% (rag)  ({acc_delta:+.1f}pp)")
    print(f"  Avg tokens:   {s['avg_tokens_in']:.0f} (static) -> {r['avg_tokens_in']:.0f} (rag)  ({tok_delta:+.0f} per case)")
    if s['avg_tokens_in'] > 0:
        print(f"  Token savings: {abs(tok_delta) / s['avg_tokens_in'] * 100:.0f}%")
    else:
        print(f"  Token savings: N/A (no static data)")
    print(f"  Avg latency:  {s['avg_latency_ms']:.0f}ms (static) -> {r['avg_latency_ms']:.0f}ms (rag)")

    if acc_delta <= -5:
        print(f"\n  !  Accuracy drop >5pp — RAG may be too aggressive for this category")
    elif acc_delta <= 0:
        print(f"\n  +  Negligible accuracy impact ({acc_delta:+.1f}pp) with significant token savings")
    else:
        print(f"\n  +  RAG improves accuracy by {acc_delta:+.1f}pp with token savings")

    print(f"{SEP}\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Volt BFCL Benchmark")
    parser.add_argument("--mode", choices=["static", "rag", "both"], default="both")
    parser.add_argument("--category", choices=list(BFCL_DATA_FILES.keys()) + ["all"], default="all")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--top-k", type=int, default=DEFAULT_TOP_K)
    parser.add_argument("--limit", type=int, default=0, help="Run only N cases per category (0 = all)")
    parser.add_argument("--distractors", type=int, default=0,
                        help="Add N distractor functions per case to simulate large tool registries")
    parser.add_argument("--api-key", help="OpenAI API key (default: $OPENAI_API_KEY)")
    parser.add_argument("--base-url", help="API base URL (default: $OPENAI_BASE_URL or https://api.openai.com/v1)")
    args = parser.parse_args()

    if args.api_key:
        os.environ["OPENAI_API_KEY"] = args.api_key
    if args.base_url:
        os.environ["OPENAI_BASE_URL"] = args.base_url
    # Fallback: try to load .env from Volt project root (ALWAYS overrides)
    env_path = Path(__file__).parent.parent / ".env"
    if env_path.exists():
        with open(env_path) as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#"):
                    if "=" in line:
                        k, v = line.split("=", 1)
                        os.environ[k.strip()] = v.strip()
    # Map to OPENAI_API_KEY
    if not os.getenv("OPENAI_API_KEY") and os.getenv("GROQ_API_KEY"):
        os.environ["OPENAI_API_KEY"] = os.environ["GROQ_API_KEY"]
    if not os.getenv("OPENAI_BASE_URL") and os.getenv("LLM_BASE_URL"):
        os.environ["OPENAI_BASE_URL"] = os.environ["LLM_BASE_URL"]

    categories = list(BFCL_DATA_FILES.keys()) if args.category == "all" else [args.category]

    for cat in categories:
        print(f"\n{'#' * 60}")
        print(f"# CATEGORY: {cat}")
        print(f"{'#' * 60}")

        static_data, rag_data = None, None

        total = len(load_test_cases(cat))
        limit = args.limit if args.limit > 0 else total
        print(f"Total cases: {total}, running: {limit}")

        if args.mode in ("static", "both"):
            print(f"\n>>> Running STATIC mode (all functions injected per case)")
            results, summary = run_benchmark("static", cat, args.model, args.top_k, limit, args.distractors)
            static_data = {"results": results, "summary": summary}

        if args.mode in ("rag", "both"):
            print(f"\n>>> Running RAG mode (top-{args.top_k} functions via embedding similarity)")
            results, summary = run_benchmark("rag", cat, args.model, args.top_k, limit, args.distractors)
            rag_data = {"results": results, "summary": summary}

        if static_data and rag_data:
            compare_modes(static_data, rag_data)

    print("Done.")
