# Provider Defaults & Onboarding Audit

**Date:** 2026-06-09
**Scope:** Hardcoded providers, silent fallbacks, onboarding gaps

## Direct answers to the user's specific questions

- **Is there a settings UI in the WebUI?** Partially. `src/webui/pages.rs:582-778` (`SettingsPage`) has dropdowns for `provider` and a text field for `model`, but it only writes to the in-memory `WebuiConfig` (a `RwLock` in the runtime) — the API key is **not** editable in the settings form. The actual key-entry flow lives in a separate full-screen overlay: `src/webui/setup_wizard.rs` (first-run) or by clicking the "Run API Key Setup" button in Settings (line 695), which re-opens that same wizard.
- **Is there a CLI command to set keys?** **No.** `src/main.rs:17-246` enumerates 30+ subcommands — `Init`, `InitDb`, `Doctor`, `Update`, `Completion`, `Worktree`, `Jobs`, `Routines` — but **no `Commands::Config` / `volt config set`** variant. The only way to set a key from the CLI is to edit `.env` manually; `save_api_key()` in `src/config.rs:916-947` is invoked only by the WebUI handler at `src/webui/runtime.rs:1754`.
- **Is `.env` loaded at startup?** Yes, twice. `src/main.rs:396-404` calls `dotenvy::dotenv()` (and falls back to the binary directory), and `src/bin/webui.rs:14-19` walks CWD/binary-dir/ancestor Cargo.toml root loading `.env` files via `dotenvy::from_path`. `src/webui/runtime.rs:220` also calls `dotenvy::dotenv()`. There is even env-shadow detection at `src/main.rs:394, 401` and `src/config.rs:113-180`.
- **If `.env` is missing/incomplete, what happens?** The CLI prints a per-key warning at `src/main.rs:406-415` (skips to next command) — there is **no hard error**. If `DATABASE_URL` is missing, `Settings::from_env` (`src/config.rs:579-589`) hard-fails. If no LLM key is present, `Commands::AgentRun` and `volt agent` will call `build_provider` and either fail at the first HTTP call with a raw 401, or — worse — silently route to a hardcoded fallback (see Issue 3).

---

## Numbered findings (severity-tagged)

### 1. `src/main.rs:619` — Hardcoded default model for `volt agent run` — **P0**
`model: model_name.unwrap_or_else(|| "gemma4:e4b".into())`
When the user runs `volt agent run --input "..."` with no preset, no `--model` flag, and no `LLM_MODEL` env, the literal string `gemma4:e4b` is injected and routed to Ollama Cloud (`https://ollama.com/v1`) because of the colon-naming rule in `src/orchestrator.rs:342-362`. There is no error, no wizard, no warning — Volt will issue a real billable request to Ollama Cloud with no key configured, returning a 401.
**Fix:** Replace the fallback with a hard error: `anyhow::bail!("No LLM model specified. Pass --model, set LLM_MODEL, or use a preset.")`.

### 2. `src/orchestrator.rs:262-410` — Massive silent-fallback chain in `resolve_provider` — **P0**
The function tries 6 paths in order: `LLM_MODEL_ROUTES` JSON → Claude→Anthropic → GPT→OpenAI → NIM→NVIDIA → `LLM_BASE_URL` → colon-named→Ollama Cloud → NIM catch-all → Groq default. Each path's API key is fetched with `.unwrap_or_default()` (lines 297, 310, 320, 332, 343, 354, 367, 384, 400) and the function **never returns an error** — it always succeeds with some `ProviderRoute` even if `api_key == ""`.
A user with no keys, no `.env`, no config, and no `LLM_BASE_URL` will still get a `ProviderRoute` pointing at `https://api.groq.com/openai/v1` (line 398) with `api_key=""`. The OpenAIProvider then sends an `Authorization: Bearer ` header (line 37 of `src/llm/openai.rs`) and the user gets a generic 401. The fallback chain silently masks configuration errors.
**Fix:** When `api_key.is_empty()` for the resolved provider, return an `Err` variant (or change `ProviderRoute` to `Result`) with a message listing exactly which env var the user should set. Surface the error in `build_provider()` at line 747.

### 3. `src/orchestrator.rs:342-362` — Colon-named models route to Ollama Cloud without confirmation — **P0**
A model name like `llama3.2:3b` (a *local* Ollama tag, not an Ollama Cloud model) gets routed to `ollama.com/v1` if any OLLAMA key is set, even though the user intended their local `localhost:11434` server. This also conflicts with the discoverable-blueprint path in `src/agent/router.rs:185-205` which reads `~/.volt/blueprints/*.toml`. There is no UI exposing this routing decision.
**Fix:** Require `OLLAMA_CLOUD=1` to opt into cloud routing, or remove the colon heuristic entirely and rely on the blueprints router (which the user can see in `blueprints/`).

### 4. `src/orchestrator.rs:366-375` — NIM catch-all for any vendor-prefixed model — **P1**
Any model string containing `/` (e.g. `unknown-org/weird-model` from a typo) silently routes to NVIDIA NIM if `NVIDIA_API_KEY` is set. The user gets a 404 with no hint that the catch-all was responsible.
**Fix:** Log a `warn!` when triggering the catch-all, or refuse to route unknown vendor prefixes and surface a `ModelNotRecognized` error.

### 5. `src/embedding/providers.rs:124-193` — `auto_detect_providers` silently builds a working list — **P1**
Probes Ollama on `localhost:11434` (line 197) and pushes a `ProviderConfig` for every cloud provider whose key is in the environment, with hardcoded URLs. Reads `KIMI_API_KEY` even though deprecated.
**Fix:** Add a startup log line that lists every active embedding provider; require explicit opt-in for remote providers.

### 6. `src/embedding/mod.rs:30-37` — `EmbeddingClient::new` hardcodes NVIDIA NIM as the default — **P0**
Any caller that doesn't go through `new_smart()` gets NIM by default with no config check.
**Fix:** Remove this constructor or change the default to `EmbeddingProvider::Local` requiring explicit override. At minimum, make it `Result<Self, Error>`.

### 7. `src/agent/router.rs:38-45` — `get_active_providers` auto-adds local fallbacks when no remote key is present — **P1**
When no remote key is configured, the router *automatically advertises* Ollama/llama.cpp/LiteRT-LM to the LLM-driven blueprint selector as if they were available. The blueprint selector at line 124-182 picks one, then the build fails because no such server is actually running.
**Fix:** Return the actual active list (just `ollama` if `OLLAMA_HOST` is set, etc.). When none are configured, return an empty vec and have `route_task` return `None` with a clear error.

### 8. `src/agent/router.rs:73-85` — `route_task` falls back to the full blueprint list when filtering leaves 0 matches — **P1**
The router `tracing::warn!`'d that no blueprints match — then ignores the warning and offers the LLM the full list anyway.
**Fix:** Return `None` from `route_task` when `filtered.is_empty()`.

### 9. `src/commands/agent_run.rs:538-542` & `src/commands/agent_tui.rs:218-222` — Hardcoded fallback to `llama-3.1-8b-instant` — **P1**
Both `volt agent-run` and `volt agent-tui` silently default to Groq's `llama-3.1-8b-instant` when `LLM_MODEL` is unset.
**Fix:** Replace the `unwrap_or_else` with an `anyhow::bail!`. Unify default between the two entry points.

### 10. `src/config.rs:615-624` — Embedding model/endpoint default to NVIDIA NIM with no key check — **P1**
**Fix:** Default `embedding_provider` to `Local` or `None` when no remote key is set.

### 11. `src/webui/runtime.rs:72-83` — `WebuiConfig::default()` hardcodes Groq — **P2**
**Fix:** In `Default::default()`, return a state that signals "unconfigured".

### 12. `src/agent/router.rs:989-1019` — `handle_list_models` reports any model as `available: true` for the catch-all — **P2**
**Fix:** Default `has_key` to `false` for unknown slugs; default `supports_tools` to `false`.

### 13. `src/agent/router.rs:185-205` — `discover_blueprints` only checks CWD and `$HOME/.volt/blueprints` — **P2**
The 67 shipped blueprint files are invisible unless running from the repo root.
**Fix:** Add `volt_home().join("blueprints")` and a directory adjacent to the executable as discoverable paths. Or, ship blueprints via `include_str!`.

### 14. `src/main.rs:406-415` — Missing-key check only prints a warning, not an error — **P1**
Placeholder value (`starts_with("your_")`) only catches `your_…`. Pastes a real-looking prefix → accepted → 401.
**Fix:** Make the check a hard error for LLM-using subcommands.

### 15. `src/agent/run.rs:228-242` — Auth errors caught but message not friendly — **P2**
String-matched on substrings — different provider error formats may or may not contain "401".
**Fix:** Attach a hint based on the route.

### 16. `src/embedding/mod.rs:60-115` — `new_smart` returns OK even with zero configured providers — **P2**
Falls through to `deterministic_placeholder_embedding` → garbage vectors → silently degrades retrieval.
**Fix:** Log a `warn!` at startup with the list of configured providers.

### 17. `src/config.rs:585-589` — `DATABASE_URL` missing produces an error, but no link to wizard — **P2**
**Fix:** Replace with: `anyhow!("DATABASE_URL is not set. Run `volt setup` to configure...")`.

### 18. `src/config.rs:217-223` — `first_run_wizard` is a no-op in non-TTY environments — **P1**
Wizard is silently skipped on double-click from Windows Explorer, CI shell, or service. First-run experience is "API key not set" warning then 401.
**Fix:** For the GUI/WebUI binary, always show the setup wizard as a UI overlay. For the CLI, in non-TTY mode, write a minimal `.env` template.

### 19. `src/config.rs:498-512` — Wizard hardcodes `LLM_BASE_URL` only for non-default Ollama URLs — **P3**
A user who picks Ollama and accepts the default URL gets no entry in `.env`.
**Fix:** Always write `LLM_BASE_URL` when the user picks Ollama.

### 20. `src/tools/registration.rs` (not yet read, but implied) — Tools gated by env var *advertise themselves* in `volt list-tools` — **P2**
**Fix:** Add a "Available Tools" section to `volt doctor` that lists each tool's gating env var and current status.

### 21. `src/main.rs:412` — Placeholder detection uses only `starts_with("your_")` — **P2**
**Fix:** Maintain a list of common placeholder patterns.

### 22. `src/llm/provider.rs` (not read but referenced) — `LLM_HTTP_TIMEOUT` may be too short for first-run key validation — **P3**
**Fix:** After `save_api_key()`, perform a one-shot probe.

### 23. `src/agent/blueprint.rs` + `src/agent/router.rs:124-181` — Blueprint router uses `llama-3.1-8b-instant` to pick a blueprint — **P2**
**Fix:** Make the routing model configurable via `LLM_ROUTER_MODEL` env var.

### 24. `src/orchestrator.rs:600-602` — Supervisor synthesizer hardcodes `qwen/qwen3-32b` — **P1**
**Fix:** Add a `--supervisor-model` CLI flag and `LLM_SUPERVISOR_MODEL` env var.

### 25. `src/orchestrator.rs:731-744` — `parse_agent_specs` defaults `model` to `llama-3.1-8b-instant` per-spec — **P2**
Same pattern as Issue 9.

### 26. `src/commands/agent_run.rs:60` — Embedder is built eagerly with no error path — **P2**
**Fix:** At the end of `new_smart`, return `(EmbeddingClient, EmbedderDiagnostics)`.

### 27. `src/config.rs:505-509` — Wizard writes key with no validation — **P2**
**Fix:** After saving, do a `GET /models` HTTP probe to the provider.

### 28. `src/main.rs:489-501` — `volt agent run` mutates `model` from preset — **P3**
Subtle precedence issues.
**Fix:** Document the precedence or emit a notice.

### 29. `src/llm/openai.rs:34-39` — Empty `api_key` produces `Authorization: Bearer ` header — **P2**
**Fix:** When `api_key.is_empty()` for a cloud provider, log a startup warning and refuse to construct.

### 30. `src/llm/openai.rs:283-285` — `supported_models` returns `vec!["*".into()]` — **P3**
**Fix:** Wire up the 67 shipped blueprints to filter `supported_models` per-provider.

### 31. `src/webui/state.rs` (not read) + `src/webui/setup_wizard.rs:74-79` — `SubmitApiKey` is one-shot; no retry on provider-build failure — **P2**
**Fix:** Have `build_provider` return a `Result` and have `handle_submit_api_key` check it.

### 32. `src/orchestrator.rs:747-766` — `build_provider` swallows provider kind — **P3**
**Fix:** Use `provider_kind` to validate the model name.

### 33. `src/commands/agent_tui.rs:36` — TUI uses `provider_kind` from `build_provider` but does not validate — **P3**
Same as Issue 32.

### 34. `src/agent/router.rs:208-213` — `load_all_blueprints` silently returns empty on parse error — **P3**
**Fix:** Log the parse error and the path of the malformed file.

### 35. `src/llm/openai.rs:322-326` & `src/llm/anthropic.rs` (presumed) — 401 from provider surfaced as raw HTTP status — **P2**
**Fix:** Map `401`/`403` to a friendly `ProviderAuthError` enum.

---

## Summary

**P0 issues (must fix before production):**
- Issue 1: `gemma4:e4b` hardcoded in `volt agent run`
- Issue 2: `resolve_provider` silent-fallback chain in orchestrator
- Issue 3: Colon-named models silently route to Ollama Cloud
- Issue 6: `EmbeddingClient::new` hardcodes NIM as default

**P1 issues (should fix):** 12 items

**User-facing impact:** The 67 shipped blueprints and 5 hardcoded fallback providers create an illusion of a working system that fails opaquely. The WebUI's setup wizard is the only end-to-end functional onboarding path; the CLI's interactive wizard is TTY-gated. A user with a fresh install and no `.env` will see a warning, then a generic 401, with no clear "open settings and add a key" recovery.
