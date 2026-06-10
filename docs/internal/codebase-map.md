# Volt Codebase Map

Generated: 2026-06-09 — ~130 source files in `src/`, ~49K lines Rust

---

## Directory Tree

```
volt/
├── Cargo.toml              — Package manifest (27+ features, 80+ deps)
├── .env.example            — Template for environment variables
├── AGENTS.md               — Operating procedures (agent instructions)
├── MEMORY.md               — Persistent agent memory
│
├── src/
│   ├── lib.rs              — Library root: declares 47 modules
│   ├── main.rs             — CLI entry: 15+ subcommands
│   │
│   ├── agent/              — Agent loop, builder, hooks, routing
│   │   ├── mod.rs          — Agent struct, ApprovalDecision, ApprovalCallback
│   │   ├── builder.rs      — Agent::new(), with_*() builder methods
│   │   ├── run.rs          — Agent::run(), run_once(), loop orchestration
│   │   ├── compression.rs  — Prompt compression at 80% context budget
│   │   ├── prompt.rs       — System prompt builder with time injection
│   │   ├── prompt_builder.rs — Tool-call prompt template engine
│   │   ├── tool_parser.rs  — validate_tool_call() against JSON Schema
│   │   ├── router.rs       — Keyword-based task routing
│   │   ├── blueprint.rs    — Blueprint loading (MissingFinalAnswer variant kept)
│   │   ├── hooks.rs        — HookRegistry, pre/post tool hooks
│   │   ├── cot.rs          — CoT (deprecated #[serde(skip_serializing)])
│   │   ├── model_registry.rs — Model capabilities lookup
│   │   ├── multimodal.rs   — Text + image message handling
│   │   └── preset.rs       — Model preset loading
│   │
│   ├── bin/
│   │   ├── bfcl_bench.rs   — BFCL v4 benchmark runner (486 lines)
│   │   └── webui.rs        — WebUI binary entry point (75 lines)
│   │
│   ├── channels/           — Notification channels
│   │   ├── mod.rs          — Channel trait
│   │   ├── telegram.rs     — Telegram bot channel
│   │   └── webhook.rs      — Generic webhook channel
│   │
│   ├── commands/           — CLI subcommand implementations
│   │   ├── mod.rs          — Module declarations, AgentMode enum
│   │   ├── agent.rs        — volt agent subcommand group
│   │   ├── agent_run.rs    — Single-turn agent execution
│   │   ├── agent_tui.rs    — Interactive TUI mode
│   │   ├── config.rs       — volt config {list,get,set,unset,doctor,wizard}
│   │   ├── daemon.rs       — volt daemon background worker
│   │   ├── doctor.rs       — volt doctor diagnostics
│   │   ├── eval.rs         — volt eval evaluation runner
│   │   ├── init.rs         — volt init project setup
│   │   ├── mcp.rs          — volt mcp server mode
│   │   ├── provision.rs    — volt provision data seeding
│   │   ├── skills.rs       — volt skills management
│   │   ├── tools.rs        — volt tools listing
│   │   ├── workflow.rs     — volt workflow DAG runner
│   │   └── worktree.rs     — volt worktree management
│   │
│   ├── context/            — 3-Kind Context Store
│   │   ├── mod.rs          — ContextStore, ContextKind (12 variants, 3 default)
│   │   ├── search.rs       — Hybrid BM25+dense RRF fusion search
│   │   ├── eviction.rs     — Four-pillar eviction (dedup, quota, score, merge)
│   │   ├── clustering.rs   — Episodic merging cluster detection
│   │   └── persistence.rs  — Context DB operations
│   │
│   ├── db/                 — PostgreSQL persistence (pgvector)
│   │   ├── mod.rs          — connect(), init_schema(), pool management
│   │   ├── context.rs      — Context CRUD
│   │   ├── executions.rs   — Execution history
│   │   ├── memory.rs       — Long-term memory CRUD
│   │   ├── routines.rs     — Routines CRUD
│   │   ├── skills.rs       — Skills CRUD
│   │   └── tools.rs        — Tools CRUD
│   │
│   ├── embedding/          — Embedding pipeline
│   │   ├── mod.rs          — EmbeddingClient, embedding_dimension(), compute_embeddings()
│   │   └── providers.rs    — ProviderConfig: NVIDIA, Ollama, OpenAI, HuggingFace
│   │
│   ├── llm/                — LLM provider implementations
│   │   ├── mod.rs          — Module declarations, re-exports
│   │   ├── provider.rs     — LLMProvider trait, ProviderKind enum
│   │   ├── provider_detector.rs — ProviderDetector, auto-detect active providers
│   │   ├── openai.rs       — OpenAI/Groq/NVIDIA NIM compatible provider
│   │   ├── anthropic.rs    — Anthropic/Claude provider
│   │   ├── ollama.rs       — Ollama native provider
│   │   ├── riva.rs         — NVIDIA Riva speech/audio provider
│   │   └── poll_async.rs   — Async polling for NVIDIA NIM inference
│   │
│   ├── mcp/                — Model Context Protocol
│   │   ├── mod.rs          — Module declarations
│   │   ├── client.rs       — MCP Client with Bearer token auth
│   │   ├── server.rs       — MCP Server (stdio + HTTP), protocol compliance
│   │   └── grpc.rs         — gRPC transport (feature: tools-mcp-grpc)
│   │
│   ├── tools/              — All tool implementations (~28 files)
│   │   ├── mod.rs          — Module declarations (26 items)
│   │   ├── registration.rs — register_all_tools() with feature/env gating
│   │   ├── registry.rs     — ToolRegistry: register, search (hybrid RRF), execute
│   │   ├── read_tool.rs    — read file reader
│   │   ├── write_tool.rs   — write file writer (auto-creates parent dirs)
│   │   ├── edit.rs         — edit find-and-replace
│   │   ├── bash.rs         — bash shell executor
│   │   ├── glob_tool.rs    — glob file pattern matching
│   │   ├── grep_tool.rs    — grep content search
│   │   ├── web_tool.rs     — web_fetch with optional selector param
│   │   ├── you_tools.rs    — web_search, you_research, you_contents
│   │   ├── git_tool.rs     — git_query, git_mutate (collapsed from 12)
│   │   ├── time_utils.rs   — sleep_until (RFC 3339, max 24h)
│   │   ├── csv_tool.rs     — csv_read, csv_write
│   │   ├── archive_tool.rs — archive_create, archive_extract
│   │   ├── chart_tool.rs   — create_bar_chart, create_line_chart
│   │   ├── pdf_tool.rs     — create_pdf (feature: tools-pdf)
│   │   ├── desktop_tool.rs — Desktop automation (feature: tools-desktop)
│   │   ├── browser_tool.rs — Browser automation (feature: tools-browser)
│   │   ├── delegate.rs     — Agent-to-agent delegation
│   │   ├── mcp_client.rs   — MCP client tool for remote servers
│   │   ├── searchhq.rs     — SearchHQ MCP (19 tools)
│   │   ├── nvidia_cloud_functions.rs — NVIDIA Cloud Funcs (NVIDIA_API_KEY)
│   │   ├── ollama_web_tools.rs — Ollama web search/fetch (OLLAMA_API_KEY)
│   │   ├── litertlm.rs     — LiteRT edge inference (env-gated)
│   │   ├── llamacpp.rs     — Local llama.cpp inference (env-gated)
│   │   ├── mtp.rs          — Multi-turn planning (env-gated)
│   │   ├── cli_tools/
│   │   │   └── mod.rs      — cli_exec, cli_query (env-gated)
│   │   ├── path.rs         — Path sanitization utilities
│   │   └── groups/         — Logical tool groupings
│   │       ├── mod.rs      — 7 group declarations
│   │       ├── core.rs     — read, write, edit, bash, glob, grep, sleep_until
│   │       ├── web.rs      — web_fetch, web_search, you_research, you_contents
│   │       ├── git.rs      — git_query, git_mutate
│   │       ├── data.rs     — csv, archive, charts, pdf
│   │       ├── desktop.rs  — Desktop tools (feature-gated)
│   │       ├── browser.rs  — Browser tools (feature-gated)
│   │       └── llm.rs      — LLM tools (env-gated)
│   │
│   ├── webui/              — Web UI (feature: webui)
│   │   ├── mod.rs
│   │   ├── app.rs          — axum router setup
│   │   ├── routes.rs       — Route definitions
│   │   ├── pages.rs        — HTML page rendering (1487 lines)
│   │   ├── state.rs        — AppState
│   │   ├── runtime.rs      — Background runtime, ContextStore wiring
│   │   ├── layout.rs       — Layout templates
│   │   ├── commands.rs     — WebSocket command handling
│   │   └── setup_wizard.rs — Interactive setup wizard
│   │
│   ├── jobs/               — Background job scheduling
│   │   ├── mod.rs          — JobEngine
│   │   └── monitor.rs      — Job health monitoring
│   │
│   ├── routines/           — Reactive event-driven routines
│   │   ├── mod.rs
│   │   └── engine.rs       — RoutineEngine (subscribe, dispatch, retry)
│   │
│   ├── secrets/            — Secret management
│   │   └── mod.rs          — SecretStore trait + EnvSecretStore
│   │
│   ├── skills/             — Skill system (import/export/catalog)
│   │   ├── mod.rs          — SkillRegistry
│   │   ├── catalog.rs      — Built-in skill definitions
│   │   └── importer.rs     — Import opencode/Claude skills → Volt format
│   │
│   ├── models.rs           — Core types: Message, ToolCall, AgentConfig
│   ├── config.rs           — Settings from env vars + .volt/config.toml
│   ├── orchestrator.rs     — DAG multi-agent orchestration (1400 lines)
│   ├── session.rs          — SQLite session storage & conversation history
│   ├── worker.rs           — AutoSeedWorker background daemon
│   ├── metrics.rs          — Prometheus metrics endpoint
│   ├── events.rs           — EventBus (tokio broadcast)
│   ├── capability.rs       — JWT capability tokens
│   ├── attenuation.rs      — Trust attenuation chains
│   ├── command_guard.rs    — Shell command whitelist
│   ├── network_policy.rs   — Network access policy
│   ├── safety_layer.rs     — XML safety wrapping
│   ├── sandbox.rs          — Interactive sandbox
│   ├── leak_detector.rs    — Context leak detection
│   ├── checkpoint_journal.rs — Execution checkpoint logging
│   ├── code_parser.rs      — Code extraction from LLM output
│   ├── graph_rag.rs        — Graph-based RAG relationships
│   ├── eval.rs             — Evaluation harness
│   ├── tui.rs              — Terminal UI (ratatui)
│   ├── vector_index.rs     — BM25+ scorer + RRF fusion
│   ├── turbovec_index.rs   — TurboQuantIndex wrapper (feature: tools-turbovec)
│   ├── local_embed.rs      — Local ONNX embedder (ort, BGE-large-en-v1.5, 1024d)
│   ├── validation.rs       — Validation helpers
│   ├── skill_scorer.rs     — Skill relevance scoring
│   ├── tool_failure_tracker.rs — Tool failure tracking & avoidance
│   ├── heartbeat.rs        — Health check endpoint
│   ├── telemetry.rs        — OpenTelemetry tracing (stdout/OTLP)
│   ├── safety_layer.rs     — Content filtering, injection detection
│   ├── registry.rs         — Central component registry
│   └── test_utils.rs       — Shared test utilities (feature: testutils)
│
├── tests/                  — 18 integration test files
│   ├── agent_tests.rs
│   ├── attenuation_tests.rs
│   ├── bfcl_pipeline.rs    — Requires GROQ_API_KEY
│   ├── cli_integration_tests.rs
│   ├── event_bus_tests.rs
│   ├── hooks_integration.rs
│   ├── network_policy_tests.rs
│   ├── professional_workflows.rs   — 24 workflow tests
│   ├── profile_mode_tests.rs
│   ├── program_bench.rs            — 8 coding puzzles
│   ├── real_world_benchmarks.rs    — 11 tests
│   ├── searchhq_all_tools_test.rs
│   ├── skill_scorer_tests.rs
│   ├── tool_failure_tests.rs
│   ├── validation_tests.rs
│   ├── webui_e2e.rs
│   ├── workflow_bench.rs
│   └── worktree_integration.rs
│
├── migrations/
│   ├── 0001_core.sql               — Core schema + pgvector
│   ├── 0002_jobs_and_routines.sql  — Jobs, routines, secrets
│   ├── 0003_storage_optimizations.sql — HNSW indexes (fixed lowercase)
│   └── 0004_audit_log.sql          — Append-only audit log (EU AI Act Art. 12)
│
└── proto/
    └── mcp.proto           — gRPC service definition (feature: tools-mcp-grpc)
```

---

## Architecture Summary

```
┌──────────────────────────────────────────────────────────┐
│                    CLI (main.rs)                          │
│  webui │ run │ tui │ config │ doctor │ daemon │ mcp │... │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│                 Agent Loop (agent/run.rs)                 │
│  ┌──────────┐ ┌───────────┐ ┌────────────────────────┐   │
│  │  LLM     │ │  Prompt   │ │  Tool Parser            │   │
│  │ Provider │ │  Builder  │ │  (JSON Schema val.)     │   │
│  └──────────┘ └───────────┘ └────────────────────────┘   │
└──────────────────────┬───────────────────────────────────┘
                       │
         ┌─────────────┼──────────────┐
         ▼             ▼              ▼
  ┌──────────┐ ┌────────────┐ ┌──────────────┐
  │  Tools   │ │  Context   │ │  MCP Client  │
  │  (~20)   │ │  3-Kind    │ │  (remote)    │
  └──────────┘ └────────────┘ └──────────────┘
                    │
         ┌──────────┼──────────┐
         ▼          ▼          ▼
  ┌──────────┐ ┌──────────┐ ┌──────────┐
  │PostgreSQL│ │  SQLite  │ │ ONNX     │
  │(sqlx)    │ │(session) │ │Embedder  │
  └──────────┘ └──────────┘ └──────────┘

Background:
┌───────────────────────────────────────────────────┐
│           AutoSeedWorker (worker.rs)               │
│  MPSC → tools, memories, conversations → DB       │
│  Four-pillar eviction + episodic merging           │
└───────────────────────────────────────────────────┘
```

Key differences from earlier architecture:
- **3 default context kinds** (Tool, Memory, Conversation) — 9 others queryable via `search_by_kind()`
- **~20 active tools** — deleted `final_answer`, `sequentialthinking`, `get_current_time`, `memory_append`, `todo_add`, `json_query`, `web_scrape`, `screenshot`
- **ProviderDetector** — auto-detects configured providers, no hardcoded defaults
- **Keyword routing** — ~100µs substring matching replaces LLM blueprint selection
- **Auto-migration** — `init_schema()` on every connect, no manual `init-db`
- **EMBEDDING_DIMENSION** — env-var configurable (default 1024)
- **Supervisor synthesizer opt-in** — default: direct concatenation
