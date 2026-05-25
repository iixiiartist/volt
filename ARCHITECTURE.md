# Volt Architecture Documentation

## Overview

Volt is a Rust-native AI agent framework implementing a **Unified RAG Loop** for dynamic retrieval across 12 context fields. It replaces static injection with vector similarity search, backed by a background auto-seeding worker, pgvector persistence, and four-pillar eviction.

**Verified results**: 100% accuracy at 200 distractors with argument-level validation (BFCL V4, llama-3.1-8b-instant). Flat tool-count scaling curve. 74% token savings. Full methodology in [`paper/draft.md`](paper/draft.md).

---

## Core Design Decisions

### 1. Unified RAG Loop

Every agent turn performs semantic search across all context kinds:

```
User Query + Context
    ↓
[Embed via 7-provider fallback chain]
    ↓
[Cosine similarity search across 12 context kinds]
    ↓
┌──────────┬──────────┬──────────┬──────────┐
│ Top-8    │ Top-3    │ Top-5    │ Top-3    │
│ Tools    │ Skills   │ Memories │ Conversations │
└──────────┴──────────┴──────────┴──────────┘
    ↓
[XML-tagged context injection]
    ↓
[LLM Call with tool execution + auto-seeding]
```

### 2. Unified Context Store (Everything-as-RAG)

12 context kinds, each with per-kind quota and dynamic retrieval:

| Kind | Quota | Source |
|---|---|---|
| Tool | 500 | Tool schemas (name + description + JSON Schema) |
| Skill | 200 | Compiled SKILL.md manifests from PostgreSQL |
| Conversation | 300 | SeedEvent::EpisodeComplete after each agent run |
| Memory | 500 | MEMORY.md + DB memories |
| AgentRun | 200 | Full LLM turn audit logs (EU AI Act Art. 12) |
| Artifact | 300 | Write/edit/bash execution side effects |
| SystemPrompt | 20 | SOUL.md |
| FewShot | 50 | Reserved |
| Policy | 50 | AGENTS.md |
| Permission | 50 | Per-tool allow/prompt rules |
| Security | 30 | Sandbox limits, EU AI Act Art. 14 oversight |
| MCPConfig | 100 | MCP server schema distillation |

Each entry stores: UUID, kind, content, 1024d embedding, JSON metadata, frequency counter, success rate, usage count, and timestamps.

### 3. Four-Pillar Eviction

1. **Semantic Dedup**: Cosine ≥ 0.92 on same kind → merge frequency, skip insert
2. **Per-Kind Quotas**: Evict lowest composite-score entries when kind exceeds quota
3. **Composite Score**: 0.4×recency + 0.3×success + 0.2×log(frequency) + 0.1×density
4. **Episodic Merging**: Every 10 batches, cluster Conversation entries ≥0.85 cosine with ≥3 members; replace with high-density merged entry

### 4. Background Auto-Seeding Worker

A Tokio MPSC channel architecture maintains the context store asynchronously:

```
[Agent Loop] → SeedChannel.send(SeedEvent) → [AutoSeedWorker daemon]
                                                ├─ Batch drain (≤32 events)
                                                ├─ Embed via 7-provider chain (semaphore=5)
                                                ├─ seed_batch() with dedup + eviction
                                                └─ Episodic merge (every 10 batches)
```

Three event types: `EpisodeComplete`, `ArtifactCreated`, `MCPRegistered`. Pre-warm at startup: workspace files, tool intents, permissions, security policy, skills from DB.

### 5. Multi-Provider Embedding Fallback

7-provider chain, tried in order:
1. Ollama (local, mxbai-embed-large, 1024d)
2. llama.cpp (local, OpenAI-compatible endpoint)
3. NVIDIA NIM (cloud, llama-nemotron-embed-1b-v2, 2048d→1024d)
4. OpenAI (cloud, text-embedding-3-small)
5. HuggingFace Inference API (cloud, BAAI/bge-small-en-v1.5, 384d→1024d)
6. Moonshot (cloud, moonshot-v1-embed)
7. Deterministic (SHA-256, always works, zero network)

All embeddings normalized to 1024d via `normalize_dims()` (pad shorter, truncate longer). Exponential backoff retry (1s/2s/4s) on 429 rate limits and connection errors. Text truncated to 2000 chars for Ollama's 512-token context window.

### 6. Production Hardening

| Feature | Implementation |
|---|---|
| **Tool registry** | DashMap lock-free concurrent HashMap |
| **Tool execution** | `futures::join_all` parallel execution |
| **Path safety** | RwLock-cached root with staleness check |
| **Sandbox** | env_clear() on Windows + Unix, timeout, output limits |
| **Feature flags** | All opt-in (`default = []`) |
| **Agent state** | SQLite session persistence |
| **Token counting** | tiktoken-rs cl100k_base (replaces chars/3) |
| **OpenTelemetry** | tracing→OTel bridge with OTLP export support |
| **GraphRAG** | petgraph ToolGraph with BFS traversal |
| **HNSW index** | In-memory cosine similarity for ContextStore |
| **tree-sitter** | Feature-gated AST parsing (tools-ast) |
| **candle** | Feature-gated local embeddings (tools-local-embeddings) |

### 7. Permission System

23 tools default to `PermissionLevel::Prompt`. Three modes:
- **Autonomous** (`--allow` / `-a`): no prompts, unattended execution
- **Semi-autonomous** (no `-a`): individual approvals, answer `a` for session
- **Human-in-the-loop**: default prompt gating on destructive operations

---

## Tool Registry

### Built-in Tools (38)

| Category | Tools | Feature Flag |
|---|---|---|
| **File I/O** | `read`, `write`, `edit`, `glob`, `grep` | built-in |
| **Shell** | `bash` | built-in |
| **Web** | `web_fetch`, `web_scrape`, `web_scrape_all` | built-in |
| **Data** | `json_validate`, `json_prettify`, `json_query`, `csv_read`, `csv_write` | built-in |
| **Archives** | `archive_extract`, `archive_create` | built-in |
| **Memory** | `memory_append`, `todo_add` | built-in |
| **Git** | `git_status`, `git_diff_*` (2), `git_commit`, `git_add`, `git_reset`, `git_log`, `git_*` (5) | built-in |
| **Time** | `get_current_time`, `convert_time` | built-in |
| **Reasoning** | `sequentialthinking` | built-in |
| **Charts** | `create_bar_chart`, `create_line_chart` | built-in |
| **Screenshot** | `screenshot` | tools-screenshot |
| **PDF** | `create_pdf` | tools-pdf |
| **Desktop** | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` | tools-desktop |
| **Browser** | `browser_navigate`, `browser_extract`, `browser_screenshot` | tools-browser |
| **Delegation** | `delegate`, `run_workflow` | built-in |
| **MCP** | `searchhq_*` (19 tools) | runtime registration |

---

## Database Schema

### Core Tables

```sql
-- Context entries (unified RAG persistence)
CREATE TABLE context_entries (
    id UUID PRIMARY KEY,
    kind VARCHAR(32) NOT NULL,
    content TEXT NOT NULL,
    embedding vector(1024),
    metadata JSONB,
    frequency INT DEFAULT 0,
    success_rate REAL DEFAULT 0.0,
    usage_count INT DEFAULT 0,
    last_used_at TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- HNSW indexes on all vector columns
CREATE INDEX ON context_entries USING hnsw (embedding vector_cosine_ops);
CREATE INDEX ON agent_tools USING hnsw (embedding vector_cosine_ops);
CREATE INDEX ON skills USING hnsw (embedding vector_cosine_ops);
CREATE INDEX ON memories USING hnsw (embedding vector_cosine_ops);
```

---

## Agent Loop

```rust
async fn run(&self, input: &str) -> Result<String> {
    // 1. Push user message + sanitize
    // 2. Build context (embed last 3 msgs + input)
    // 3. Retrieve 12-kind unified context via ContextStore
    // 4. Retrieve skills (3) and memories (5) via pgvector
    // 5. Search tools via RAG (top-8 + 4 essential)
    // 6. Compress context if over token budget
    // 7. Build LLM request with XML-tagged context
    // 8. Call LLM with 3-retry exponential backoff
    // 9. Execute tool calls (parallel join_all, permission-gated)
    // 10. Emit SeedEvent for background worker
    // 11. Store memory to pgvector
    // 12. Persist session to SQLite
}
```

---

## Benchmarks

### BFCL V4 (End-to-End Volt Binary)

| Model | Distractors | Accuracy | Evaluation |
|---|---|---|---|
| llama-3.1-8b-instant | 200 | **100%** | Argument-aware |
| llama-3.3-70b-versatile | 200 | 90% | Argument-aware |

### Tool-Count Scaling Ablation

| Distractors | Accuracy | Avg Latency |
|---|---|---|
| 0 | 100% | 30.8s |
| 10 | 100% | 33.2s |
| 50 | 100% | 38.6s |
| 100 | 100% | 42.7s |
| 200 | 100% | 54.0s |

**Flat curve.** Accuracy invariant from 0 to 200+ distractors.

### Python Raw-API (470 Cases)

74% token savings, +4.8pp accuracy. Full table in [`paper/draft.md`](paper/draft.md).

---

## Performance

| Metric | Value |
|---|---|
| Binary size | Linux glibc ~10 MB, Linux musl ~8 MB, Windows ~20 MB (compressed) |
| Cold start | <100ms |
| Tool search | <1ms (in-memory) |
| Memory search | <5ms (pgvector HNSW) |
| Source lines | ~13,000 (57 files) |
| Tests | 66 (54 unit + 4 agent + 3 workflow + 5 integration) |

---

*Built in Rust by [Setique Labs, Inc.](https://setique.com)*
