"""Python ContextStore — mirrors the Rust implementation for benchmark testing.

Provides unified retrieval for all context types with enhanced ranking
(cosine similarity + success_rate + frequency + recency).
"""

import hashlib
import json
import math
import time
from datetime import datetime, timezone
from typing import Any
from uuid import uuid4

# ── Context kinds ──────────────────────────────────────────────────────────

CONTEXT_KINDS = {
    "skill", "memory", "agent_run", "artifact",
    "system_prompt", "few_shot", "policy",
}

# ── Embedding helpers (mirrors benchmark.py) ────────────────────────────────

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
        self.embedding = _fallback_embed(text)

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

    def add(self, kind: str, content: str, metadata: dict | None = None) -> str:
        entry = ContextEntry(kind, content, metadata)
        entry.compute_embedding()
        self.entries.append(entry)
        return entry.id

    def search(self, query: str, limit: int = 8, kind_filter: str | None = None,
               min_score: float = 0.25) -> list[ContextEntry]:
        query_emb = _fallback_embed(query)
        scored = []

        for e in self.entries:
            if e.embedding is None:
                continue
            if kind_filter and e.kind != kind_filter:
                continue

            sim = cosine_sim(query_emb, e.embedding)

            # Recency: exp(-days_since_last_use / 30)
            days_since = (time.time() - e.last_used_at) / 86400.0
            recency = math.exp(-days_since / 30.0)

            # Frequency: log(1 + count)
            freq = math.log(1.0 + e.frequency)

            # Success rate: default 0.5 if no data
            success = e.success_rate if e.usage_count > 0 else 0.5

            # Weighted score: same formula as Rust ContextStore
            score = 0.6 * sim + 0.2 * success + 0.1 * recency + 0.1 * freq

            if score >= min_score:
                scored.append((score, e))

        scored.sort(key=lambda x: -x[0])
        results = [e for _, e in scored[:limit]]

        # Update frequency/recency on retrieval
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
        """Seed the store with tool definitions as skill + few_shot entries."""
        for f in functions:
            name = f.get("name", f.get("function", {}).get("name", ""))
            desc = f.get("description", f.get("function", {}).get("description", ""))
            params = json.dumps(f.get("parameters", f.get("function", {}).get("parameters", {})))
            self.add("skill", f"{name}: {desc} {params}", {"tool_name": name})
            self.add("few_shot",
                     f"Example: when asked about '{desc}', call {name}",
                     {"tool_name": name})

    def populate_from_runs(self, runs: list[dict]):
        """Seed historical runs as memories."""
        for r in runs:
            self.add("memory",
                     f"Previous run: {r.get('query', '')} → used {r.get('tool', '')}: {r.get('result', '')}",
                     {"tool_name": r.get("tool", ""), "success": r.get("success", True)})

    def enrich_tools(self, query: str, tool_defs: list[dict],
                     top_k: int = 5) -> tuple[list[dict], str]:
        """Enrich tool definitions with retrieved context. Returns (tools, context_text)."""
        retrieved = self.search(query, limit=top_k)
        context_parts = []
        for e in retrieved:
            if e.kind == "skill":
                # Skills are already embedded in tool defs; just log
                pass
            elif e.kind == "memory":
                context_parts.append(f"[Past Experience]\n{e.content}")
            elif e.kind == "agent_run":
                context_parts.append(f"[Previous Run]\n{e.content}")
            elif e.kind == "few_shot":
                context_parts.append(f"[Example]\n{e.content}")

        context_text = "\n---\n".join(context_parts) if context_parts else ""

        # Record this retrieval as a run
        for td in tool_defs[:1]:  # log for first tool only to avoid spam
            self.record_run(query, td.get("name", "unknown"), True)

        return tool_defs, context_text

    def __len__(self):
        return len(self.entries)
