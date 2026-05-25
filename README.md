# Volt — The Autonomous Systems Engine

> **Rust-native AI agent framework with unified RAG across 12 context fields, background auto-seeding worker, multi-agent orchestration, and 38 built-in tools. 100% accuracy at 200 distractors (BFCL-verified). [Paper.](paper/draft.md)**

[![GitLab CI](https://gitlab.com/iixiiartist/volt/badges/main/pipeline.svg)](https://gitlab.com/iixiiartist/volt/-/pipelines) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT) [![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org) [![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.20371211.svg)](https://doi.org/10.5281/zenodo.20371211)

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
- **38 Built-in Tools**: File I/O, shell, web, git, time, reasoning, data, PDF, charts, desktop, browser
- **Multi-Agent Orchestration**: Parallel, pipeline, and supervisor patterns
- **pgvector Persistence**: PostgreSQL with HNSW indexes, context survives restarts
- **7 Embedding Providers**: Ollama, llama.cpp, NVIDIA, OpenAI, HuggingFace, Moonshot, deterministic
- **Single Binary**: Rust-compiled, ~13k lines, 57 source files, MIT license

## Quick Start

```bash
# Prerequisites: Rust 1.85+, PostgreSQL 16+ with pgvector (optional for full persistence)
git clone https://github.com/iixiiartist/volt.git
cd volt
cargo build --release

# First-run wizard (interactive LLM + DB setup)
./target/release/volt init

# Interactive chat with dynamic tool selection
./target/release/volt agent-chat

# Single-shot execution (autonomous mode)
./target/release/volt agent-run --input "Analyze this codebase" --allow

# Multi-agent workflow
./target/release/volt workflow --pattern parallel \
  --agents '[{"name":"analyst"},{"name":"reviewer"}]' \
  --tasks '["Analyze code","Review security"]'

# End-to-end benchmark with argument validation
python volt-bfcl/volt_bench.py --category simple_python --distractors 200

# Tool-count scaling ablation
python volt-bfcl/volt_bench.py --category simple_python --sweep

# Multi-turn episodic memory benchmark
python volt-bfcl/multi_turn_bench.py --mode episodic
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

### Built-in Tools (38)

| Category | Tools | Permission |
|---|---|---|
| **File I/O** | `read`, `write`, `edit`, `glob`, `grep` | Prompt/Allow |
| **Shell** | `bash` | Prompt |
| **Web** | `web_fetch`, `web_scrape`, `web_scrape_all` | Prompt |
| **Data** | `json_*` (3), `csv_*` (2), `archive_*` (2) | Allow |
| **Memory** | `memory_append`, `todo_add` | Allow |
| **Git** | `git_status`, `git_diff`, `git_add`, `git_commit`, `git_log` (12) | Allow |
| **Time** | `get_current_time`, `convert_time` | Allow |
| **Reasoning** | `sequentialthinking` | Allow |
| **Charts** | `create_bar_chart`, `create_line_chart` | Allow |
| **Screenshot/PDF/Desktop/Browser** | Feature-gated (12 tools) | Prompt |
| **Delegation** | `delegate`, `run_workflow` | Prompt/Allow |
| **MCP** | SearchHQ (19 tools), extensible | Allow |

### Embedding Providers

7-provider fallback chain: Ollama → llama.cpp → NVIDIA → OpenAI → HuggingFace → Moonshot → deterministic. All normalized to 1024d via pad/truncate. Auto-detection with `EMBEDDING_PROVIDER=auto` or explicit via env var.

### Multi-Agent Orchestration

Parallel, pipeline, and supervisor patterns with per-agent token tracking. 3/3 workflow tests pass.

### Permission System

23 tools default to `PermissionLevel::Prompt`. Autonomous mode with `--allow` (`-a`). Session-level approval with `allow_session`. Human-in-the-loop enforced at the Rust compiler level.

## Testing

```bash
# Full test suite (66 tests)
cargo test --features testutils

# Specific test categories
cargo test --lib                          # 54 unit tests
cargo test --test agent_tests             # 4 agent tests
cargo test --test workflow_bench          # 3 multi-agent tests
cargo test --test bfcl_pipeline           # BFCL pipeline test

# Benchmarks
python volt-bfcl/volt_bench.py --distractors 200 --model llama-3.1-8b-instant
python volt-bfcl/volt_bench.py --sweep      # Tool-count scaling
python volt-bfcl/multi_turn_bench.py         # Episodic memory
```

## CI/CD

Volt uses **GitLab CI** (unlimited free minutes for public repos). The pipeline runs on every push to `main`/`develop` and on every tag for releases.

**Stages:** `test` → `lint` → `security` → `docs` → `build` → `release`

### Setup
1. Mirror this repo to GitLab: **Settings → Repository → Mirroring repositories** (or push directly to GitLab)
2. GitLab CI will auto-detect `.gitlab-ci.yml` and run pipelines

**Required CI/CD variables** (Settings → CI/CD → Variables):
- `DATABASE_URL` — Postgres connection string (optional; tests spin up a pgvector service container)
- `GITHUB_TOKEN` — GitHub personal access token (optional; for pushing releases back to GitHub)

### Self-hosted runners (for macOS & Windows)
GitLab shared runners are Linux-only on the free tier. To build macOS and Windows binaries, add self-hosted runners:
- **macOS**: Install [GitLab Runner](https://docs.gitlab.com/runner/install/osx.html) on a Mac, tag it `macos`
- **Windows**: Install [GitLab Runner](https://docs.gitlab.com/runner/install/windows.html) on a Windows machine, tag it `windows`
- Uncomment the `build_macos_*` and `build_windows` jobs in `.gitlab-ci.yml`

## Performance

| Metric | Value |
|---|---|
| Binary size | ~18MB |
| Cold start | <100ms |
| Tool search | <1ms (in-memory cosine) |
| Memory search | <5ms (pgvector HNSW) |
| Token savings | 74% vs static injection |
| Accuracy (200 distractors) | 100% (argument-validated) |

## License

MIT — see [LICENSE](./LICENSE) for details.

**Volt** — The Autonomous Systems Engine. Built in Rust by [Setique Labs](https://setique.com).
