# Volt Architecture Documentation

## Overview

Volt is an AI agent framework built in Rust that implements a **Unified RAG Loop** for dynamic tool, skill, and memory retrieval. Rather than hardcoding all available tools into every LLM call, Volt embeds the current query context and retrieves only the most relevant tools, skills, and memories per turn. This reduces per-call context overhead on registries with many tools and avoids priming the model with irrelevant capabilities.

The project is under active development. The architecture described here reflects what is currently implemented.

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
The benefit scales with registry size. With 12 built-in tools, the savings are modest. As the tool registry grows — especially with domain-specific registered tools — retrieving only the 8 most relevant per query keeps the system prompt lean regardless of how large the registry gets.

Latencies on current hardware:
- Tool search: <1ms (HNSW index, small registry)
- Memory search: <5ms (pgvector)
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

Three patterns are implemented in the core:

| Pattern      | Use Case            | Example                        |
| ------------ | ------------------- | ------------------------------ |
| **Parallel** | Independent tasks   | Analyze code + Review security |
| **Pipeline** | Sequential chaining | Extract → Transform → Load     |
| **Supervisor** | Dynamic delegation | One agent delegates to workers |

### 4. Permission System

Destructive operations require human approval before execution:

```
[approval] tool 'bash({"command": "rm -rf /tmp/*"})' requires approval.
Proceed? [y/N] y
```

**Protected tools:**
- `bash` — Shell command execution
- `write` — File modification
- `edit` — In-place file editing

### 5. Memory as Temporal RAG

Conversations are stored in PostgreSQL with pgvector, enabling semantic retrieval of past context:

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

Useful for long-running tasks and cross-session knowledge.

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
    session_id UUID
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

## Tool Registry

### Built-in Tools

| Tool            | Permission | Description              |
| --------------- | ---------- | ------------------------ |
| `read`          | Allow      | Read file contents       |
| `write`         | Prompt     | Write to file            |
| `edit`          | Prompt     | Modify file in-place     |
| `bash`          | Prompt     | Execute shell command    |
| `glob`          | Allow      | Find files by pattern    |
| `grep`          | Allow      | Search file contents     |
| `web_fetch`     | Allow      | Fetch URL contents       |
| `memory_append` | Allow      | Store in memory          |
| `todo_add`      | Allow      | Add to task list         |
| `delegate`      | Allow      | Spawn sub-agent          |
| `run_workflow`  | Allow      | Execute multi-agent flow |

### Dynamic Tool Selection

At runtime:
1. Embed the current query context
2. Search `agent_tools` via pgvector cosine similarity
3. Return top-8 most similar tools
4. Always include fallback tools (`read`, `glob`, `grep`, `web_fetch`) regardless of similarity score

```rust
let query_embedding = embedder.embed(&context_query).await?;
let tools = tools.search_tools(&query_embedding, 8, &["read", "glob", "grep", "web_fetch"]).await;
```

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
1. Parse frontmatter (YAML)
2. Extract description for embedding
3. Generate 1024-dim vector via embedding model
4. Insert into `skills` table with HNSW index
5. Map allowed tools to `skill_tools` table

### Runtime Retrieval

```rust
let context_embedding = embedder.embed(&query).await?;
let skills = skill_registry.search(&context_embedding, 3).await;
// Inject skill instructions as system messages
```

## Agent Loop

### Execution Flow

```rust
async fn run(&self, input: &str) -> Result<String> {
    // 1. Build context query
    let context = build_context(&self.messages, input);

    // 2. Embed context
    let embedding = embedder.embed(&context).await?;

    // 3. Search tools (dynamic)
    let tools = tools.search(&embedding, 8, &fallback).await;

    // 4. Search skills (context priming)
    let skills = skills.search(&embedding, 3).await;

    // 5. Search memories (temporal RAG)
    let memories = memories.search(&embedding, 5).await;

    // 6. Construct system prompt
    let prompt = build_prompt(&tools, &skills, &memories);

    // 7. Call LLM
    let response = llm.complete(&prompt).await?;

    // 8. Execute tool calls (with permission checks)
    for tool_call in response.tool_calls {
        if needs_approval(&tool_call) {
            ask_user()?;
        }
        execute_tool(&tool_call)?;
    }

    // 9. Store memory
    memories.store(&response.content).await?;

    Ok(response.content)
}
```

## Performance

Current benchmarks. These will be updated as the registry grows and production workloads are characterized.

| Metric          | Value                   | Notes                         |
| --------------- | ----------------------- | ----------------------------- |
| Binary Size     | ~18MB                   | Statically linked Rust        |
| Cold Start      | <100ms                  | No interpreter startup        |
| Tool Search     | <1ms                    | HNSW, small registry          |
| Skill Search    | <1ms                    | HNSW, small registry          |
| Memory Search   | <5ms                    | HNSW, up to ~10K entries      |
| Context Overhead | Reduced vs. static lists | Depends on registry size and tool description verbosity |

## Security Model

### Permission Levels

```rust
enum PermissionLevel {
    Allow,   // No approval needed
    Prompt,  // Human approval required
}
```

### Sandboxing

Provisioned tools run in isolated subprocesses:
- **Environment clearing**: `env_clear()` before execution
- **Explicit PATH**: Only `/usr/bin:/bin`
- **Timeout**: 5s default, configurable
- **Output truncation**: 256KB max stdout

Note: Current sandboxing uses subprocess isolation. More robust microVM-based isolation (gVisor, Firecracker) is on the roadmap.

### Audit Logging

Every tool execution is recorded:

```sql
INSERT INTO tool_executions (
    tool_name, input, output, status, duration_ms, execution_id
) VALUES (...);
```

## Deployment

### Requirements

- **Rust**: 1.95+
- **PostgreSQL**: 16+ with `pgvector` extension
- **LLM Provider**: Ollama, NVIDIA NIM, or any OpenAI-compatible endpoint
- **RAM**: 4GB minimum; 16GB recommended when running local models alongside the agent

### Configuration

```bash
export LLM_MODEL="phi4-mini:3.8b"
export EMBEDDING_MODEL="mxbai-embed-large"
export DATABASE_URL="postgres://volt:volt@localhost:5432/volt"
```

### CI/CD

See `.github/workflows/ci.yml` for:
- PostgreSQL service setup
- Schema migration
- Test suite
- Binary size check
- Security audit (`cargo audit`)

## Roadmap

### v0.1 (current)

- [x] Dynamic RAG Loop (Tools + Skills + Memories)
- [x] Compiled Manifest Pattern
- [x] Multi-Agent Orchestration
- [x] Permission System
- [x] TUI with cursor editing

### Near-term

- [ ] Binary releases (Linux/macOS)
- [ ] Improved sandbox isolation
- [ ] Local skill registry for sharing compiled manifests
- [ ] VS Code extension

### Later

- [ ] gVisor / Firecracker integration for stronger tool sandboxing
- [ ] Web dashboard for agent monitoring and skill management
- [ ] Git-aware diff visualization for code review workflows
- [ ] Multi-modal input (images, PDFs via vision models)
- [ ] Distributed agent coordination

---

*Built in Rust by [Setique Labs, Inc.](https://setique.com)*
