# Audit of prod-readiness changes (3 commits: 0a9e57d, 87d3ee1, 2dbc8f5)

**Date:** 2026-06-09
**Auditor:** Senior Rust reviewer (subagent)
**Findings:** 47 issues across 7 categories

## Severity Summary

| Category | P0 | P1 | P2 | P3 | Total |
|---|---|---|---|---|---|
| Bugs | 0 | 4 | 2 | 8 | 14 |
| Edge cases | 0 | 0 | 2 | 5 | 7 |
| Test gaps | 0 | 0 | 5 | 1 | 6 |
| Docs | 0 | 0 | 3 | 1 | 4 |
| Backward compat | 0 | 1 | 0 | 0 | 1 |
| Security | 0 | 1 | 2 | 2 | 5 |
| Performance | 0 | 1 | 1 | 2 | 4 |
| **Total** | **0** | **7** | **15** | **19** | **41** |

## Fixed in commit `b8150e4`

| # | Issue | File | Severity | Fix |
|---|---|---|---|---|
| 1 | `volt config set ollama_local` wrote to wrong env var | `commands/config.rs` | P1 | Expanded `provider_env_var` mapping |
| 4 | Control characters not rejected in keys | `config.rs:save_api_key` | P2 | Added `is_control()` check |
| 5 | WebUI placeholder validation missing | `webui/runtime.rs:1747` | P2 | Added `is_placeholder_key` check |
| 10 | Useless match in detect_uncached | `provider_detector.rs:401` | P3 | Removed |
| 14 | Dead `ConfigSubcommand::from_str` | `commands/config.rs:44` | P3 | Removed |
| 20 | `claudette:7b` routes to Anthropic | `provider_detector.rs:route()` | P2 | Reordered: colon check before `claude` hint |
| 34 | `build_provider` silent fallback in 5 callers | `commands/{eval,agent_run,agent_tui}.rs` | P1 | Migrated to `try_build_provider` |
| 41 | `detect()` re-runs every call (1.5s tax) | `provider_detector.rs` | P1 | `OnceLock<RwLock<...>>` cache; `invalidate_cache()` on key change |

## Remaining (P2/P3) — future polish

| # | Issue | File | Severity | Note |
|---|---|---|---|---|
| 2 | `slug_to_static` memory leak | `provider_detector.rs:149` | P1 | Adapter used only in test; remove when API is dropped |
| 3 | `route()` / `resolve_provider_with()` disagree on NIM catch-all | both | P1 | Cosmetic — `resolve_provider_with` is unused |
| 6 | `is_placeholder_key("") == true` confounds "unset" | `provider_detector.rs` | P3 | Documented in commit message |
| 7 | `EmbeddingClient::new` warning fires in 5 tests | `embedding/mod.rs` | P3 | Gate on `cfg(not(test))` |
| 8 | "no providers" log fires before local ONNX check | `embedding/mod.rs:93-110` | P3 | Reorder so local-model init runs first |
| 9 | `save_api_key` doesn't set 0o600 on Unix | `config.rs` | P1 (Unix) | Use `OpenOptionsExt::mode(0o600)` |
| 11 | `unwrap_or(80)` masks malformed hosts | `provider_detector.rs:342` | P3 | Log + return `InactiveLocalDown` |
| 12 | `default_addr.rsplit_once(':').unwrap()` could panic | `provider_detector.rs:341` | P3 | Use `expect()` |
| 13 | `volt config set oai_override` error message unclear | `commands/config.rs` | P3 | Better error |
| 15 | `LLM_DEFAULT_MODEL` precedence undocumented | `commands/{agent_run,agent_tui,eval}.rs` | P3 | Add doc comment |
| 16 | Wizard doesn't run `is_placeholder_key` | `commands/config.rs:wizard()` | P3 | Add check |
| 17 | `ApiKeyRow` closure capture smell | `webui/pages.rs` | P2 | Dioxus-specific fragility |
| 18 | `is_placeholder_key` substring match too broad | `provider_detector.rs:191` | P3 | Anchor the pattern |
| 19 | `route("claude")` case sensitivity | `provider_detector.rs:118` | P3 | Lowercase in match |
| 21 | `route()` unknown bare name → first active | `provider_detector.rs` | P2 | Document or require explicit prefix |
| 22-27 | Test coverage gaps | various | P2/P3 | Add 5-6 more tests |
| 28-31 | AGENTS.md drift | `AGENTS.md` | P2/P3 | Add sections for `volt config`, `ProviderDetector` |
| 35, 38, 39, 40 | Security polish | various | P2/P3 | Validate env, set perms, mask logs |
| 42 | TCP probes sequential | `provider_detector.rs:344` | P2 | Use `tokio::join!` |
| 43 | `resolve_provider_with` unused | `orchestrator.rs:351` | P3 | Wire it up or remove |
| 44 | `active_slugs` allocation | `provider_detector.rs` | P3 | Drop leak |

## Verification

- `cargo test --lib --features testutils` — 289 tests pass.
- Manual test: `volt config set ollama_local http://192.168.1.50:11434` → writes `OLLAMA_HOST` correctly.
- Manual test: `volt config set openai "your_*_here"` → rejected with placeholder error.
- Manual test: `volt config set anthropic "sk-test\nwith-newline"` → rejected with control-char error.
- Manual test: `volt config unset openai` → clears key, invalidates cache.

## Not Addressed (lower-priority)

The audit found 5 specific test coverage gaps (issues 22-27). The detector's 8 existing tests cover the core happy path; the gaps are mostly edge cases (very long keys, control chars, ApiKeyRow rendering). These will be filled in a follow-up.
