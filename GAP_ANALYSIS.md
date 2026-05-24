# Volt Architecture — Gap Analysis & Recommendations

## 1. Critical Gaps

### 1.1 ContextStore is In-Memory Only (No Persistence)
The entire unified context store (12 kinds, dedup, eviction, episodic merge) lives in `RwLock<Vec<StoredEntry>>`. On restart, all seeded context is lost — workspace files, tool intents, permissions, security policies, and episodic memories must be re-seeded from scratch.

**Recommendation**: Back ContextStore with the same pgvector table used for `memories`. Add a `context_entries` table with HNSW index. The worker daemon seeds to both in-memory store (fast search) and pgvector (persistence). On startup, hydrate the in-memory store from pgvector with a `SELECT ... ORDER BY created_at DESC LIMIT N`.

**Crate needed**: No new crate — sqlx + pgvector already in Cargo.toml.

---

### 1.2 O(n) Brute-Force Search in ContextStore
`search()` loops over all entries computing cosine similarity. At 5,000 entries this is sub-millisecond but at 500,000 entries it becomes a bottleneck. The `memories` and `skills` tables already use pgvector HNSW via `<=>` operator — the in-memory store should use the same approach.

**Recommendation**: Add an `IndexMap` or in-memory HNSW index. Alternative: use `usearch` crate (Rust bindings to USearch, a lighter single-file vector index, <2MB compile cost). Or simply move ContextStore search to pgvector entirely and keep in-memory only as a write-through cache.

**Crate**: `usearch = "2"` — single-header C++ library with Rust bindings. Or `hnsw_rs` — pure Rust HNSW implementation.

---

### 1.3 Embedding Dimension Mismatch (1024d vs 384d)
The migration schema defines `vector(1024)` on all tables. The deterministic placeholder produces 1024 dimensions. But HuggingFace `BAAI/bge-small-en-v1.5` produces 384 dimensions. This mismatch means HF embeddings can't be stored in pgvector (dimension mismatch error), and hybrid search across differently-dimensioned vectors is mathematically undefined.

**Recommendation**: Normalize to 384d as the canonical dimension. Update migrations to use `vector(384)`. Update `deterministic_placeholder_embedding()` to produce 384d vectors. This matches the BGE-small model used in benchmarks (the one that delivered +12pp). For backward compat, 1024d providers (NVIDIA NIM, OpenAI) can be truncated to 384d or padded.

**Code change**: `const EMBEDDING_DIMENSIONS: usize = 384;` in `embedding.rs`. Update `migrations/0001_core.sql` and `0002_skills.sql`.

---

### 1.4 No Local Embedding Model for Air-Gapped Deployments
The fallback chain relies entirely on cloud APIs (Ollama counts as local but requires a separate service). For enterprises that can't call external APIs (Capital One, DoD, EU-regulated banks), the only option is the deterministic SHA-256 placeholder — which drops accuracy by ~12pp.

**Recommendation**: Add `candle` (HuggingFace's Rust ML framework) as an optional feature flag. With `candle-transformers`, Volt can run `BAAI/bge-small-en-v1.5` locally with ~130MB model download and sub-10ms inference on CPU. This gives air-gapped deployments the same +12pp accuracy as the HF cloud API.

**Crate**: `candle-core = "0.10"`, `candle-transformers = "0.10"`, `candle-nn = "0.10"`, `hf-hub = "0.4"`, `tokenizers = "0.21"`. Feature-flag gated behind `tools-local-embeddings`.

---

### 1.5 Token Counting Is Heuristic (chars/3)
`ModelContext::estimate_tokens(text)` uses `text.len() / 3` which is a rough approximation. For Claude, Groq, and OpenAI models with different tokenizers, the error margin can be 20-30%. This causes inaccurate context compression and can result in the LLM rejecting requests that exceed its actual token limit.

**Recommendation**: Add `tiktoken-rs` for accurate token counting on OpenAI-compatible models. Use `tokenizers` (HF crate, already a dependency of `candle`) for non-OpenAI models. Add a tokenizer selection table: Groq/Llama → cl100k_base, Anthropic/Claude → separate counter (Anthropic doesn't use tiktoken but cl100k is close enough), local models → model-specific.

**Crate**: `tiktoken-rs = "0.11"` — 391 GitHub stars, MIT licensed, supports all OpenAI models including o200k_base for GPT-5.

---

### 1.6 AgentChat and AgentTui Don't Wire ContextStore + Worker
Only `AgentRun` creates the ContextStore, SeedChannel, and AutoSeedWorker. `AgentChat` and `AgentTui` don't have RAG-based tool selection or context enrichment. They call `register_all_tools()` without computing embeddings, and don't benefit from the auto-seeding pipeline.

**Recommendation**: Extract the ContextStore + SeedChannel + Worker wiring into a shared `setup_agent_context()` function called by all three commands. This is a pure refactor — no new crates needed.

---

### 1.7 No HF API Retry on Rate Limit
The embedding client tries each provider once and moves on. If the HuggingFace API returns 429 (rate limit), it falls through to the next provider. With the current fallback chain, a single 429 drops embedding quality from 384d dense to SHA-256 deterministic.

**Recommendation**: Add exponential backoff retry (3 attempts, 1-4-16 second delays) specifically for 429 responses in the embedding client. The MPSC worker already batches and rate-limits via the semaphore — adding per-request retries makes the pipeline resilient to transient rate limits.

---

## 2. High-Impact Enhancements

### 2.1 GraphRAG for Cross-Domain Context
Current RAG retrieves by vector similarity alone. For enterprise deployments with cross-domain tooling (e.g., CRM + billing + support), a vector match on "customer" might return the wrong system's tools. GraphRAG (Microsoft, 2024) augments vector search with a knowledge graph of entity relationships.

**Recommendation**: Store tool-to-tool, tool-to-skill, and MCP-to-tool relationships in the existing `asset_relationships` table. Before injecting tool results, traverse the relationship graph to include prerequisite tools (e.g., `get_customer` → `create_invoice`). Use `pathfinding` crate for graph traversal. This is a lightweight complement to vector search, not a replacement.

**Crate**: `petgraph = "0.7"` or `pathfinding = "4"`. Lightweight, pure Rust, no external deps.

---

### 2.2 Automatic Tool Extraction from Source Code (AST Parsing)
The paper mentions "Automated Codebase Documenting (Passive Sync)" but no implementation exists. When `bash`, `write`, or `edit` tools create/modify code files, the worker should parse them to extract function signatures, class definitions, and dependency graphs.

**Recommendation**: Add `tree-sitter` with language-specific grammars (Rust, Python, TypeScript, Go). After a tool execution modifies a file, the worker parses the file with tree-sitter and creates an `Artifact` entry with extracted symbols. Feature-flag gated since tree-sitter grammars add ~2-5MB each.

**Crate**: `tree-sitter = "0.24"`, `tree-sitter-rust = "0.23"`, `tree-sitter-python = "0.23"`, `tree-sitter-typescript = "0.23"`. MIT licensed.

---

### 2.3 Observability / OpenTelemetry
`tracing` is set up but only used with `info!()` macros in the worker and agent loop. There's no structured logging, no metrics, no distributed tracing. For enterprise deployments, operators need latency histograms, error rates, and token usage dashboards.

**Recommendation**: Add `opentelemetry` + `opentelemetry-otlp` behind a feature flag. Instrument: tool execution latency (histogram), LLM call latency, embedding call latency, token usage (counter), context store size (gauge), eviction events (counter). Export to OTLP-compatible backends (Jaeger, Grafana, Datadog).

**Crate**: `opentelemetry = "0.27"`, `opentelemetry_sdk = "0.27"`, `opentelemetry-otlp = "0.27"`, `tracing-opentelemetry = "0.28"`. All Apache 2.0 / MIT.

---

### 2.4 Streaming Tool Execution (Parallel Tool Calls)
The agent loop executes tool calls sequentially in a `for` loop. When the LLM returns multiple tool calls (e.g., `read_file` + `glob` + `grep`), Volt should execute them concurrently via `tokio::join!`. This is especially impactful for the 70B model which makes multiple independent tool calls.

**Recommendation**: Replace the sequential `for tc in tool_calls` loop with `futures::future::join_all(tool_calls.iter().map(|tc| self.tools.execute(...)))`. Track per-tool results and push messages in order. This is a pure refactor — `tokio` and `futures` are already in Cargo.toml.

---

### 2.5 Agent State Persistence Across Restarts
`AgentState` is entirely in-memory. If the machine reboots mid-workflow, all agent progress is lost. Session messages are saved to SQLite but only in AgentChat/TUI mode — not in AgentRun.

**Recommendation**: Save `AgentState` (messages, iteration, token counts) to the SQLite sessions DB after each turn. On agent startup, check for an interrupted session and offer resume. Use the same `volt_sessions.db` with a `CHECKPOINT` pragma for crash safety.

---

### 2.6 Migration Schema Drift
`0001_core.sql` and `0002_skills.sql` both create a `skills` table with different column types (JSONB vs TEXT). Running both migrations sequentially fails. This is a known issue — the repo has two competing skill table definitions.

**Recommendation**: Consolidate to a single migration. Drop `0002_skills.sql` and merge its `skill_tools` table into `0001_core.sql`. Add a `DROP TABLE IF EXISTS skills CASCADE` guard in 0001. This is a one-time schema fix with no code changes needed.

---

## 3. Paper-Specific Gaps

### 3.1 BFCL Evaluator Is Name-Only
The current evaluator (`evaluate_case` in `volt_bench.py`) checks only whether the correct tool name was called — not whether the arguments are correct. The full BFCL evaluator checks argument types, values, and edge cases. This is a known limitation stated in the paper ("name-only eval") but undermines the claim of 90% accuracy.

**Recommendation**: Port the BFCL evaluation logic from the Python BFCL repo to `volt_bench.py`. The BFCL evaluator checks:
- Was the correct function called?
- Are all required parameters present?
- Do the parameter values match expected types?
- Are the values within expected ranges (for numeric params)?

This would likely drop reported accuracy by 5-10pp but provides a more honest assessment.

---

### 3.2 No Multi-Turn Benchmark
The paper acknowledges single-turn limitation but no multi-turn tests exist. GAIA and Tau-Bench are listed in `benchmarks.md` as planned but not implemented. Multi-turn agent behavior is where context enrichment matters most.

**Recommendation**: Implement the GAIA benchmark adapter (already scaffolded in `gaia_bench.rs`) and Tau-Bench. GAIA tests multi-step web search + reasoning — exactly where episodic memory from `SeedEvent::EpisodeComplete` would surface relevant past strategies.

---

### 3.3 Missing Ablation Studies
The paper claims 5 contributions but only ablation-tests one (embedding quality). The other 4 (RAG vs static, model substitution, multi-agent, Rust performance) lack controlled experiments. A strong paper needs per-contribution ablation.

**Recommended experiments**:
- **Tool count scaling**: Run at 0, 50, 100, 200, 500, 1000 distractors. Plot accuracy vs registry size.
- **RAG top-K sweep**: Run at top-1, 3, 5, 8, 12, 20. Find optimal retrieval depth.
- **Context kind ablation**: Run with Tool-only, Tool+Skill, Tool+Skill+Conversation, etc. Measure marginal contribution of each context kind.
- **Latency breakdown**: Profile tool search (μs), embedding (ms), LLM call (s), context injection (μs).

---

## 4. Production Hardening

### 4.1 Tool Registry Thread Contention
`ToolRegistry` uses a single `RwLock<HashMap>` — all tool lookups, searches, and executions contend on one lock. At scale with concurrent agents, this becomes a bottleneck.

**Recommendation**: Use `dashmap` for lock-free concurrent HashMap. `DashMap` provides per-shard locking with near-linear scaling. Embeds remain in the map but searches still need to scan all entries — add a separate `RwLock<Vec<EmbeddedTool>>` for search-only access.

**Crate**: `dashmap = "6"` — 3,000+ GitHub stars, Apache/MIT, no unsafe usage in public API.

---

### 4.2 Global Path Root Staleness
`path.rs` uses `OnceLock` for project root detection. If the CWD changes between agent runs (e.g., in a multi-project setup), the cached root is stale and path traversal checks fail or pass incorrectly.

**Recommendation**: Replace `OnceLock` with per-request resolution. Cache the root for the duration of one agent `run()` call but re-resolve on each new command.

---

### 4.3 Sandbox Is OS-Aware But Fragile
`sandbox.rs` detects Windows vs Unix but the shell spawn may inherit environment variables. On Unix, it clears PATH and HOME. On Windows, it doesn't — the entire user environment leaks into sandboxed execution.

**Recommendation**: Clear all environment variables on both platforms (except explicitly allowlisted ones). Use `std::process::Command::env_clear()` followed by selective `env()` calls. This matches the paper's claim of "sandboxed execution with no network or filesystem access beyond working directory."

---

## 5. Dependency & Binary Size

### 5.1 Heavy Feature Gates
All four feature flags (tools-screenshot, tools-pdf, tools-desktop, tools-browser) are default-enabled. This means every `cargo build` compiles windows-capture, image, lopdf, enigo, and headless_chrome — even on Linux CI where they're useless.

**Recommendation**: Make feature flags opt-in, not default. Add OS-specific default features in Cargo.toml:
```toml
[target.'cfg(windows)'.dependencies]
windows-capture = { version = "2", optional = true }

[target.'cfg(not(windows))'.dependencies]
# Skip Windows-only crates
```

### 5.2 Binary Size Growth
Current release binary is ~18MB. Adding candle (+15MB for model weights + libtorch), tree-sitter grammars (+2-5MB each), and tiktoken-rs (+2MB for tokenizer data) could push past 50MB. This matters for edge deployments and CI caching.

**Recommendation**: Use Cargo features aggressively. Core Volt binary stays <20MB. Add `volt-full` meta-feature for all extras. Use `upx` compression in CI release builds (typically 40-50% reduction).

---

## 6. Priority Implementation Order

### Week 1: Critical Fixes (arXiv-ready)
1. Fix embedding dimension mismatch (1024→384) — 1 file change
2. Fix HF API retry on 429 — 10 lines in embedding.rs
3. AgentChat/TUI wire ContextStore + Worker — ~30 lines in main.rs
4. Parallel tool execution — 5 lines in loop_rs.rs
5. Consolidate migrations — merge 0002 into 0001

### Week 2: Accuracy Hardening
6. Accurate token counting (tiktoken-rs) — 1 new dep
7. BFCL full evaluator (argument checking) — volt_bench.py
8. Ablation studies: top-K sweep, context kind ablation, tool count scaling
9. Multi-turn benchmarks (GAIA, Tau-Bench)

### Week 3: Production Readiness
10. ContextStore pgvector persistence — new migration + 50 lines
11. OpenTelemetry instrumentation — feature-gated
12. DashMap for ToolRegistry — drop-in replacement
13. Streaming tool execution — `join_all` refactor

### Week 4: Advanced Features
14. Local embeddings via candle (air-gapped mode) — feature-gated
15. GraphRAG relationship traversal — petgraph + asset_relationships
16. AST-based artifact extraction (tree-sitter) — feature-gated
17. Agent state persistence — session.rs extension

---

## 7. Summary Matrix

| Gap | Severity | Effort | Crate Required | Paper Impact |
|---|---|---|---|---|
| Embedding dimension mismatch | **Critical** | Small | — | High (bug) |
| ContextStore no persistence | **Critical** | Medium | — | Medium |
| AgentChat/TUI no RAG | **High** | Small | — | Medium |
| No HF API 429 retry | **High** | Small | — | Low |
| Migration schema drift | **High** | Small | — | Low |
| Token counting heuristic | Medium | Small | `tiktoken-rs` | Medium |
| O(n) brute-force search | Medium | Medium | `usearch` or `hnsw_rs` | Low |
| No local embedding model | Medium | Large | `candle-core` | High |
| Sequential tool execution | Medium | Small | — | Medium |
| Agent state not persisted | Medium | Medium | — | Low |
| BFCL name-only eval | Medium | Medium | — | **High** |
| No multi-turn benchmarks | Medium | Large | — | **High** |
| Missing ablation studies | Low | Large | — | **High** |
| Observability gaps | Low | Medium | `opentelemetry` | Low |
| Tool registry contention | Low | Small | `dashmap` | Low |
| Global path root staleness | Low | Small | — | Low |
| Sandbox env leak on Windows | Low | Small | — | Low |
| Heavy default features | Low | Small | — | Low |
| GraphRAG relationships | Low | Large | `petgraph` | Medium |
| AST artifact extraction | Low | Large | `tree-sitter` | Medium |
