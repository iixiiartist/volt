# Volt Architecture Documentation

## Overview

Volt is an AI agent framework built in Rust that implements a **Unified RAG Loop** for dynamic tool, skill, and memory retrieval. Rather than hardcoding all available tools into every LLM call, Volt embeds the current query context and retrieves only the most relevant tools, skills, and memories per turn.

The project is under active development. The architecture described here reflects what is currently implemented.

**Verified result**: On BFCL V4 with a 51-tool registry, dynamic RAG selection reduces per-turn prompt tokens by **74%** (2,248 → 579 avg) and improves function-calling accuracy by **+6.7 percentage points**. Full methodology in [`paper/draft.md`](paper/draft.md).

---

## Core Design Decisions

### 1. Unified RAG Loop

Every agent turn performs semantic search across three knowledge sources:

```
User Query + Context
    ↓
[pgvector Cosine Search - HNSW Index]
    ↓
┌──────────┬──────────┬──────────┐
│ Top-8    │ Top-3    │ Top-5    │
│ Tools    │ Skills   │ Memories │
└──────────┴──────────┴──────────┘
    ↓
[System Prompt Construction]
    ↓
[LLM Call]
```

**Why this matters:**
The benefit scales with registry size. Token savings at different registry sizes (BFCL-verified):

| Registry Size | Token Savings |
| ------------- | ------------- |
| 20 tools      | ~72%          |
| 51 tools      | 74%           |
| 100 tools     | ~92%          |
| 500 tools     | ~98.4%        |

Fallback tools (`read`, `glob`, `grep`, `web_fetch`) are always included regardless of similarity score to ensure basic capabilities are never absent.

Latencies on current hardware:
- Tool search: <1ms (in-memory cosine similarity)
- Memory search: <5ms (pgvector HNSW)
- Cold start: <100ms

### 2. Compiled Manifest Pattern

Volt uses a compile-time approach to skill definition:

```
SKILL.md (Human-Readable)
    ↓ [volt provision-skill]
PostgreSQL + pgvector (Runtime)
    ↓ [HNSW Index]
Sub-millisecond Vector Search
```

**Why not parse Markdown at runtime?**
- Regex-based frontmatter parsing is brittle across formatting variations
- Markdown has no relational structure — can't enforce foreign key constraints between skills and tools
- MCP expects JSON Schema; Markdown needs a compilation step regardless

**The approach:**
- Author in Markdown (developer-friendly, diffable)
- Compile to relational tables at provision time
- Query via HNSW index at runtime

### 3. Multi-Agent Orchestration

Three patterns are implemented in the core, each with per-agent token tracking:

| Pattern        | Use Case            | Example                        |
| -------------- | ------------------- | ------------------------------ |
| **Parallel**   | Independent tasks   | Analyze code + Review security |
| **Pipeline**   | Sequential chaining | Extract → Transform → Load     |
| **Supervisor** | Dynamic delegation  | One agent delegates to workers |

Token usage is surfaced per step:

```text
Step: [PASS] data-agent (877 ms, 3,094P+476C tokens)
Total: 12,703 prompt + 2,078 completion = 14,781 tokens
```

### 4. Permission System

Destructive operations require human approval before execution by default. Pass `--allow` to skip for CI/automation.

```
[approval] tool 'bash({"command": "rm -rf /tmp/*"})' requires approval.
Proceed? [y/N] y
```

**Protected tools (default: Prompt):**
- `bash` — Shell command execution
- `read` — File read
- `write` — File modification
- `edit` — In-place file editing
- `web_fetch` — External HTTP requests
- `web_scrape` / `web_scrape_all` — Web scraping
- `delegate` — Sub-agent spawning
- `screenshot` — Screen capture
- `create_pdf` — PDF generation
- `desktop_click` / `desktop_type` / `desktop_key` — Desktop input
- `browser_navigate` / `browser_extract` / `browser_screenshot` — Browser control

**Autonomous mode:**
```bash
volt agent-chat --allow           # Skip all approvals for the session
volt agent-chat --allow-session   # Approve once, persist for session
```

### 5. Memory as Temporal RAG

Conversations are stored in PostgreSQL with pgvector, enabling semantic retrieval of past context across turns and sessions:

```sql
CREATE TABLE memories (
    id BIGSERIAL PRIMARY KEY,
    kind VARCHAR(100),
    content TEXT,
    embedding vector(1024),
    session_id UUID,
    created_at TIMESTAMPTZ
);

CREATE INDEX ON memories USING hnsw (embedding vector_cosine_ops);
```

### 6. Smart Embedding Router

Volt auto-detects available embedding providers and builds a fallback chain at startup:

1. **Ollama** — local, auto-detected via health check, no API key needed
2. **NVIDIA NIM** — cloud, if `NVIDIA_API_KEY` or `EMBEDDING_API_KEY` is set and non-placeholder
3. **OpenAI** — cloud, if `OPENAI_API_KEY` is set
4. **Moonshot** — cloud, if `KIMI_API_KEY` is set
5. **Deterministic placeholder** — SHA-256-based, always available, no network required

Set `EMBEDDING_PROVIDER=auto` (default) for full auto-detection, or pin to a specific provider.

---

## Tool Registry

### Built-in Tools (17)

All tools are behind Cargo feature flags. All flags are enabled by default.

| Category     | Tool                  | Permission | Feature Flag        |
| ------------ | --------------------- | ---------- | ------------------- |
| **File I/O** | `read`                | Prompt     | built-in            |
|              | `write`               | Prompt     | built-in            |
|              | `edit`                | Prompt     | built-in            |
|              | `glob`                | Allow      | built-in            |
|              | `grep`                | Allow      | built-in            |
| **Shell**    | `bash`                | Prompt     | built-in            |
| **Web**      | `web_fetch`           | Prompt     | built-in            |
|              | `web_scrape`          | Prompt     | built-in            |
|              | `web_scrape_all`      | Prompt     | built-in            |
| **Data**     | `json_validate`       | Allow      | built-in            |
|              | `json_prettify`       | Allow      | built-in            |
|              | `json_query`          | Allow      | built-in            |
|              | `csv_read`            | Allow      | built-in            |
|              | `csv_write`           | Allow      | built-in            |
| **Archives** | `archive_extract`     | Allow      | built-in            |
|              | `archive_create`      | Allow      | built-in            |
| **Memory**   | `memory_append`       | Allow      | built-in            |
|              | `todo_add`            | Allow      | built-in            |
| **Charts**   | `create_bar_chart`    | Allow      | built-in            |
|              | `create_line_chart`   | Allow      | built-in            |
| **Screenshot** | `screenshot`        | Prompt     | `tools-screenshot`  |
| **PDF**      | `create_pdf`          | Prompt     | `tools-pdf`         |
| **Desktop**  | `desktop_click`       | Prompt     | `tools-desktop`     |
|              | `desktop_type`        | Prompt     | `tools-desktop`     |
|              | `desktop_key`         | Prompt     | `tools-desktop`     |
|              | `desktop_find_window` | Allow      | `tools-desktop`     |
| **Browser**  | `browser_navigate`    | Prompt     | `tools-browser`     |
|              | `browser_extract`     | Prompt     | `tools-browser`     |
|              | `browser_screenshot`  | Prompt     | `tools-browser`     |
| **Delegation** | `delegate`          | Prompt     | built-in            |
|              | `run_workflow`        | Allow      | built-in            |
| **MCP (external)** | `searchhq_*` (19 tools) | Allow | `register_searchhq_tools()` |

External MCP tools (e.g., SearchHQ's 19 research tools) are **not compiled in**. They are discovered at runtime via `register_searchhq_tools()` which calls the MCP server's `tools/list`, then registers each tool in the ToolRegistry dynamically. These tools go through the same embedding + cosine similarity RAG pipeline as built-in tools — the same 74% token savings applies.

```rust
let registry = ToolRegistry::new();
let count = volt::tools::searchhq::register_searchhq_tools(&registry, api_token).await?;
// 19 tools now participate in RAG-based retrieval
```

### Dynamic Tool Selection

At runtime:
1. Embed the current query + last 3 messages as context
2. Search `agent_tools` via in-memory cosine similarity
3. Return top-8 most similar tools
4. Always include fallback tools (`read`, `glob`, `grep`, `web_fetch`) regardless of score

```rust
let query_embedding = embedder.embed(&context_query).await?;
let tools = tools.search_tools(&query_embedding, 8, &["read", "glob", "grep", "web_fetch"]).await;
```

### OS-Aware Shell

The `bash` tool dispatches to the correct shell per platform:
- **Unix/macOS**: `/bin/bash` with env_clear
- **Windows**: `cmd.exe` with automatic fallback

---

## MCP Client

Volt has a built-in JSON-RPC MCP client (`src/mcp/client.rs`) for connecting to external MCP servers. It supports both HTTP and stdio transports.

### HTTP Transport with Bearer Auth

```rust
let transport = MCPTransport::Http {
    url: "https://server.example.com/mcp".into(),
    headers: None,  // or pass custom headers
};
let client = MCPClient::new(transport);
client.set_token("eyJ...");  // Bearer token

// List tools
let tools = client.list_tools().await?;

// Call a tool
let result = client.call_tool("tool_name", &json!({...})).await?;
```

### Stdio Transport

```rust
let transport = MCPTransport::Stdio {
    command: "npx".into(),
    args: vec!["@modelcontextprotocol/server-filesystem".into(), "/path".into()],
};
```

### Dynamic Registration

The `register_searchhq_tools()` function in `src/tools/searchhq.rs` demonstrates the adapter pattern: it calls the MCP server's `tools/list`, maps each returned tool to a Volt `ToolDefinition`, and registers it in the ToolRegistry. Once registered, external MCP tools are indistinguishable from built-in tools — they are embedded, retrieved via cosine similarity, and injected only when relevant (top-8 per turn).

---

## Skills System

### SKILL.md Format

```yaml
---
name: "github_pr_reviewer"
version: "1.0.0"
description: "Automated PR reviewer"
mcp_servers: ["github-api"]
---
# GitHub PR Reviewer

Detailed description...

## Allowed Tools
- `read` - Read files
- `grep` - Search patterns
```

### Compilation Process

```bash
volt provision-skill --path ./examples/github-pr-reviewer/SKILL.md
```

Steps:
1. Parse YAML frontmatter
2. Extract description for embedding
3. Generate 1024-dim vector via embedding router
4. Insert into `skills` table with HNSW index
5. Map allowed tools to `skill_tools` join table

### Skill Catalog & Importer

**Catalog**: Remote skill index with curated skills. Commands:
```bash
volt list-catalog-skills
volt search-catalog-skills --query "code review"
volt install-skill --name "github-pr-reviewer"
```

**Importer**: Auto-detects and converts skills from 5 source formats into Volt-native SKILL.md:
- Claude (`CLAUDE.md`)
- Cursor (`.cursorrules`)
- GitHub Copilot (`.github/copilot-instructions.md`)
- OpenCode (`.opencode/AGENTS.md`)
- Vanilla Markdown

```bash
volt import-skill --path /path/to/other-platform-skill.md
```

Supports batch import (e.g., 269 OpenCode skills in one pass).

---

## Agent Loop

### Execution Flow

```rust
async fn run(&self, input: &str) -> Result<String> {
    // 1. Sanitize input (null bytes, control chars, length limits)
    let input = sanitize_prompt_input(input);

    // 2. Build context query
    let context = build_context(&self.messages, input);

    // 3. Embed context via smart embedding router
    let embedding = embedder.embed(&context).await?;

    // 4. Search tools (dynamic RAG)
    let tools = tools.search(&embedding, 8, &fallback).await;

    // 5. Search skills (context priming)
    let skills = skills.search(&embedding, 3).await;

    // 6. Search memories (temporal RAG)
    let memories = memories.search(&embedding, 5).await;

    // 7. Construct system prompt
    let prompt = build_prompt(&tools, &skills, &memories);

    // 8. Call LLM, track tokens
    let response = llm.complete(&prompt).await?;
    track_tokens(&response.usage);

    // 9. Execute tool calls (with permission checks)
    for tool_call in response.tool_calls {
        if needs_approval(&tool_call) && !self.allow_mode {
            spawn_blocking(|| ask_user()).await?;
        }
        execute_tool(&tool_call)?;
    }

    // 10. Store memory
    memories.store(&response.content).await?;

    Ok(response.content)
}
```

### First-Run Wizard

`volt init` runs an interactive setup that configures:
- LLM provider, model, and API key
- Database URL
- Embedding provider

Writes `.volt/config.toml` and `.env`. Also runs automatically on first startup when stdin is a TTY.

---

## Database Schema

### Core Tables

```sql
-- Tools (built-in and registered)
CREATE TABLE agent_tools (
    id SERIAL PRIMARY KEY,
    tool_name VARCHAR(255) UNIQUE,
    description TEXT,
    embedding vector(1024),
    parameter_schema JSONB,
    is_marketplace_verified BOOLEAN
);

-- Skills (compiled from SKILL.md)
CREATE TABLE skills (
    id UUID PRIMARY KEY,
    name TEXT UNIQUE,
    description TEXT,
    content TEXT,
    embedding vector(1024),
    mcp_servers TEXT[],
    source_path TEXT
);

-- Memories (conversation history)
CREATE TABLE memories (
    id BIGSERIAL PRIMARY KEY,
    kind VARCHAR(100),
    content TEXT,
    embedding vector(1024),
    session_id UUID,
    created_at TIMESTAMPTZ
);

-- Tool executions (audit log)
CREATE TABLE tool_executions (
    id BIGSERIAL PRIMARY KEY,
    tool_name VARCHAR(255),
    input JSONB,
    output JSONB,
    status VARCHAR(40),
    duration_ms INT,
    execution_id UUID
);
```

### Indexes

```sql
-- HNSW for vector search
CREATE INDEX ON agent_tools USING hnsw (embedding vector_cosine_ops);
CREATE INDEX ON skills USING hnsw (embedding vector_cosine_ops);
CREATE INDEX ON memories USING hnsw (embedding vector_cosine_ops);

-- B-tree for exact lookups
CREATE INDEX ON agent_tools(tool_name);
CREATE INDEX ON skills(name);
CREATE INDEX ON memories(kind);
```

---

## Security Model

### Permission Levels

```rust
enum PermissionLevel {
    Allow,   // No approval needed
    Prompt,  // Human approval required (skip with --allow)
}
```

### Path Traversal Protection

```rust
fn sanitize_path(path: &str, project_root: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    if !canonical.starts_with(project_root) {
        return Err(anyhow!("Path traversal outside project root"));
    }
    Ok(canonical)
}
```

### SSRF Protection

`validate_url()` blocks:
- Private IP ranges (`10.x`, `172.16–31.x`, `192.168.x`, `127.x`)
- Disallowed schemes (`file:`, `gopher:`, etc.)
- Suspicious ports

### Prompt Injection Defense

`sanitize_prompt_input()`:
- Strips null bytes and control characters
- Truncates context to 2KB, delegate tasks to 5KB
- Adds injection guard marker to delineate user content from system content

### Async Safety

All blocking `stdin().read_line()` calls are wrapped in `spawn_blocking` to prevent tokio worker thread starvation during approval prompts.

### Sandboxing

Provisioned tools run in isolated subprocesses:
- **Environment clearing**: `env_clear()` before execution
- **Explicit PATH**: Only `/usr/bin:/bin` (Unix) or system default (Windows)
- **Timeout**: 5s default, configurable
- **Output truncation**: 256KB max stdout

### Audit Logging

Every tool execution is recorded to `tool_executions` with full input/output, status, and duration.

---

## Benchmarks

### BFCL V4 (Berkeley Function Calling Leaderboard)

Tests function-calling accuracy with controlled distractor tool counts. Volt's benchmark harness compares static injection (all tools in every prompt) vs. dynamic RAG selection (top-8 per turn).

**Results (51-tool registry, 50 distractors, Groq llama-3.1-8b-instant):**

| Metric                      | Static (all 51 tools) | Volt RAG (top-8) | Improvement  |
| --------------------------- | --------------------- | ---------------- | ------------ |
| Avg prompt tokens/task      | 2,248                 | 579              | **−74%**     |
| Avg latency/task            | 328ms                 | 224ms            | **−32%**     |
| Avg accuracy                | 34.3%                 | 41.0%            | **+6.7pp**   |
| Token cost/1k tasks         | ~$0.15                | ~$0.04           | **−73%**     |
| `simple_python` accuracy    | 80.0%                 | 98.0%            | **+18pp**    |
| `simple_javascript` accuracy| 58.0%                 | 68.0%            | **+10pp**    |

Token savings scale with registry size: 72% at 20 tools → 98.4% at 500 tools.

Full methodology: [`paper/draft.md`](paper/draft.md)

```bash
# Run the benchmark
python volt-bfcl/benchmark.py --mode both --category simple_python --distractors 50 --limit 30
```

### ProgramBench

8 programming puzzles exercised through the actual `Agent::run()` loop — not a simulation harness. Current pass rate: **100%** on the included puzzle set.

```bash
python volt-bfcl/program_bench.py --model llama-3.1-8b-instant --limit 10
```

### GAIA

General AI assistant benchmark. Currently validated on 3 QA questions via `Agent::run()`. Full 165-question dev set evaluation is on the roadmap (requires `huggingface-cli login`).

```bash
python volt-bfcl/gaia_benchmark.py --model llama-3.1-8b-instant --limit 10
```

---

## Performance

| Metric              | Value                    | Notes                                          |
| ------------------- | ------------------------ | ---------------------------------------------- |
| Binary size         | ~18MB                    | Statically linked Rust                         |
| Cold start          | <100ms                   | No interpreter startup                         |
| Tool search         | <1ms                     | In-memory cosine similarity                    |
| Memory search       | <5ms                     | pgvector HNSW, up to ~10K entries              |
| Avg prompt tokens   | 579/task                 | BFCL V4, 51-tool registry, top-8 selection     |
| Token savings       | 74%                      | vs. static injection, BFCL-verified            |
| Function-call acc.  | +6.7pp                   | vs. static injection, BFCL V4                  |

---

## Deployment

### Docker (Recommended)

```bash
git clone https://github.com/iixiiartist/volt.git
cd volt
docker compose up -d
```

PostgreSQL 16 with pgvector starts first (health-checked), then Volt connects. Environment variables passed through from `.env`.

### Build from Source

```bash
# Prerequisites: Rust 1.85+, PostgreSQL 16+ with pgvector
git clone https://github.com/iixiiartist/volt.git
cd volt
cargo build --release
# Binary at ./target/release/volt
```

### Configuration

```bash
# LLM
export LLM_MODEL="phi4-mini:3.8b"
export LLM_BASE_URL="http://localhost:11434/v1"
export LLM_API_KEY=""

# Embedding (auto-detect by default)
export EMBEDDING_PROVIDER="auto"

# Database
export DATABASE_URL="postgres://volt:volt@localhost:5432/volt"
```

### CI/CD

See `.github/workflows/ci.yml` for:
- PostgreSQL service setup and schema migration
- Full test suite (`cargo test --features testutils`)
- Binary size check
- Security audit (`cargo audit`)

---

## Roadmap

### v0.1 (shipped)

- [x] Dynamic RAG Loop (Tools + Skills + Memories)
- [x] Compiled Manifest Pattern
- [x] Multi-Agent Orchestration (parallel, pipeline, supervisor)
- [x] Permission System + Autonomous Mode
- [x] TUI with cursor editing
- [x] Security hardening (SSRF, path traversal, prompt injection, async safety)
- [x] Smart Embedding Router (5-provider auto-detect + fallback)
- [x] Skill Catalog + Cross-Platform Importer
- [x] First-Run Wizard + Docker Compose

### v0.2 (shipped)

- [x] 17 built-in tools (PDF, charts, desktop, browser, screenshot)
- [x] Multi-agent token tracking
- [x] OS-aware shell (cmd/powershell on Windows, bash on Unix)
- [x] BFCL benchmark harness + academic paper draft
- [x] ProgramBench + GAIA adapters

### Near-term

- [ ] Binary releases (Linux/macOS, Windows)
- [ ] GAIA full evaluation (165-question dev set)
- [ ] SWE-bench Lite evaluation
- [ ] IDE extensions (VS Code)
- [ ] Web dashboard for agent monitoring

### Later

- [ ] Multi-modal input (images, PDFs via vision models)
- [ ] Distributed agent coordination
- [ ] gVisor / Firecracker microVM sandboxing

---

*Built in Rust by [Setique Labs, Inc.](https://setique.com)*
