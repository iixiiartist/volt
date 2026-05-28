# Volt — The Autonomous Systems Engine

> **Rust-native AI agent framework with unified RAG across 12 context fields, background auto-seeding worker, multi-agent orchestration, and 39+ built-in tools. 100% accuracy at 200 distractors (BFCL-verified). [Paper.](paper/draft.md)**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT) [![Rust](https://img.shields.io/badge/Rust-1.95+-orange.svg)](https://www.rust-lang.org) [![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.20371211.svg)](https://doi.org/10.5281/zenodo.20371211)

## Why Volt?

Most agent frameworks inject every available tool into every LLM call. Volt replaces static injection with **unified dynamic RAG** — tools, skills, memories, conversation history, artifacts, MCP configs, permissions, and security policies are all retrieved via vector similarity, so the model only sees what's relevant.

**Verified results (BFCL V4, 200 distractors, argument-aware evaluation):**
- **100% accuracy** on llama-3.1-8b-instant — matching 70b-class performance
- **Flat tool-count scaling curve** — accuracy invariant from 1 to 200+ tools
- **74% token savings** vs static injection (470 cases, ~$0.37 total)

Key design decisions:

- **Everything-as-RAG**: 12 context kinds dynamically retrievable from unified vector store
- **Background Auto-Seeding Worker**: MPSC channel daemon maintains context autonomously via Tokio
- **Four-Pillar Eviction**: Semantic dedup, per-kind quotas, composite scores, episodic merging
- **39+ Built-in Tools**: File I/O, shell, web, git, time, reasoning, data, PDF, charts, desktop, browser
- **Multi-Agent Orchestration**: Parallel, pipeline, supervisor, and DAG patterns
- **pgvector Persistence**: PostgreSQL with HNSW indexes, context survives restarts
- **Local ONNX Embeddings**: BGE-large-en-v1.5 (1024d) via tract-onnx, no C++ dependency
- **Single Binary**: Rust-compiled, MIT license

## Quick Start

```bash
# Prerequisites: Rust 1.85+, PostgreSQL 16+ with pgvector (optional)
git clone https://github.com/iixiiartist/volt.git
cd volt
cargo build --release

# Initialize database schema
./target/release/volt init-db

# Single-shot execution
./target/release/volt agent-run --input "Analyze this codebase" --allow

# Multi-agent parallel workflow (use --agents-file to avoid shell quoting issues)
cat > agents.json << 'EOF'
[{"name":"analyst"},{"name":"reviewer"}]
EOF
./target/release/volt workflow --pattern parallel \
  --agents-file agents.json \
  --tasks '["Analyze code","Review security"]'

# Interactive TUI session
./target/release/volt agent-tui

# End-to-end benchmark with argument validation
python volt-bfcl/volt_bench.py --category simple_python --distractors 200
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Volt Agent Loop                          │
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

### Benchmark Results

| Configuration | Accuracy | Latency | n |
|---|---|---|---|
| 8B + 200 distractors | **100%** | 53s/case | 20 |
| 70B + 200 distractors | 90% | 48s/case | 10 |
| 8B + 0 distractors | 100% | 31s/case | 5 |
| 8B + 100 distractors | 100% | 43s/case | 5 |

**Tool-count scaling: flat curve.** Accuracy invariant from 0 to 200 distractors.

### Built-in Tools (39+)

| Category | Tools | Feature Flag |
|---|---|---|
| **File I/O** | `read`, `write`, `edit`, `glob`, `grep` | built-in |
| **Shell** | `bash` | built-in |
| **Web** | `web_fetch`, `web_scrape`, `web_scrape_all`, `web_search`, `you_research`, `you_contents` | built-in |
| **Data** | `json_validate`, `json_prettify`, `json_query`, `csv_read`, `csv_write`, `archive_extract`, `archive_create` | built-in |
| **Memory** | `memory_append`, `todo_add` | built-in |
| **Git** | `git_status`, `git_diff`, `git_diff_unstaged`, `git_diff_staged`, `git_add`, `git_commit`, `git_reset`, `git_log`, `git_branch`, `git_checkout`, `git_show`, `git_create_branch` | built-in |
| **Time** | `get_current_time`, `convert_time` | built-in |
| **Reasoning** | `sequentialthinking` | built-in |
| **Charts** | `create_bar_chart`, `create_line_chart` | built-in |
| **Screenshot/PDF/Desktop/Browser** | Feature-gated (12 tools) | opt-in features |
| **Delegation** | `delegate`, `run_workflow`, `final_answer` | built-in |
| **MCP** | SearchHQ (19 tools), extensible via `volt mcp-serve` | built-in |

### Embedding

Local ONNX inference via tract-onnx with `Xenova/bge-large-en-v1.5` (1024d, ~337MB). Configure via `VOLT_ONNX_MODEL_DIR` or `EMBEDDING_MODEL`. Falls back to deterministic SHA-256 placeholder embeddings when no network or local model is available.

### Multi-Agent Orchestration

Parallel, pipeline, supervisor, and DAG-based multi-agent patterns with topological scheduling and parallel level execution.

### Permission System

23 tools default to `PermissionLevel::Prompt`. Autonomous mode with `--allow` (`-a`). Human-in-the-loop enforced at the Rust compiler level via the `attenuation` module.

## Testing

```bash
# Full test suite
cargo test --features testutils

# Unit tests (63)
cargo test --lib --features testutils

# Professional workflow tests (24)
cargo test --test professional_workflows --features testutils

# Real-world benchmarks (11)
cargo test --test real_world_benchmarks --features testutils

# Benchmarks
python volt-bfcl/volt_bench.py --distractors 200 --model llama-3.1-8b-instant
```

## CI/CD

Volt uses **GitLab CI** with automated testing and cross-platform builds.

**Required CI/CD variables:**
- `DATABASE_URL` — Postgres connection string
- `GITHUB_TOKEN` — GitHub personal access token (for release syncing)

## Performance

| Metric | Value |
|---|---|
| Binary size | ~10 MB Linux, ~20 MB Windows |
| Cold start | <100ms |
| Tool search | <5µs (in-memory cosine, DashMap single-pass) |
| Memory search | <5ms (pgvector HNSW) |
| Token savings | 74% vs static injection |
| Accuracy (200 distractors) | 100% (argument-validated) |

## License

MIT — see [LICENSE](./LICENSE) for details.

**Volt** — The Autonomous Systems Engine. Built in Rust by [Setique Labs](https://setique.com).
