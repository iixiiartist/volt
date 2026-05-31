# Refactor Audit — Volt Codebase

Generated from structural analysis of all 17 candidate files ≥300 lines (minus exempt: `mod.rs`, `models.rs`, `registration.rs`, `test_utils.rs`, `bfcl_bench.rs`, `.sql`, `.proto`).

## Summary

| File | Lines | Core Lines | Concerns | Priority | Split Target |
|------|-------|------------|----------|----------|-------------|
| `src/agent/loop_rs.rs` | 1461 | 1461 | agent builder, run loop, prompt compression, session persistence, checkpoint, failure tracking, capability, event bus, streaming | **High** | `agent/builder.rs`, `agent/run.rs`, `agent/compression.rs` |
| `src/context.rs` | 971 | 869 | search, eviction, staging buffer, episodic clustering, turbovec, pgvector | **High** | `context/search.rs`, `context/eviction.rs`, `context/persistence.rs`, `context/clustering.rs` |
| `src/db.rs` | 735 | 735 | pool, tools CRUD, memory CRUD, executions, skills, context entries, routines | **High** | `db/tools.rs`, `db/memory.rs`, `db/executions.rs`, `db/context.rs`, `db/skills.rs`, `db/routines.rs` |
| `src/embedding.rs` | 679 | 586 | 6+ provider configs, fallback chain, local ONNX, HF API, deterministic fallback | **High** | `embedding/providers.rs`, `embedding/local.rs`, `embedding/hf.rs`, `embedding/fallback.rs` |
| `src/orchestrator.rs` | 842 | 842 | DAG workflow, parallel exec, pipeline, supervisor, single-agent | **Medium** | `orchestrator/dag.rs`, `orchestrator/runner.rs` |
| `src/capability.rs` | 540 | 491 | token issue/verify, rate budget, execution guards, token list/consume | **Medium** | `capability/token.rs`, `capability/budget.rs` |
| `src/worker.rs` | 533 | 533 | MPSC channel, seeding, clustering, pre-warm | **Medium** | `worker/channel.rs`, `worker/seeder.rs` |
| `src/session.rs` | 523 | 447 | session-message-checkpoint CRUD, circuit breaker | **Medium** | `session/storage.rs`, `session/messages.rs` |
| `src/config.rs` | 557 | 557 | env loading, project file config parsing | **Low-Medium** | `config/env.rs`, `config/project.rs` |
| `src/tools/registry.rs` | 408 | 408 | tool registration, execution dispatch, hybrid search | **Medium** | `registry/search.rs` |
| `src/vector_index.rs` | 351 | 330 | BM25+, LSH index, RRF fusion | **Low-Medium** | `vector/bm25.rs`, `vector/lsh.rs`, `vector/rrf.rs` |
| `src/agent/tool_parser.rs` | 431 | 262 | JSON Schema validation, tag formatting | **Low** | — |
| `src/main.rs` | 429 | 429 | CLI subcommand dispatch | **Low** | — |
| `src/turbovec_index.rs` | 332 | 235 | TurboQuantIndex wrapper | **Low** | — |
| `src/tools/you_tools.rs` | 324 | 324 | 3 you.com API wrappers (same domain) | **Low** | — |
| `src/skills/importer.rs` | 324 | 194 | skill format conversion | **Low** | — |
| `src/tui.rs` | 308 | 308 | terminal UI: input, render, signals | **Low** | — |

## High Priority — Detailed Refactor Plans

### 1. `src/agent/loop_rs.rs` (1461 lines)

**Current structure:** Single `impl Agent` spanning two blocks. Builder pattern (`with_*` methods, ~200 lines) followed by `run()` (~800+ lines containing tool dispatch, LLM calls, context compression, session persistence, checkpoint journaling, failure tracking, capability verification, event bus broadcasting, and streaming callback orchestration).

**Proposed split into 4 files:**

| New File | What goes there | Est. Lines |
|----------|----------------|------------|
| `agent/builder.rs` | `Agent` struct fields, all `with_*()` methods, `config()`/`state()` accessors | 200 |
| `agent/run.rs` | `run()` entry point — loop orchestration (tool dispatch ↔ LLM, iterations tracking) | 500 |
| `agent/compression.rs` | `compress_if_needed()` — prompt compression strategy + token counting | 150 |
| `agent/mod.rs` | Re-exports; `#[cfg(test)]` integration test references | 20 |

**Seam signals:**
- `run()` imports 15+ dependencies (llm, context, tools, session, checkpoint_journal, tool_failure_tracker, capability, events, compression)
- `compress_if_needed()` has no dependency on `Agent` struct — pure function
- Builder methods only mutate `self` fields — no runtime dependency

### 2. `src/context.rs` (971 → 869 core lines)

**Current structure:** `ContextStore` struct + huge `impl` block with search (`search()`), eviction (`evict()`, `enforce_quotas()`), staging buffer (`seed_batch()`), clustering (`find_clusters()`, `merge_episodic_cluster()`), DB sync (`set_db()`, `hydrate_from_db()`), turbovec integration (`with_turbovec()`), and utility methods.

**Proposed split into `context/` module:**

| New File | What goes there | Est. Lines |
|----------|----------------|------------|
| `context/mod.rs` | `ContextStore` struct, `ContextKind`, `ContextEntry`, `StoredEntry`, `new()`, `new_with_db()`, `set_db()`, `set_quotas()`, `set_evict_every()` | 200 |
| `context/search.rs` | `search()`, `compute_embeddings()`, `cosine_similarity` integration, kind-ablation filtering | 200 |
| `context/eviction.rs` | `composite_score()`, `enforce_quotas()`, semantic dedup, `append_entries()` | 150 |
| `context/persistence.rs` | `hydrate_from_db()`, `record_run()`, `learn()`, `seed_truncated_history_persistent()` | 150 |
| `context/clustering.rs` | `find_clusters()`, `merge_episodic_cluster()`, `remove_indices()` | 100 |

**Seam signals:**
- `search.rs` can depend on `mod.rs` types + external `EmbeddingClient` — no DB function coupling
- `clustering.rs` is already a pure function of `&[StoredEntry]` — no `ContextStore` state mutation except at merge
- `eviction.rs` needs `ContextStore.entries` read access plus quota configuration

### 3. `src/db.rs` (735 lines)

**Current structure:** 22 public free functions. No `impl` blocks. Covers 7 distinct data domains sharing only `PgPool` and `serialization_retry`.

**Proposed split into `db/` module:**

| New File | What goes there | Est. Lines |
|----------|----------------|------------|
| `db/mod.rs` | `build_shared_pg_pool()`, `connect()`, `execute_with_serialization_retry()`, `init_schema()`, re-exports | 80 |
| `db/tools.rs` | `upsert_tool()`, `list_tools()`, `get_tool_by_name()`, `get_tool_source()`, `list_tools_with_schema()` | 120 |
| `db/executions.rs` | `record_execution()`, `list_executions()`, `record_registry_event()` | 60 |
| `db/memory.rs` | `store_memory()`, `search_memories()` | 100 |
| `db/context.rs` | `insert_context_entry()`, `search_context_entries()`, `load_context_entries()` | 90 |
| `db/skills.rs` | `upsert_skill()`, `search_skills()`, `list_skills()` | 80 |
| `db/routines.rs` | `list_routines()` | 30 |

**Seam signals:**
- Zero cross-domain coupling — no function in `tools.rs` calls any function in `memory.rs`
- All functions take `&PgPool` as first arg — pure database access
- SQL `CREATE TABLE` statements in `init_schema()` can stay in `mod.rs`

### 4. `src/embedding.rs` (679 → 586 core lines)

**Current structure:** `EmbeddingClient` struct with 6 provider configuration structs, fallback chain, local ONNX embedding via tract-onnx, remote HF API embedding, and deterministic zero-vector fallback.

**Proposed split into `embedding/` module:**

| New File | What goes there | Est. Lines |
|----------|----------------|------------|
| `embedding/mod.rs` | `EmbeddingClient` struct, `EmbeddingProvider` enum, `embed()`/`embed_batch()` dispatch, `from_env()` builder | 150 |
| `embedding/providers.rs` | `ProviderConfig` struct, `from_env()` per-provider init (Ollama, LlamaCpp, Nvidia, OpenAI, Moonshot, HuggingFace) | 120 |
| `embedding/local.rs` | `LocalEmbeddingEngine` — tract-onnx model init, mean pooling, L2 normalization (feature-gated `#[cfg(feature = "tools-local-embeddings")]`) | 150 |
| `embedding/hf.rs` | HF API embedding via `reqwest`, `embed_remote_batch()`, caching | 80 |
| `embedding/fallback.rs` | Deterministic zero-vector placeholder, dimension constant | 30 |

**Seam signals:**
- `local.rs` is already feature-gated (`tools-local-embeddings`) — natural compilation boundary
- `hf.rs` is network-only, `local.rs` is onnx-only — never both needed in same context

## Medium Priority — Moderate Split Benefit

### 5. `src/orchestrator.rs` (842 lines)
**4 impl blocks on 2 types** (`Orchestrator`, `DagScheduler`). `run_parallel()`, `run_pipeline()`, `run_workflow()`, `run_supervisor()` are 4 different orchestration patterns sharing only tool registry access and LLM agent creation.

**Split:** `orchestrator/dag.rs` for `DagWorkflow`/`DagScheduler`/`DagNode` (topological sort + execution levels) and `orchestrator/runner.rs` for the 4 runner methods.

### 6. `src/capability.rs` (540 → 491 core lines)
**Token lifecycle:** `issue()`/`verify()` (JWT), `reserve()`/`refund()`/`acquire_execution_guard()` (rate-limit budget), `consume()`/`defuse()`/`disarm()` (token consumption). The budget tracking (HashMap of nonces + amounts) is separable from JWT signing.

**Split:** `capability/token.rs` for JWT operations + `find_token()`, `capability/budget.rs` for reservation/refund/guards.

### 7. `src/worker.rs` (533 lines)
**Two structural groups:** `SeedChannel` + `SeedEvent` types (channel wiring), `AutoSeedWorker` (background daemon logic: drain batches, embed, seed, dedup, merge, pre-warm). Has a clean MPSC split but is one file.

**Split:** `worker/channel.rs` for `SeedChannel`/`SeedEvent` types, `worker/mod.rs` for `AutoSeedWorker`/`spawn()`.

### 8. `src/session.rs` (523 → 447 core lines)
**3 CRUD domains:** session lifecycle (`create`, `list`, `delete`), message I/O (`save_message`, `save_atomic`, `load`, `delete`), checkpoint save/load (`save_checkpoint`, `load_latest_checkpoint`), plus circuit breaker.

**Split:** `session/mod.rs` for pool setup + schemas + session CRUD, `session/messages.rs` for message I/O, `session/mod.rs` or `session/checkpoint.rs` for checkpoint + circuit breaker.

### 9. `src/tools/registry.rs` (408 lines)
**3 concerns:** `register_tool()` / `register_tools()` (registration), `execute()` (tool dispatch by name), `search_tools()` (hybrid BM25+dense retrieval). Registration and search are largely independent.

**Split:** Keep registration + execution in `registry/mod.rs`, extract `registry/search.rs` for `search_tools()`.

### 10. `src/config.rs` (557 lines)
**2 config sources:** Environment variable loading (`AgentConfig::from_env()`), project config TOML parsing (`load_project_config()`). The env vars section is long because every tool + LLM model has a `VOLT_*` var, but it's enumerative boilerplate that doesn't benefit from split.

**Split:** Defer. Extract `config/project.rs` if it grows further, but 557 lines for 2 sources is acceptable.

## Low Priority — Cleared (Cohesive)

| File | Rationale |
|------|-----------|
| `src/agent/tool_parser.rs` | 262 core lines. JSON validation + tag formatting are tightly coupled to same concern (tool call argument handling). Single impl block. |
| `src/main.rs` | 429 lines. CLI dispatch is inherently enumerative (15 match arms). Any split would scatter the dispatch table without benefit. |
| `src/turbovec_index.rs` | 235 core lines. Single wrapper type (`TurbovecIndex`), single responsibility (ANN search via TurboQuantIndex). |
| `src/tools/you_tools.rs` | 324 lines. 3 you.com API functions — same domain, same client, same error handling. |
| `src/skills/importer.rs` | 194 core lines. Format detection + conversion for skill manifests. Single pipeline. |
| `src/tui.rs` | 308 lines. Terminal UI with input/rendering/signal — expected coupling in a UI file. |
| `src/vector_index.rs` | 330 core lines. 3 algorithms (BM25, LSH, RRF) but each is <120 lines; splitting would add module overhead without clarity gain. |

## Recommended Sequencing

```
Phase 1 — High impact, low risk (pure extractions)
└── context/         # Staging buffer + clustering are already isolated
└── db/              # Zero cross-domain coupling, pure mechanical split

Phase 2 — High impact, moderate risk
└── embedding/       # Feature-gated local.rs makes this safe
└── agent/           # Requires verifying all existing call sites (Agent::new() chaining)

Phase 3 — Medium priority
└── orchestrator/    # DAG types independent of runner fns
└── capability/      # JWT vs budget: different dependencies
└── session/         # Message I/O separable from checkpointing

Phase 4 — Polish
└── worker/          # Clean but low urgency (533 lines)
└── registry/        # Search extraction is optional (408 lines)
└── config/          # Defer until growth demands split
```

## Lib.rs Impact

Converting these files to directories requires only changing `pub mod X` to `pub mod X` (Rust auto-resolves `X/` directory when `X.rs` is absent) plus adding `pub use` re-exports in `X/mod.rs`. No `lib.rs` changes beyond removing the now-stale `src/X.rs` file.

Current `lib.rs` registration (78 lines, 44 modules) stays identical — all callers continue using `crate::context::ContextStore`, `crate::db::build_shared_pg_pool`, etc.
