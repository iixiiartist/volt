# Volt - The Autonomous Systems Engine

> **Production-grade, locally-runnable AI agent framework with dynamic RAG, multi-agent orchestration, and compiled manifest pattern.**

[![CI](https://github.com/iixiiartist/volt/actions/workflows/ci.yml/badge.svg)](https://github.com/iixiiartist/volt/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.95+-orange.svg)](https://www.rust-lang.org)

## Why Volt?

Volt is not just another AI agent framework. It's a **production-grade Autonomous Systems Engine** built in Rust with:

- **Dynamic RAG Loop**: Tools, skills, and memories are retrieved via pgvector cosine similarity — not hardcoded. Up to **75% fewer input tokens** per LLM call.
- **Compiled Manifest Pattern**: Author skills in Markdown (`SKILL.md`), compile into PostgreSQL with HNSW indexing. Human-friendly authoring, machine-optimized runtime.
- **Multi-Agent Orchestration**: Parallel, pipeline, and supervisor patterns built-in.
- **Polyglot Execution Sandbox**: Tools written in Python, TypeScript, Bash, or Mojo run in kernel-isolated `unshare` namespaces with `prlimit` boundaries.
- **Zero Dependencies**: Single 18MB Rust binary. No Python, Node, or Docker required at runtime.
- **MCP Native**: Full Model Context Protocol support for tool interoperability.

## Quick Start

### Install

```bash
# Download the binary (Linux/Mac/Windows)
curl -fsSL https://github.com/iixiiartist/volt/releases/latest/download/volt | sh

# Or build from source
cargo install volt
```

### Run Your First Agent

```bash
# Interactive chat with dynamic tool selection
volt agent-chat

# Single-shot execution
volt agent-run --input "Analyze this codebase for security issues"

# Compile a skill
volt provision-skill --path ./examples/github-pr-reviewer/SKILL.md
```

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
│  LLM Call (Kimi/Claude/GPT) - 75% Fewer Tokens              │
└─────────────────────────────────────────────────────────────┘
```

## Features

### Dynamic RAG Loop

Every agent turn performs **semantic search** across three knowledge sources:

1. **Tools**: 12+ built-in tools (`read`, `write`, `bash`, `grep`, `glob`, `web_fetch`, etc.) + registry tools. Only the top-8 most relevant are shown to the LLM.
2. **Skills**: Compiled from `SKILL.md` files. Context-priming instructions injected as system messages.
3. **Memories**: Persistent conversation history stored in PostgreSQL with pgvector. Temporal RAG for long-running tasks.

### Compiled Manifest Pattern

```markdown
---
name: "github_pr_reviewer"
version: "1.0.0"
description: "Automated PR reviewer"
mcp_servers: ["github-api"]
---
# GitHub PR Reviewer

An intelligent agent that performs comprehensive code reviews...

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

Destructive tools (`bash`, `write`, `edit`) require **human approval** before execution:

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

### From Source

```bash
# Prerequisites: Rust 1.95+, PostgreSQL 16+ with pgvector
git clone https://github.com/iixiiartist/volt.git
cd volt
cargo install --path .
```

### System Requirements

- **Rust**: 1.95+
- **PostgreSQL**: 16+ with `pgvector` extension (for memory/skills RAG)
- **LLM Provider**: Ollama (local), NVIDIA NIM, or any OpenAI-compatible API
- **RAM**: 4GB minimum (16GB recommended for local models)

## Configuration

### Environment Variables

```bash
# LLM Configuration
export LLM_MODEL="phi4-mini:3.8b"           # or "claude-3-5-sonnet"
export LLM_BASE_URL="http://localhost:11434/v1"
export LLM_API_KEY=""                       # Empty for local Ollama

# Embedding Configuration
export EMBEDDING_MODEL="mxbai-embed-large"
export EMBEDDING_PROVIDER="ollama"
export EMBEDDING_ENDPOINT="http://localhost:11434/v1"

# Database
export DATABASE_URL="postgres://volt:volt@localhost:5432/volt"
```

### Project Config (`.volt/config.toml`)

```toml
[agent]
name = "my-agent"
model = "phi4-mini:3.8b"
max_iterations = 25
temperature = 0.3

[embedding]
model = "mxbai-embed-large"
provider = "ollama"

[sandbox]
timeout_ms = 5000
max_stdout_bytes = 262144
```

## Examples

See [`examples/`](./examples/) for production-ready skills:

- **GitHub PR Reviewer**: Automated code review with security scanning
- **System Diagnostics**: Local system health checks
- **Data Pipeline**: ETL with error handling

## Testing

```bash
# Run all tests
cargo test

# Run with coverage
cargo tarpaulin --out Html

# Check formatting
cargo fmt -- --check

# Lint
cargo clippy -- -D warnings
```

## Performance

| Metric | Value |
|--------|-------|
| Binary Size | 18MB (statically linked) |
| Tool Search Latency | <1ms (HNSW index) |
| Memory Search Latency | <5ms (pgvector) |
| Token Reduction | 75% vs. static tool list |
| Cold Start | <100ms |

## Security

- **Permission Gating**: Destructive tools require human approval
- **Sandbox Execution**: Provisioned tools run in isolated environments
- **Input Validation**: All tool arguments are validated against JSON Schema
- **No Code Injection**: SKILL.md is compiled, not interpreted at runtime
- **Audit Logging**: All tool executions are recorded in PostgreSQL

## Roadmap

### Q1 2026
- [x] Dynamic RAG Loop (Tools + Skills + Memories)
- [x] Compiled Manifest Pattern
- [x] Multi-Agent Orchestration
- [x] Permission System
- [x] TUI with cursor editing

### Q2 2026
- [ ] **gVisor & Firecracker Integration**: Transitioning local sub-process execution from kernel namespaces to dedicated microVM structures
- [ ] **The Volt Skill Marketplace**: A decentralized, cryptographic ledger registry for distributing compiled multi-language manifests safely
- [ ] **Native C-FFI / Mojo Matrix Bridges**: Native bindings for ultra-low latency local tensor math tools without interpreter overhead
- [ ] **IDE Extensions**: VS Code and JetBrains plugins for in-editor agent assistance

### Q3 2026
- [ ] **Distributed Agent Federation**: Asynchronous secure TCP nodes for multi-machine agent coordination
- [ ] **Web Dashboard**: Real-time agent monitoring and skill management UI
- [ ] **Git-Aware Diff Visualization**: Contextual code review with semantic diff highlighting
- [ ] **Multi-modal Support**: Image, PDF, and document understanding via vision models

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing`)
3. Commit your changes (`git commit -am 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing`)
5. Open a Pull Request

See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.

## License

MIT License - see [LICENSE](./LICENSE) for details.

## Acknowledgments

Built with:
- [Rust](https://www.rust-lang.org) - Performance and safety
- [pgvector](https://github.com/pgvector/pgvector) - Vector similarity
- [ratatui](https://ratatui.rs) - TUI framework
- [tokio](https://tokio.rs) - Async runtime
- [sqlx](https://github.com/launchbadge/sqlx) - Database access

---

**Volt** — The Autonomous Systems Engine. Maintained by [Setique Labs, Inc.](https://volt.setique.com). Built for production, designed for developers.