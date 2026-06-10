# Volt — Virtual Orchestrator for Local Tasks

> **Volt** is a Rust-native AI agent middleware built for enterprises that need to run LLM workflows on their own infrastructure. Default inference is **vLLM** (the production-grade open-source serving stack); the same workflow can route to different models for different roles via `volt.models.toml`. Cloud providers (Groq, OpenAI, Anthropic, NVIDIA NIM, Ollama Cloud) are an explicit opt-in for development. Workflows tagged `environment: "prod"` are enforced to use only allowlisted providers — your data never leaves the box.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT) [![Rust](https://img.shields.io/badge/Rust-1.95+-orange.svg)](https://www.rust-lang.org) [![Pipeline Status](https://gitlab.com/iixiiartist-volt/volt/badges/main/pipeline.svg)](https://gitlab.com/iixiiartist-volt/volt/-/commits/main)

## Why Volt?

Most agent frameworks either hardcode their provider choice (you use OpenAI) or hand you a generic "BYO LLM" abstraction without any enterprise guardrails. Volt does neither:

- **vLLM-first**: Auto-detects a local vLLM server (`http://localhost:8000`) and makes it the default inference backend. The same vLLM process can serve multiple model roles (supervisor / classifier / coder / embedder) — one HTTP endpoint, many models.
- **Role-based model routing**: A workflow declares a *role* (`role: "supervisor"`); `volt.models.toml` maps role → model ID. Switch models by editing the TOML, not the workflow.
- **Per-environment provider allowlist**: A workflow tagged `environment: "prod"` cannot route to a non-allowlisted provider, even if the env var is set. Cloud providers are opt-in via `VOLT_ENABLE_CLOUD_PROVIDERS=1`; the gate defaults to off.
- **~20 curated tools** (not 50+): Read, write, edit, glob, grep, bash, web_fetch, web_search, git operations, CSV, PDF, charts, desktop/browser automation, DAG workflows. Unconfigured tools are not registered.
- **3-kind context store** (not 12): Tool schemas, conversation history, long-term memory — the three signal sources that matter for tool selection.
- **Keyword routing over LLM routing**: Task-to-agent dispatch uses keyword table matching (~100µs) instead of an LLM call.
- **DAG-based multi-agent orchestration**: Parallel, pipeline, supervisor, and arbitrary DAG patterns with `{input}` / `{node_id}` templating.
- **MCP protocol server**: Expose tools to external clients (Claude Desktop, Cline, Goose) over stdio or HTTP with permission-gated execution.
- **PostgreSQL persistence**: pgvector with HNSW indexes, append-only audit log (EU AI Act Art. 12).
- **Single executable**: No Python, Node.js, or Java runtime required.

## vLLM integration status

The vLLM provider is **structurally complete but not yet validated against a live vLLM endpoint**. The request body, response parsing, and streaming chunk format follow the OpenAI spec that vLLM commits to. The `vllm` provider passes all unit tests (request shape, response parsing, error handling) and is registered in the provider detector. **An integration test gated on `VLLM_INTEGRATION_URL` is the next step** — see [`docs/vllm-deployment.md`](docs/vllm-deployment.md) for the deployment runbook and test plan.

Treat vLLM-tagged workflows as `environment: dev|staging` until a vLLM deployment is available for validation. Cloud providers (Groq etc.) remain the testing path in the interim.

**Verified results (BFCL v4, 400 cases, argument-aware evaluation):**
- **95.0% accuracy** on llama-3.1-8b-instant (380/400) — all failures are Groq API schema validation errors
- **Flat tool-count scaling curve** — accuracy invariant from 1 to 200+ tools
- **74% token savings** vs static injection (470 cases, ~$0.37 total)
- **qwen3-32b scores 100%** on simple_python tool selection (3/3 cases)

Key design decisions:

- **3-Kind Context Store**: Tool schemas (500), Memory (500), Conversation (300) — the three signal sources for tool selection. Background auto-seeding worker with MPSC channel architecture keeps the store populated.
- **Four-Pillar Eviction**: Semantic dedup, per-kind quotas, composite scores, episodic merging
- **~20 Active Tools**: File I/O, shell, web fetch/search, git, CSV, PDF, charts, desktop, browser, CLI gateway, DAG workflows. Broken/optional tools auto-gated by env vars (VOLT_ENABLE_CLI_TOOLS, NVIDIA_API_KEY, etc.)
- **Deleted bloat**: No `final_answer`, `sequentialthinking`, `get_current_time`, `memory_append`, `todo_add`, `json_query` — 9 source files removed. 12 git tools collapsed to 2 (`git_query`/`git_mutate`). `web_scrape` merged into `web_fetch` with optional `selector` param.
- **Multi-Agent Orchestration**: Parallel, pipeline, and DAG patterns. Supervisor synthesizer is opt-in (default: direct concatenation).
- **Auto-Detecting Provider Router**: `ProviderDetector` checks environment variables and local hosts at startup — no hardcoded default model or provider. `volt config wizard` guides first-time setup.
- **Auto-Migration**: `init_schema()` runs on every `connect()` — no manual `volt init-db` step needed.
- **PostgreSQL Persistence**: pgvector with HNSW indexes, append-only audit log (EU AI Act Art. 12), connection pooling.
- **ONNX Runtime**: Hardware-accelerated embeddings via DirectML/OpenVINO/CUDA fallback chain — auto-detects Intel GPU, NVIDIA GPU, or CPU.
- **MCP Protocol Server**: Expose tools to external clients (Claude Desktop, Cline, Goose) over stdio or HTTP with permission-gated execution.
- **Single Executable**: No Python, Node.js, or Java runtime required. ONNX Runtime libraries download on first use.

## Quick Start

### Option A: Run against vLLM (production target)

```bash
# 1. Start vLLM (single-model, single-GPU; see docs/vllm-deployment.md for multi-model)
pip install vllm
vllm serve meta-llama/Llama-3.1-8B-Instruct \
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json

# 2. Download volt.exe from https://github.com/iixiiartist/volt/releases
# 3. Tell volt where vLLM is:
export VLLM_HOST=http://localhost:8000

# 4. Run — schema auto-migrates on first connect, no manual init needed:
volt webui
```

The first time volt starts, it creates `~/.volt/volt.models.toml` with sensible
defaults mapping `supervisor` / `classifier` / `coder` / `summarizer` roles to
specific model IDs. Edit that file to match the models served by your vLLM
instance.

### Option B: Run against a cloud provider (development)

```bash
# 1. Download volt.exe from https://github.com/iixiiartist/volt/releases
# 2. (Optional) Run PostgreSQL with pgvector for persistence:
docker compose -f docker-compose.db.yml up -d

# 3. Set your preferred provider's API key. Cloud providers require
#    VOLT_ENABLE_CLOUD_PROVIDERS=1 to be active (off by default — local
#    vLLM/Ollama is the enterprise path).
set GROQ_API_KEY=gsk_your_key_here
set VOLT_ENABLE_CLOUD_PROVIDERS=1

# 4. Run — schema auto-migrates on first connect, no manual init needed:
volt.exe webui
```

> **No PostgreSQL?** Volt runs without it (SQLite used for sessions). `DATABASE_URL` is optional.
> **No API key?** The WebUI shows a setup wizard where you can enter keys interactively, or run `volt config wizard`.
> **Workflows tagged `environment: "prod"`** are enforced to use only providers in `VOLT_PROD_PROVIDER_ALLOWLIST` (default: `vllm,ollama_local`). A prod workflow cannot route to Groq even if `GROQ_API_KEY` is set.

### Option A+: Install with desktop shortcut (Windows)

```powershell
# 1. Download volt-windows.zip from the latest release
# 2. Right-click the zip → Extract All, or:
Expand-Archive .\volt-windows.zip -DestinationPath C:\volt-install

# 3. Run the installer (creates Start Menu + Desktop shortcuts, registers in Add/Remove Programs):
powershell -ExecutionPolicy Bypass -File C:\volt-install\install.ps1

# 4. Launch from Start Menu → Volt → Volt WebUI
#    Or from the Desktop shortcut. Or from anywhere:
"%LOCALAPPDATA%\Volt\webui.exe"

# Uninstall: Start Menu → Volt → Uninstall Volt WebUI
# Or: settings → Apps → Installed apps → Volt WebUI → Uninstall
```

### Option A++: Install on Linux/macOS

```bash
# 1. Download volt-linux-x86_64.tar.gz (or macos) from the latest release
tar xzf volt-linux-x86_64.tar.gz
cd volt-linux-x86_64

# 2. Run the installer (creates .desktop file, adds to PATH):
./install.sh

# 3. Launch from your application launcher, or:
~/.local/bin/webui

# Uninstall:
./install.sh --uninstall
```

### Option B: Build from source

**Linux:**
```bash
git clone https://github.com/iixiiartist/volt.git
cd volt
cargo build --release

# Initialize, then run
./VOLT init-db
./VOLT agent-run --input "Analyze this codebase" --allow
```

**Windows (MSVC — 49 MB binary):**
```powershell
# Requires Visual Studio 2022 Build Tools with "Desktop development with C++"
# Install: https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022

git clone https://github.com/iixiiartist/volt.git
cd volt

# Build using the MSVC toolchain (default after v0.5.1):
cargo build --release

# The resulting volt.exe depends on standard Windows DLLs + VCRUNTIME140.dll
# (MSVC Redistributable, pre-installed on most Windows). ONNX Runtime shared
# libraries (onnxruntime.dll, DirectML.dll, etc.) are downloaded on first use
# to ~/.cache/ort.pyke.io/ (~50–150 MB depending on Execution Provider).
```

> **Note for MinGW users:** If you use the GNU toolchain (`x86_64-pc-windows-gnu`), the binary will depend on `libstdc++-6.dll`, `libgcc_s_seh-1.dll`, and `libwinpthread-1.dll`. Use the MSVC toolchain instead.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    VOLT Agent Loop                          │
│  User Query → Embed → RAG (12 kinds) → LLM → Tools → Seed  │
└─────────────────────────────────────────────────────────────┘
```
┌─────────────────────────────────────────────────────────────┐
│                    VOLT Agent Loop                          │
│  User Query → Embed → RAG (12 kinds) → LLM → Tools → Seed  │
└─────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────────┐
          ▼                   ▼                       ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────────┐
│ Unified Context  │ │ Auto-Seeding    │ │ Permission System   │
│ Store (12 kinds) │ │ Worker (MPSC)   │ │ 23 Prompt-gated     │
│ pgvector HNSW    │ │ Batch + Merge   │ │ tools, --allow flag │
└─────────────────┘ └─────────────────┘ └─────────────────────┘
```

## Features

### 3-Kind Context Store

Default retrieval includes only 3 context kinds (others still stored/queryable but excluded from default context window):

| Kind | Quota | Source |
|---|---|---|
| Tool | 500 | All registered tool schemas (name + description + JSON schema) |
| Memory | 500 | MEMORY.md + DB memories |
| Conversation | 300 | Episodic memory after each agent run |

### BFCL v4 Results (llama-3.1-8b-instant, 400 cases)

| Suite | Cases | Accuracy | Avg Latency |
|---|---|---|---|
| `simple_python` | 400 | **95.0%** | 234ms |

Full 17-category BFCL v4 sweep pending. All failures were Groq API schema validation (model passes `"true"` as string instead of boolean `true`, or `"null"` for optional integer). No cases failed due to wrong function selection.

**Tool-count scaling: flat curve.** Accuracy invariant from 0 to 200+ distractors (VOLT RAG benchmark, 50-case ablation verified).

### Built-in Tools (~20 active)

| Category | Tools | Gate |
|---|---|---|
| **File I/O** | `read`, `write`, `edit`, `glob`, `grep` | always |
| **Shell** | `bash` | always (hidden in VOLT_BFCL_MODE) |
| **Web** | `web_fetch` (with `selector` param), `web_search`, `you_research`, `you_contents` | search tools require YOUCOM_API_KEY |
| **Data** | `csv_read`, `csv_write`, `archive_extract`, `archive_create`, `create_bar_chart`, `create_line_chart`, `create_pdf` | charts/PDF hidden in VOLT_MINIMAL_TOOLS |
| **Git** | `git_query`, `git_mutate` (raw subcommand strings, collapsed from 12) | always |
| **Orchestration** | `delegate`, `run_workflow` | always |
| **Desktop** | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` | tools-desktop feature |
| **Browser** | `browser_navigate`, `browser_extract`, `browser_screenshot` | tools-browser feature |
| **NVIDIA Cloud** | `nvidia_list_functions`, `nvidia_call_function`, `nvidia_deploy_function` | NVIDIA_API_KEY |
| **Ollama Web** | `ollama_web_search`, `ollama_web_fetch` | OLLAMA_API_KEY |
| **CLI Gateway** | `cli_exec`, `cli_query` | VOLT_ENABLE_CLI_TOOLS=1 |
| **Local LLM** | `litertlm`, `llamacpp`, `mtp` | VOLT_ENABLE_LOCAL_LLM_TOOLS=1 |
| **MCP** | SearchHQ (19 tools), extensible via `VOLT mcp-serve` | runtime registration |

**Deleted bloat:** `final_answer`, `sequentialthinking`, `get_current_time`, `convert_time`, `memory_append`, `todo_add`, `json_validate`, `json_prettify`, `json_query`, `web_scrape`, `web_scrape_all`, `screenshot`, 10 individual git tools.

### Embedding

Hardware-accelerated ONNX Runtime (ort) with auto-detecting Execution Provider fallback chain: OpenVINO → DirectML → CUDA → CPU. Uses `Xenova/bge-large-en-v1.5` (1024d, int8 quantized, ~337MB). Configure via `VOLT_ONNX_MODEL_DIR` or `EMBEDDING_MODEL`. Falls back to deterministic SHA-256 placeholder + BM25 hybrid retrieval when no network or local model is available.

> **MSVC build required:** ort prebuilt binaries ship for `x86_64-pc-windows-msvc` only. Build from source with VS 2022 Build Tools and the MSVC Rust toolchain (`rustup default stable-x86_64-pc-windows-msvc`).

### MCP Protocol Server

VOLT exposes its full 38+ tool registry over the [Model Context Protocol](https://modelcontextprotocol.io) (MCP) — the open standard for AI tool integration. Run `VOLT mcp-serve` to start the stdio server, then connect any MCP-compatible client (Claude Desktop, Cline, Goose, etc.).

- **Full lifecycle**: `initialize` handshake with capability declaration (`protocolVersion: 2024-11-05`), `notifications/initialized` support
- **Stdout-safe**: All JSON-RPC messages go to stdout; tracing/logging routes to stderr — no protocol corruption
- **Permission-gated**: Tool calls pass through Volt's `execute_gated()` approval layer — same safety rails as internal agents
- **HTTP mode**: Use `MCPServer::serve_http()` for agent-to-agent tool sharing over TCP

### gRPC MCP Transport (Experimental)

The `tools-mcp-grpc` feature flag enables a gRPC MCP transport (`MCPTransport::Grpc`) with bidirectional streaming for agent-to-agent coordination. The server side (`list_tools`, `call_tool`, `call_tool_stream`) is fully implemented via tonic + prost. The client side is scaffolded but requires generated tonic stubs — use `MCPTransport::Http` for remote agent connections.

### Blueprint Scaffolding & Quirk Coercion ("Edge Model Exoskeleton")

VOLT's **Edge Model Exoskeleton** — the "Local Tasks" in *Virtual Operations for Local Tasks* — compensates for systematic failure modes in small/edge models (<8B parameters) via **cognitive scaffolding** and **AST-level quirk coercion**.

**67 production blueprints** ship in `blueprints/` across Groq (19), NVIDIA NIM (20), Ollama Cloud (25), and Edge (3):

| Blueprint | Model | Dialect | Quirks | Mode |
|---|---|---|---|---|
| `gemma4_e2b_voice.toml` | gemma-4-e2b | ChatMlTools | SchemaLimitTen, MissingFinalAnswer | strict, 1 tool/turn |
| `llama3_8b_local.toml` | llama-3.1-8b | LlamaChat | StringifiedBooleans, StringifiedIntegers, ChainOfThoughtLeak | relaxed, 3 tools/turn |

**How it works:**
- **`strict_mode`** — strips `Tool` and `Skill` from RAG retrieval, hard-binding only explicitly listed `core_tools`. Prevents small models from hallucinating under context overload.
- **`max_tools_per_turn`** — truncates excess tool calls with a synthetic error message instructing the LLM to retry remaining tools.
- **`FormatDialect`** routes prompt construction to model-native formats (ChatMlTools, LlamaChat, StandardXml, ClaudeXml, OpenAiJson).
- **Quirk interceptors** run *before* JSON Schema validation in `tool_parser.rs`:
  - `StringifiedBooleans` — coerces `"true"` → `true`
  - `StringifiedIntegers` — coerces `"42"` → `42`
  - `ChainOfThoughtLeak` — strips conversational preamble/aftermath outside structured tags
  - `SchemaLimitTen` — caps tool retrieval to 10 items when not in strict mode
  - `MissingFinalAnswer` — auto-wraps plain-text response as `final_answer(answer: ...)` tool call

**Usage:**
```bash
# Explicit blueprint
VOLT agent-run --input "Create hello.txt" --blueprint blueprints/gemma4_e2b_voice.toml

# Auto-orchestrate: selects blueprint based on prompt heuristics
VOLT agent-run --input "Create hello.txt" --auto-blueprint
```

### Multi-Agent Orchestration

Parallel, pipeline, supervisor, and DAG-based multi-agent patterns with topological scheduling and parallel level execution.

### Permission System

23 tools default to `PermissionLevel::Prompt`. Autonomous mode with `--allow` (`-a`). Human-in-the-loop enforced at the Rust compiler level via the `attenuation` module.

## Commands Reference

### Setup & Database

| Command | Description |
|---------|-------------|
| `VOLT init-db` | Initialize PostgreSQL schema (tables, indexes, pgvector HNSW) — one-time setup |
| `VOLT migrate` | Apply database schema migrations |

### Running Agents

| Command | Description |
|---------|-------------|
| `VOLT agent-run --input <query>` | Single-shot agent execution (non-interactive) |
| `VOLT agent-tui` | Interactive terminal chat session with the agent |

Common flags for both:

| Flag | Default | Description |
|------|---------|-------------|
| `--model <name>` | `LLM_MODEL` or `llama-3.1-8b-instant` | LLM model to use |
| `--allow`, `-a` | off | Autonomous mode (bypass permission prompts) |
| `--mode <mode>` | `balanced` | Context profile: `precision`, `balanced`, `autonomous` |
| `--session-id <uuid>` | — | Resume a previous session |
| `--max-iterations <n>` | 8 (agent-run) / 25 (agent-tui) | Max agent loop iterations |
| `--load-tools <file>` | — | Path to BFCL tool stubs JSONL (for benchmarks) |
| `--context-kinds <list>` | mode-driven | Comma-delimited context kinds to enable |
| `--blueprint <path>` | — | Load Agent Blueprint TOML (overrides dialect, quirks, strict mode, tools) |
| `--auto-blueprint` | off | Auto-select blueprint from prompt heuristics (simple tasks → gemma4, complex → llama3) |

**Context modes:**
- `precision` — Tool + Artifact only (2 kinds, best for function-calling benchmarks)
- `balanced` — Tool + Skill + Memory + Conversation + Artifact (5 kinds, default)
- `autonomous` — All 12 context kinds (best for open-ended research tasks)

### Multi-Agent Workflows

| Command | Description |
|---------|-------------|
| `VOLT workflow --pattern <p> --agents <json> --tasks <json>` | Run a multi-agent workflow |

Patterns: `parallel`, `pipeline`, `supervisor`, or inline DAG JSON.

Use `--agents-file` / `--tasks-file` to pass from files instead of CLI args.

### Tools & Skills Management

| Command | Description |
|---------|-------------|
| `VOLT list-tools` | List all registered tools as JSON |
| `VOLT execute --tool <name> --params <json>` | Execute a tool directly by name |
| `VOLT validate --manifest <path>` | Validate a tool manifest file |
| `VOLT sandbox --command <cmd>` | Run an arbitrary command in the sandbox |
| `VOLT history --limit <n>` | Show recent tool execution history |
| `VOLT mcp-serve` | Serve all tools over MCP stdio transport (stdin/stdout JSON-RPC) |
| `VOLT provision --pkg-id <id>` | Provision a tool from the remote registry |
| `VOLT provision-file --manifest <path>` | Provision a tool from a local manifest file |
| `VOLT provision-skill --path <path>` | Compile and store a skill from SKILL.md |
| `VOLT import-skill --path <file> --format <fmt>` | Import a skill from Claude, Cursor, Copilot, OpenCode, or Markdown |
| `VOLT install-skill --name <name>` | Install a skill from the catalog |
| `VOLT list-catalog-skills` | List available skills in the catalog |
| `VOLT search-catalog-skills --query <q>` | Search the skill catalog |

### Evaluation & Benchmarks

| Command | Description |
|---------|-------------|
| `VOLT eval --suite <file> --model <name>` | Run an evaluation suite against the agent |
| `bfcl_bench` (separate binary) | BFCL v4 benchmark runner (use `--help` for flags) |

### Daemons (Background Services)

| Command | Description |
|---------|-------------|
| `VOLT heartbeat` | Periodic heartbeat loop (60s interval) |
| `VOLT jobs-monitor` | Self-repair job monitor (check 30s, repair 300s) |
| `VOLT routines-engine` | Routine scheduling engine (60s check) |
| `VOLT jobs list` | List all jobs from the database |
| `VOLT routines list` | List all routines from the database |

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string (e.g. `postgres://volt:volt@localhost:5432/volt`) |
| `GROQ_API_KEY` | For Groq | — | Groq API key for LLM access |
| `LLM_MODEL` | No | `llama-3.1-8b-instant` | Default LLM model |
| `LLM_BASE_URL` | No | `http://localhost:11434/v1` | For Ollama or custom OpenAI-compatible endpoints |
| `LLM_API_KEY` | No | — | API key for custom LLM providers |
| `OPENAI_API_KEY` | No | — | OpenAI API key |
| `ANTHROPIC_API_KEY` | No | — | Anthropic API key |
| `NVIDIA_API_KEY` | No | — | NVIDIA NIM API key |
| `EMBEDDING_PROVIDER` | No | `nvidia` | Embedding provider: `nvidia`, `ollama`, `openai`, `huggingface`, `moonshot`, `llamacpp` |
| `EMBEDDING_MODEL` | No | `nvidia/llama-nemotron-embed-1b-v2` | Embedding model ID |
| `EMBEDDING_ENDPOINT` | No | NVIDIA NIM endpoint | Embedding API URL |
| `EMBEDDING_API_KEY` | No | — | Embedding API key (NVIDIA NIM) |
| `HF_TOKEN` | No | — | HuggingFace token (downloads BGE-small-en-v1.5 ONNX model) |
| `YOUCOM_API_KEY` | No | — | you.com API key for web search/research tools |
| `VOLT_REGISTRY_BASE_URL` | No | `https://registry.voltagents.com/v1` | Tool registry URL |
| `VOLT_REGISTRY_TOKEN` | No | — | Tool registry auth token |
| `VOLT_ONNX_MODEL_DIR` | No | HF cache | Local directory with `model.onnx` + `tokenizer.json` |
| `RUST_LOG` | No | `info` | Logging/tracing level |

### Feature Flags (compile-time)

| Flag | Enables |
|------|---------|
| `tools-screenshot` | Screenshot capture tool |
| `tools-pdf` | PDF generation tool |
| `tools-desktop` | Desktop automation (click, type, key, find_window) |
| `tools-browser` | Browser automation (navigate, extract, screenshot) |
| `tools-local-embeddings` | Local ONNX embeddings via ort (DirectML/OpenVINO/CUDA) (**default**) |
| `tools-mcp-grpc` | gRPC MCP transport (experimental) |
| `tools-telegram` | Telegram bot integration |

### Config File

VOLT reads `.volt/config.toml` for persistent settings (auto-generated by first-run wizard):

```toml
[agent]
model = "llama-3.1-8b-instant"
provider = "groq"
max_iterations = 25
temperature = 0.3

[embedding]
model = "Xenova/bge-large-en-v1.5"
provider = "local"

[database]
url = "postgres://volt:volt@localhost:5432/volt"
```

## Testing

```bash
# Full test suite
cargo test --features testutils

# Unit tests (198)
cargo test --lib --features testutils

# Professional workflow tests (24)
cargo test --test professional_workflows --features testutils

# Real-world benchmarks (11)
cargo test --test real_world_benchmarks --features testutils
```

## Downloads

Pre-built binaries for Linux and Windows are available on the [Releases page](https://github.com/iixiiartist/volt/releases).

| Platform | Binary Size (compressed) |
|---|---|---|
| Linux (x86_64) | ~17 MB `.tar.gz` |
| Windows (x86_64, MSVC) | ~17 MB `.zip` (49 MB uncompressed) |

## Performance

| Metric | Value |
|---|---|---|
| Binary size | ~52 MB Linux (17 MB gzipped), ~49 MB Windows (17 MB zipped, MSVC) |
| Cold start | <100ms |
| Tool search | <5µs (in-memory cosine, DashMap single-pass) |
| Memory search | <5ms (pgvector HNSW) |
| Token savings | 74% vs static injection |
| BFCL accuracy (400 cases) | 95.0% (llama-3.1-8b-instant) |

## License

MIT — see [LICENSE](./LICENSE) for details.

**VOLT** — *Virtual Operations for Local Tasks*. Built in Rust by [Setique Labs](https://setique.com).
