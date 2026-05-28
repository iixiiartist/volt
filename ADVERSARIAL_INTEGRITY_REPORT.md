# Adversarial Integrity Report

**Date:** 2026-05-28  
**Tester:** Hostile QA / Systems Security Auditor  
**Target:** Volt Rust agent framework (commit HEAD, release build)  
**Environment:** Windows 11, PostgreSQL 18 (Docker pgvector/pgvector:pg16), SQLite (WAL mode)

---

## Phase 1: Deprecated Compiler Extermination

### Methodology
- Built with `$env:RUSTFLAGS="-D warnings -D deprecated"; cargo check --all-targets --features testutils`
- Grepped for `#[allow(deprecated)]` and `ToolRegistry::execute()` call sites

### Findings

**PASS — No deprecation leaks into production.**

| Check | Result |
|---|---|
| `#[allow(deprecated)]` annotations | **0 found** — zero across entire codebase |
| `ToolRegistry::execute()` call sites | **0 found** in production code |
| `ToolRegistry::execute_gated()` call sites | 2 confirmed production paths (`loop_rs.rs:1170`, `mcp/server.rs:115`) |
| Build failure with `-D deprecated` | **Build failed** — but only for dead_code, NOT deprecated |

The deprecated `ToolRegistry::execute()` (`registry.rs:176`) is properly marked `#[deprecated]` and has **zero callers**. All `execute_gated` call sites verified.

### Warnings Discovered
1. **`push_message`** (src/agent/loop_rs.rs:820) — private async fn never called. Dead code.
2. **`TelegramChannel::token`** (src/channels/telegram.rs:4) — struct field never read. Dead code.

These are warnings (not deprecations) and do not affect security. Should be removed or annotated `#[expect(dead_code)]`.

---

## Phase 2: Live CLI Stress-Testing — SQLite & Postgres Contention

### Methodology
- Launched 8 parallel agents via `volt workflow --pattern parallel` with `web_search` and `web_scrape_all` tool calls
- Each agent ran 5 iterations with `--allow-all`
- SQLite (WAL mode, busy_timeout=5s, max_connections=16) for session persistence
- Postgres (max_connections=50, acquire_timeout=10s) for tool registry, context store

### Results

| Contention Signal | Occurred? | Details |
|---|---|---|
| SQLITE_BUSY ("database is locked") | **NO** | Zero occurrences across 8 concurrent agents. WAL mode + busy_timeout=5s effective. |
| AcquireTimeout / max_connections(50) exhaustion | **NO** | Postgres pool never saturated. Each agent uses `sqlx::PgPool` from shared pool. |
| SQLSTATE 40001 (Serialization Failure) | **NO** | No serialization errors. No retry loop triggered. |
| Thread panic or agent drop-out | **NO** | All 8 agents completed: each produced real web_search tool output successfully. |

### Verdict: **PASS** — no contention failures. The tokio-based parallel agent execution (via `tokio::spawn` + `futures::future::join_all`) correctly shares DB pools without deadlock or exhaustion.

---

## Phase 3: RefundGuard & Panic Leaks CLI Drill

### Methodology
1. Injected `panic!("Adversarial Crash")` into `web_search` handler when query contains `PANIC_TRIGGER`
2. Ran 5 parallel agents: one triggering panic, four calling web_search on normal queries
3. Verified RefundGuard::drop() interception, HMAC signature integrity, sibling task non-corruption

### Results

#### Bug Discovered: **CRITICAL — HMAC Signature Corruption in RefundGuard::drop()**

**Location:** `src/capability.rs:415` (original, pre-fix)

**Root Cause:** `RefundGuard::drop()` performed `token.remaining = token.remaining.saturating_add(amount)` to refund budget after panic/failure, but **never re-signed the HMAC-SHA256 signature**. The `verify()` call on the next tool invocation computed `sign_payload(payload_with_refunded_remaining)` which produced a different signature than the stored stale signature, resulting in:

```
capability verify failed for tool 'web_search': signature mismatch — token tampered
```

**Impact:** After ANY RefundGuard-triggered refund (panic OR tool failure), ALL subsequent capability-gated tool calls by ALL agents would fail with `SignatureMismatch` until the token expired. This is a **complete denial-of-service** of the capability system after any refund event.

**Evidence:** Before the fix, 4/4 normal agents failed with "signature mismatch" after the panicking agent's RefundGuard fired. After the fix, the same test showed 0/4 signature mismatches — only legitimate HTTP 404 errors.

#### Fix Applied
```rust
// In RefundGuard::drop(), after remaining += amount:
let payload = build_token_payload(
    &token.scope,
    token.max_budget,
    token.remaining,
    &token.expires_at,
    &token.nonce,
);
token.signature = sign_payload(&key, &payload);
```

Required API change: `RefundGuard` now stores `key: Option<Vec<u8>>` (the manager's HMAC signing key), passed through from `CapabilityManager::acquire_execution_guard()`.

#### Panic Safety Results (After Fix)

| Agent | Outcome | Duration | Capability Status |
|---|---|---|---|
| panic-agent (PANIC_TRIGGER) | success=false, "unknown" | 0ms | RefundGuard refunded, **properly re-signed** |
| normal-a (web_search) | HTTP 404 (legitimate) | 3252ms | **No signature error** ✓ |
| normal-b (web_search) | HTTP 404 (legitimate) | 2900ms | **No signature error** ✓ |
| normal-c (web_search) | HTTP 404 (legitimate) | 3162ms | **No signature error** ✓ |
| normal-d (web_search) | **Full success** | 3021ms | **No signature error** ✓ |

#### Potential Secondary Concern
`RefundGuard::drop()` uses `tokio::task::block_in_place` + `blocking_lock()`. During panic unwinding, if the mutex was already poisoned (another panic in the same task tree), `blocking_lock()` will panic, causing a **double-panic abort**. This is a theoretical risk but was not triggered in testing. A `catch_unwind` wrapper around the refund logic would harden this.

### Verdict: **CRITICAL BUG FOUND AND FIXED**

---

## Phase 4: Lossless RAG Seeding — Live Validation

### Methodology
- Ran session-enabled agent (`volt agent-run --session-id`) to force conversation history
- Attempted cross-session context retrieval via `volt agent-run --session-id` with a question requiring prior context
- Queried PostgreSQL `context_entries` table directly

### Results

#### Finding 1: `seed_truncated_context()` is in-memory only

**File:** `src/context.rs:207-223`

The `ContextStore::add()` method pushes entries to an in-memory `Vec<StoredEntry>` but does **NOT** persist to PostgreSQL. Only `seed_batch()` (line 461) calls `insert_context_entry()` against the DB.

```rust
// add() — line 221 (NO DB persistence)
self.entries.write().await.push(StoredEntry { entry });

// seed_batch() — line 497 (DOES persist)
let _ = crate::db::insert_context_entry(db, &entry).await;
```

**Impact:** `Compressor::seed_truncated_context()` (loop_rs.rs:1026) and `seed_truncated_context_llm()` (loop_rs.rs:1052) both call `store.add()`. Truncated conversation history entries are **lost on process restart** and **NOT retrievable by the pgvector search path** (which queries `context_entries` table).

The in-memory-only `add()` path means:
- Truncated conversations survive only within a single agent session lifetime
- After restart: context_entries table = 0 rows (confirmed: `SELECT COUNT(*)` returned 0)
- RAG retrieval for cross-session memory relies solely on the in-memory store

#### Finding 2: No compression triggered in normal operation

With llama-3.1-8b-instant's 131K context budget, typical agent sessions (5-30 messages) never approach the `compress_if_needed()` threshold (`budget = 131072 - 2048 = 129024` tokens). `seed_truncated_context()` is a dead code path for most models with large context windows.

#### Finding 3: Cross-session memory retrieval failed

The second agent-run (same session ID) could not answer "What is the river I asked about?" — it defaulted to `web_search` for fresh data. The `Conversation` context kind was never seeded (0 rows in `context_entries`).

### Verdict: **MEDIUM — RAG persistence gap.** Truncated conversation storage is in-memory only, which violates the "lossless" claim for context after process restart. Fix: change `add()` to persist to DB, or have `seed_truncated_context()` use `seed_batch()`.

---

## Summary

| # | Finding | Severity | Status |
|---|---|---|---|
| 1 | Dead code: `push_message` (loop_rs.rs:820), `TelegramChannel::token` (telegram.rs:4) | Low | Unresolved — should clean up |
| 2 | SQLite + Postgres contention under 8-agent parallel load | None | PASS — no issues |
| 3 | **RefundGuard::drop() HMAC signature corruption** | **CRITICAL** | **FIXED** — re-sign on refund |
| 4 | `seed_truncated_context()` in-memory only, lost on restart | Medium | Unresolved — use `seed_batch()` |
| 5 | CLI JSON argument parsing breaks under PowerShell (no `--agents-file` originally) | Medium | **FIXED** — added `--agents-file` / `--tasks-file` |
| 6 | Panic in tokio task caught cleanly, sibling tasks not short-circuited | None | PASS |
| 7 | Zero `#[allow(deprecated)]` annotations, zero `execute()` call sites | None | PASS |
| 8 | `RefundGuard::drop()` double-panic risk (blocking_lock during unwind) | Low | Not exercised, theoretical |
