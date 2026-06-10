# Everything-as-RAG / PostgreSQL Production-Readiness Audit

**Date:** 2026-06-09
**Scope:** Volt's Unified ContextStore, AutoSeedWorker, PostgreSQL schema/migrations, embedding subsystem
**Verdict:** **NOT PRODUCTION-READY.** Architecture and types are sound; production wiring has 4 P0 and 8+ P1 issues.

---

## Severity Roll-Up

| Severity | Count | Impact |
|---|---|---|
| **P0** | 4 | Block shipping today |
| **P1** | 8+ | Serious — fix before next release |
| **P2** | 9 | Should fix in next sprint |
| **P3** | 6+ | Nice-to-have |

---

## 1. ContextStore (`src/context/*`)

### 1.1 What matches spec
- `ContextKind` enum with all 12 variants at `src/context/mod.rs:19-32`
- Quotas at `src/context/mod.rs:52-67` match AGENTS.md exactly
- Composite score `0.4×recency + 0.3×success + 0.2×freq + 0.1×density` at `src/context/mod.rs:112-125`
- Dedup at cosine ≥ 0.92 (`src/context/eviction.rs:7`)
- BM25+ / RRF hybrid retrieval (`src/context/search.rs:184-228`)
- Episodic clustering ≥ 0.85, ≥ 3 members (`src/context/clustering.rs`)

### 1.2 [P0] WebUI never wires ContextStore
**File:** `src/webui/runtime.rs:386-446`
**Issue:** `Runtime::start` builds `Agent` with `with_workspace`, `with_cancel`, `with_event_bus`, `with_approval`, `with_stream`, `with_sqlite_pool` — **no `.with_context()`, no `.with_seed_channel()`, no `AutoSeedWorker::spawn`, no `seed_background`**. The entire RAG subsystem is inert for every WebUI user.
**Working reference:** `src/commands/agent_run.rs:280-320`
**Fix:** Replicate the wiring from `agent_run.rs` inside `Runtime::start`.

### 1.3 [P1] Embedding dimension hardcoded to 1024
**Files:** `src/embedding/mod.rs:8` (`EMBEDDING_DIMENSIONS = 1024`), `src/embedding/mod.rs:332` (`normalize_dims()` truncates)
**Issue:** `nvidia/llama-nemotron-embed-1b-v2` (native 2048d, default at `src/embedding/providers.rs:149`) and `text-embedding-3-small` (1536d, line 161) lose 25-50% of their information capacity before they hit pgvector.
**Fix:** Read dim from env (`EMBEDDING_DIMS`) and parameterize the `vector(?)` migrations.

### 1.4 [P2] In-memory store unbounded across kinds between eviction windows
**File:** `src/context/mod.rs:129` (`ContextStore.entries: RwLock<Vec<StoredEntry>>`)
**Issue:** Quotas are only enforced when `insert_count >= evict_every` (default 100, `src/context/eviction.rs:88`). Burst inserts (e.g. `seed_tool_intents` for 55 tools + `seed_permissions` for 55 more on startup) can push individual kinds past quota for several minutes.
**Fix:** Check `kind_count >= kind.quota() * 2` per-insert and force a kind-local evict.

---

## 2. PostgreSQL (`src/db/*`, `migrations/*`)

### 2.1 [P0] Schema is never auto-migrated on first connect
**File:** `src/db/mod.rs:71` (`init_schema` runs 3 SQL files unconditionally)
**Issue:** `init_schema` is only invoked by `volt init-db` (`src/commands/tools.rs:9`) and `volt migrate` (`src/main.rs:706`). `agent_run`/`webui` simply `db::connect()` (which builds a pool but runs no DDL); on a fresh database every query fails with "relation does not exist". `hydrate_from_db` errors are then silently swallowed (`src/commands/agent_run.rs:283` — `_ => {}`), masking the failure as "0 entries hydrated".
**Fix:** Call `init_schema` once inside `build_shared_pg_pool` behind a `DO $$ ... IF NOT EXISTS ...` guard, or refuse to start without `schema_version` present.

### 2.2 [P0] Partial HNSW indexes have a case-mismatch bug
**File:** `migrations/0003_storage_optimizations.sql:14,18,22`
**Issue:** Migrations declare `WHERE kind = 'Tool'`, `'Skill'`, `'Memory'` (PascalCase). But `ContextKind::as_str()` returns lowercase `"tool"`, `"skill"`, `"memory"` (`src/context/mod.rs:36-49`), which is what gets bound to `$2` (`src/db/context.rs:85`) and inserted (`src/db/context.rs:24`). **None of the partial indexes will ever match a row** — every query degrades to a sequential scan.
**Severity:** P0 (silent perf cliff once context_entries exceeds ~10k rows).
**Fix:** Change the WHERE clauses to lowercase or add a `LOWER()`-cast helper.

### 2.3 [P1] `schema_version` table is declared but never populated
**Files:** `migrations/0001_core.sql:7-10` (table created), no migration inserts a row.
**Issue:** The SQLite session DB tracks versions properly (`src/session.rs:94,123,152,174`) but the Postgres equivalent is dead code.
**Fix:** At the end of each migration file, `INSERT INTO schema_version (version) VALUES (N) ON CONFLICT DO NOTHING;`.

### 2.4 [P1] Multiple ad-hoc pools per agent invocation
**File:** `src/tools/registration.rs:12` (50 connections), `src/commands/agent_run.rs:258` (50 more)
**Issue:** `setup_tools` opens its own pool, then `agent_run` opens another, and `seed_skills_from_db` runs on yet another clone. With `max_connections=50` × 2-3 pools, a single `volt agent run` can consume 100-150 PG connections. The shared pool exists (`build_shared_pg_pool` returns `Arc<PgPool>`) but `connect()` immediately `Arc::unwrap_or_clone`s it (`src/db/mod.rs:40`), defeating the purpose.
**Fix:** Thread the `Arc<PgPool>` through `setup_tools` / `seed_skills_from_db` instead of re-connecting.

### 2.5 [P2] `bulk_insert_context_entries` interpolates vectors into SQL
**File:** `src/db/context.rs:169` (`b.push(format!("{}::vector", vector_literal(emb)))`)
**Issue:** Not parameterized; builds the literal `[0.123,0.456,...]::vector` into the query string. Same pattern at line 223 (`bulk_update_embeddings`). Bypasses prepared-statement caching (every batch is a unique query string), causing PG to re-plan.
**Fix:** Bind embeddings as `&[String]` parameters using `UNNEST($N::text[])::vector[]`.

### 2.6 [P3] Pool config: `min_connections=5` is high for a CLI
**File:** `src/db/mod.rs:26-31`
**Issue:** Holds 5 connections open at idle per pool. Combined with #2.4, a single agent run holds 10-15 always-open connections.
**Fix:** `min_connections(1)` for CLI usage, keep 5 for daemons.

---

## 3. AutoSeedWorker (`src/worker.rs`)

### 3.1 [P1] Worker cannot be shut down cleanly
**File:** `src/worker.rs:229` (`spawn()` returns `()`)
**Issue:** The shutdown loop checks `self.cancel.is_cancelled()` at line 236, then calls `rx.recv().await` at line 242 which blocks indefinitely; even if `cancel.cancel()` is called afterwards, the worker won't notice until a new event arrives. In practice the worker only exits when **every** `SeedChannel` clone is dropped. On process exit, the in-flight batch (≤32 events + their embeddings) is lost.
**Fix:** `tokio::select!` between `cancel.cancelled()` and `rx.recv()`; return a `JoinHandle` from `spawn` so `Runtime::shutdown` can `.await` a final drain.

### 3.2 [P2] CancelToken passed in is never signaled
**Files:** `src/agent_tui.rs:102`, `src/commands/agent_run.rs:308-313`
**Issue:** Both create a fresh `CancelToken::new()` and hand it to the worker without ever flipping it. The worker's cancel check is therefore decorative.
**Fix:** Pass the agent's own cancel token, and wire ctrl-c handler to cancel it.

### 3.3 [P1] MCPRegistered events are never emitted
**Files:** `src/worker.rs:31` (`SeedEvent::MCPRegistered` defined), `src/worker.rs:182` (`SeedChannel::mcp_registered` defined)
**Issue:** `grep -r mcp_registered src` finds **zero** call sites outside the worker module itself. Neither `src/tools/searchhq.rs:11` (`register_searchhq_tools`) nor `src/webui/runtime.rs:1647` (`handle_register_mcp_server`) emit the seed event. AGENTS.md's claim "MCPConfig (100 quota), Seeded From SeedEvent::MCPRegistered" is aspirational — the MCPConfig kind will always be empty in production.
**Fix:** Emit `seed_channel.mcp_registered(...)` from every MCP registration path.

### 3.4 [P2] Failed embeddings produce un-searchable entries
**File:** `src/worker.rs:276-282`
**Issue:** On embedder failure sets `embedding = None` and proceeds to `seed_batch`, which persists the row with `NULL::vector` (`src/db/context.rs:171`). The in-memory `search()` filters these out (`src/context/search.rs:130`) but the DB row exists and counts against the kind quota. Composite score will keep these phantom entries alive (recency is recent).
**Fix:** Drop entries with `None` embedding from the batch, log + metric the drop, and let the next agent run re-emit.

### 3.5 [P1] No PII/secret redaction on seeded content
**File:** `src/worker.rs:39-114` (`SeedEvent::to_context_entry()`)
**Issue:** Puts raw `task`, `resolution`, `file_path`, `description` into `ContextEntry.content`. The user-input leak detector runs at `agent/run.rs:723` for the **prompt** path only; nothing scans the LLM's resolution text or tool output before it hits the worker. A model that recites a secret from a prior turn will persist it forever in pgvector.
**Fix:** Apply `LeakDetector::scan` inside `SeedEvent::to_context_entry()` before construction.

### 3.6 [P3] Pre-warm batches are inconsistent
**File:** `src/worker.rs:449,492,501,591,630`
**Issue:** `seed_tool_intents` uses 2-pass (`seed_batch` then `compute_embeddings`). `seed_permissions` does it differently: embeds **inline** inside the loop, then `seed_batch` once. The inline pattern in `seed_permissions` is serial (no semaphore), so embedding 50 permission rules calls the embedder 50 times sequentially.
**Fix:** Unify on the `seed_batch → compute_embeddings` pattern.

---

## 4. Seeding Triggers

### 4.1 [P2] `EpisodeComplete` is wired but always `success: true`
**File:** `src/agent/run.rs:548` (emission), `src/agent/run.rs:1339` (hardcoded `true`)
**Issue:** Failed runs (max iterations, errors) never emit at all. Composite score `success_rate` therefore overstates effectiveness.
**Fix:** Emit `EpisodeComplete` from the error branch with `success: false`.

### 4.2 [P3] `ArtifactCreated` `file_path` extraction is fragile
**File:** `src/agent/run.rs:1350-1416`
**Issue:** File path extraction is text-scraping the tool output (`l.contains("Wrote to") || l.contains("Edited")`), which silently fails for any tool that doesn't print those exact phrases.
**Fix:** Have `write`/`edit` return structured metadata (path) on their `ToolResult`.

### 4.3 [P1] MCPRegistered NOT wired — see 3.3.

---

## 5. Embedding (`src/embedding/*`)

### 5.1 [P3] HF API is the 5th-choice fallback, not primary
**File:** `src/embedding/providers.rs:124` (`auto_detect_providers`)
**Issue:** Provider order: Ollama (if running) → NVIDIA → OpenAI → HF → Moonshot. AGENTS.md's "Embed via HF API (semaphore=5)" mischaracterizes the actual default path.
**Fix:** Update AGENTS.md or change provider precedence.

### 5.2 [P1] Deterministic SHA256 placeholder embeddings are catastrophic
**File:** `src/embedding/mod.rs:353-373` (`deterministic_placeholder_embedding`)
**Issue:** Returns a SHA256-derived vector when all providers fail. Stable per input string but not aligned with any real embedding distribution. Queries against a store mixing real + placeholder embeddings will return placeholder vectors as "near misses" for unrelated text.
**Fix:** Mark placeholder-embedded entries with `metadata.placeholder = true` and filter them out of `search()` until re-embedded.

### 5.3 [P2] Embedder retry is only at the request layer, not at the worker batch level
**Issue:** If HF returns 5xx for an entire batch, all 32 embeddings get `None` and the worker advances. No batch-level retry, no dead-letter queue.
**Fix:** Re-queue failed events with a retry counter; abandon after 3 attempts.

### 5.4 [P3] Pool of HTTP clients not reused across embedder instances
**Files:** `webui/runtime.rs:263`, `agent_run.rs:60`, `agent_tui.rs:24`, `webui/runtime.rs:1535`
**Issue:** `EmbeddingClient::new_smart()` is called separately in 4+ places. Each call rebuilds providers + tries to load the ONNX model.
**Fix:** Cache a single `EmbeddingClient` per process.

---

## 6. Production-Readiness Cross-Cutting

### 6.1 [P1] No graceful shutdown for WebUI
**File:** `src/webui/runtime.rs:210`
**Issue:** `Runtime::start` spawns the command loop with no signal handler; there's no `Runtime::shutdown()` method. On SIGTERM the process is killed mid-batch; in-flight chat turns are aborted with no SQLite flush guarantee.
**Fix:** Install ctrl-c handler that flips `cancel_token`, awaits `command_loop` join, then drops `Runtime`.

### 6.2 [P0] In-memory audit log is not durable
**File:** `src/webui/runtime.rs:46,184,1824` (`audit_log: Arc<Mutex<Vec<AuditEntry>>>`, cap 2000)
**Issue:** AGENTS.md claims "AgentRun ... EU AI Act Art. 12 compliant" but Art. 12 requires "automatic recording" with traceability — an in-memory ring buffer that vanishes on restart fails the requirement.
**Fix:** Persist `AuditEntry` to a dedicated `audit_log` Postgres table with append-only semantics and never-delete retention.

### 6.3 [P1] No metrics, only tracing
**Issue:** No `metrics` crate. Only `tracing::info!`/`warn!`. No way to measure embed latency p50/p99, batch drain rate, channel saturation, or eviction throughput. OpenTelemetry **traces** are emitted (`src/telemetry.rs`) but **metrics** are not.
**Fix:** Add `metrics` crate, expose a `/metrics` endpoint, instrument worker drain + embedder roundtrip.

### 6.4 [P2] Channel saturation has no alarm
**File:** `src/worker.rs:136-146` (`SeedChannel::send`)
**Issue:** Logs a `tracing::warn` per dropped event. Under sustained load this fills the log with thousands of identical lines and no counter.
**Fix:** Rate-limit the warning (log every Nth drop) and bump a `seed_events_dropped_total` counter.

### 6.5 [P2] No connection-failure recovery on Postgres
**File:** `src/webui/runtime.rs:251` (logs and proceeds with `pg_pool = None`)
**Issue:** If Postgres restarts mid-session, sqlx will reopen connections, but if the initial connect fails (e.g. PG starts a moment later) the whole RAG subsystem is offline for the rest of the process lifetime.
**Fix:** Retry initial connect with backoff (10×, 30s total), and add a background task that re-attempts `build_shared_pg_pool` periodically when `pg_pool == None`.

### 6.6 [P1] Encrypted secrets table is never used by context store
**File:** `migrations/0002:48-53` (`secrets` table)
**Issue:** Defines a `secrets` table for the `crate::secrets` module, but the seed pipeline doesn't consult it. If a workspace `MEMORY.md` contains `OPENAI_API_KEY=sk-...`, it gets embedded and persisted verbatim.
**Fix:** Gate workspace file ingestion through `crate::leak_detector` and the `secrets` redaction layer.

### 6.7 [P1] `hydrate_from_db` silently swallows errors
**File:** `src/commands/agent_run.rs:282-285`
**Issue:** `match store.hydrate_from_db(2000).await { Ok(n) if n > 0 => ..., _ => {} }` — Err and Ok(0) both ignored. A schema-not-found error here yields zero user-visible output; the agent runs as if RAG were empty.
**Fix:** Log `Err` cases at `warn` level and surface a `chat!` line so the user knows RAG is degraded.

### 6.8 [P2] `split_sql_statements` is a homegrown parser
**File:** `src/db/mod.rs:96-121`
**Issue:** Splits on `;` but only handles `$$` dollar-quote. Any `;` inside a single-quoted string literal, a `--`-comment, or a `/* */` block will misparse.
**Fix:** Use `sqlx::migrate!` and the `migrations/` table convention instead of `include_str! + split`.

### 6.9 [P2] No backpressure on the broadcast channel
**File:** `src/webui/runtime.rs:42,325` (`event_tx: broadcast::Sender<UiEvent>`, cap 256)
**Issue:** If a UI subscriber is slow, the broadcast drops messages for that subscriber (sqlx broadcast lag error); the runtime never notices. ToolCallEnd and ChatChunk events can be silently lost.
**Fix:** On `RecvError::Lagged(n)`, emit a `UiEvent::Warning` and increment a counter.

### 6.10 [P3] Embedding endpoint URL `http://localhost:11434` not configurable from CLI flags
**File:** `src/embedding/providers.rs:29`
**Issue:** Ollama default hardcoded. Only `EMBEDDING_ENDPOINT` env var changes it. Not actionable but undocumented in `volt --help`.
**Fix:** Add `--embedding-endpoint` to the agent commands.

---

## Additional Items Discovered Post-Audit

- **No settings/onboarding flow** — App silently uses default provider precedence; user is never prompted to enter API keys. Keys must be set in `.env` before launch or features silently fail.
- **No key validation** — Invalid keys (e.g. `GROQ_API_KEY=garbage`) cause cryptic API errors at runtime rather than a clean onboarding error.
- **No "first run" wizard** — New users hit `pgvector extension missing`, `migrations not run`, `GROQ_API_KEY not set` with no recovery path.
- **Hardcoded `LLM_DEFAULT_PROVIDER=groq`** — App defaults to Groq even if no key is set; user gets cryptic 401s.

---

## Recommended Fix Order

1. **P0-2.2** (HNSW case mismatch) — One-line SQL fix, eliminates a silent perf cliff.
2. **P0-2.1** (auto-migrate) — Critical for any non-init-db workflow.
3. **P0-1.2** (WebUI ContextStore wiring) — RAG is currently inert in WebUI.
4. **P0-6.2** (durable audit log) — Compliance requirement.
5. **Settings/onboarding flow** — Required by user directive; no hardcoded providers.
6. **P1-1.3, P1-2.4, P1-3.1, P1-3.3, P1-3.5, P1-5.2, P1-6.1, P1-6.3, P1-6.6, P1-6.7** — Serious but each is a discrete unit.
7. **All P2/P3** — Polish pass.
