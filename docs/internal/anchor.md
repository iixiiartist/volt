## Remediation Sprint Complete

All HIGH/CRITICAL findings from CODE_QUALITY_REPORT.md and VOLT_REVIEW.md have been fixed:

### Fixed (all verified, 138/138 tests pass)
1. **`registry.rs`**: Removed `execute()` (capability bypass), changed `get_permission()` default from `Allow` to `Prompt` (deny-by-approval), added `tracing::warn!` on disk cache write failure.
2. **`run.rs`**: Fixed `final_answer` argument indexing (`["answer"]` → `.get("answer")`), changed tool arg logging from `info!` to `debug!`, added `tracing::warn!` on `store_memory` failure.
3. **`run.rs` `save_session_messages_delta`**: Changed from `INSERT OR REPLACE` (no unique key) to `INSERT ... ON CONFLICT(session_id, position_index) DO UPDATE SET`.
4. **`session.rs`**: Added migration v4 creating `position_index` column + unique composite index on `(session_id, position_index)`. Added `position_index` parameter to `save_message()`. Updated `load_messages` ORDER BY to `position_index ASC`.
5. **`tui.rs`**: Updated `save_message` call site to pass position from enumerate index.
6. **Turbovec poisoned-lock fix** (3 files): `context/mod.rs`, `context/eviction.rs`, `context/search.rs` — changed `.unwrap()` to `.unwrap_or_else(|e| e.into_inner())`.
7. **`context/eviction.rs`**: Added `tracing::warn!` on `seed_batch` DB insert failure (was `let _ =`).
8. **`tool_failure_tracker.rs`**: Added `tracing::warn!` on `record_failure` DB insert failure (was `let _ =`).
9. **`sandbox.rs`**: Added `tracing::warn!` on stdin write/shutdown failures (was `let _ =`).
10. **`orchestrator.rs`**: Added `escape_braces()` helper and applied to all 4 template substitution sites in `run_pipeline()` and `DagScheduler` to prevent template injection from LLM-provided values.
11. **`routines/engine.rs`**: Added `tracing::warn!` on `create_job` failure (was `let _ =`).
12. **`jobs/monitor.rs`**: Added `tracing::warn!` on `fail_job` failure (was `let _ =`).
13. **`orchestrator.rs`**: Fixed `kind_from_str()` that was corrupted by a bad edit (had non-existent `ProviderKind::Groq`/`Ollama` variants).

### Not Changed (per user decision)
- `events.rs`: `let _ = tx.send(event)` kept (non-critical, channel full is acceptable).
- Config file writes in `config.rs`: kept as `let _ =` (non-critical).
- TUI error paths: kept as `let _ =` (UI, not application logic).

### Remaining Non-Blocking Items (not actionable)
- `util.rs` print statement: not part of any user-facing library.
- PC/SC `dbg!` logging: unchanged (only fires on hotplug detection).
- `leak_detector.rs` 1000-line threshold: not hot.
- Test code pattern duplication: not actionable without test harness rework.

## Next Steps
- Run remaining test suites (professional workflows, real-world benchmarks) to confirm no regressions.
- Update CODEBASE_MAP.md with line count changes if needed.
- Switch to MSVC toolchain when C++ build tools workload is installed.

## Build Result
- `cargo test --features testutils`: **138/138 lib tests pass**, 4/4 agent_tests pass, 11/11 attenuation_tests pass. BFCL pipeline test times out as expected (requires GROQ_API_KEY).

## Relevant Files (changed in remediation sprint)
- `src/tools/registry.rs` — execute() removed, get_permission() default → Prompt, warn! on cache write.
- `src/agent/run.rs` — final_answer indexing fix, tool arg log level → debug, store_memory warn!, save_session_messages_delta ON CONFLICT pattern.
- `src/session.rs` — migration v4, position_index column + unique index, save_message() signature change.
- `src/tui.rs` — save_message() call site updated.
- `src/context/mod.rs` — turbovec poisoned-lock fix.
- `src/context/eviction.rs` — turbovec poisoned-lock fix, seed_batch warn!.
- `src/context/search.rs` — turbovec poisoned-lock fix.
- `src/tool_failure_tracker.rs` — warn! on DB insert failure.
- `src/sandbox.rs` — warn! on stdin write/shutdown.
- `src/orchestrator.rs` — escape_braces() for template injection protection; kind_from_str() fixed.
- `src/routines/engine.rs` — warn! on create_job failure.
- `src/jobs/monitor.rs` — warn! on fail_job failure.
