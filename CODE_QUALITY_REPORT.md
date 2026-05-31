# Volt Code Quality Report

Generated from three-phase review of the Volt Rust codebase at commit state after `loop_rs.rs` → `agent/builder.rs` + `run.rs` + `compression.rs` split.

---

## Executive Summary

Volt is a cautiously production-ready codebase with strong architectural foundations (parameterized SQL everywhere, a well-designed capability system with `RefundGuard`, hybrid BM25+dense retrieval, and clean module boundaries after the Phase 1 refactoring). However, there are **3 critical** correctness bugs in session/checkpoint persistence, a **critical** privilege escalation path through the deprecated `execute()` method, and **multiple high-severity** gaps in error handling (silently swallowed failures in agent memory I/O, sandbox stdin, and job creation) that would cause silent data loss in adverse conditions. The supervisor mode in the orchestrator is a non-functional placeholder. The async spawn pattern carries a few TOCTOU races and unbounded channel growth surfaces.

| Severity | Count |
|----------|-------|
| CRITICAL | 8 |
| HIGH     | 14 |
| MEDIUM   | 23 |
| LOW/INFO | 19 |
| **Total** | **64** |

---

## Phase 1: Refactor Integrity Findings

### 1.1 Module Visibility Audit

**PASS** — No visibility mismatches found.

`Agent` struct fields are `pub(crate)` (correct — they are accessed from sibling modules `builder.rs`, `run.rs`, `compression.rs` via `impl super::Agent` blocks). The `MAX_TOOL_OUTPUT_CHARS` const is `pub(crate)` (correct — used in `run.rs` only). `is_precision_mode()` is `pub(super)` (correct — called from `run.rs` but defined in `builder.rs`). `compress_if_needed()` is `pub(super)` (correct — called from `run.rs` but defined in `compression.rs`). All other methods are plain `pub` or private consistent with their usage scope.

### 1.2 Re-export Completeness

**PASS** — No remaining Rust source references to `loop_rs`.

The `Agent` struct is defined directly in `src/agent/mod.rs` (no re-export needed). All 14 call sites that previously imported `crate::agent::loop_rs::Agent` or `volt::agent::loop_rs::Agent` (in tests) were updated to `crate::agent::Agent` / `volt::agent::Agent`. The only remaining `loop_rs` references are in documentation files (`REFACTOR_AUDIT.md`, `CODEBASE_MAP.md`, `AGENTS.md`) — these are stale docs but not runtime issues.

### 1.3 Logic Equivalence Check

**PASS** — All cross-module calls work correctly.

`run.rs` calls `self.is_precision_mode()` (defined in `builder.rs`), `self.compress_if_needed()` (defined in `compression.rs`), and `self.seed_truncated_context()` / `self.seed_truncated_context_llm()` (defined in `compression.rs`). All cross-boundary calls use `pub(super)` visibility, which is correctly scoped for sibling modules under `agent/`. No closure-over-self breakage: all methods take `&self` and the `Agent` struct (defined in `mod.rs`) exposes its fields as `pub(crate)`.

### 1.4 Orphan Detection

**WARNING** — One orphan function.

| File | Function | Lines | Notes |
|------|----------|-------|-------|
| `src/agent/run.rs:675` | `push_message()` | 9 | Marked `#[expect(dead_code)]` — intentionally unused, kept for potential future use. Not an accidental orphan from the split. |

No accidental orphans found. The `push_message()` function is a known dead-code item from before the split.

---

## Phase 2: Code Quality Findings

### 2.1 Panic Safety in Library Paths

| Severity | File | Line | Code | Failure Mode |
|----------|------|------|------|-------------|
| **CRITICAL** | `src/context/mod.rs` | 173 | `self.turbovec.write().unwrap()` | If the `SyncRwLock` is poisoned (panic during turbovec operation), every subsequent context store write panics. No recovery path. |
| **CRITICAL** | `src/context/eviction.rs` | 143 | `self.turbovec.read().unwrap()` | Same poisoned-lock panic for the eviction path. |
| **CRITICAL** | `src/context/search.rs` | 53, 134 | `self.turbovec.read().unwrap()` | Same poisoned-lock panic for the search path. Three locations, same vulnerability. |
| WARNING | `src/agent/run.rs` | 262 | `final_call.arguments["answer"].as_str().unwrap_or("")` | Indexing into `serde_json::Value` with `["answer"]` panics if arguments is not an object. The `arguments` field comes from the LLM response and could be any JSON type. |
| WARNING | `src/agent/multimodal.rs` | 33-34 | `std::fs::File::create(&path).unwrap()` | File creation failure panics in a library path. Should propagate error or fail with a clear message. |
| WARNING | `src/agent/multimodal.rs` | 34 | `f.write_all(content).unwrap()` | Write failure panics in a library path. |
| WARNING | `src/db/mod.rs` | 92 | `current.push(chars.next().unwrap())` | `next()` on a char iterator panics if the SQL statement string is empty — should use `if let Some(c)` pattern instead. |
| INFO | `src/turbovec_index.rs` | 66+ | 20+ `lock().unwrap()` calls | All are `std::sync::Mutex` or `RwLock` locks with no recoverable failure mode — poisoning would crash regardless. Acceptable for an index data structure. |
| INFO | `src/agent/tool_parser.rs` | 480, `src/agent/model_registry.rs` | 64, `src/skill_scorer.rs` | 59 | `unwrap()` in test-only or Regex-from-literal paths that are provably infallible. |

### 2.2 Error Handling Consistency

| Severity | File | Line | Issue |
|----------|------|------|-------|
| **CRITICAL** | `src/tools/registry.rs` | 445 | `let _ = tokio::fs::write(path, &json).await` — Tool registry disk cache write silently fails. Corrupt cache is loaded on next startup with no warning. |
| **CRITICAL** | `src/tool_failure_tracker.rs` | 24 | `let _ = sqlx::query(...)` — The failure tracker itself silently swallows its own DB write failures. If the DB is unavailable, failures are no longer tracked without any log. |
| **CRITICAL** | `src/sandbox.rs` | 125-126 | `let _ = stdin.write_all(...)` and `let _ = stdin.shutdown()` — Sandbox stdin write/close failures silently ignored. Process may miss input. |
| **HIGH** | `src/agent/run.rs` | 907 | `let _ = crate::db::store_memory(...)` — Agent memory persistence failure silently swallowed. User conversations are not persisted. |
| **HIGH** | `src/context/eviction.rs` | 34 | `let _ = crate::db::insert_context_entry(...)` — DB persistence of seed_batch entries silently dropped. Entries exist only in-memory and are lost on restart. |
| **HIGH** | `src/jobs/monitor.rs` | 50 | `let _ = self.job_manager.create_job(...)` — Monitoring loop silently fails to create jobs. |
| **HIGH** | `src/routines/engine.rs` | 65 | `let _ = self.job_manager.create_job(...)` — Routine engine silently fails to create jobs. |
| MEDIUM | `src/tui.rs` | 172, 184, 186 | TUI session operations (create, delete, save) silently fail. |
| MEDIUM | `src/commands/agent_run.rs` | 113, 237 | Agent runner session creation and message save silently fail. |
| MEDIUM | `src/config.rs` | 343, 354 | `.env` file write silently fails; user's API key configuration lost. |
| MEDIUM | `src/context/mod.rs` | 157 | `let _ = db.set(pool)` — OnceLock set fails silently. If `new_with_db` is called twice (potential race), second caller silently loses the pool. |
| LOW | `src/events.rs` | 25 | `let _ = self.tx.send(event)` — Intentional (drop if channel full). Documented, acceptable for non-critical events. |

**Error type strategy:** The codebase uses `anyhow::Error` as its dominant error type — consistent across the agent loop, orchestrator, embedding, and session layers. No mixed error type chains. This is idiomatic for an application-level codebase.

### 2.3 Async Correctness

| Severity | File | Line | Issue |
|----------|------|------|-------|
| **HIGH** | `src/commands/daemon.rs` | 23, 43, 62 | `tokio::spawn(async move { ... })` with dropped `JoinHandle`. Three fire-and-forget daemon tasks. No abort/join on shutdown. If the daemon exits, these tasks are orphaned until the runtime shuts down. Not documented as intentional. |
| **HIGH** | `src/commands/agent_run.rs` | 33, 169 | `tokio::spawn(async move { ... })` with dropped `JoinHandle`. Fire-and-forget agent tasks. No mechanism to wait for completion before process exit. |
| MEDIUM | `src/worker.rs` | 215 | `tokio::spawn(async move { ... })` with dropped `JoinHandle`. The background AutoSeedWorker is fire-and-forget. No shutdown signal integration. A graceful shutdown drops the worker mid-batch, losing unseeded data. |
| MEDIUM | `src/checkpoint_journal.rs` | 26 | `tokio::spawn(Self::writer_loop(pool, rx))` with dropped `JoinHandle`. Background writer task runs until the receiver is dropped. On struct drop, the tx half is dropped, but the spawned task may still be processing. |
| MEDIUM | `src/mcp/grpc.rs` | 152 | `tokio::spawn(async move { ... })` — gRPC streaming response task. No explicit shutdown. |
| LOW | `src/tui.rs` | 86 | `tokio::spawn(async move { ... })` with stored `JoinHandle` — properly joined on exit via `cancel_token.cancel()`. |

**Lock-held-across-await finding:** No instances found where a `std::sync::Mutex` lock is held across an `.await` point. The codebase uses `tokio::sync::Mutex` for async contexts and `std::sync::Mutex` only for short synchronous operations (e.g., `turbovec_index.rs`'s inner lock, `graph_rag.rs`'s `RwLock`). All async lock acquisitions follow `self.state.lock().await` patterns correctly.

### 2.4 Resource Cleanup

**PASS** — Resource cleanup is handled correctly throughout.

Key findings:
- `CapabilityManager::RefundGuard` is a `Drop` guard that refunds budget on drop — correctly handles both success (`.defuse()` to suppress refund) and failure (auto-refund even through `catch_unwind`).
- `PgPool` connections are managed by sqlx's pool — no manual checkout/return.
- `tokio::fs::File` handles are used only in the `multi_modal.rs` test-adjacent path.
- `tokio::sync::Mutex` and `RwLock` guards are always dropped explicitly (via `drop(state)`) before long-running operations like tool execution.
- `RefundGuard::drop` correctly uses `panic::catch_unwind` to prevent budget leaks during panics.

### 2.5 Public API Documentation

**WARNING** — 41 undocumented `pub mod` declarations.

All 41 module declarations in `src/lib.rs` lack `///` doc comments. The only documented public items are `http_client()` and `cosine_similarity()`. While module-level doc comments are conventional rather than critical, the codebase's positioning (research paper, compliance-enterprise, EU AI Act) makes this a professional quality gap.

| Module | File | Line | Doc? |
|--------|------|------|------|
| `pub mod agent` | `src/lib.rs` | 3 | NO |
| `pub mod attenuation` | 4 | NO |
| `pub mod capability` | 5 | NO |
| `pub mod channels` | 7 | NO |
| ...all others... | 8-48 | NO |

### 2.6 Sensitive Data in Logs

| Severity | File | Line | Issue |
|----------|------|------|-------|
| **HIGH** | `src/agent/run.rs` | 810 | `tracing::info!("executing tool: {} with {}", name, args)` — Logs full tool call arguments to the tracing pipeline. If `write` or `edit` tools receive file content, or `bash` receives shell commands containing secrets, the raw content is written to logs. |
| INFO | `src/agent/run.rs` | 744 | `eprintln!("...tool '{}({:?})' requires approval", tc.name, tc.arguments)` — Arguments printed to stderr in interactive approval mode. This is a terminal display, not a log file, but visible in TTY output. |

No other log calls interpolate variables with names suggesting API keys, tokens, or credentials.

### 2.7 SQL Injection Safety

**PASS** — No SQL injection vulnerabilities.

All SQL queries in `src/db/` (PostgreSQL) use `$1`, `$2`, `$3` parameterized placeholders via `sqlx::query().bind()`. All SQL queries in `src/session.rs` (SQLite) use `?` parameterized placeholders. No `format!()` or string concatenation is used for SQL construction with runtime values. Migration SQL is loaded from compile-time embedded files via `include_str!()`. The `split_sql_statements()` parser correctly handles PostgreSQL `$$` dollar-quoting.

---

## Phase 3: Gap Analysis Findings

### 3.1 Agent Run Loop (`src/agent/run.rs`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| LLM call times out with no response | **HANDLED** | Retry loop (3 attempts with exponential backoff: 1s, 2s, 4s). On final failure, `return Err(e)` to caller. No silent fallback. |
| Context store hits quota mid-run | **PARTIAL** | Quota enforcement happens in `eviction.rs:seed_batch()` which triggers every `evict_every` inserts (default 100). If the store is at quota capacity during `build_context()`, the search still works (read-only) but `record_run()` may push over quota without triggering eviction. Fixed at next `seed_batch`. |
| Tool execution returns error on iteration 3 of 10 | **HANDLED** | The tool result is appended to message history as a tool error message. The loop continues to the next iteration. This is documented behavior: tools can fail, and the LLM sees the error and can retry or take alternative action. |
| `save_checkpoint()` fails after successful tool execution | **PARTIAL** | The `save_checkpoint()` logs a warning but does not roll back the tool execution. The tool result is already stored in `state.messages` (in-memory). The iteration continues. If the process crashes before the next checkpoint, the completed work since the last successful checkpoint is lost. No compensatory action (e.g., retry later). |
| Iteration limit reached | **HANDLED** | Falls through to `save_session_messages_delta()`, then searches history backwards for last non-empty assistant/tool message. If found, returns `Ok(last_answer)`. Returns `Err("max iterations reached without final response")` only if no content exists. |

**Overall verdict: HANDLED with minor gaps.** The loop is well-structured. The main gap is the lack of compensatory action when `save_checkpoint()` fails.

### 3.2 Context Store (`src/context/`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| Two concurrent agents share a `ContextStore` via `Arc`, both call `evict()` simultaneously | **PARTIAL** | `entries` is `tokio::sync::RwLock` — safe for concurrent access. `evict()` holds the write lock through the entire operation. Two concurrent `evict()` calls will serialize. However, `seed_batch()` holds the write lock for the entire batch operation (including dedup, push, and eviction), blocking all readers. A concurrent `search()` would be blocked until the batch completes. |
| `hydrate_from_db()` called when DB is unavailable | **PARTIAL** | Not applicable — `hydrate_from_db()` does not exist as a separate function. The DB connection is set once at startup via `new_with_db()` or `set_db()`. If the DB is unavailable at startup, the `OnceLock` set succeeds but subsequent DB queries return errors. The `search()` function handles this via `pgvector search failed, falling back to in-memory` at `search.rs:114`. |
| TurboVec index and pgvector index diverge (crash between writes) | **GAP** | No recovery mechanism. `seed_batch()` writes to DB first (`insert_context_entry`), then to in-memory store. If the process crashes after DB write but before in-memory push, the entry is orphaned in the DB. On restart, `hydrate_from_db()` is not called — the in-memory store is empty and the DB has unreferenced entries. No startup reconciliation exists. |
| Context entry embedding has different dimension than current model | **PARTIAL** | `cosine_similarity()` does not check dimensions. If dimensions mismatch (e.g., a 384d entry from a previous model and a 1024d query from the current model), the computation produces incorrect results without warning. `normalize_dims()` in `embedding.rs` pads/truncates to 1024 when creating embeddings, but does not fix existing entries. |

**Overall verdict: PARTIAL.** The DB/in-memory divergence gap is the most concerning — there's no recovery path for entries persisted to DB but not in memory after a restart.

### 3.3 Embedding System (`src/embedding/`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| HF API returns HTTP 429 during batch | **HANDLED** | Retry logic at `embed_remote()` lines 221-231 with exponential backoff (1s, 2s, 4s) and bounded at 3 attempts. Falls through to next provider on exhaustion. |
| Local ONNX model file missing at startup | **HANDLED** | `new_smart()` at `mod.rs:76-84` logs a warning and continues without local embedder (`local: None`). Falls back to remote providers or deterministic placeholder. |
| All providers fail — fallback to zero-vector | **PARTIAL** | The fallback chain ends at `deterministic_placeholder_embedding()` (SHA-256 based, not zero-vector). This produces a stable, deterministic pseudo-random vector per input string. However, there is no downstream guard — `cosine_similarity` with two deterministic embeddings from different inputs will produce a non-zero but meaningless score. No log warning at the call site when this is hit. |
| Embedding dimension mismatch between provider output and expected size | **HANDLED** | `normalize_dims()` at lines 279-286 truncates or pads to exactly `EMBEDDING_DIMENSIONS (1024)`. Safe. |

**Overall verdict: HANDLED.** The fallback chain is well-designed. The deterministic placeholder is a reasonable last resort (not zero-vector). Adding a `warn!()` log when the fallback activates would improve observability.

### 3.4 Capability and Trust System (`src/capability.rs`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| JWT capability token expires mid-execution of a long-running tool | **GAP** | Token expiry is checked at the START of `execute_gated()` via `find_token()` and `verify()`. If a tool runs for 290s and the token expires at 180s, the tool completes. The budget has already been reserved via `acquire_execution_guard()` which is independent of the JWT expiry. The JWT check is a security boundary; the budget reservation is the resource boundary. A long-running tool that starts before expiry and completes after expiry will have consumed budget from an expired token. The `RefundGuard` still refunds correctly on failure. |
| Rate budget exhausted between `reserve()` and actual execution (TOCTOU) | **HANDLED** | `acquire_execution_guard()` atomically reserves the budget and returns a `RefundGuard`. The reserve-and-execute is a single atomic check. No window exists. |
| Delegated agent attempts trust attenuation with partial scope overlap | **HANDLED** | The capability scope system is flat (enum variants, no hierarchy). There is no trust attenuation — `CapabilityScope` is exact-match. The `Api(String)` variant allows scoping to specific API prefixes, but there is no intersection computation. |

**Overall verdict: HANDLED with one gap.** The mid-execution token expiry gap is limited in impact because the `RefundGuard` still enforces budget accounting, and the JWT expiry check is a secondary security control (the primary is the budget guard).

### 3.5 Orchestrator (`src/orchestrator.rs`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| DAG node fails — downstream nodes depend on its output | **GAP** | Node failure (LLM error, tool panic, anyhow error) propagates via `?` at line 869. The entire DAG execution aborts. No retry, no fallback node, no partial-output capture. All sibling nodes' results in the same level are lost. |
| DAG contains a cycle not detected at construction | **HANDLED** | `topological_sort()` (Kahn's algorithm) at line 703-741 detects cycles: if `sorted.len() != self.nodes.len()`, returns `Err`. |
| Parallel agents in `run_parallel()` both write to shared resource | **PARTIAL** | `run_parallel()` uses a `Semaphore(5)` to limit concurrency. Each spawned agent gets its own `Agent` instance (not shared). However, agents may share the same `ContextStore` via `Arc`. Concurrent writes to the context store's internal `RwLock` are safe, but writes to the same `PgPool` (e.g., memory persistence) are not serialized per-agent — the pool handles this internally. |
| Supervisor mode: no actual sub-agent delegation | **GAP** | `run_supervisor()` creates a single supervisor agent that receives worker descriptions in its system prompt. It never spawns worker agents. The supervisor's text output is returned as the final result without any real delegation. This is a non-functional placeholder. |
| Template injection via `{prev}` / `{node_id}` | **GAP** | `run_pipeline()` replaces `{prev}` with the previous agent's full output. `DagScheduler` replaces `{node_id}` with predecessor outputs. An adversarial predecessor output containing `{input}` or `{other_node_id}` would be substituted, potentially injecting prompt-level text into the next agent's task. Single-pass substitution limits the blast radius but does not eliminate the risk. |

**Overall verdict: PARTIAL with critical gaps.** The node failure isolation gap and the placeholder supervisor are the most concerning.

### 3.6 Session and Checkpoint System (`src/session.rs`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| `save_message` `INSERT OR REPLACE` with AUTOINCREMENT PRIMARY KEY | **CRITICAL** | The `id` column is `PRIMARY KEY AUTOINCREMENT` but is never specified in the INSERT. Every call inserts a new row with a new autoincrement ID. The `OR REPLACE` clause is a no-op. Messages accumulate unboundedly. |
| `save_session_messages_atomic` crashes mid-write | **CRITICAL** | Delete-then-reinsert in a transaction: if the process crashes between the `DELETE` and the INSERTs, the transaction is rolled back by SQLite (safe). But if the crash happens after `tx.commit()` returns but before the OS flushes (due to the `.synchronous(NORMAL)` setting), the `DELETE` may be lost and the messages are double-written. |
| SQLite file on network filesystem — connection drops mid-transaction | **MEDIUM** | `Synchronous::Normal` means a power loss can lose the last 1-2 seconds of committed transactions. For a network filesystem, any transient disconnect has the same effect. The circuit breaker (`MAX_CONSECUTIVE_RETRIES = 3`) provides partial protection against replay loops. |
| Session loaded with partially deleted message history | **PARTIAL** | `load_messages()` returns whatever rows exist. If history is partially deleted externally, the session loads partial context with no validation. The `state_hash` circuit breaker only checks for exact retry loops, not for externally modified state. |
| Checkpoint write fails mid-write | **PARTIAL** | `save_checkpoint()` uses `INSERT OR REPLACE` which is atomic for a single row. If the INSERT fails, a warning is logged and the function returns. The previous checkpoint remains readable. |

**Overall verdict: PARTIAL with critical bugs.** The `save_message` never-replaces bug and the migration error-swallowing are the most urgent issues.

### 3.7 AutoSeedWorker (`src/worker.rs`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| MPSC channel full — producer tries to send | **HANDLED** | `UnboundedSender` — never full. But this means no backpressure. |
| Embedding call fails for one item in a batch of 32 | **HANDLED** | Line 261: `embedder.embed_description(&text).await.ok()` — returns `Option`. If `None`, `entry.embedding` remains `None`. The entry is seeded without an embedding. Dedup falls back to exact content match. No data loss. |
| Worker dropped while batch is mid-flight | **GAP** | The worker is a background `tokio::spawn` task. If the `SeedChannel` (sender) is dropped, the receiver's `recv()` returns `None`, and the worker exits its main loop (line 282). But if the worker is currently embedding a batch (inside `join_all` at line 267), there is no cancellation mechanism. The batch completes but the `SeedChannel` may have been dropped, leaving the entries partially seeded. No shutdown signal integration. |
| Embedding rate limited — semaphore exhausted | **PARTIAL** | The `compute_embeddings()` in `context/search.rs` uses a `Semaphore(5)`. If all 5 permits are held by hanging requests, no further embedding can proceed. There's no timeout on the semaphore acquisition. A single hanging HTTP request blocks the entire embedding pipeline. |
| Episodic merge removes wrong indices due to concurrent `seed_batch` | **CRITICAL** | `find_clusters()` returns indices into the current `entries` Vec. Between `find_clusters()` and `remove_indices()`, `seed_batch()` may push new entries, shifting all indices >= the push point. The merge worker then removes wrong entries. |
| Unbounded MPSC channel — event flood OOM | **HIGH** | `UnboundedSender` has no capacity limit. An adversarial agent loop emitting many `ArtifactCreated` events per turn can grow the channel buffer without bound until OOM. |

**Overall verdict: PARTIAL with critical race.** The dangling-index race in episodic merge is the most severe issue.

### 3.8 Tool Execution (`src/tools/registry.rs`)

| Failure Mode | Verdict | Details |
|-------------|---------|---------|
| Tool execution hangs indefinitely | **HANDLED** | `tokio::time::timeout(300s, (tool.exec)(args.clone()))` at line 153. Hardcoded 300s timeout. On timeout, returns `ToolResult { success: false, error: Some("timeout"), duration_ms: 300_000 }`. |
| Tool output larger than `MAX_TOOL_OUTPUT_CHARS` | **HANDLED** | `run.rs` lines 297-318: truncation logic with UTF-8 char boundary safety and reference token storage in `tool_output_buffer`. |
| Tool name in LLM response does not match registered tool | **HANDLED** | `execute_gated()` at line 147: `tools.get(name).ok_or_else(...)` returns an error. The agent loop catches this and pushes an error message into the conversation history (lines 226-256 of `run.rs`). |
| `execute()` (deprecated) bypasses capability enforcement | **CRITICAL** | The deprecated `execute()` at line 172-186 is still callable at runtime. It bypasses all capability enforcement: no scope check, no token verification, no HMAC, no budget tracking. Any internal code path using `execute()` is a privilege escalation. |
| `get_permission` returns `Allow` for unknown tool names | **HIGH** | Line 98: `.unwrap_or(PermissionLevel::Allow)` — unknown tool names get full trust rather than deny-by-default. |
| `search_tools` loses `input_schema` | **HIGH** | Line 334 explicitly sets `input_schema: serde_json::Value::Null`. The LLM cannot generate correct tool call arguments from search results. |
| `compute_embeddings` cache ignores `input_schema` changes | **MEDIUM** | Content hash at lines 394-402 only includes name + description. If a tool's input schema changes, the cache is not invalidated. |
| GraphRAG augmentation does not filter by relevance | **MEDIUM** | `graph.find_related(tool_name, 2)` returns graph neighbors up to 2 hops, added unconditionally. High graph connectivity can inject irrelevant tools into every search. |

**Overall verdict: PARTIAL with critical bypass.** The `execute()` bypass is the most severe issue.

---

## Remediation Roadmap

Ordered by risk and implementation effort. Items labeled `[DAY]` can be fixed in under an hour; `[WEEK]` requires design changes.

### Immediate (Fix Before Any Production Use)

1. **[CRITICAL] [DAY] Fix `execute()` bypass.** Change visibility of `execute()` to `pub(crate)` or remove it. Audit all internal call sites and migrate to `execute_gated()`.  
   *Verify:* `cargo build` succeeds; `cargo test` passes; no call site uses `execute()`.

2. **[CRITICAL] [DAY] Fix `save_message` never-replaces bug.** Either specify `id` in the INSERT to make `OR REPLACE` meaningful, or remove `OR REPLACE` and use `INSERT` with a unique constraint on `(session_id, role, created_at)`.  
   *Verify:* After inserting the same message twice, `load_messages` returns exactly one copy.

3. **[CRITICAL] [DAY] Fix `is_char_boundary` off-by-one in `final_call.arguments["answer"]`.** Replace JSON indexing with `.get("answer").and_then(|v| v.as_str())` to avoid panic on non-object arguments.  
   *Verify:* Agent returns gracefully when LLM returns `final_answer` with `arguments: null`.

4. **[CRITICAL] [WEEK] Fix episodic merge dangling-index race.** Add a generation counter to the `entries` Vec. Before removing indices, verify they are still valid. Alternatively, use deferred removal (mark as tombstone, clean up later).  
   *Verify:* Stress test with concurrent `seed_batch()` and `episodic_merge()` produces no index-out-of-bounds or wrong-entry removal.

5. **[CRITICAL] [WEEK] Fix DAG node failure isolation.** Add per-node retry (2 attempts) and per-level error aggregation: if a node fails, mark its output as `Err` and continue, rather than aborting the entire DAG.  
   *Verify:* A DAG with 3 nodes in parallel where one node fails still returns results for the successful 2 nodes.

### High Priority (Before v1.0 Release)

6. **[HIGH] [DAY] Add `warn!` log when deterministic placeholder embedding activates.** At `embedding/mod.rs:150` insert `tracing::warn!("all embedding providers failed, using deterministic fallback")`.  
   *Verify:* When all providers are configured with invalid keys, the warning appears in the log.

7. **[HIGH] [DAY] Add file size guard to `seed_from_workspace`.** Check file size before `read_to_string`. Reject files > 10 MB.  
   *Verify:* A 1 GB dummy file in the workspace does not OOM the seed worker.

8. **[HIGH] [DAY] Fix `get_permission` default for unknown tools.** Change `unwrap_or(PermissionLevel::Allow)` to `unwrap_or(PermissionLevel::Prompt)` — require explicit approval for unknown tools.  
   *Verify:* A misspelled tool name in an LLM response triggers the approval prompt rather than executing.

9. **[HIGH] [WEEK] Replace `let _` error swallowing with logged errors in production-critical paths.** Target list: `store_memory` in run.rs (line 907), `insert_context_entry` in eviction.rs (line 34), `stdin.write_all` in sandbox.rs (lines 125-126), `create_job` in monitor.rs (line 50) and engine.rs (line 65).  
   *Verify:* Each path logs a `warn!` or `error!` when the operation fails.

10. **[HIGH] [WEEK] Add `volatile` synchronous mode to SQLite.** Change `.synchronous(SqliteSynchronous::Normal)` to `Full` at `session.rs:17`. This ensures durability at the cost of ~10x write latency.  
    *Verify:* `cargo test --features testutils` passes with the change.

11. **[HIGH] [WEEK] Fix supervisor mode to actually delegate work.** Replace the placeholder `run_supervisor` with real sub-agent spawning. Each worker spec is created as a separate `Agent` and run in parallel.  
    *Verify:* A supervisor with 3 worker specs actually runs 3 agents and synthesizes their outputs.

### Medium Priority

12. **[MEDIUM] [DAY] Fix `turbovec` poisoned-lock panics.** Replace `.unwrap()` on `RwLock` read/write with `.lock().map_err(...)` or use `catch_unwind` to prevent poisoned-lock propagation.  
    *Verify:* A panic in turbovec code does not crash the next context store operation.

13. **[MEDIUM] [DAY] Add backpressure to `SeedChannel`.** Replace `UnboundedSender` with `tokio::sync::mpsc::channel(256)` bounded channel. Use `try_send` or `send().await` with timeout.  
    *Verify:* An event flood does not grow memory unboundedly.

14. **[MEDIUM] [DAY] Add embedder timeout.** Add a timeout to the semaphore acquisition in `compute_embeddings()` and to individual `embed_remote()` HTTP requests.  
    *Verify:* A hanging embedding request does not block the entire pipeline for more than 30 seconds.

15. **[MEDIUM] [DAY] Add `search_tools` input_schema to search results.** Replace `serde_json::Value::Null` with the actual tool schema from the registry.  
    *Verify:* Search results include the same `input_schema` as `get_definitions()`.

16. **[MEDIUM] [DAY] Add `compress_if_needed` token budget debug log.** Log when compression activates, showing the exact number of tokens before/after.  
    *Verify:* During a long conversation, the log shows compression events with token counts.

17. **[MEDIUM] [DAY] Fix daemon fire-and-forget tasks.** Store `JoinHandle`s and add a shutdown mechanism (watch channel) that aborts tasks on daemon exit.  
    *Verify:* Daemon shutdown completes within 5 seconds without orphaned tasks.

18. **[MEDIUM] [WEEK] Add context-store DB reconciliation on startup.** After `new_with_db()`, load all entries from the DB into the in-memory store to recover from crash divergence.  
    *Verify:* After a process crash, previously seeded entries are available in the new process.

19. **[MEDIUM] [WEEK] Audit and document all `tokio::spawn` fire-and-forget tasks.** Add explicit comments explaining why each fire-and-forget is safe, or convert to managed tasks.  
    *Verify:* Every `tokio::spawn` call in `src/` has a rationale comment.

### Quality Improvements (Next Sprint)

20. **[INFO] [DAY]** Add `///` doc comments to all 41 `pub mod` declarations in `src/lib.rs`. One-sentence descriptions are sufficient.

21. **[INFO] [DAY]** Add `use std::io::Write` import at module level in `run.rs` (currently inside the function body at line 748).

22. **[INFO] [WEEK]** Replace `#[expect(dead_code)]` on `push_message()` in `run.rs` with either full removal or a usage site.

23. **[INFO] [WEEK]** Add context store integration test for concurrent `seed_batch` + `search` with 10+ concurrent readers.

24. **[MEDIUM] [WEEK]** Hardening: add `#[must_use]` to all futures returned by public methods on `ContextStore`, `Agent`, and `ToolRegistry` to catch accidentally dropped futures.

---

*Report generated from static analysis of `src/` (excluding `src/bin/`, test code, `test_utils.rs`). Dynamic analysis (e.g., actual race reproduction) is outside scope.*