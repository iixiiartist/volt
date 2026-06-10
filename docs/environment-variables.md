# Volt Environment Variables Reference

This document lists every environment variable that Volt reads at runtime, organized by functional area. Values set in the shell take priority over `.volt/config.toml` for most fields.

---

## LLM / Routing

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
 | `LLM_MODEL` | Model ID passed to the active provider. | *(none — app prompts or reads from .env)* | `llama-3.3-70b-versatile` | `src/orchestrator.rs`, `src/commands/agent_run.rs` |
| `LLM_API_KEY` | Fallback API key used when a provider-specific key is not set. | *(none)* | `gsk_xxxxxxxxxxxx` | `src/orchestrator.rs`, `src/config.rs` |
| `LLM_BASE_URL` | Override the inference endpoint (Ollama, vLLM, LM Studio, etc.). | *(none)* | `http://localhost:11434/v1` | `src/orchestrator.rs`, `src/config.rs` |
| `LLM_MODEL_ROUTES` | JSON array of custom routing rules. Each object can contain `model`, `provider`, `base_url`, and `api_key_env`. | *(none)* | `[{"model":"custom","provider":"openai","base_url":"..."}]` | `src/orchestrator.rs` |
| `LLM_DEFAULT_PROVIDER` | Provider slug to use when no other route matches. | *(none — ProviderDetector auto-discovers)* | `ollama`, `nvidia`, `anthropic` | `src/orchestrator.rs` |
| `OLLAMA_HOST` | Base URL for a local Ollama instance (no API key required). | *(none)* | `http://localhost:11434` | `src/agent/router.rs` |
| `LLAMA_CPP_HOST` | Base URL for a local llama.cpp server. | *(none)* | `http://localhost:8080` | `src/agent/router.rs` |
| `LITERTLM_HOST` | Base URL for a local LiteRT-LM server. | *(none)* | `http://localhost:8080` | `src/agent/router.rs` |

---

## Provider API Keys

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `GROQ_API_KEY` | Groq cloud API key. Unlocks Groq-hosted models. | *(none)* | `gsk_xxxxxxxxxxxx` | `src/orchestrator.rs`, `src/agent/router.rs`, `src/main.rs` |
| `NVIDIA_API_KEY` | NVIDIA NIM / cloud API key. Also accepted by NVCF tools. | *(none)* | `nvapi-xxxxxxxxxxxx` | `src/orchestrator.rs`, `src/tools/nvidia_cloud_functions.rs`, `src/embedding/providers.rs` |
| `NVCF_API_KEY` | NVIDIA Cloud Functions API key (alias for `NVIDIA_API_KEY`). | *(none)* | `nvapi-xxxxxxxxxxxx` | `src/tools/nvidia_cloud_functions.rs` |
| `OLLAMA_API_KEY` | Ollama Cloud API key. Enables `ollama_web_search` and `ollama_web_fetch` tools. | *(none)* | `sk_ollama_...` | `src/orchestrator.rs`, `src/tools/ollama_web_tools.rs`, `src/agent/router.rs` |
| `OPENAI_API_KEY` | OpenAI native API key. | *(none)* | `sk-xxxxxxxxxxxx` | `src/orchestrator.rs`, `src/embedding/providers.rs` |
| `ANTHROPIC_API_KEY` | Anthropic (Claude) API key. | *(none)* | `sk-ant-...` | `src/orchestrator.rs`, `src/config.rs` |
| `RIVA_API_KEY` | NVIDIA Riva speech/audio API key. Used for STT/TTS via `RivaProvider`. | *(none)* | `nvapi-xxxxxxxxxxxx` | `src/llm/riva.rs` |
| `YOUCOM_API_KEY` | you.com API key. Enables `web_search`, `you_research`, and `you_contents` tools. | *(none)* | `xxxxxxxxxxxx` | `src/tools/you_tools.rs` |

---

## Embedding

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `EMBEDDING_PROVIDER` | Backend for text embeddings. | `local` (ONNX Runtime) | `nvidia`, `ollama`, `openai`, `huggingface` | `src/config.rs`, `src/embedding/mod.rs` |
| `EMBEDDING_MODEL` | Model name/id for the embedding provider. | `Xenova/bge-large-en-v1.5` | `nvidia/llama-nemotron-embed-1b-v2`, `mxbai-embed-large` | `src/config.rs`, `src/embedding/providers.rs`, `src/local_embed.rs` |
| `EMBEDDING_DIMENSION` | Vector dimension for context embeddings. Parametrizes DB schema at init. | `1024` | `384`, `768`, `1536` | `src/db/mod.rs`, `src/embedding/mod.rs` |
| `EMBEDDING_ENDPOINT` | Custom endpoint URL for the embedding provider. | *(none — local ONNX used by default)* | `http://localhost:11434/v1` | `src/config.rs`, `src/embedding/providers.rs` |
| `EMBEDDING_API_KEY` | API key for the embedding endpoint (fallback if provider-specific key missing). | *(none)* | `nvapi-xxxxxxxxxxxx` | `src/config.rs`, `src/embedding/providers.rs` |
| `VOLT_ONNX_MODEL_DIR` | Local directory containing `model.onnx` + `tokenizer.json` for local ONNX embedding inference. | *(none — uses default HF model)* | `C:\models\bge-large-en-v1.5` | `src/local_embed.rs` |
| `HF_TOKEN` | HuggingFace token for downloading ONNX models. | *(none)* | `hf_xxxxxxxxxxxx` | `src/embedding/providers.rs`, `src/main.rs` |
| `HUGGINGFACE_TOKEN` | Alias for `HF_TOKEN`. | *(none)* | `hf_xxxxxxxxxxxx` | `src/embedding/providers.rs` |

---

## Tool Gating

These variables are **opt-in**. Tools are only registered when the corresponding variable is set.

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `VOLT_ENABLE_LOCAL_LLM_TOOLS` | Set to `1` to register `litertlm`, `llamacpp`, and `mtp` tools (requires binaries on disk). | *(unset)* | `1` | `src/tools/groups/llm.rs` |
| `VOLT_ENABLE_CLI_TOOLS` | Set to `1` to register `cli_exec` and `cli_query` enterprise CLI gateway tools. | *(unset)* | `1` | `src/tools/registration.rs`, `src/tools/cli_tools/mod.rs` |
| `VOLT_MINIMAL_TOOLS` | Set to enable minimal toolset mode (charts, PDF, desktop, browser only; disables web/search). | *(unset)* | `1` | `src/tools/registration.rs` |
| `VOLT_BFCL_MODE` | Set to enable BFCL benchmark mode (gates `bash`, `web_search`, `you_research`, `you_contents` to avoid interference with benchmark stubs). | *(unset)* | `1` | `src/tools/groups/core.rs`, `src/tools/groups/web.rs` |

---

## Sandbox / Security

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `VOLT_SANDBOX_TIMEOUT_MS` | Maximum milliseconds a sandboxed subprocess may run. | `5000` | `10000` | `src/config.rs` |
| `VOLT_SANDBOX_MAX_STDOUT_BYTES` | Maximum bytes captured from sandboxed subprocess stdout/stderr. | `262144` (256 KiB) | `524288` | `src/config.rs` |
| `VOLT_ALLOWED_HOSTS` | Comma-separated allowlist of hosts for `web_fetch` / `web_search`. If set, only these hosts are permitted. | *(none)* | `api.example.com,docs.example.com` | `src/tools/web_tool.rs` |
| `VOLT_COMMAND_GUARD` | Set to `false` to disable the command-injection guard for `bash` tool. | `true` (guard enabled) | `false` | `src/tools/bash.rs` |
| `VOLT_FAILURE_TRACKING` | Set to `false` to disable automatic tracking of repeated tool failures in Postgres. | `true` (tracking enabled) | `false` | `src/tool_failure_tracker.rs` |
| `VOLT_FAILURE_THRESHOLD` | Number of failures in the last 10 minutes before the agent is warned to avoid a tool. | `3` | `5` | `src/tool_failure_tracker.rs` |
| `VOLT_LEAK_DETECTOR` | Set to `false` to disable the prompt-leak detector (scrubs secrets from tool inputs). | `true` (detector enabled) | `false` | `src/agent/run.rs` |
| `VOLT_WRAP_TOOL_OUTPUT` | Set to `true` to wrap tool output with `[Tool Output]` markers in the agent loop. | *(unset)* | `true` | `src/agent/run.rs` |
| `SANDBOX_SHELL` | Shell executable used by the sandbox runner. | `cmd.exe` (Windows) / `bash` (Unix) | `powershell.exe` | `src/sandbox.rs` |

---

## Local LLM Binaries

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `VOLT_TOOL_BIN_DIR` | Directory containing local inference binaries (`litertlm`, `llamacpp`, etc.). Used when `VOLT_ENABLE_LOCAL_LLM_TOOLS=1`. | *(none)* | `C:\volt\bin` | `src/tools/groups/llm.rs` |

---

## Registry / Telemetry

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `VOLT_METRICS_PORT` | Port for the Prometheus metrics HTTP endpoint (set to `0` to disable). | `9100` | `9090` | `src/metrics.rs` |
| `VOLT_REGISTRY_BASE_URL` | Base URL for the Volt agent-registry service. | `https://registry.voltagents.com/v1` | `http://localhost:8080/v1` | `src/config.rs` |
| `VOLT_REGISTRY_TOKEN` | Bearer token for authenticated registry requests. | *(none)* | `volt_xxxxxxxxxxxx` | `src/config.rs` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP endpoint for OpenTelemetry trace export. If unset, traces go to stdout. | *(none)* | `http://localhost:4317` | `src/telemetry.rs` |

---

## Agent Behavior

These variables control agent loop features and are overridden by `.volt/config.toml` when present.

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `VOLT_USE_MTP` | Enable Multi-Token Prediction (MTP) draft model acceleration. | `false` | `true` | `src/config.rs` |
| `VOLT_USE_COT` | Enable Chain-of-Thought prompting mode. **Deprecated — field retained for backward compat but unused.** | `false` | `true` | `src/config.rs` |
| `VOLT_ALLOW_WRITE` | Allow the agent to use the `write` tool without prompting. | `false` | `true` | `src/config.rs` |
| `VOLT_FRAMEWORK` | Optional agent framework identifier (e.g. `react`, `reflexion`). | *(none)* | `react` | `src/config.rs` |
| `VOLT_MODEL_VARIANT` | Optional model variant tag (e.g. `instruct`, `chat`). | *(none)* | `instruct` | `src/config.rs` |
| `VOLT_QUANTIZATION` | Optional quantization hint (e.g. `int8`, `fp16`). | *(none)* | `int8` | `src/config.rs` |

---

## Deprecated

| Variable | Description | Replacement | Used In |
|---|---|---|---|
| `KIMI_API_KEY` | Legacy Moonshot API key for embeddings. | `EMBEDDING_API_KEY` or `OPENAI_API_KEY` | `src/embedding/providers.rs`, `src/config.rs` |
| `KIMI_EMBEDDING_MODEL` | Legacy Moonshot embedding model name. | `EMBEDDING_MODEL` | `src/config.rs` |

> **Note:** Deprecated variables still work for backward compatibility but are no longer recommended for new setups.

---

## Database

| Variable | Description | Default | Example | Used In |
|---|---|---|---|---|
| `DATABASE_URL` | PostgreSQL 16+ connection string with pgvector extension. Schema auto-migrates on first connect — no manual `init-db`. If unset, Volt runs without persistence (sessions use SQLite). | *(none)* | `postgres://volt:volt@localhost:5432/volt` | `src/config.rs`, `src/main.rs`, tests |
