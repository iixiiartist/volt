# Tool Gating in Volt

Volt registers tools in **six phases**, each with progressively stricter gating. This document explains why tools are gated, which tools are gated by what, and how to enable them.

---

## 1. What is Tool Gating?

**Tool gating** is the practice of conditionally registering agent tools based on runtime configuration, API keys, feature flags, or binary availability.

### Why Gate Tools?

1. **Prevent broken tools from being called**
   - Local inference binaries (`litertlm`, `llamacpp`, `mtp`) are often not installed or compiled for the host platform. Without gating, the model may attempt to call them and fail with opaque errors.
   - Enterprise CLIs (`task`, `hledger`, `khal`) are not present on most machines. Exposing `cli_exec`/`cli_query` universally causes the model to hallucinate CLI commands for simple text questions.

2. **Reduce tool noise for simple queries**
   - A user asking "What is the capital of France?" does not need 40 tools in the prompt. Gating shrinks the tool registry to only what is relevant and functional, improving:
     - **Latency**: fewer tools → smaller system prompt → faster tokenization
     - **Accuracy**: fewer distractors → better tool selection (validated on BFCL: +4.8pp with 74% token savings)
     - **Cost**: shorter prompts → fewer tokens billed

3. **Security and sandboxing**
   - `bash` and `write` are powerful. Hiding them behind opt-in flags prevents accidental data loss or code execution in untrusted environments.

4. **Benchmark reproducibility**
   - `VOLT_BFCL_MODE=1` hides built-in tools like `bash` and `web_search` so they do not interfere with BFCL-provided function stubs, ensuring clean benchmark scores.

---

## 2. The Tool Registration System

Tools are registered during Volt startup in groups, each with its own gating condition.

### Core (Always Registered, with caveats)

These tools compile in by default but some can be hidden at runtime:

- `read`, `write`, `edit`, `bash`, `glob`, `grep` — always available (bash hidden by `VOLT_BFCL_MODE=1`)
- `web_fetch` — always available (with optional `selector` param)
- `web_search`, `you_research`, `you_contents` — require `YOUCOM_API_KEY`
- `sleep_until` — always available
- `git_query`, `git_mutate` — always available
- `delegate`, `run_workflow` — always available
- `csv_read`, `csv_write`, `archive_extract`, `archive_create` — always available
- `create_bar_chart`, `create_line_chart`, `create_pdf` — hidden by `VOLT_MINIMAL_TOOLS=1`
- `desktop_*`, `browser_*` — gated by feature flags + hidden by `VOLT_MINIMAL_TOOLS=1`

**Note**: Several tools historically listed as "core" were deleted (no more `final_answer`, `sequentialthinking`, `get_current_time`, `memory_append`, `todo_add`, `json_query`, `json_validate`, `json_prettify`, `web_scrape`, `web_scrape_all`, `screenshot`).

### Phase 2: Dynamic (Delegate, Workflow)

These tools are injected at runtime based on the active agent configuration:

- `delegate_to_agent` — available when subagent blueprints are loaded
- `run_dag_workflow` — available when DAG orchestration is initialized
- MCP tools from configured MCP servers (`register_searchhq_tools()`, etc.)

### Phase 3: Feature-Gated

Compiled behind Cargo feature flags and additionally gated by `VOLT_MINIMAL_TOOLS=1`:

| Feature Flag | Tools |
|--------------|-------|
| `tools-screenshot` | `screenshot` |
| `tools-pdf` | `create_pdf` |
| `tools-desktop` | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` |
| `tools-browser` | `browser_navigate`, `browser_extract`, `browser_screenshot` |

When `VOLT_MINIMAL_TOOLS=1` is set, these tools are hidden from the model even if their feature flag is enabled.

### Phase 4: NVIDIA Cloud Functions

Gated by `NVIDIA_API_KEY` or `NVCF_API_KEY`:

- `nvidia_list_functions`
- `nvidia_call_function`
- `nvidia_deploy_function`

These tools call the NVIDIA Cloud Functions API (`api.nvcf.nvidia.com`) and require a valid NVIDIA API key.

### Phase 5: Ollama Cloud

Gated by `OLLAMA_API_KEY`:

- `ollama_web_search`
- `ollama_web_fetch`

These tools use Ollama Cloud's built-in web search and fetch APIs.

### Phase 6: CLI Gateway

Gated by `VOLT_ENABLE_CLI_TOOLS=1`:

- `cli_exec`
- `cli_query`

These tools spawn whitelisted enterprise CLIs (`task`, `crm`, `hledger`, `khal`, `vdirsyncer`, `qsv`, `himalaya`). They are **never** registered by default.

---

## 3. Complete Tool Gating Matrix

| Tool | Category | Gated By | Default | Why Gated |
|------|----------|----------|---------|-----------|
| `read`, `write`, `edit`, `glob`, `grep` | Core | Always | ON | — |
| `bash` | Core | `VOLT_BFCL_MODE=1` hides | ON | Benchmark mode |
| `web_fetch` | Web | Always | ON | — |
| `web_search`, `you_research`, `you_contents` | Web | `YOUCOM_API_KEY` | OFF | Requires you.com API key |
| `sleep_until` | Core | Always | ON | — |
| `git_query`, `git_mutate` | Git | Always | ON | — |
| `delegate`, `run_workflow` | Orchestration | Always | ON | — |
| `csv_read`, `csv_write`, `archive_extract`, `archive_create` | Data | Always | ON | — |
| `create_bar_chart`, `create_line_chart` | Data | `VOLT_MINIMAL_TOOLS=1` hides | ON | Optional visualization |
| `create_pdf` | Data | `VOLT_MINIMAL_TOOLS=1` hides | ON | Optional PDF generation |
| `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` | Desktop | `VOLT_MINIMAL_TOOLS=1` hides + `tools-desktop` feature | ON | Requires desktop env |
| `browser_navigate`, `browser_extract`, `browser_screenshot` | Browser | `VOLT_MINIMAL_TOOLS=1` hides + `tools-browser` feature | ON | Requires Chrome |
| `litertlm`, `llamacpp`, `mtp` | LLM Local | `VOLT_ENABLE_LOCAL_LLM_TOOLS=1` + binary in `PATH` | OFF | Binary often broken/missing |
| `cli_exec`, `cli_query` | CLI | `VOLT_ENABLE_CLI_TOOLS=1` | OFF | Requires enterprise CLIs |
| `nvidia_list_functions`, `nvidia_call_function`, `nvidia_deploy_function` | NVIDIA | `NVIDIA_API_KEY` or `NVCF_API_KEY` | OFF | Requires NVIDIA cloud |
| `ollama_web_search`, `ollama_web_fetch` | Ollama | `OLLAMA_API_KEY` | OFF | Requires Ollama cloud |

### Legend

- **ON**: Tool is registered and visible to the model by default (unless hidden by a "hides" flag).
- **OFF**: Tool is not registered unless the gating condition is met.
- **+ binary exists**: The binary must be found in `PATH` in addition to the env var being set.
- **+ feature flag**: The corresponding Cargo feature must be enabled at compile time.

---

## 4. How to Enable Gated Tools

### Enable Local LLM Tools

```bash
# .env
VOLT_ENABLE_LOCAL_LLM_TOOLS=1
```

Ensure the binaries are in your `PATH`:
- `litertlm` — LiteRT-LM inference engine
- `llamacpp` — llama.cpp server
- `mtp` — Model Transfer Protocol binary

### Enable CLI Gateway

```bash
# .env
VOLT_ENABLE_CLI_TOOLS=1
```

Install the whitelisted CLIs you need:
- `task` — Taskwarrior
- `hledger` — Plain-text accounting
- `khal` — Calendar CLI
- `vdirsyncer` — CardDAV/CalDAV sync
- `qsv` — CSV toolkit
- `himalaya` — Email CLI

### Enable NVIDIA Cloud Functions

```bash
# .env
NVIDIA_API_KEY=nvapi-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
# or
NVCF_API_KEY=nvcf-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

### Enable Ollama Cloud Tools

```bash
# .env
OLLAMA_API_KEY=ollama_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

### Enable Web Search (You.com)

```bash
# .env
YOUCOM_API_KEY=you_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

### Benchmark Mode (Hides Interfering Tools)

```bash
# .env — for clean BFCL runs
VOLT_BFCL_MODE=1
```

This hides `bash`, `web_search`, `you_research`, and `you_contents` so the model uses only BFCL-provided function stubs.

### Minimal Tool Mode (Hides Optional Tools)

```bash
# .env — for minimal/fast runs
VOLT_MINIMAL_TOOLS=1
```

This hides Charts, PDF, Desktop, and Browser tools from the model.

---

## 5. Troubleshooting

### "My model is calling broken tools"

**Symptom**: The model calls `litertlm` or `cli_exec` and the tool returns an error like "binary not found" or "command failed".

**Diagnosis**: Check your tool gating configuration.

```bash
# Run Volt with debug logging to see which tools are registered
cargo run -- --debug 2>&1 | grep -i "registering tool"
```

**Fix**:
1. If you don't need the tool, ensure the gating env var is **not** set:
   - Unset `VOLT_ENABLE_LOCAL_LLM_TOOLS`
   - Unset `VOLT_ENABLE_CLI_TOOLS`
2. If you do need the tool, ensure the binary is installed and in `PATH`:
   ```bash
   which litertlm  # should return a path
   which task      # should return a path for cli_exec
   ```

### "My model won't use web search"

**Symptom**: The model answers "I don't have access to real-time information" despite `web_search` being available.

**Diagnosis**:
1. Check if `VOLT_BFCL_MODE=1` is set — this hides `web_search`.
2. Check if `YOUCOM_API_KEY` is set — `web_search` requires a you.com API key.

**Fix**:
```bash
# .env
VOLT_BFCL_MODE=0        # or unset entirely
YOUCOM_API_KEY=you_xxx  # required for web search
```

### "Desktop/browser tools are missing"

**Symptom**: The model does not see `desktop_click` or `browser_navigate`.

**Diagnosis**:
1. Check if `VOLT_MINIMAL_TOOLS=1` is set — this hides optional tools.
2. Check if the feature flags are enabled in your build:
   ```bash
   cargo build --features tools-desktop,tools-browser
   ```

**Fix**:
```bash
# .env
VOLT_MINIMAL_TOOLS=0  # or unset entirely
```

And rebuild with the correct features if needed.

**Note**: `screenshot` tool (`src/tools/screenshot.rs`) was **deleted** — it existed but was never wired into a tool group.

### "NVIDIA tools don't appear"

**Symptom**: `nvidia_call_function` is not in the tool registry.

**Diagnosis**: Check if `NVIDIA_API_KEY` or `NVCF_API_KEY` is exported.

**Fix**:
```bash
# Verify the key is set
echo $NVIDIA_API_KEY

# .env
NVIDIA_API_KEY=nvapi-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

---

## 6. Summary Cheat Sheet

| Goal | Env Var(s) |
|------|-----------|
| Fastest startup, minimal tools | `VOLT_MINIMAL_TOOLS=1` |
| Clean benchmark runs | `VOLT_BFCL_MODE=1` |
| Web search (you.com) | `YOUCOM_API_KEY=...` |
| NVIDIA cloud functions | `NVIDIA_API_KEY=...` |
| Ollama cloud web tools | `OLLAMA_API_KEY=...` |
| Local LLM inference | `VOLT_ENABLE_LOCAL_LLM_TOOLS=1` + binaries in PATH |
| Enterprise CLI integration | `VOLT_ENABLE_CLI_TOOLS=1` + whitelisted CLIs installed |

---

*Last updated: June 2026*
