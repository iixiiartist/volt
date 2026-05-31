# Volt Codebase Map

Generated: 2026-05-30 — 132 source files, ~38K lines Rust

---

## Directory Tree

```
volt/
├── Cargo.toml              — Package manifest (25+ features, 80+ deps)
├── .env.example            — Template for environment variables
├── AGENTS.md               — Operating procedures (agent instructions)
├── SOUL.md                 — Core identity prompt
├── MEMORY.md               — Persistent agent memory
│
├── src/
│   ├── lib.rs              — Library root: declares all modules
│   ├── main.rs             — CLI entry: 15 subcommands
│   │
│   ├── agent/              — Agent loop & LLM interaction
│   │   ├── mod.rs
│   │   ├── loop_rs.rs      — Core agent loop (run, stream, compress)
│   │   ├── prompt.rs       — System prompt builder
│   │   ├── prompt_builder.rs — Tool-call prompt template engine
│   │   ├── tool_parser.rs  — Validate tool calls against JSON Schema
│   │   ├── cot.rs          — Chain-of-thought planning & step extraction
│   │   ├── multimodal.rs   — Multi-modal message handling (text + images)
│   │   ├── model_registry.rs — Model name → backend routing
│   │   └── preset.rs       — Model preset loading (gemma4, qwen, etc.)
│   │
│   ├── bin/
│   │   └── bfcl_bench.rs   — BFCL v4 benchmark runner (535 lines)
│   │
│   ├── channels/           — Notification channels
│   │   ├── mod.rs
│   │   ├── telegram.rs     — Telegram bot channel
│   │   └── webhook.rs      — Generic webhook channel
│   │
│   ├── commands/           — CLI subcommand implementations
│   │   ├── mod.rs
│   │   ├── agent.rs        — `volt agent` subcommand group
│   │   ├── agent_run.rs    — `volt agent-run` headless execution
│   │   ├── agent_tui.rs    — `volt agent-tui` interactive TUI
│   │   ├── daemon.rs       — `volt daemon` background worker
│   │   ├── eval.rs         — `volt eval` evaluation runner
│   │   ├── mcp.rs          — `volt mcp` MCP server mode
│   │   ├── provision.rs    — `volt provision` data seeding
│   │   ├── skills.rs       — `volt skills` skill management
│   │   ├── tools.rs        — `volt tools` tool listing/validation
│   │   └── workflow.rs     — `volt workflow` DAG workflow runner
│   │
│   ├── jobs/               — Background job scheduling
│   │   ├── mod.rs
│   │   └── monitor.rs      — Job health monitoring
│   │
│   ├── llm/                — LLM provider implementations
│   │   ├── mod.rs
│   │   ├── anthropic.rs    — Anthropic/Claude provider
│   │   ├── openai.rs       — OpenAI/Groq/OpenAI-compatible provider
│   │   └── provider.rs     — Provider trait + routing logic
│   │
│   ├── mcp/                — Model Context Protocol
│   │   ├── mod.rs
│   │   ├── models.rs       — MCP transport: Stdio, Http, WebSocket
│   │   ├── client.rs       — MCP client (list_tools, call_tool, stream)
│   │   ├── server.rs       — MCP server (axum HTTP + stdio JSON-RPC)
│   │   └── grpc.rs         — gRPC MCP transport (tonic/prost)
│   │
│   ├── routines/           — Reactive event-driven routines
│   │   ├── mod.rs
│   │   └── engine.rs       — Routine engine (subscribe, dispatch, retry)
│   │
│   ├── secrets/            — Secret management
│   │   ├── mod.rs          — SecretStore trait + EnvSecretStore
│   │   └── encrypted.rs    — EncryptedSecretStore (AES-GCM)
│   │
│   ├── skills/             — Skill system (import/export/catalog)
│   │   ├── mod.rs
│   │   ├── catalog.rs      — Built-in skill definitions
│   │   ├── importer.rs     — Import opencode/Claude skills → Volt format
│   │
│   ├── tools/              — All tool implementations
│   │   ├── mod.rs          — Module declarations
│   │   ├── registration.rs — Bulk tool registration for all features
│   │   ├── registry.rs     — ToolRegistry: register, search, execute
│   │   ├── read_tool.rs    — `read` file reader
│   │   ├── write_tool.rs   — `write` file writer
│   │   ├── edit.rs         — `edit` find-and-replace
│   │   ├── bash.rs         — `bash` shell executor
│   │   ├── glob_tool.rs    — `glob` file pattern matching
│   │   ├── grep_tool.rs    — `grep` content search
│   │   ├── web_tool.rs     — `web_fetch` HTTP client
│   │   ├── scrape_tool.rs  — `web_scrape` CSS selector extraction
│   │   ├── you_tools.rs    — `web_search`, `you_research`, `you_contents`
│   │   ├── git_tool.rs     — Git operations (8 sub-tools)
│   │   ├── screenshot.rs   — `screenshot` (feat: tools-screenshot)
│   │   ├── pdf_tool.rs     — `create_pdf` (feat: tools-pdf)
│   │   ├── chart_tool.rs   — Charts: bar, line (built-in)
│   │   ├── json_tool.rs    — `json_query`
│   │   ├── csv_tool.rs     — `csv_read`, `csv_write`
│   │   ├── archive_tool.rs — `archive_create`, `archive_extract`
│   │   ├── desktop_tool.rs — Desktop automation (feat: tools-desktop)
│   │   ├── browser_tool.rs — Browser automation (feat: tools-browser)
│   │   ├── memory_tool.rs  — `memory_append`, `memory_query`
│   │   ├── todo_tool.rs    — `todo_add`, `todo_list`, `todo_complete`
│   │   ├── time_tool.rs    — `get_current_time`, `convert_time`
│   │   ├── mcp_client.rs   — MCP client tools
│   │   ├── searchhq.rs     — SearchHQ MCP (19 tools)
│   │   ├── sequential_thinking.rs — Step-by-step reasoning tool
│   │   ├── delegate.rs     — Agent-to-agent delegation
│   │   ├── mtp.rs          — Multi-turn planning
│   │   ├── llamacpp.rs     — Local llama.cpp inference
│   │   ├── litertlm.rs     — LiteRT edge inference
│   │   ├── final_answer.rs — `final_answer` termination tool
│   │   ├── path.rs         — Path sanitization utilities
│   │   ├── groups/         — Logical tool groupings
│   │   │   ├── mod.rs
│   │   │   ├── core.rs     — Core tools: read, write, edit, bash, glob, grep
│   │   │   ├── web.rs      — Web tools: web_search, web_fetch, you_research
│   │   │   ├── data.rs     — Data tools: csv, json, archive, charts, pdf
│   │   │   ├── desktop.rs  — Desktop tools
│   │   │   ├── browser.rs  — Browser tools
│   │   │   ├── git.rs      — Git tools
│   │   │   ├── llm.rs      — LLM tools: llamacpp, litertlm, mtp
│   │   │   ├── memory.rs   — Memory tools
│   │   │   └── time_sequential.rs — Time + sequential thinking tools
│   │   └── cli_tools/
│   │       └── mod.rs      — cli_exec, cli_query (whitelisted 7 binaries)
│   │
│   ├── agent/
│   ├── llm/
│   ├── mcp/
│   ├── context.rs          — ContextStore: 12-kind RAG with hybrid search
│   ├── embedding.rs        — EmbeddingClient trait + HF API embedder
│   ├── local_embed.rs      — Local ONNX embedder (tract-onnx, BGE-large)
│   ├── vector_index.rs     — BM25+ scorer + RRF fusion
│   ├── turbovec_index.rs   — TurboQuantIndex wrapper for ANN search
│   ├── orchestrator.rs     — DAG multi-agent orchestration
│   ├── models.rs           — Core types: Message, ToolCall, AgentConfig
│   ├── config.rs           — Settings struct from env vars
│   ├── db.rs               — PostgreSQL persistence layer
│   ├── session.rs          — SQLite session storage
│   ├── worker.rs           — AutoSeedWorker background daemon
│   ├── events.rs           — EventBus (tokio broadcast)
│   ├── capability.rs       — JWT capability tokens
│   ├── attenuation.rs      — Trust attenuation chains
│   ├── command_guard.rs    — Shell command whitelist
│   ├── network_policy.rs   — Network access policy
│   ├── safety_layer.rs     — XML safety wrapping
│   ├── sandbox.rs          — Interactive sandbox
│   ├── telemetry.rs        — OpenTelemetry tracing
│   ├── heartbeat.rs        — Health check endpoint
│   ├── validation.rs       — Validation helpers
│   ├── skill_scorer.rs     — Skill relevance scoring
│   ├── graph_rag.rs        — Graph-based RAG relationships
│   ├── leak_detector.rs    — Context leak detection
│   ├── checkpoint_journal.rs — Execution checkpoint logging
│   ├── code_parser.rs      — Code extraction from LLM output
│   ├── eval.rs             — Evaluation harness
│   ├── tui.rs              — Terminal UI for interactive mode
│   ├── tool_failure_tracker.rs — Tool failure tracking & avoidance
│   ├── test_utils.rs       — Shared test utilities (feat: testutils)
│   └── registry.rs         — Central component registry
│
├── tests/                  — Integration tests
│   ├── agent_tests.rs              — Agent loop integration
│   ├── professional_workflows.rs   — 24 professional workflow tests
│   ├── real_world_benchmarks.rs    — 11 benchmark tests (DAG, RRF, MCP)
│   ├── program_bench.rs            — 25 coding puzzles
│   ├── bfcl_pipeline.rs            — BFCL eval (requires GROQ_API_KEY)
│   ├── workflow_bench.rs           — Workflow performance benchmarks
│   ├── attenuation_tests.rs        — Trust attenuation tests
│   ├── daemon_tests.rs             — Daemon lifecycle tests
│   ├── event_bus_tests.rs          — EventBus tests
│   ├── network_policy_tests.rs     — Network policy tests
│   ├── profile_mode_tests.rs       — Profile/preset mode tests
│   ├── searchhq_all_tools_test.rs  — SearchHQ tool coverage
│   ├── skill_scorer_tests.rs       — Skill scoring tests
│   ├── tool_failure_tests.rs       — Failure tracker tests
│   ├── validation_tests.rs         — Config validation tests
│   ├── webhook_channel_tests.rs    — Webhook channel tests
│
├── migrations/
│   ├── 0001_core.sql       — Core schema: tools, executions, memory, context
│   └── 0002_jobs_and_routines.sql — Jobs, routines, secrets tables
│
└── proto/
    └── mcp.proto           — gRPC service definition
```

---

## File-by-File Descriptions

### Root config

| File | Lines | Description |
|---|---|---|
| `Cargo.toml` | ~200 | Package manifest. 25+ feature flags (default: `tools-local-embeddings`). Key deps: tokio, serde, sqlx, tract-onnx, reqwest, axum, tonic. Two bins: `volt`, `bfcl_bench`. |
| `.env.example` | ~30 | Template: GROQ_API_KEY, DATABASE_URL, YOUCOM_API_KEY, model settings |
| `AGENTS.md` | ~200 | Operating procedures for the agent. Commands, workspace structure, conventions |
| `SOUL.md` | ~50 | Core identity prompt — agent's self-description |
| `MEMORY.md` | ~50 | Persistent facts the agent has learned across sessions |

### `src/lib.rs` — Library root

~60 lines. Declares all modules (`pub mod ...`). 45+ module declarations.

### `src/main.rs` — CLI entry

~200 lines. Defines `Commands` enum with 15 subcommands, each dispatching to `src/commands/`.

### `src/agent/`

| File | Lines | Key exports | Description |
|---|---|---|---|
| `mod.rs` | ~5 | Module declarations | `loop_rs`, `prompt`, `tool_parser`, `cot`, `model_registry`, `multimodal`, `preset`, `prompt_builder` |
| `loop_rs.rs` | ~600 | `Agent` struct, `run()`, `run_stream()`, `compress_if_needed()` | Core agent loop. Builder pattern with tools, model, workspace. Handles tool calls → LLM → tool calls. Prompt compression at 80% context budget |
| `prompt.rs` | ~150 | `build_system_prompt(workspace)` | Constructs system prompt from SOUL.md, AGENTS.md, tool descriptions |
| `prompt_builder.rs` | ~200 | `build_tool_call_prompt()`, `truncate_to_budget()` | Template engine for tool-call formatting. Supports different model conventions |
| `tool_parser.rs` | ~200 | `validate_tool_call()`, `validate_tool_calls()` | Validates tool call JSON against JSON Schema. Checks required fields, types, nested objects, enums |
| `cot.rs` | ~170 | `extract_plan()`, `extract_steps()`, `extract_tool_name()` | Chain-of-thought planning: parse numbered steps, extract tool names, detect "using" keyword patterns |
| `multimodal.rs` | ~85 | `MultimodalMessage`, `process_multimodal()` | Handles text + image messages for vision-capable models |
| `model_registry.rs` | ~75 | `ModelRegistry`, `lookup()`, `supports_vision()` | Maps model names to capabilities (supports_vision, max_tokens, provider) |
| `preset.rs` | ~85 | `Preset`, `load_preset()`, `list_presets()` | Loads model presets from TOML files (gemma4-e2b, gemma4-e4b, qwen3.5-9b) |

### `src/commands/`

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~20 | Module declarations |
| `agent.rs` | ~90 | `volt agent` subcommand group. Routes to run/tui sub-modes |
| `agent_run.rs` | ~200 | Headless agent run. Loads context, builds agent, executes, handles `--allow` flag |
| `agent_tui.rs` | ~200 | TUI agent mode. Loads context, builds agent with streaming, handles user interrupt |
| `daemon.rs` | ~100 | Background daemon. Starts AutoSeedWorker, routine engine, health endpoint |
| `eval.rs` | ~80 | Evaluation runner. Runs benchmarks, collects metrics |
| `mcp.rs` | ~60 | MCP server mode. Serves tools via stdio or HTTP |
| `provision.rs` | ~100 | Seeds data: tools, skills, permissions into DB |
| `skills.rs` | ~80 | Skill management: list, import, export |
| `tools.rs` | ~60 | Tool listing and validation |
| `workflow.rs` | ~80 | Multi-agent DAG workflow execution |

### `src/llm/`

| File | Lines | Key exports | Description |
|---|---|---|---|
| `mod.rs` | ~20 | Module declarations | |
| `openai.rs` | ~300 | `OpenAIProvider` | OpenAI / Groq / any OpenAI-compatible endpoint. Supports streaming via SSE |
| `anthropic.rs` | ~250 | `AnthropicProvider` | Claude API with tool use support |
| `provider.rs` | ~100 | `LLMProvider` trait, `resolve_provider()` | Trait: `chat_complete()`, `chat_complete_stream()`, `count_tokens()`. Routes model names → provider |

### `src/mcp/`

| File | Lines | Key exports | Description |
|---|---|---|---|
| `mod.rs` | ~10 | Module declarations | |
| `models.rs` | ~150 | `MCPTransport` (Stdio, Http, WebSocket), `MCPRequest`, `MCPResponse` | Data models for JSON-RPC MCP protocol |
| `client.rs` | ~200 | `MCPClient::connect()`, `list_tools()`, `call_tool()`, `call_tool_stream()` | Remote tool invocation over MCP transport |
| `server.rs` | ~250 | `MCPServer::serve_stdio()`, `serve_http()` | axum HTTP server with `/mcp/tools/list`, `/mcp/tools/call` routes. Agent-to-agent tool sharing |
| `grpc.rs` | ~215 | gRPC server: `ListTools`, `CallTool`, `CallToolStream` RPCs | tonic/prost bidirectional streaming gRPC transport |

### `src/tools/`

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~30 | Module declarations for ~30 tool modules |
| `registration.rs` | ~200 | `register_core_tools()`, `register_web_tools()`, etc. Feature-gated bulk registration |
| `registry.rs` | ~400 | `ToolRegistry` — `register()`, `execute_tool()`, `search_tools()` (hybrid BM25 + dense). Intent extraction for semantic matching |
| `read_tool.rs` | ~80 | `read` — reads file contents with path validation |
| `write_tool.rs` | ~80 | `write` — writes content to file |
| `edit.rs` | ~100 | `edit` — find-and-replace string editing |
| `bash.rs` | ~150 | `bash` — shell command execution with sandbox and timeout |
| `glob_tool.rs` | ~80 | `glob` — file pattern matching |
| `grep_tool.rs` | ~80 | `grep` — regex content search |
| `web_tool.rs` | ~120 | `web_fetch` — HTTP GET with URL validation (private IP blocking) |
| `scrape_tool.rs` | ~100 | `web_scrape`, `web_scrape_all` — CSS selector extraction |
| `you_tools.rs` | ~250 | `web_search` (you.com Search API), `you_research` (Research API), `you_contents` (Contents API). Livescrawl + result formatting |
| `git_tool.rs` | ~200 | `git_status`, `git_diff`, `git_commit`, `git_add`, `git_branch`, `git_log`, `git_checkout`, `git_reset` |
| `screenshot.rs` | ~100 | `screenshot` — screen capture (feat: tools-screenshot) |
| `pdf_tool.rs` | ~80 | `create_pdf` — PDF generation (feat: tools-pdf) |
| `chart_tool.rs` | ~150 | `create_bar_chart`, `create_line_chart` — chart image generation |
| `json_tool.rs` | ~60 | `json_query` — JSON querying |
| `csv_tool.rs` | ~100 | `csv_read`, `csv_write` — CSV parsing |
| `archive_tool.rs` | ~100 | `archive_create`, `archive_extract` — tar/zip operations |
| `desktop_tool.rs` | ~120 | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` (feat: tools-desktop) |
| `browser_tool.rs` | ~100 | `browser_navigate`, `browser_extract`, `browser_screenshot` (feat: tools-browser) |
| `memory_tool.rs` | ~80 | `memory_append`, `memory_query` — long-term memory |
| `todo_tool.rs` | ~100 | `todo_add`, `todo_list`, `todo_complete` — task management |
| `time_tool.rs` | ~80 | `get_current_time`, `convert_time` — time queries |
| `mcp_client.rs` | ~120 | MCP client tool — connects to remote MCP servers |
| `searchhq.rs` | ~200 | `register_searchhq_tools()` — 19 SearchHQ MCP tools |
| `sequential_thinking.rs` | ~80 | `sequential_thinking` — structured reasoning |
| `delegate.rs` | ~80 | `delegate` — agent-to-agent task handoff |
| `mtp.rs` | ~65 | Multi-turn planning: `generate_plan`, `execute_step` |
| `llamacpp.rs` | ~60 | Local llama.cpp inference bridge |
| `litertlm.rs` | ~50 | LiteRT edge inference bridge |
| `final_answer.rs` | ~30 | `final_answer` — terminates agent loop with final response |
| `path.rs` | ~80 | `sanitize_path()` — path traversal prevention |
| `cli_tools/mod.rs` | ~100 | `cli_exec`, `cli_query` — whitelisted CLI gateways (7 binaries) |

**Tool groups** (`src/tools/groups/`):

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~10 | Module declarations |
| `core.rs` | ~180 | Registers core tools: read, write, edit, bash, glob, grep |
| `web.rs` | ~160 | Registers web tools: web_search, web_fetch, web_scrape, you_research, you_contents |
| `data.rs` | ~145 | Registers data tools: csv, json, archive, charts, pdf |
| `desktop.rs` | ~40 | Registers desktop tools (feat: tools-desktop) |
| `browser.rs` | ~30 | Registers browser tools (feat: tools-browser) |
| `git.rs` | ~150 | Registers all git sub-tools |
| `llm.rs` | ~130 | Registers LLM tools: llamacpp, litertlm, mtp, sequential_thinking |
| `memory.rs` | ~50 | Registers memory tools |
| `time_sequential.rs` | ~60 | Registers time and sequential_thinking tools |

### Core infrastructure

| File | Lines | Key exports | Description |
|---|---|---|---|
| `context.rs` | ~970 | `ContextStore`, `ContextKind` (12 variants), `ContextEntry` | Unified RAG store. Hybrid BM25+dense search. Four-pillar eviction. Staging buffer pattern for deadlock-free writes. Turbovec integration |
| `embedding.rs` | ~200 | `EmbeddingClient` trait, `HfEmbedder` | Embedding with fallback chain: HF Inference API → local ONNX → Ollama |
| `local_embed.rs` | ~300 | `LocalEmbedder` | tract-onnx based BGE-large-en-v1.5 (1024d). Mean pooling + L2 norm. Pure Rust, no C++ dep |
| `vector_index.rs` | ~200 | `Bm25Scorer`, `reciprocal_rank_fusion()` | BM25+ with k1=1.2, b=0.75. RRF with k=60 constant |
| `turbovec_index.rs` | ~360 | `TurbovecIndex` | TurboQuantIndex wrapper with position mapping, dimension validation, reindex support |
| `orchestrator.rs` | ~300 | `Orchestrator`, `DagWorkflow`, `DagScheduler` | DAG parsing (JSON), topological sort (Kahn's), parallel execution levels, template substitution |
| `models.rs` | ~300 | `Message`, `ToolCall`, `ToolResult`, `AgentConfig`, `LLMRequest`, `Role` | Core data types used across the entire codebase |
| `config.rs` | ~200 | `Settings` | Loads configuration from `.env` + environment variables. Database, API keys, model, sandbox settings |
| `db.rs` | ~780 | `init_db()`, `save_execution()`, `insert_context_entry()`, `upsert_tool()`, `store_memory()` | PostgreSQL persistence. Connection pooling (sqlx). CRUD for all tables |
| `session.rs` | ~550 | `SessionStore` | SQLite session storage. Messages, session CRUD, atomic save |
| `worker.rs` | ~300 | `SeedChannel`, `AutoSeedWorker` | Background daemon. MPSC channel architecture. Drains batches (≤32), embeds via HF API, dedup + eviction. Episodic merger every 10 batches |
| `events.rs` | ~80 | `EventBus` | tokio broadcast channel. Event types: MemoryCreated, ToolExecuted, AgentStarted, AgentCompleted |
| `capability.rs` | ~150 | `CapabilityToken` | JWT-based tokens encoding tool permissions, expiration, delegation depth. `issue()`, `verify()`, `attenuate()` |
| `attenuation.rs` | ~100 | `AttenuationChain`, `TrustTier` | Delegation depth limiting, scope narrowing |
| `command_guard.rs` | ~80 | `CommandGuard`, `ShellPolicy` | Whitelist-based shell command enforcement |
| `network_policy.rs` | ~80 | `NetworkPolicy` | Domain allow/block lists, port restrictions |
| `safety_layer.rs` | ~60 | XML safety wrapper | Content filtering, injection detection. EU AI Act Art. 14 oversight |
| `sandbox.rs` | ~200 | Interactive sandbox | CLI sandbox for testing tool calls interactively |
| `telemetry.rs` | ~80 | OpenTelemetry tracing | OTEL stdout exporter. Span generation for agent runs |
| `heartbeat.rs` | ~40 | Health check | Simple health endpoint for monitoring |
| `validation.rs` | ~100 | Config validation | Validates Settings struct at startup |
| `skill_scorer.rs` | ~80 | Skill relevance scoring | Pattern-based skill matching against user intent |
| `graph_rag.rs` | ~150 | Graph-based RAG | Relationship tracking between context entries |
| `leak_detector.rs` | ~80 | Context leak detection | Detects unintended context bleeding between agents |
| `checkpoint_journal.rs` | ~80 | Checkpoint logging | Writes execution checkpoints for recovery |
| `code_parser.rs` | ~80 | Code extraction | Extracts code blocks from LLM responses |
| `eval.rs` | ~100 | Evaluation harness | Runs BFCL and program benchmarks |
| `tui.rs` | ~350 | Terminal UI | Interactive TUI with input, render, and signal handling |
| `tool_failure_tracker.rs` | ~80 | Failure tracking | Avoids repeatedly calling failed tools |
| `test_utils.rs` | ~100 | Test helpers | Shared utilities for integration tests (feat: testutils) |
| `registry.rs` | ~60 | Component registry | Central registry for services, providers, and plugins |

### `src/channels/`

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~10 | Module declarations |
| `telegram.rs` | ~100 | Telegram bot integration. `send_message()`, `handle_update()` |
| `webhook.rs` | ~80 | Generic webhook notification. `send_webhook()` with HMAC signing |

### `src/jobs/`

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~10 | Module declarations |
| `monitor.rs` | ~80 | Job health monitor. Tracks failures, retries, alerting |

### `src/routines/`

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~10 | Module declarations |
| `engine.rs` | ~150 | `RoutineEngine` — subscribes to EventBus, dispatches events to registered routines with retry |

### `src/secrets/`

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~50 | `SecretStore` trait: `get()`, `set()`, `delete()`, `list()`. `EnvSecretStore` fallback |
| `encrypted.rs` | ~100 | `EncryptedSecretStore` — AES-GCM encrypted local storage |

### `src/skills/`

| File | Lines | Description |
|---|---|---|
| `mod.rs` | ~10 | Module declarations |
| `catalog.rs` | ~100 | Built-in skill definitions with metadata |
| `importer.rs` | ~150 | Import opencode/Claude skill formats → Volt internal format. Roundtrip testing |

### `src/bin/`

| File | Lines | Description |
|---|---|---|
| `bfcl_bench.rs` | ~535 | Rust-native BFCL v4 benchmark runner. 16 category mappings. `--limit`, `--categories`, `--model`, `--output` flags |

---

## Migration Files

| File | Tables created | Description |
|---|---|---|
| `0001_core.sql` | `tools`, `executions`, `memory`, `context_entries`, `sessions` | Core schema with pgvector, HNSW index, timestamps, cascade deletes |
| `0002_jobs_and_routines.sql` | `jobs`, `job_executions`, `routines`, `routine_events`, `secrets`, `failures` | Phase 3: scheduling, routines, encrypted secrets |

## `proto/mcp.proto`

gRPC service definition with `McpService` RPCs: `ListTools`, `CallTool`, `CallToolStream`. Used by `src/mcp/grpc.rs`.

---

## Architecture Summary

```
┌──────────────────────────────────────────────────────┐
│                    CLI (main.rs)                      │
│   agent-run │ agent-tui │ eval │ daemon │ mcp │ ...  │
└──────────────────────┬───────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────┐
│                   Agent Loop (loop_rs.rs)             │
│  ┌─────────┐  ┌──────────┐  ┌─────────────────────┐  │
│  │  LLM    │  │  Prompt  │  │   Tool Parser       │  │
│  │Provider │  │  Builder │  │  (JSON Schema val.) │  │
│  └─────────┘  └──────────┘  └─────────────────────┘  │
└──────────────────────┬───────────────────────────────┘
                       │
          ┌────────────┼────────────┐
          ▼            ▼            ▼
   ┌──────────┐ ┌──────────┐ ┌──────────────┐
   │  Tools   │ │ Context  │ │  MCP Client  │
   │  (30+)   │ │  Store   │ │  (remote)    │
   └──────────┘ └──────────┘ └──────────────┘
                     │
          ┌──────────┼──────────┐
          ▼          ▼          ▼
   ┌──────────┐ ┌──────────┐ ┌──────────┐
   │PostgreSQL│ │  SQLite  │ │ ONNX     │
   │(sqlx)    │ │(session) │ │Embedder  │
   └──────────┘ └──────────┘ └──────────┘

Background:
┌────────────────────────────────────────────┐
│         AutoSeedWorker (worker.rs)          │
│  MPSC ← tools, permissions, memory → DB   │
│  Episodic merger every 10 batches          │
└────────────────────────────────────────────┘
```
