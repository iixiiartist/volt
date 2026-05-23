# Volt - The Autonomous Systems Engine

> **Locally-runnable AI agent framework with dynamic RAG, multi-agent orchestration, and compiled manifest pattern. Early development — built in public.**

[![CI](https://github.com/iixiiartist/volt/actions/workflows/ci.yml/badge.svg)](https://github.com/iixiiartist/volt/actions) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT) [![Rust](https://img.shields.io/badge/Rust-1.95+-orange.svg)](https://www.rust-lang.org)

## Why Volt?

Most agent frameworks inject every available tool into every LLM call. Volt takes a different approach — tools, skills, and memories are retrieved dynamically via vector similarity, so the model only sees what's relevant to the current task.

Key design decisions:

- **Dynamic RAG Loop**: Tools, skills, and memories are retrieved via pgvector cosine similarity rather than hardcoded into the system prompt. This reduces context overhead on tool-heavy registries.
- **Compiled Manifest Pattern**: Author skills in Markdown (`SKILL.md`), compile into PostgreSQL with HNSW indexing. Human-friendly authoring, efficient runtime retrieval.
- **Multi-Agent Orchestration**: Parallel, pipeline, and supervisor patterns built-in.
- **Polyglot Execution Sandbox**: Tools written in Python, TypeScript, Bash, or Mojo run in isolated subprocesses with environment clearing and output limits.
- **Single Binary**: Rust-compiled, no Python or Node required at runtime. PostgreSQL with pgvector is required for memory and skill storage.
- **MCP Native**: Model Context Protocol support for tool interoperability.
- **Autonomous Mode**: `--allow` flag for unattended execution with session-level approval persistence.

## Status

Volt is under active development. The core agent loop, dynamic RAG, compiled manifest, TUI, skill catalog, cross-platform skill import, and Docker Compose are implemented. Binary releases are not yet published — build from source for now.

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
```

## Features

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

### Dynamic RAG Loop

Every agent turn performs semantic search across three knowledge sources:

1. **Tools**: 12+ built-in tools (`read`, `write`, `bash`, `grep`, `glob`, `web_fetch`, `fetch`, `delegate`, etc.) plus registry tools. Only the top-8 most relevant are included in the LLM call.
2. **Skills**: Compiled from `SKILL.md` files. Context-priming instructions injected as system messages.
3. **Memories**: Persistent conversation history stored in PostgreSQL with pgvector. Useful for long-running tasks and cross-session context.

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

## Testing

```bash
# Run all tests (includes integration tests)
cargo test --features testutils

# Run lib tests only (faster, no DB needed)
cargo test --lib

# Run with coverage
cargo tarpaulin --out Html

# Check formatting
cargo fmt -- --check

# Lint
cargo clippy -- -D warnings
```

## Performance

These numbers reflect benchmarks on the implemented components. Claims will be updated as the system matures.

| Metric                | Value                          |
| --------------------- | ------------------------------ |
| Binary Size           | ~18MB (statically linked)      |
| Tool Search Latency   | <1ms (HNSW, small registry)    |
| Memory Search Latency | <5ms (pgvector)                |
| Context Reduction     | Fewer tools per call vs. static lists (varies by registry size) |
| Cold Start            | <100ms                         |

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

### Near-term

- [ ] Binary releases (Linux/macOS, Windows)
- [ ] Improved sandbox isolation
- [ ] IDE extensions (VS Code)
- [ ] Web dashboard for agent monitoring

### Later

- [ ] Git-aware diff visualization in code review flows
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
