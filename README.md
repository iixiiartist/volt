# Volt — The Autonomous Systems Engine

> **Locally-runnable AI agent framework with dynamic RAG-based tool selection, multi-agent orchestration, and compiled manifest pattern. 74% token savings vs static tool injection. [BFCL-verified.](paper/draft.md)**

[![CI](https://github.com/iixiiartist/volt/actions/workflows/ci.yml/badge.svg)](https://github.com/iixiiartist/volt/actions) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT) [![Rust](https://img.shields.io/badge/Rust-1.95+-orange.svg)](https://www.rust-lang.org)

## Why Volt?

Most agent frameworks inject every available tool into every LLM call. Volt replaces static injection with **dynamic RAG-based tool selection** — tools are retrieved via vector similarity, so the model only sees what's relevant. On the [Berkeley Function Calling Leaderboard](https://gorilla.cs.berkeley.edu/leaderboard.html) (BFCL V4) with a 51-tool registry, this cuts per-turn prompt tokens by **74%** (2,248 → 579 avg) while improving function-calling accuracy by **6.7 percentage points**.

Key design decisions:

- **Dynamic RAG Tool Selection**: Tools are embedded and retrieved via cosine similarity rather than hardcoded into every prompt. Only the top-8 most relevant tools are injected per turn. Scales to registries of any size (98.4% savings at 500 tools).
- **17 Built-in Tools**: File I/O, shell, web, data processing, PDF, charts, desktop automation, browser automation, JSON, CSV, archives, and more — all behind Cargo feature flags.
- **Multi-Agent Orchestration**: Parallel, pipeline, and supervisor patterns built-in with per-agent token tracking.
- **Compiled Manifest Pattern**: Author skills in Markdown (`SKILL.md`), compile into PostgreSQL with HNSW indexing.
- **Polyglot Execution Sandbox**: Tools run in isolated subprocesses with environment clearing and output limits.
- **Single Binary**: Rust-compiled, no Python or Node required at runtime.
- **MCP Native**: Model Context Protocol support for tool interoperability.
- **Autonomous Mode**: `--allow` flag for unattended execution with session-level approval persistence.

## Quick Start

### Docker (Recommended)

```bash
# Prerequisites: Docker Compose v2+
git clone https://github.com/iixiiartist/volt.git
cd volt
docker compose up -d
```

This starts PostgreSQL 16 with pgvector and Volt. The first-run wizard will prompt for LLM provider and API key on first attach.

### Build from Source

```bash
# Prerequisites: Rust 1.85+, PostgreSQL 16+ with pgvector
git clone https://github.com/iixiiartist/volt.git
cd volt
cargo build --release
```

### Run Your First Agent

```bash
# First-run wizard (interactive LLM + DB setup)
volt init

# Interactive chat with dynamic tool selection
volt agent-chat

# Single-shot execution
volt agent-run --input "Analyze this codebase for security issues"

# Autonomous mode (skip all approval prompts)
volt agent-chat --allow

# Compile a skill from a SKILL.md file
volt provision-skill --path ./examples/github-pr-reviewer/SKILL.md

# Import a skill from another platform (Claude, Cursor, Copilot, OpenCode)
volt import-skill --path /path/to/other-platform-skill.md

# Install a skill from the catalog
volt install-skill --name "github-pr-reviewer"

# List available catalog skills
volt list-catalog-skills

# Search the skill catalog
volt search-catalog-skills --query "code review"

# Run BFCL benchmark (static vs RAG comparison)
python volt-bfcl/benchmark.py --mode both --category simple_python --distractors 50

# Run ProgramBench coding benchmark
python volt-bfcl/program_bench.py --model llama-3.1-8b-instant

# Run GAIA benchmark (requires GAIA dataset download)
python volt-bfcl/gaia_benchmark.py --model llama-3.1-8b-instant
```

## Features

### Built-in Tools

Volt ships with 17 built-in tools organized by category, all behind Cargo feature flags:

| Category | Tools | Feature Flag | Default |
|---|---|---|---|
| **File I/O** | `read`, `write`, `edit`, `glob`, `grep` | built-in | ✅ |
| **Shell** | `bash` | built-in | ✅ |
| **Web** | `web_fetch`, `web_scrape`, `web_scrape_all` | built-in | ✅ |
| **Data** | `json_validate`, `json_prettify`, `json_query`, `csv_read`, `csv_write` | built-in | ✅ |
| **Archives** | `archive_extract`, `archive_create` | built-in | ✅ |
| **Memory** | `memory_append`, `todo_add` | built-in | ✅ |
| **Screenshot** | `screenshot` | `tools-screenshot` | ✅ |
| **Charts** | `create_bar_chart`, `create_line_chart` | built-in | ✅ |
| **PDF** | `create_pdf` | `tools-pdf` | ✅ |
| **Desktop** | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` | `tools-desktop` | ✅ |
| **Browser** | `browser_navigate`, `browser_extract`, `browser_screenshot` | `tools-browser` | ✅ |
| **Delegation** | `delegate`, `run_workflow` | built-in | ✅ |

### Token Tracking

Every agent turn tracks `prompt_tokens` and `completion_tokens` from the LLM API. The orchestrator surfaces per-step token usage in multi-agent workflows:

```text
Step: [PASS] data-agent (877 ms, 3,094P+476C tokens)
Total: 12,703 prompt + 2,078 completion = 14,781 tokens
```

### Smart Embedding Router

Volt automatically detects available embedding providers and builds a fallback chain:

1. **Ollama** (local, auto-detected via health check) — no API key needed
2. **NVIDIA NIM** (cloud, if `NVIDIA_API_KEY` or `EMBEDDING_API_KEY` set and non-placeholder)
3. **OpenAI** (cloud, if `OPENAI_API_KEY` set)
4. **Moonshot** (cloud, if `KIMI_API_KEY` set)
5. **Deterministic placeholder** (SHA-256-based, always works, no network)

Set `EMBEDDING_PROVIDER` to a specific value (e.g., `openai`) to pin a provider with auto-detected fallbacks. Set to `auto` or unset for full auto-detection.

### Skill Catalog & Import

- **Catalog**: Remote skill index with 5+ curated skills. `list-catalog-skills`, `search-catalog-skills`, `install-skill` commands.
- **Import**: Auto-detects 5 source formats — Claude, Cursor, Copilot, OpenCode, vanilla Markdown — and converts to Volt-native SKILL.md.
- **Batch Import**: Import 269 OpenCode skills in one pass.

### First-Run Wizard

`volt init` (or auto-runs on first startup when stdin is a TTY) interactively configures:
- LLM provider + model + API key
- Database URL
- Writes `.volt/config.toml` and `.env`

### Autonomous Mode

Pass `--allow` / `-a` to `agent-run`, `agent-chat`, `agent-tui`, or `workflow` commands to skip all approval prompts. Supports `--allow-session` to approve once per session.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Volt Agent Loop                          │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  User Query + Last 3 Messages (Context)                     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
              ┌────────────────────────────────┐
              │  pgvector Cosine Similarity    │
              │  (HNSW Index - Sub-ms Search)  │
              └────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│ Top-8 Tools   │   │ Top-3 Skills  │   │ Top-5 Memories│
│ (Dynamic)     │   │ (Priming)     │   │ (Temporal)    │
└───────────────┘   └───────────────┘   └───────────────┘
        │                     │                     │
        └─────────────────────┼─────────────────────┘
                              ▼
              ┌────────────────────────────────┐
              │  System Prompt Construction    │
              │  (Context-Aware, Pruned)       │
              └────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  LLM Call (Ollama / Claude / OpenAI-compatible)             │
└─────────────────────────────────────────────────────────────┘
```

## Features

### Dynamic RAG Tool Selection

Every agent turn performs semantic search across three knowledge sources:

1. **Tools**: 17+ built-in tools plus registry tools. Only the top-8 most relevant are included in the LLM call. This replaces static injection used by Claude Code, OpenClaw, and Hermes Agent.
2. **Skills**: Compiled from `SKILL.md` files. Context-priming instructions injected as system messages.
3. **Memories**: Persistent conversation history stored in PostgreSQL with pgvector. Useful for long-running tasks and cross-session context.

**Benchmark results (BFCL V4, 51-tool registry, Groq llama-3.1-8b):**

| Metric | Static (all 51 tools) | Volt RAG (top-8) | Improvement |
|---|---|---|---|
| Avg prompt tokens/task | 2,248 | 579 | **74% savings** |
| Function-calling accuracy | 34.3% | 41.0% | **+6.7pp** |
| simple_python accuracy | 80.0% | 98.0% | **+18pp** |
| simple_javascript accuracy | 58.0% | 68.0% | **+10pp** |

Using 50 distractor tools to match real-world registry sizes (Claude Code ~36, OpenClaw ~55, Hermes ~52). Full methodology in [`paper/draft.md`](paper/draft.md).

### Compiled Manifest Pattern

```yaml
---
name: "github_pr_reviewer"
version: "1.0.0"
description: "Automated PR reviewer"
mcp_servers: ["github-api"]
---
# GitHub PR Reviewer

An agent that performs code reviews...

## Allowed Tools
- `read` - Read changed files
- `grep` - Search for security patterns
```

**Author in Markdown** → **Compile to PostgreSQL** → **Runtime via pgvector**

### Multi-Agent Orchestration

```bash
# Parallel: Multiple agents work simultaneously
volt workflow --pattern parallel \
  --agents '[{"name":"analyst"},{"name":"reviewer"}]' \
  --tasks '["Analyze code","Review security"]'

# Pipeline: Chain agents (output of A → input of B)
volt workflow --pattern pipeline \
  --agents '[{"name":"extractor"},{"name":"summarizer"}]' \
  --tasks '["Extract data","Summarize"]'

# Supervisor: One agent delegates to workers
volt workflow --pattern supervisor \
  --agents '[{"name":"worker1"},{"name":"worker2"}]' \
  --tasks '["Complete complex task"]'
```

### Permission System

Destructive tools (`bash`, `write`, `edit`) require human approval before execution:

```
[approval] tool 'bash({"command": "rm -rf /tmp/*"})' requires approval.
Proceed? [y/N] y
```

### TUI Chat

Interactive terminal UI with:

- Cursor-based input editing (left/right arrows, delete)
- Scrollable message history
- Real-time streaming output
- Ctrl+C interrupt handling

## Installation

### System Requirements

- **Rust**: 1.95+
- **PostgreSQL**: 16+ with `pgvector` extension (required for memory and skill storage)
- **LLM Provider**: Ollama (local), NVIDIA NIM, or any OpenAI-compatible API
- **RAM**: 4GB minimum (16GB recommended when running local models)

### Build from Source

```bash
git clone https://github.com/iixiiartist/volt.git
cd volt
cargo build --release
# Binary at ./target/release/volt
```

## Configuration

### Environment Variables

```bash
# LLM Configuration
export LLM_MODEL="phi4-mini:3.8b"           # or "claude-sonnet-4-5", etc.
export LLM_BASE_URL="http://localhost:11434/v1"
export LLM_API_KEY=""                       # Empty for local Ollama

# Embedding Configuration (auto-detect by default)
export EMBEDDING_PROVIDER="auto"            # "auto", "ollama", "nvidia", "openai", "moonshot"
export EMBEDDING_MODEL="mxbai-embed-large"  # Model override (per-provider default if empty)
export EMBEDDING_ENDPOINT=""                # Endpoint override (per-provider default if empty)
export EMBEDDING_API_KEY=""                 # API key (falls back to NVIDIA_API_KEY / OPENAI_API_KEY)

# Database
export DATABASE_URL="postgres://volt:volt@localhost:5432/volt"
```

### Project Config (`.volt/config.toml`)

Generated by the first-run wizard. Example:

```toml
[agent]
name = "volt-agent"
model = "phi4-mini:3.8b"
max_iterations = 25
temperature = 0.3

[embedding]
model = "mxbai-embed-large"
provider = "auto"

[sandbox]
timeout_ms = 5000
max_stdout_bytes = 262144

[database]
url = "postgres://volt:volt@localhost:5432/volt"
```

### Docker Compose

```bash
# One-command startup
docker compose up -d

# Services: PostgreSQL 16 + pgvector, Volt agent
# Health-check ensures DB is ready before Volt connects
# Environment variables passed through from .env
```

```yaml
# docker-compose.yml (simplified)
services:
  db:
    image: pgvector/pgvector:pg16
    environment:
      POSTGRES_USER: volt
      POSTGRES_PASSWORD: volt
      POSTGRES_DB: volt
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U volt"]
      
  volt:
    build: .
    depends_on:
      db:
        condition: service_healthy
    env_file: .env
```

## Examples

See [`examples/`](./examples) for reference skills:

- **GitHub PR Reviewer**: Code review with security pattern scanning
- **System Diagnostics**: Local system health checks
- **Data Pipeline**: ETL with error handling

## Testing & Benchmarks

```bash
# Run all Rust tests
cargo test --features testutils

# Run lib tests only (faster, no DB needed)
cargo test --lib

# BFCL benchmark (function-calling accuracy, static vs RAG)
python volt-bfcl/benchmark.py --mode both --category simple_python --distractors 50 --limit 30

# ProgramBench (coding puzzles)
python volt-bfcl/program_bench.py --model llama-3.1-8b-instant --limit 10

# GAIA benchmark (general AI assistants, requires GAIA dataset)
python volt-bfcl/gaia_benchmark.py --model llama-3.1-8b-instant --limit 10

# Rust integration tests (multi-agent workflows)
cargo test --test workflow_bench -- --nocapture

# Run with coverage
cargo tarpaulin --out Html

# Check formatting
cargo fmt -- --check

# Lint
cargo clippy -- -D warnings
```

## Performance

Benchmarked on BFCL V4 with Groq llama-3.1-8b-instant (50 distractor tools).

| Metric | Static (all 51 tools) | Volt RAG (top-8) |
|---|---|---|
| **Avg prompt tokens/task** | 2,248 | **579** |
| **Avg latency/task** | 328ms | **224ms** |
| **Avg accuracy** | 34.3% | **41.0%** |
| **Token cost/1k tasks** | ~$0.15 | **~$0.04** |
| **Tool search latency** | — | <1ms (in-memory cosine sim) |
| **Cold start** | <100ms | <100ms |
| **Binary size** | ~18MB | ~18MB |

Token savings scale with registry size: **72% at 20 tools, 92% at 100, 98.4% at 500**.
Full methodology in [`paper/draft.md`](paper/draft.md).

## Security

- **Permission Gating**: Destructive tools (`bash`, `write`, `edit`, `web_fetch`, `delegate`) require human approval by default
- **Autonomous Mode**: `--allow` flag skips all approval prompts for CI/automation
- **Path Traversal Protection**: `sanitize_path()` uses `canonicalize()` + project-root jail — blocks traversal outside project directory
- **SSRF Protection**: `validate_url()` blocks private IPs (10.x, 172.16-31.x, 192.168.x, 127.x), disallowed schemes (file:, gopher:, etc.), and suspicious ports
- **Prompt Injection Defense**: `sanitize_prompt_input()` strips null bytes and control characters; truncates context to 2KB and delegate tasks to 5KB; adds injection guard marker
- **Async Safety**: All blocking `stdin().read_line()` calls wrapped in `spawn_blocking` to prevent tokio worker starvation
- **No Hardcoded Credentials**: `DATABASE_URL` must be set via env var or `.volt/config.toml`; no default
- **Sandbox Execution**: Provisioned tools run in isolated subprocesses with cleared environments
- **Input Validation**: Tool arguments validated against JSON Schema
- **No Runtime Parsing**: SKILL.md is compiled at provision time, not interpreted during execution
- **Audit Logging**: All tool executions recorded in PostgreSQL

## Roadmap

### v0.1 (current)

- [x] Dynamic RAG Loop (Tools + Skills + Memories)
- [x] Compiled Manifest Pattern
- [x] Multi-Agent Orchestration (parallel, pipeline, supervisor)
- [x] Permission System
- [x] TUI with cursor editing
- [x] Security hardening (SSRF, path traversal, prompt injection, async safety)
- [x] Smart Embedding Router (auto-detect + multi-provider fallback)
- [x] Skill Catalog (remote + local, 5 curated skills, install/search/list)
- [x] Cross-Platform Skill Importer (Claude, Cursor, Copilot, OpenCode, Markdown)
- [x] First-Run Wizard (interactive LLM + DB setup)
- [x] Docker Compose (PostgreSQL 16 + pgvector + Volt)
- [x] Autonomous Mode (`--allow` flag)

### v0.2 (next)

- [x] Multi-agent token tracking
- [x] OS-aware shell tool (cmd/powershell on Windows, bash on Unix)
- [x] 17 built-in tools (PDF, charts, desktop automation, browser, screenshot)
- [x] BFCL benchmark harness (static vs RAG comparison)
- [x] ProgramBench + GAIA benchmark adapters
- [x] Academic paper draft (74% token savings verified)

### Near-term

- [ ] Binary releases (Linux/macOS, Windows)
- [ ] GAIA full evaluation (165-dev set)
- [ ] SWE-bench Lite evaluation
- [ ] IDE extensions (VS Code)
- [ ] Web dashboard for agent monitoring

### Later

- [ ] Multi-modal support (image, PDF input via vision models)
- [ ] Distributed agent coordination

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/your-feature`)
3. Commit your changes (`git commit -am 'Add your feature'`)
4. Push and open a Pull Request

See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.

## License

MIT — see [LICENSE](./LICENSE) for details.

## Built With

- [Rust](https://www.rust-lang.org) — Performance and memory safety
- [pgvector](https://github.com/pgvector/pgvector) — Vector similarity search
- [ratatui](https://ratatui.rs) — Terminal UI framework
- [tokio](https://tokio.rs) — Async runtime
- [sqlx](https://github.com/launchbadge/sqlx) — Database access
- [axum](https://github.com/tokio-rs/axum) — HTTP server
- [Docker](https://www.docker.com) — Containerized deployment
- [PostgreSQL](https://www.postgresql.org) — Relational + vector storage

---

**Volt** — The Autonomous Systems Engine. Maintained by [Setique Labs, Inc.](https://setique.com)
