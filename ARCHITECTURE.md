# Volt Architecture

## Overview

Volt is a Rust-native AI agent middleware with a **3-kind context store**, **~20 curated tools**, **provider-agnostic routing**, and **DAG-based multi-agent orchestration**. It replaces the common pattern of hardcoded provider defaults + 50+ tool injection with a focused, auto-detecting, self-hosted design.

**Key differentiators from standard agent frameworks:**
- No hardcoded provider or model defaults — auto-detects whatever you have configured
- 3 context kinds (not 12) — Tool schemas, Memory, Conversation — the three signals for tool selection
- ~20 tools (not 50+) — deleted `final_answer`, `sequentialthinking`, `get_current_time`, 9 other files
- 12 git tools collapsed to 2 (`git_query` + `git_mutate` taking raw subcommand strings)
- `web_scrape` merged into `web_fetch` with optional `selector` param
- Keyword table routing (~100µs) replaces LLM-based blueprint selection
- Supervisor synthesizer opt-in (default: direct concatenation, saves 1+N+1 LLM cascade)
- Append-only PostgreSQL audit log (EU AI Act Art. 12)

---

## Core Design Decisions

### 1. 3-Kind Context Store (was 12-kind)

| Kind | Quota | Source |
|---|---|---|
| Tool | 500 | All registered tool schemas (name + description + JSON schema) |
| Memory | 500 | MEMORY.md workspace file + DB memories |
| Conversation | 300 | `SeedEvent::EpisodeComplete` after each agent run |

Remaining 9 kinds (Skill, AgentRun, Artifact, SystemPrompt, FewShot, Policy, Permission, Security, MCPConfig) are still seedable and queryable via explicit `store.search_by_kind()` but excluded from the default context window to reduce noise.

### 2. ~20 Active Tools (dynamically gated)

| Category | Tools | Gate |
|---|---|---|
| **Core** | `read`, `write`, `edit`, `bash`, `glob`, `grep`, `sleep_until` | Always (bash hidden in VOLT_BFCL_MODE) |
| **Web** | `web_fetch` (with `selector`), `web_search` | YOUCOM_API_KEY for search |
| **Data** | `csv_read`, `csv_write`, `archive_extract`, `archive_create`, `create_bar_chart`, `create_line_chart`, `create_pdf` | Charts/PDF hidden in VOLT_MINIMAL_TOOLS |
| **Git** | `git_query`, `git_mutate` (raw subcommand strings) | Always |
| **Orchestration** | `delegate`, `run_workflow` | Always |
| **Desktop** | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` | tools-desktop feature |
| **Browser** | `browser_navigate`, `browser_extract`, `browser_screenshot` | tools-browser feature |
| **NVIDIA Cloud** | `nvidia_list_functions`, `nvidia_call_function`, `nvidia_deploy_function` | NVIDIA_API_KEY |
| **Ollama Web** | `ollama_web_search`, `ollama_web_fetch` | OLLAMA_API_KEY |
| **CLI Gateway** | `cli_exec`, `cli_query` | VOLT_ENABLE_CLI_TOOLS=1 |
| **Local LLM** | `litertlm`, `llamacpp`, `mtp` | VOLT_ENABLE_LOCAL_LLM_TOOLS=1 |

### 3. Auto-Detecting Provider Router

`ProviderDetector` (`src/llm/provider_detector.rs`) checks at startup:
- Environment variables (`GROQ_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OLLAMA_HOST`, `NVIDIA_API_KEY`)
- Running local servers (Ollama, llama.cpp, LiteRT-LM)
- Custom base URLs (`LLM_BASE_URL`)

Returns `Result<ProviderRoute, ResolveError>` with descriptive error variants — no silent fallback to unconfigured providers. Vendor-prefixed model names (e.g., `openai/gpt-oss-20b`, `qwen/qwen3-32b`) are routed to the correct provider automatically.

### 4. Keyword Table Routing (was LLM Blueprint Selector)

Agent blueprint selection uses substring matching against a keyword table instead of an LLM call. Keywords are defined per blueprint in `keywords: Vec<String>` on `AgentBlueprint`. First match wins; fallback matches against blueprint name/description words.

### 5. Four-Pillar Eviction

1. **Semantic Dedup**: Cosine ≥ 0.92 on same kind → merge frequency, skip insert
2. **Per-Kind Quotas**: Evict lowest composite-score entries when kind exceeds quota
3. **Composite Score**: 0.4×recency + 0.3×success + 0.2×frequency + 0.1×density
4. **Episodic Merging**: Cluster Conversation entries ≥0.85 cosine with ≥3 members; replace with high-density merged entry

### 6. Background Auto-Seeding Worker

MPSC channel architecture (`src/worker.rs`):
- `SeedChannel` — clone-able sender; agent loop emits events without blocking
- `AutoSeedWorker` — `tokio::spawn` daemon drains batches (≤32), embeds (semaphore=5), seeds with dedup + eviction
- Episodic merger runs every 10 batches
- Pre-warms at startup from workspace files (SOUL.md, MEMORY.md, AGENTS.md), tool intents, permissions, security policy
- **LeakDetector** scanning before creating ContextEntry — files with detected leaks are skipped and logged
- **Workspace gate**: accepts `Option<PathBuf>` — workspace seeding skipped when path doesn't exist

### 7. Embedding Pipeline

- Dimension configurable via `EMBEDDING_DIMENSION` env var (default: 1024)
- Provider chain: local ONNX (ort) → configured remote providers
- Deterministic SHA-256 placeholder when no provider is available
- Placeholder/broken embeddings (all-zeros, NaN) filtered at application layer in `compute_embeddings()`

### 8. Agent Loop

```
1. Push user message + sanitize (LeakDetector)
2. Build context: single unified store.search() across 3 kinds
3. Retrieve tools from ToolRegistry with hybrid RRF (BM25 + dense cosine)
4. Compress context if over token budget (80% of model max)
5. Build system prompt with time injection + tool descriptions
6. Call LLM → parse tool calls with schema validation
7. Execute tool calls (parallel join_all, permission-gated via capability manager)
8. Emit SeedEvent for background worker
9. Store conversation to session DB
10. Fallback: last assistant message on max_iterations exhaustion
```

### 9. Database Schema

```sql
-- Context entries (3-kind unified persistence)
CREATE TABLE context_entries (
    id UUID PRIMARY KEY,
    kind VARCHAR(32) NOT NULL,
    content TEXT NOT NULL,
    embedding vector($EMBEDDING_DIMENSION),
    metadata JSONB NOT NULL DEFAULT '{}',
    frequency INT NOT NULL DEFAULT 0,
    success_rate REAL NOT NULL DEFAULT 0.0,
    usage_count INT NOT NULL DEFAULT 0,
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Per-kind HNSW partial indexes (lowercase kind values)
CREATE INDEX ON context_entries USING hnsw (embedding vector_cosine_ops) WHERE kind = 'tool';
CREATE INDEX ON context_entries USING hnsw (embedding vector_cosine_ops) WHERE kind = 'memory';
CREATE INDEX ON context_entries USING hnsw (embedding vector_cosine_ops) WHERE kind = 'conversation';

-- Append-only audit log (EU AI Act Art. 12)
CREATE TABLE audit_log (
    id UUID PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL DEFAULT '',
    result TEXT NOT NULL DEFAULT 'ok',
    detail JSONB NOT NULL DEFAULT '{}',
    session_id UUID
);
```

---

## Files (Key)

### Deleted
- `src/tools/final_answer.rs`, `json_tool.rs`, `memory_tool.rs`, `scrape_tool.rs`, `sequential_thinking.rs`, `time_tool.rs`, `todo_tool.rs`
- `src/tools/groups/memory.rs`, `src/tools/groups/time_sequential.rs`

### Created
- `src/tools/git_tool.rs` (rewritten with git_query/git_mutate)
- `src/tools/time_utils.rs` (sleep_until)
- `src/llm/provider_detector.rs` (ProviderDetector)
- `src/commands/config.rs` (volt config CLI)
- `migrations/0004_audit_log.sql`
- `src/metrics.rs` (Prometheus endpoint)

---

## Tests

283 lib tests pass (`cargo test --lib --features testutils`). 24 professional workflow tests, 11 real-world benchmarks, 1 program benchmark. Coverage includes: DAG orchestration, RRF hybrid retrieval, MCP agent-to-agent, prompt compression, tool validation, provider detection, keyword routing, schema migration.

---

## Benchmarks

| Configuration | Accuracy | Notes |
|---|---|---|
| llama-3.1-8b-instant, BFCL v4 simple_python (400 cases) | 95.0% | 20 Groq API schema errors, not bad tool selection |
| qwen3-32b, BFCL v3 | 75.7% | #2 globally behind GLM 4.5 |
| Dense tools only, 200 distractors | 86% | +12pp over TF-IDF baseline |
| Token savings vs static injection (470 cases) | 74% | ~$0.37 total |

---

*Built in Rust by [Setique Labs, Inc.](https://setique.com)*
