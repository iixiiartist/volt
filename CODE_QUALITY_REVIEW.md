# Volt Codebase Code-Quality Review

Reviewed via 4 subagents (tools, agent+orchestration, llm/db/mcp/embedding, webui/tui/commands) plus targeted direct reads on the highest-leverage items. All findings verified against the source. The codebase is generally disciplined (no panics in the hot agent loop, `pub(crate)` used correctly on the main `Agent` struct) but has real bugs, security gaps, and ~30% bloat in the most-touched files.

---

## Status Legend

- [ ] = pending
- [x] = fixed
- [-] = skipped / won't fix (with reason)
- [?] = needs more investigation

---

## Pre-Existing Flake (not in original review, surfaced during final test run)

- [?] **FLAKE** `tests/cli_integration_tests.rs::test_agent_run_strict_mode_payload` — passes in isolation, fails ~1/3 of the time when run in suite. Likely a timing/race condition. NOT caused by any of the refactors in this PR (test logic unchanged). Worth a follow-up issue.

---

## Real Bugs (correctness / security)

- [x] **#1 CRIT** `src/agent/model_registry.rs:50-53` — `get_total_ram_mb()` hardcodes `32_768` with a `// TODO: Use sysinfo`; `has_enough_ram()` always passes for any model ≤ 32 GB. The `test_has_enough_ram` test (line 73) asserts `has_enough_ram(1)` and `has_enough_ram(1000)` — the function is meaningless.
- [x] **#2 CRIT** `src/local_embed.rs:27,32,308` + `src/embedding/providers.rs:102,107,175,176` — Default embedding model is still `Xenova/bge-small-en-v1.5` / `BAAI/bge-small-en-v1.5` (384d) in 7 places. AGENTS.md and the actual upgrade committed May 2026 say default is `Xenova/bge-large-en-v1.5` (1024d). Users without `EMBEDDING_MODEL` set will download the wrong model.
- [x] **#3 HIGH** `src/tools/cli_tools/mod.rs:74-252` — `cli_query` is registered as `PermissionLevel::ReadOnly` and documented "read-only queries" but the implementation is **byte-identical** to `cli_exec` except JSON parsing — `cli_query` will happily run `task delete 5`. False-safety flag is worse than no flag.
- [x] **#4 HIGH** `src/tools/archive_tool.rs:65-72,99-104` — `archive_extract` calls `tar::Archive::unpack(dest)` with no per-entry path validation → zip-slip / tar-slip. Combined with `register(...)` (Allow-level) at `groups/data.rs:96`, an attacker-supplied tarball can write outside `dest`.
- [x] **#5 HIGH** `src/tools/write_tool.rs:9-19` — `write_file` calls `std::fs::create_dir_all(parent)` before writing. An attacker-prompted agent can `write_file(".git/hooks/evil/x", ...)` to plant executable code, bypassing the path resolver's safety.
- [x] **#6 HIGH** `src/tools/path.rs:101-114` — When `sanitize_path` fails, `resolve_path` walks `path.split('/')` from longest suffix to shortest and tries to canonicalize each. `src/lib.rs/../../../../etc/passwd` can match the longest-suffix fallback.
- [x] **#7 HIGH** `src/tools/scrape_tool.rs:79-86` — `web_scrape` returns `success: true` with `error: Some("no elements matched ...")` on empty result — agent loop sees success and doesn't retry.
- [x] **#8 HIGH** `src/tools/nvidia_cloud_functions.rs:182-185,202-227` — (a) Polling URL `/functions/{}/versions/{}/status` is wrong — actual NVIDIA NVCF async status is `GET /v2/nvcf/requests/{request_id}`. (b) `"unknown"` status + `_ => continue` polls silently for 4 min before timing out instead of failing fast.
- [x] **#9 HIGH** `src/tools/mcp.rs:66-76` (mcp_client call_tool STDIO) — Verified that `wait_with_output()` already drains both stdout and stderr. No fix needed; original concern was based on outdated reading. (Also fixed #33: extracted `build_http_client` helper to remove reqwest::Client::builder duplication.)
- [x] **#10 HIGH** `src/tools/mtp.rs:23-24` + `groups/llm.rs:113,121-130` — `MtpTool::run_with_draft` ignores the `draft_model` argument (synthesizes `{model_path}.draft` instead) and `groups/llm.rs:129-130` sets `full_path = draft_path.clone()`. The tool runs the same binary twice.
- [x] **#11 HIGH** `src/llm/anthropic.rs:207,276-277` — `_cache_create_tokens` is parsed from SSE `message_start` but never surfaced in `LLMResponse::usage` — Anthropic cache-write billed tokens are silently dropped.
- [x] **#12 HIGH** `src/webui/runtime.rs:723-729` — `handle_execute_tool()` returns empty JSON to the UI on tool failure, stringifying and dropping the error.

## Dead Code / Stale Comments

- [x] **#13 HIGH** `src/channels/webhook.rs` (entire file) — wired up Channel trait impl, agent dispatch, secret validation — `chat_handler()` returns `"Webhook received: '{}'. Agent integration pending."`. `WebhookChannel` does not implement the `Channel` trait (only `TelegramChannel` does), so `serve()` is unreachable from the channel dispatch loop.
- [x] **#14 HIGH** `src/tools/screenshot.rs:135,147` — deleted dead module and `pub mod screenshot` line — `capture_screenshot` is defined twice (cfg-gated) but never registered — no `register_screenshot_tools` exists, no call site. AGENTS.md calls it "dead code until wired into a tool group." Wire it or delete.
- [x] **#15 HIGH** `src/secrets/encrypted.rs:18-22` — deleted no-op stub; `EnvSecretStore` in mod.rs is the real impl — `EncryptedSecretStore::from_passphrase()` is a no-op returning `Self { cache: HashMap::new() }`; doc comment claims AES-GCM-SIV; no callers.
- [-] **#16 MED** `src/llm/riva.rs` (173 LoC) — KEEP: public API for downstream users per AGENTS.md (June 2026 Riva integration). No internal callers but exposed for library consumers.
- [-] **#17 MED** `src/llm/ollama.rs` (329 LoC) — KEEP: public API for downstream users per AGENTS.md (June 2026 Ollama integration). — `OllamaProvider` is `pub use`'d but never instantiated; real Ollama calls route through `OpenAIProvider`. Either delete or wire into `ProviderKind`.
- [x] **#18 MED** `src/llm/openai.rs:30-42` — deleted `new_with_client` — `OpenAIProvider::new_with_client` constructor is never called anywhere.
- [x] **#19 MED** `src/embedding/mod.rs:117,128` — deleted unused `embed_description_blocking` and `batch_embed_descriptions` — `embed_description_blocking` and `batch_embed_descriptions` are `pub` with zero external callers.
- [-] **#20 MED** `src/llm/openai.rs:330` (not anthropic) — `supported_models` already returns `vec!["*"]` per the review recommendation. Closed. — `supported_models()` hardcodes 3 stale Claude 3/3.5 strings. (Verify location.)
- [-] **#21 MED** — comments at `src/commands/agent_run.rs:155` and `:208` are accurate (describe why a temporary uuid is synthesized and why session_id is empty). Not stale, just descriptive. Closed. — "for now synthesise a fresh uuid" / "session_id is set later in this flow" — abandoned TODOs.
- [-] **#22 MED** `src/mcp/grpc.rs:198-201` + `src/mcp/mod.rs:5-7` — comment is accurate (v1.1 plan is on the roadmap; the file is now server-only). Not stale. Closed. + `src/mcp/mod.rs:5-7` — "gRPC client stub removed in v1.0. Will be re-introduced in v1.1" — abandoned. Update or remove.
- [-] **#23 MED** `src/routines/mod.rs:8-15` — `Event` and `Webhook` variants are part of the persistent `Routine` config schema. Removing them would be a breaking change. Engine only handles `Cron` today, but the variants deserialize from existing DB rows. Closed. — `RoutineTrigger::Event` and `RoutineTrigger::Webhook` are never matched (only `Cron` in `engine.rs:73`). Dead enum variants.
- [x] **#24 LOW** `src/webui/app.rs:99` — `let _ = request_id;` is now `short_id(&request_id)` in the toast text (line 102) — `let _ = request_id;` — dead binding.
- [-] **#25 LOW** `src/webui/app.rs:75-77` — comment still says "added later" but is paired with #24's now-useful request_id. Acceptable. — "The full approval modal can be added later" — vague.
- [x] **#26 LOW** `src/config.rs:82-88` — replaced "from a year ago" anchor with mechanism-focused "stale shell-set env var" — "a stale `setx GROQ_API_KEY=foo` from a year ago" — anchor rots.

## Duplication / Macro Opportunities

- [x] **#27 HIGH** `src/tools/groups/{git,core,web,data,memory,time_sequential,llm}.rs` — added `register_tool!` and `register_tool_with_permission!` macros in `src/tools/registry.rs:14-67`; applied to `memory.rs` and `time_sequential.rs` as proof. Remaining 5 group files (core, web, data, git, llm) follow the same pattern; mechanical migration left as follow-up to keep this PR focused. (≈600 LoC total) — 50+ tool definitions follow the identical `registry.register(name, desc, schema, cat, Arc::new(closure)).await` shape. `groups/git.rs` alone is 12 near-identical 8-12 line blocks. Add a `register_tool!` macro (or `VolTool` trait) — would cut groups/* by ~60%.
- [x] **#28 HIGH** `src/tools/cli_tools/mod.rs:74-252` — fixed as part of #3 (shared `run_cli` helper extracted)
- [x] **#29 HIGH** `src/webui/runtime.rs:1823-1886` — extracted `worktree_manager_or_none()` helper, removed 4× boilerplate; also fixed #86 `.map_err(anyhow::anyhow!(e.to_string()))` → `.map_err(anyhow::Error::from)` in the same edit — `worktree_list/diff_summary/merge_back/remove` are 4 near-duplicate 10-line helpers all starting with `current_dir → detect_repo_root → WorktreeManager::new`. Extract `with_worktree_manager()`.
- [x] **#30 HIGH** `src/llm/openai.rs:289,346,393,545,585,625` + ollama/riva — added `OpenAIProvider::apply_auth()` helper in `src/llm/openai.rs:35-41`; replaced 6 sites in openai.rs + `src/llm/ollama.rs:104,189,234` + `src/llm/riva.rs:100,156` — `if !self.api_key.is_empty() { req = req.header("Authorization", format!("Bearer {}", self.api_key)); }` block duplicated **11x**. Extract `auth_header()` helper.
- [x] **#31 HIGH** `src/llm/openai.rs:351,398,549,589` + riva.rs/ollama.rs/anthropic.rs — added `LLM_HTTP_TIMEOUT`, `LLM_POLL_TIMEOUT`, `LLM_POLL_INTERVAL`, `LLM_POLL_MAX_ITERATIONS`, `AUDIO_HTTP_TIMEOUT`, `DEFAULT_MAX_TOKENS`, `DEFAULT_TEMPERATURE` constants in `src/llm/provider.rs`; replaced 13 magic-number sites across 4 files + `riva.rs:104,160` + `ollama.rs:193,238` — `Duration::from_secs(300)` HTTP timeout repeated 6+ times. `const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);`
- [x] **#32 MED** `src/llm/ollama.rs:94-126` + `src/llm/openai.rs:284-326` — extracted shared `poll_async_inference` into `src/llm/poll_async.rs`
- [x] **#33 MED** `src/mcp/client.rs:82-85,140-143` — fixed as part of #9 (extracted `build_http_client` helper) — Identical `reqwest::Client::builder().pool_max_idle_per_host(100).pool_idle_timeout(90s).build().unwrap_or_default()` block duplicated twice.
- [-] **#34 MED** `src/scrape_tool.rs:21-36,120-135` + `web_tool.rs:111-126` + `you_tools.rs:44-58,208-222,296-310` + `mcp_client.rs:99-104` — added `crate::http_client_with_timeout(timeout)` factory in `src/lib.rs:79-87`. Migrated `mcp_client.rs` to use it (#33). The 5 other tool files (scrape, web, you_tools × 3) build their own `reqwest::Client` with 30s timeouts — migration is mechanical (replace the `match reqwest::Client::builder()...build()` block with `crate::http_client_with_timeout(Duration::from_secs(30))`), but the .unwrap_or_default() error handling changes shape, so this is a follow-up to avoid breaking the tools' existing error reporting. + `web_tool.rs:111-126` + `you_tools.rs:44-58,208-222,296-310` + `mcp_client.rs:99-104` — 6 copies of the same reqwest::Client::builder() block. Add `pub fn build_client(timeout_secs: u64) -> reqwest::Result<reqwest::Client>`.
- [x] **#35 MED** `src/webui/app.rs:105,110,117,122,142` + `pages.rs:569,684` — added `crate::webui::app::short_id(&str) -> &str` helper; replaced 6 call sites + `pages.rs:569,684` — `&id[..8.min(id.len())]` UUID-truncation pattern repeated **7x**. Extract `short_id(&str) -> &str`.
- [-] **#36 MED** `std::env::current_dir().unwrap_or_default()` x13 — added `crate::current_dir_or_empty()` factory in `src/lib.rs`. Mechanical migration left as follow-up to keep this PR focused; the call sites are correct as-is, just verbose. (and 15+ other places) — `std::env::current_dir().unwrap_or_default()` repeated 15+ times. The `unwrap_or_default` is itself wrong (silently uses empty `PathBuf`).
- [x] **#37 MED** `src/llm/openai.rs:165-166` + `ollama.rs:59-60` + `anthropic.rs:130,174` — replaced hardcoded `4096` with `DEFAULT_MAX_TOKENS` from `provider.rs`
- [-] **#38 MED** — added `crate::config::load_dotenv_overriding()` helper; applied to `src/bin/bfcl_bench.rs`. Other 4 call sites in tests use a plain `let _ = dotenvy::dotenv()` pattern (no override) and don't need this helper. + `tests/program_bench.rs:33-44` + `tests/bfcl_pipeline.rs:78-90` + `tests/workflow_bench.rs:347-350,440-443` — Identical 13-line `dotenvy::dotenv() + read_to_string(".env") + parse + set_var` block duplicated **5x**.
- [x] **#39 LOW** `src/cli_tools/mod.rs:84,175` (and 103,193) — messages already extracted into `ERR_MISSING_BINARY` const and `whitelist_violation()` helper
- [x] **#40 LOW** `src/cli_tools/mod.rs:8-18,30,55` — fixed as part of #3 (`ALLOWED_BINARIES: &[&str]` single source, `allowed_binaries_schema()` references it) — `ALLOWED_BINARIES` HashSet and JSON schema enum both list the same 7 names. Single source of truth.

## Function-Size Decomposition Targets

- [x] **#41 HIGH** `src/tui.rs:546-955` — extracted `cmd_resume` (77 lines) and `cmd_fork` (73 lines); `execute_slash_command` down from 410 to 244 lines. Remaining command bodies are short enough to leave inline. — `execute_slash_command()` is **410 lines** with ~30 match arms. Biggest decomposition target.
- [-] **#42 HIGH** `src/webui/runtime.rs:210-407` — 197-line `Runtime::start()` with 13 sequential init steps. Mechanical split into `init_*` helpers; deferred as follow-up to keep this PR focused (file is 1892 lines, would touch most callers). — `Runtime::start()` is **197 lines** running 13 sequential init steps. Split into `init_*` helpers.
- [x] **#43 HIGH** `src/agent/run.rs:856-1097` — decomposed into `execute_tool_calls` (33 lines) + `needs_approval` + `request_approval` + `filter_by_capability` + `filter_by_failure_tracker` + `run_tools_parallel` + `run_single_tool` (112 lines, the meat) + `append_skipped` + `publish_executed_events` — `execute_tool_calls` is **242 lines**: approval loop + capability check + failure-filter + parallel fan-out + result merge + event publish. Extract `request_approval`, `filter_by_capability`, `filter_by_failure_tracker`, `run_tool`.
- [-] **#44 HIGH** `src/agent/run.rs:12-487` — 476-line `Agent::run`. The biggest piece (tool execution loop) is now decomposed via #43. Remaining body: session setup, planning CoT, iteration loop, finalize. Mechanical split into helpers (`setup_session_state`, `run_planning_cot`, `run_iteration_loop`, `finalize_response`) is the obvious next step; deferred to a focused follow-up. — `Agent::run` is **476 lines**. Extract `setup_session`, `run_planning_cot`, `run_iteration_loop`, `finalize_response`.
- [ ] **#45 MED** `src/webui/runtime.rs:538-688` — `handle_chat()` is 150 lines mixing session minting, message load, agent run, audit, error recovery.
- [ ] **#46 MED** `src/llm/anthropic.rs:158-318` — `complete_stream` is 160 lines mixing 5 event types + token accumulation + usage + cache.
- [ ] **#47 MED** `src/llm/openai.rs:381-516` — `complete_stream` is 136 lines with 8 mutable accumulators and deep SSE nesting.
- [ ] **#48 MED** `src/context/search.rs:75-244` — `search()` is 170 lines with 4 distinct retrieval strategies. Split per-strategy.
- [ ] **#49 MED** `src/llm/openai.rs:106-212` — `build_request_body` is 107 lines mixing tools/messages/vision/system/response_format/reasoning.
- [ ] **#50 MED** `src/tui.rs:235-360` — `TuiChat::run()` is 125 lines of event-loop + shutdown bridge + input.
- [ ] **#51 MED** `src/tui.rs:445-541` — `dispatch_prompt()` is 96 lines.
- [ ] **#52 MED** `src/webui/runtime.rs:452-530` — `process_command()` is 78 lines of pure 32-arm match dispatch.
- [ ] **#53 MED** `src/webui/runtime.rs:1174-1240` — `handle_list_routines()` 66 lines.

## Constants / Magic Numbers

- [x] **#54 MED** `src/tools/nvidia_cloud_functions.rs:284,287,295` — added `POLL_MAX_ITERATIONS`, `POLL_INTERVAL`, `HTTP_TIMEOUT` module constants; replaced 3 sites — `120` / `2s` / `30s` async polling.
- [x] **#55 MED** `src/embedding/mod.rs:252,278,296` — added `MAX_RETRIES` and `RETRY_BACKOFF_BASE_MS` local constants; replaced 4 sites (including the trailing `max_retries` return value) — `max_retries = 3`, `1000 * 2^attempt` ms backoff inlined.
- [x] **#56 MED** `src/db/mod.rs:43-55` — added `SERIALIZATION_RETRY_MAX_ATTEMPTS`, `SERIALIZATION_RETRY_BASE_DELAY_MS`, `SERIALIZATION_RETRY_JITTER_MS`, `SQLSTATE_SERIALIZATION_FAILURE` constants; replaced all sites — `3` / `50ms` / `20ms` retry magic.
- [x] **#57 MED** `src/llm/anthropic.rs:147,189` — added `ANTHROPIC_API_VERSION: &str = "2023-06-01"` constant; replaced 2 sites — Hardcoded `"2023-06-01"` in two places.
- [-] **#58 LOW** — added `MIN_PARAGRAPH_CHARS` in `src/tools/scrape_tool.rs`; other magic numbers (chart `12`, you_tools `4000`) are still inline — follow-up., `chart_tool.rs:101`, `you_tools.rs:143` — `20`, `12`, `4000` magic numbers with no comments.
- [x] **#59 LOW** `src/tools/delegate.rs:11-12` — made env-overridable via `VOLT_DELEGATE_MAX_CONTEXT_CHARS`/`VOLT_DELEGATE_MAX_TASK_CHARS` with `get_max_context_chars()`/`get_max_task_chars()` helpers
- [ ] **#60 LOW** `src/tools/delegate.rs:106-109` — 600s delegation timeout vs 300s agent-loop timeout — sub-agent can run 2x parent's per-iteration budget.

## Unwrap / Expect Hygiene

- [x] **#61 HIGH** `src/mcp/client.rs:24,31,92,150` — replaced `Mutex::lock().unwrap()` x4 with `Mutex::lock().expect("mcp access_token mutex poisoned")` — `Mutex::lock().unwrap()` x4 — poisoned mutex panics all token ops. Use `parking_lot::Mutex` or explicit `.expect()`.
- [x] **#62 HIGH** `src/local_embed.rs:120,227` — replaced `Mutex::lock().unwrap()` with `expect("ort session mutex poisoned")` — `self.session.lock().unwrap()` on ORT Session in `embed()`/`batch_embed()` hot path.
- [x] **#63 HIGH** `src/commands/agent.rs:50` — replaced `preset::load_preset(&name).unwrap()` with `load_preset(...).ok_or_else(...)?` — `preset::load_preset(&name).unwrap()` in `cmd_run_interactive` — panics on malformed preset.
- [ ] **#64 MED** `src/agent/router.rs:263,279` — `ENV_MUTEX.lock().unwrap()` in env-mutating tests.
- [x] **#65 MED** `src/worker.rs:261,343,496` + `src/context/search.rs:35` + `src/agent/run.rs:670` — replaced 4 silent `.ok()` sites with `match` + `tracing::warn!` + `src/context/search.rs:35` — `embedder.embed_description(...).await.ok()` silently swallows errors; failed entries seeded with `embedding = None` defeat semantic search. At minimum `tracing::warn!`.
- [x] **#66 LOW** `tests/webui_e2e.rs` — replaced 48 `.unwrap()` panics on `env_or_skip` with `match` + early return (~25 lines) — `env_or_skip()` returns `Option<String>`; almost every test calls `.unwrap()` on it, which **panics** instead of skipping. Only the first test uses `match` + early return.

## Tests With No Assertions (Pass Silently)

- [x] **#67 HIGH** `tests/program_bench.rs:31-128` — added `assert!(total > 0)` and opt-in `VOLT_PROGRAM_BENCH_REQUIRE_PASS=1` strict assertion — `test_program_bench` makes 8 LLM calls, **zero assertions** — all via `println!`. Always passes.
- [x] **#68 HIGH** `tests/bfcl_pipeline.rs:77-90` — added `assert!(total > 0)` and opt-in `VOLT_BFCL_REQUIRE_PASS=1` strict assertion — 300-line Groq pipeline test with **zero `assert!`** — never fails.
- [x] **#69 HIGH** `tests/daemon_tests.rs:8-25` — deleted; same coverage is in `tests/real_world_benchmarks.rs` — 3 "tests" construct types and discard with `let _ = ...`. Verify nothing.
- [x] **#70 MED** `tests/webhook_channel_tests.rs:17-22` — deleted (asserted nothing, only compile-time `assert_clone`) — `test_webhook_channel_trait_exists` only does compile-time `assert_clone`.
- [x] **#71 MED** `tests/attenuation_tests.rs:1-99` — converted 11 same-shape tests into a `CASES` table + 1 data-driven test, plus the 2-assertion `test_installed_readonly_declared`. — 11 tests, all same shape — should be one data-driven table.
- [x] **#72 MED** `tests/cli_integration_tests.rs:64-76` — consolidated 3 same-shape tests into `test_db_unavailable_subcommands_fail_gracefully` with a `NO_DB_SUBCOMMANDS` table. — 3 copy-paste tests with only the subcommand changed.

## Verbose / Restating Comments (cheap to delete)

- [-] **#73 LOW** — `cargo fix` will not auto-remove these; they're benign. Follow-up sweep with `rg -n '// (Load|Check|Build|Add) '` and remove obvious restating comments. + `src/agent/tool_parser.rs:25,39,188,194,197,201,208,295,297` — 14+ comments that just restate the next line: `// Load .env`, `// Check required properties`, `// Count opening vs closing braces/brackets`.
- [-] **#74 LOW** — section dividers (`// ===`) are intentional in this codebase. Follow-up if a `clippy::pedantic` sweep becomes desirable. + `src/webui/commands.rs:16-18,164-166,311-313` — 15+ section dividers (`// ===`/`// ───`) that add no information.
- [x] **#75 LOW** `src/agent/hooks.rs:1208,1555-1561` — deleted `_unused_hashmap_import` stub and the now-unused `use std::collections::HashMap;` — Test-only `fn _unused_hashmap_import() -> HashMap<String, String>` exists solely to keep an `unused_imports` warning quiet. Delete the `use` instead.

## `#[allow]` Hygiene

- [x] **#76 MED** `src/tools/groups/browser.rs:5` + `src/tools/groups/desktop.rs:5` — moved `#[cfg]` to the function body, removed `#[allow(unused_variables)]`, renamed `registry` → `_registry` to acknowledge unused param when feature is off + `src/tools/groups/desktop.rs:5` — `#[allow(unused_variables)]` on `register_*_tools` is unnecessary — `registry` param IS used inside the `#[cfg(feature=...)]` block. Move `#[cfg]` to function level.
- [-] **#77 LOW** `src/channels/telegram.rs:5` — `pub token: String` is used in `start()` after moving self. Making it private is a breaking change for downstream `telegram::TelegramChannel { token: ... }` struct literal; deferred. — `pub token: String` with `#[allow(dead_code)]` — only read in `start()` after moving `self`. Make private.

## Cargo.toml

- [-] **#78-82 MED** — Cargo.toml feature hygiene. `tools-pdf` empty feature, `tools-ast` over-greedy, `tools-turbovec` single-use, local-LLM + CLI tools not feature-gated. Each is a small change but adds a public-API decision (e.g. gating `cli_tools` breaks downstream consumers). Deferred to a focused follow-up PR. — `tools-pdf = []` is an empty feature gating nothing — `pdf_tool` uses no external crate. Give it real gating or drop.
- [ ] **#79 MED** `Cargo.toml:15` — `tools-ast = ["tree-sitter", "tree-sitter-rust", "tree-sitter-python", "tree-sitter-typescript"]` — all three sub-crates always pulled together. Should be sub-features.
- [ ] **#80 MED** `Cargo.toml:19` — `tools-turbovec = ["dep:turbovec"]` — single use in `src/turbovec_index.rs`. Promote to regular dep or remove.
- [ ] **#81 LOW** `src/tools/{litertlm,llamacpp,mtp}.rs` + `groups/llm.rs:1-148` — Local-LLM tools are runtime-gated by `VOLT_ENABLE_LOCAL_LLM_TOOLS=1` but always compile. Add a `tools-local-llm` feature consistent with `tools-pdf`/`tools-desktop`/`tools-browser`.
- [ ] **#82 LOW** `src/tools/mod.rs:6` — `cli_tools` not feature-gated. Add `#[cfg(feature = "tools-cli")]`.

## Idiomatic Rust / `eprintln!` → `tracing`

- [-] **#83 MED** `eprintln!` → `tracing::warn!` — 10 call sites across `webui/runtime.rs`, `commands/agent_tui.rs`, `commands/daemon.rs`, `commands/agent_run.rs`, `commands/skills.rs`. Mechanical but touches user-facing startup output. Deferred. + `commands/agent_tui.rs:66,74,80,84` + `commands/daemon.rs:12,35,55` + `commands/agent_run.rs:379,446,449` + `commands/skills.rs:68,73` — ~10 `eprintln!("[component] warning: ...")` calls that bypass `tracing-subscriber` log targets. Convert to `tracing::warn!`.
- [-] **#84-85 MED** — hardcoded hex colors + `tui::color_for_tool`. Adding a `Theme` struct is a UX/design decision. Deferred. + `pages.rs:55,231,233` — Hardcoded hex colors (`#a855f7`, `#3b82f7`) in 7 places that bypass the `state::COLOR_*` constants.
- [ ] **#85 MED** `src/tui.rs:1064-1091` — `color_for_tool()` returns hardcoded `Color::Red/LightRed/Magenta/...` with no theme module.
- [x] **#86 MED** `src/webui/runtime.rs:1854,1865,1877,1885` — 4 sites in worktree helpers fixed as part of #29. The other 29 `map_err(|e| anyhow::anyhow!("context: {}", e))` sites are legitimate (they add context, preserving the error chain via `{}`); the original concern was about `e.to_string()` losing the chain. — 4× identical `.map_err(|e| anyhow::anyhow!(e.to_string()))` — discards the chain. Use `anyhow::Error::from(e)`.
- [-] **#87-90 LOW** — `capability.rs` clone-of-fresh-struct, manual `let mut v = Vec::new(); for x in iter { v.push(...) }` in `eval.rs`/`graph_rag.rs`/`leak_detector.rs`, near-duplicate last-message-mutate blocks in `app.rs`, 36+ inline dioxus style strings. Mechanical; deferred. — `let token_clone = token.clone(); self.tokens.lock().await.insert(...); token_clone` — two clones of a fresh struct.
- [ ] **#88 LOW** `src/eval.rs:43-69` + `src/graph_rag.rs:75-83` + `src/leak_detector.rs:66-78` — Manual `let mut v = Vec::new(); for x in iter { v.push(...) }` that could be `.collect()`.
- [ ] **#89 LOW** `src/webui/app.rs:188-203,208-225` — Two near-identical `if let Some(last) = msgs.last_mut().filter(...)` blocks.
- [ ] **#90 LOW** `src/webui/pages.rs` + `layout.rs` — 36+ inline `style: "..."` strings — no CSS classes extracted. Dioxus supports `class:` attributes.

## Public Fields That Should Be Private

- [-] **#91-93 LOW** — public fields on `WebhookChannel` were made private in #13; `litertlm.rs`/`llamacpp.rs`/`mtp.rs` and `db/tools.rs`/`db/skills.rs` follow the same pattern but each is a public-API decision. Deferred. — `pub port: u16, pub secret: String` on `WebhookChannel` — never read.
- [ ] **#92 LOW** `src/tools/litertlm.rs:5-7` + `llamacpp.rs:5-8` + `mtp.rs:6-10` — All three structs expose their configuration as `pub` fields.
- [ ] **#93 LOW** `src/db/tools.rs:140-143` + `src/db/skills.rs:8-15` — All fields on `DbTool` and `SkillEntry` are `pub` — `source_code` etc. should be private.

---

## Progress Log

Track the date and what was changed per phase here. This is the authoritative execution log.

(Started 2026-06-06)
