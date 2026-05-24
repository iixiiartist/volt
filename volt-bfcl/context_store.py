"""Python ContextStore — mirrors the Rust implementation for benchmark testing.

Provides unified retrieval for all context types with enhanced ranking
(cosine similarity + success_rate + frequency + recency).

Uses Ollama mxbai-embed-large for proper dense embeddings when available,
falls back to TF-IDF only if Ollama is unreachable.
"""

import hashlib
import json
import math
import time
from typing import Any
from uuid import uuid4

# ── Ollama embedding client ─────────────────────────────────────────────────

import requests

OLLAMA_EMBED_URL = "http://localhost:11434/api/embed"
OLLAMA_MODEL = "mxbai-embed-large"

_ollama_available = None


def _check_ollama() -> bool:
    global _ollama_available
    if _ollama_available is not None:
        return _ollama_available
    try:
        r = requests.get("http://localhost:11434/api/tags", timeout=3)
        data = r.json()
        models = [m["name"] for m in data.get("models", [])]
        _ollama_available = any(OLLAMA_MODEL in m for m in models)
        if _ollama_available:
            print(f"[context/embed] Ollama {OLLAMA_MODEL} available")
        else:
            print(f"[context/embed] Ollama running but {OLLAMA_MODEL} not found, fallback to TF-IDF")
        return _ollama_available
    except Exception:
        _ollama_available = False
        print("[context/embed] Ollama not available, using TF-IDF fallback")
        return False


def _embed(texts: list[str]) -> list[list[float]]:
    """Embed a batch of texts using Ollama or TF-IDF fallback."""
    if _check_ollama():
        try:
            # Batch in chunks of 10 to avoid overwhelming Ollama
            all_embs = []
            for i in range(0, len(texts), 10):
                batch = texts[i:i + 10]
                r = requests.post(OLLAMA_EMBED_URL, json={
                    "model": OLLAMA_MODEL,
                    "input": batch,
                }, timeout=30)
                r.raise_for_status()
                all_embs.extend(r.json()["embeddings"])
            return all_embs
        except Exception as e:
            print(f"[context/embed] Ollama error: {e}, falling back to TF-IDF")
            _ollama_available = False
    return [_fallback_embed(t) for t in texts]


# ── TF-IDF fallback embedding (mirrors benchmark.py) ────────────────────────

VOCAB = {}
VOCAB_COUNTER = 0


def _fallback_embed(text: str) -> list[float]:
    global VOCAB, VOCAB_COUNTER
    words = text.lower().split()
    for w in words:
        h = hashlib.md5(w.encode()).hexdigest()[:8]
        if h not in VOCAB:
            VOCAB[h] = VOCAB_COUNTER
            VOCAB_COUNTER += 1
    dim = max(len(VOCAB), 16)
    vec = [0.0] * dim
    for w in words:
        h = hashlib.md5(w.encode()).hexdigest()[:8]
        if h in VOCAB:
            vec[VOCAB[h]] += 1.0
    norm = sum(x * x for x in vec) ** 0.5
    if norm > 0:
        vec = [x / norm for x in vec]
    return vec


def cosine_sim(a: list[float], b: list[float]) -> float:
    dot = sum(x * y for x, y in zip(a, b))
    na = sum(x * x for x in a) ** 0.5
    nb = sum(x * x for x in b) ** 0.5
    return dot / (na * nb + 1e-10)


# ── ContextEntry ────────────────────────────────────────────────────────────

class ContextEntry:
    def __init__(self, kind: str, content: str, metadata: dict | None = None):
        self.id = str(uuid4())
        self.kind = kind
        self.content = content
        self.metadata = metadata or {}
        self.embedding: list[float] | None = None
        self.frequency = 0
        self.success_rate = 0.0
        self.usage_count = 0
        self.last_used_at = time.time()
        self.created_at = time.time()

    def compute_embedding(self):
        text = f"{self.kind}: {self.content}"
        self.embedding = _embed([text])[0]

    def to_dict(self) -> dict:
        return {
            "id": self.id,
            "kind": self.kind,
            "content": self.content[:200],
            "frequency": self.frequency,
            "success_rate": self.success_rate,
            "usage_count": self.usage_count,
        }


# ── ContextStore ───────────────────────────────────────────────────────────

class ContextStore:
    def __init__(self):
        self.entries: list[ContextEntry] = []
        # Pre-check Ollama availability once
        _check_ollama()

    def add(self, kind: str, content: str, metadata: dict | None = None) -> str:
        entry = ContextEntry(kind, content, metadata)
        entry.compute_embedding()
        self.entries.append(entry)
        return entry.id

    def search(self, query: str, limit: int = 8, kind_filter: str | None = None,
               min_score: float = 0.25) -> list[ContextEntry]:
        query_emb = _embed([query])[0]
        scored = []

        for e in self.entries:
            if e.embedding is None:
                continue
            if kind_filter and e.kind != kind_filter:
                continue

            sim = cosine_sim(query_emb, e.embedding)

            days_since = (time.time() - e.last_used_at) / 86400.0
            recency = math.exp(-days_since / 30.0)
            freq = math.log(1.0 + e.frequency)
            success = e.success_rate if e.usage_count > 0 else 0.5

            score = 0.6 * sim + 0.2 * success + 0.1 * recency + 0.1 * freq

            if score >= min_score:
                scored.append((score, e))

        scored.sort(key=lambda x: -x[0])
        results = [e for _, e in scored[:limit]]

        for e in results:
            e.frequency += 1
            e.last_used_at = time.time()

        return results

    def learn(self, entry_id: str, success: bool):
        for e in self.entries:
            if e.id == entry_id:
                e.usage_count += 1
                rate = e.success_rate
                count = float(e.usage_count)
                e.success_rate = (rate * (count - 1.0) + (1.0 if success else 0.0)) / count
                return

    def record_run(self, query: str, tool_name: str, success: bool,
                   metadata: dict | None = None) -> str:
        content = f"Query: {query}\nTool used: {tool_name}\nSuccess: {success}"
        meta = {"tool_name": tool_name, "success": success, **(metadata or {})}
        eid = self.add("agent_run", content, meta)
        self.learn(eid, success)
        return eid

    def populate_from_functions(self, functions: list[dict]):
        for f in functions:
            name = f.get("name", f.get("function", {}).get("name", ""))
            desc = f.get("description", f.get("function", {}).get("description", ""))
            params = json.dumps(f.get("parameters", f.get("function", {}).get("parameters", {})))
            self.add("skill", f"{name}: {desc} {params}", {"tool_name": name})
            self.add("few_shot",
                     f"Example: when asked about '{desc}', call {name}",
                     {"tool_name": name})

    def populate_from_runs(self, runs: list[dict]):
        for r in runs:
            self.add("memory",
                     f"Previous run: {r.get('query', '')} → used {r.get('tool', '')}: {r.get('result', '')}",
                     {"tool_name": r.get("tool", ""), "success": r.get("success", True)})

    def __len__(self):
        return len(self.entries)
