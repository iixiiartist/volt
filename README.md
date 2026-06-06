# VOLT — Virtual Operations for Local Tasks

> **VOLT (Virtual Operations for Local Tasks) is a Rust-native AI agent framework with unified RAG across 12 context fields, background auto-seeding worker, multi-agent orchestration (DAG/parallel/pipeline), MCP protocol server, CLI gateway, and 38 active tools (dynamically gated by env vars). ONNX Runtime with DirectML/OpenVINO/CUDA hardware acceleration. 95.0% BFCL v4 accuracy on 400 cases (llama-3.1-8b-instant).**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT) [![Rust](https://img.shields.io/badge/Rust-1.95+-orange.svg)](https://www.rust-lang.org) [![Pipeline Status](https://gitlab.com/iixiiartist-volt/volt/badges/main/pipeline.svg)](https://gitlab.com/iixiiartist-volt/volt/-/commits/main)

## Why VOLT?

**Virtual Operations for Local Tasks** — VOLT is built for agents that run *here*, not "somewhere in the cloud." Most agent frameworks inject every available tool into every LLM call. VOLT replaces static injection with **unified dynamic RAG** — tools, skills, memories, conversation history, artifacts, MCP configs, permissions, and security policies are all retrieved via vector similarity, so the model only sees what's relevant.

**Verified results (BFCL V4, 400 cases, argument-aware evaluation):**
- **95.0% accuracy** on llama-3.1-8b-instant (380/400) — all failures are Groq API schema validation errors (boolean/integer types passed as strings)
- **Flat tool-count scaling curve** — accuracy invariant from 1 to 200+ tools
- **74% token savings** vs static injection (470 cases, ~$0.37 total)

Key design decisions:

- **Everything-as-RAG**: 12 context kinds dynamically retrievable from unified vector store
- **Background Auto-Seeding Worker**: MPSC channel daemon maintains context autonomously via Tokio
- **Four-Pillar Eviction**: Semantic dedup, per-kind quotas, composite scores, episodic merging
- **38 Active Tools** (dynamically retrieved via RAG): File I/O, shell, web, git, time, reasoning, data, PDF, charts, desktop, browser, MCP, CLI gateway. Broken/optional tools auto-gated by env vars.
- **Multi-Agent Orchestration**: Parallel, pipeline, supervisor, supervisor-agenda, and DAG patterns
- **pgvector Persistence**: PostgreSQL with HNSW indexes, context survives restarts
- **Hardware-Accelerated ONNX Runtime**: ort with DirectML/OpenVINO/CUDA fallback chain — auto-detects Intel NPU/GPU, NVIDIA GPU, or CPU
- **MCP Protocol Server**: Expose 50+ tools to external clients (Claude Desktop, Cline, Goose) over stdio or HTTP with permission-gated execution
- **Single Executable**: No Python, Node.js, or Java runtime required. ONNX Runtime libraries download on first use to `~/.cache/ort.pyke.io/`.

## Quick Start

### Option A: Download binary (easiest)

```powershell
# 1. Download volt.exe from https://github.com/iixiiartist/volt/releases
# 2. Run PostgreSQL with pgvector (Docker — one command):
docker compose -f docker-compose.db.yml up -d

# 3. Set your API key and DB connection:
set GROQ_API_KEY=gsk_your_key_here
set DATABASE_URL=postgres://volt:volt@localhost:5432/volt

# 4. Initialize the database schema (one-time):
volt.exe init-db

# 5. Run:
volt.exe agent-run --input "Analyze this codebase" --allow
```

> **No Docker?** Set `DATABASE_URL` to any value (PostgreSQL unreachable is caught gracefully — runs without persistence).

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

### Unified Context Store (Everything-as-RAG)

12 context kinds, all dynamically retrievable via vector similarity:

| Kind | Quota | Source |
|---|---|---|
| Tool | 500 | All registered tool schemas |
| Skill | 200 | Compiled SKILL.md manifests |
| Conversation | 300 | Episodic memory after each run |
| Memory | 500 | MEMORY.md + DB memories |
| AgentRun | 200 | Full LLM turn audit logs |
| Artifact | 300 | Write/edit/bash side effects |
| SystemPrompt | 20 | SOUL.md |
| FewShot | 50 | Reserved |
| Policy | 50 | AGENTS.md |
| Permission | 50 | Tool allow/prompt rules |
| Security | 30 | Sandbox limits, oversight |
| MCPConfig | 100 | MCP server schema distillation |

### BFCL v4 Results (llama-3.1-8b-instant, 400 cases)

| Suite | Cases | Accuracy | Avg Latency |
|---|---|---|---|
| `simple_python` | 400 | **95.0%** | 234ms |

Full 17-category BFCL v4 sweep pending. All failures were Groq API schema validation (model passes `"true"` as string instead of boolean `true`, or `"null"` for optional integer). No cases failed due to wrong function selection.

**Tool-count scaling: flat curve.** Accuracy invariant from 0 to 200+ distractors (VOLT RAG benchmark, 50-case ablation verified).

### Built-in Tools (38 active, 55+ total)

| Category | Tools | Feature Flag |
|---|---|---|
| **File I/O** | `read`, `write`, `edit`, `glob`, `grep` | built-in |
| **Shell** | `bash` | built-in |
| **Web** | `web_fetch`, `web_scrape`, `web_scrape_all`, `web_search`, `you_research`, `you_contents` | built-in |
| **Data** | `csv_read`, `csv_write`, `archive_extract`, `archive_create`, `create_bar_chart`, `create_line_chart`, `create_pdf` | built-in / feature-gated |
| **Memory** | `memory_append`, `todo_add` | built-in |
| **Git** | `git_status`, `git_diff`, `git_diff_unstaged`, `git_diff_staged`, `git_add`, `git_commit`, `git_reset`, `git_log`, `git_branch`, `git_checkout`, `git_show`, `git_create_branch` | built-in |
| **Time** | `get_current_time`, `convert_time` | built-in |
| **Reasoning** | `sequentialthinking` | built-in |
| **Charts** | `create_bar_chart`, `create_line_chart` | built-in |
| **Screenshot/PDF/Desktop/Browser** | Feature-gated (12 tools) | opt-in features |
| **Delegation** | `delegate`, `run_workflow`, `final_answer` | built-in |
| **CLI Gateway** | `cli_exec`, `cli_query` (task, crm, hledger, khal, vdirsyncer, qsv, himalaya) | built-in |
| **MCP** | SearchHQ (19 tools), extensible via `VOLT mcp-serve` | built-in |

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

## Paper & Benchmarks

The accompanying research paper, *VOLT: Virtual Operations for Local Tasks — A Hardware-Aware Edge Exoskeleton and Compound AI Orchestrator*, is available in `paper/draft.md`. It details the four architectural pillars: Edge Model Exoskeleton (AST coercion), Cloud Optimization (prompt caching + native structured outputs), Observable DAG Orchestration, and Storage/I/O Hardening.

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
