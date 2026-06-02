## Volt Project — Current State

### Tools built (55+ total, 38 active by default)
| Category | Tools | Gating |
|---|---|---|
| **Core** | `read`, `write`, `edit`, `bash`, `glob`, `grep`, `final_answer` | `bash` hidden in `VOLT_BFCL_MODE` |
| **Web** | `web_fetch`, `web_scrape`, `web_scrape_all`, `web_search`, `you_research`, `you_contents` | Search tools require `YOUCOM_API_KEY`; 3 hidden in `VOLT_BFCL_MODE` |
| **Memory** | `memory_append`, `todo_add` | Always |
| **Data** | `csv_read`, `csv_write`, `archive_extract`, `archive_create`, `create_bar_chart`, `create_line_chart`, `create_pdf` | Charts/PDF/Desktop/Browser hidden in `VOLT_MINIMAL_TOOLS`; PDF requires `tools-pdf` feature |
| **Git** | `git_status`, `git_diff_unstaged`, `git_diff_staged`, `git_diff`, `git_commit`, `git_add`, `git_reset`, `git_log`, `git_create_branch`, `git_checkout`, `git_show`, `git_branch` | Always |
| **Time & Sequential** | `get_current_time`, `convert_time`, `sequentialthinking` | Always |
| **Orchestration** | `delegate`, `run_workflow` | Always |
| **Desktop** | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` | `tools-desktop` feature + not `VOLT_MINIMAL_TOOLS` |
| **Browser** | `browser_navigate`, `browser_extract`, `browser_screenshot` | `tools-browser` feature + not `VOLT_MINIMAL_TOOLS` |
| **LLM Local** | `litertlm`, `llamacpp`, `mtp` | `VOLT_ENABLE_LOCAL_LLM_TOOLS=1` + binary on disk |
| **NVIDIA Cloud** | `nvidia_list_functions`, `nvidia_call_function`, `nvidia_deploy_function` | `NVIDIA_API_KEY` or `NVCF_API_KEY` |
| **Ollama Web** | `ollama_web_search`, `ollama_web_fetch` | `OLLAMA_API_KEY` |
| **CLI Gateway** | `cli_exec`, `cli_query` | `VOLT_ENABLE_CLI_TOOLS=1` |
| **MCP Client** | `MCPClient` with Bearer token auth | Built-in |
| **SearchHQ MCP** | `register_searchhq_tools()` — 19 tools into ToolRegistry | Built-in |

> **Note:** The `screenshot` tool (desktop screenshot) exists in `src/tools/screenshot.rs` behind the `tools-screenshot` feature but is **not currently registered** in the tool registry. It is dead code until wired into a tool group.

### Unified Context Store ("Everything-as-RAG")
All 12 context fields embedded and dynamically retrievable via `ContextStore`:

| Kind | Quota | Seeded From |
|---|---|---|
| `Tool` | 500 | All registered tool schemas (name + description + JSON schema) |
| `Skill` | 200 | Compiled skill manifests from DB |
| `Conversation` | 300 | `SeedEvent::EpisodeComplete` after each agent run |
| `Memory` | 500 | MEMORY.md workspace file + DB memories |
| `AgentRun` | 200 | `audit_turn()` in agent loop — EU AI Act Art. 12 compliant |
| `Artifact` | 300 | `SeedEvent::ArtifactCreated` — write/edit/bash side effects |
| `SystemPrompt` | 20 | SOUL.md |
| `FewShot` | 50 | Reserved |
| `Policy` | 50 | AGENTS.md |
| `Permission` | 50 | `seed_permissions()` — every tool's allow/prompt level |
| `Security` | 30 | `seed_security_policy()` — sandbox limits, EU AI Act Art. 14 oversight |
| `MCPConfig` | 100 | `SeedEvent::MCPRegistered` — MCP server schema distillation |

### Auto-Seeding Worker (`src/worker.rs`)
Background daemon with MPSC channel architecture:
- `SeedChannel` — Clone-able sender; agent loop emits events without blocking
- `AutoSeedWorker` — `tokio::spawn` daemon drains batches (≤32), embeds via HF API (semaphore=5), seeds with dedup + eviction
- Episodic merger runs every 10 batches: clusters Conversation entries ≥0.85 cosine with ≥3 members, replaces with high-density merged entry
- Pre-warms at startup: workspace files, tool intents, permissions, security policy, skills from DB

### Four-Pillar Eviction
1. **Semantic dedup** — cosine ≥ 0.92 on same kind → merge frequency, skip insert
2. **Per-kind quotas** — evict lowest composite-score entries when kind exceeds quota
3. **Composite score** — `0.4×recency + 0.3×success + 0.2×frequency + 0.1×density`
4. **Episodic merging** — cluster detection + template summarization → high-density entries

### SearchHQ MCP fixes deployed
Three bugs fixed in SearchHQ MCP server (deployed via `npx netlify deploy --build --prod`):
1. `save_clip` — removed stale `scan`/`compare` from Zod enum (features removed)
2. `add_feed` — fixed Zod schema (was copy-pasted from `generate_sandbox`)
3. `run_agent` — added structured error logging to all catch blocks

### Benchmark Results (BFCL, live_simple, 200 distractors, 50 cases)
| Configuration | Tool Emb | Context | Acc | Δ |
|---|---|---|---|---|
| Baseline RAG | TF-IDF | None | 74% | — |
| Dense tools only | HF API 384d | None | **86%** | **+12pp** |
| Dense everything | HF API 384d | HF API (partial) | 84% | +10pp |
| Lexical context | TF-IDF | Ollama 1024d | 70% | -4pp |
| Worst case | TF-IDF | TF-IDF | 68% | -6pp |

Key finding: tool selection embedding quality is the dominant signal (+12pp). Context enrichment requires a fully-seeded store to cross into positive ROI. Full BFCL run: 470 cases, ~$0.37 total, 74% token savings, +4.8pp accuracy.
- ProgramBench: 8 coding puzzles (`tests/program_bench.rs`)
- BFCL v4: 4,241 cases across 17 categories (simple_python, parallel, multiple, live_simple, multi-turn, etc.)
- `rust bfcl_bench` (`src/bin/bfcl_bench.rs`) — Rust-native runner replacing deprecated `volt_bench.py`
- All run on Groq at ~$0.05–$0.59/1M tokens depending on model

### Primary Benchmark: BFCL (Berkeley Function Calling Leaderboard)
- **BFCL v3 leaderboard** (official, 23 models evaluated): qwen3-32b ranks **#2 globally at 75.7%**, behind only GLM 4.5 (76.7%). Average is 55.9%.
- Our BFCL v4 dataset (4,241 cases, 17 categories) is the next-gen version used for Volt evaluation

### Environment
- `.env` has GROQ_API_KEY + DATABASE_URL (not committed)
- `.env.example` for template
- Local ONNX tract-onnx BGE-large-en-v1.5 (1024d) — default embedder, no C++ dependency
- HuggingFace `HF_TOKEN` for ONNX model downloads (cached to `~/.cache/huggingface`)
- SearchHQ token: generate at searchhq.setique.com/settings/mcp
- you.com API available for web search (`https://you.com/docs/welcome`)
- PostgreSQL 16+ with pgvector for persistence (Docker: `docker compose -f docker-compose.db.yml up -d`)
- ~100 GB free disk, 8 GB free RAM at idle (this machine)

### Local ONNX Embedder Upgrade (May 2026)
- **Replaced** Candle BGE-small-en-v1.5 (384d) with tract-onnx BGE-large-en-v1.5 (1024d)
- **Pure Rust:** tract-onnx ONNX inference engine — no C++/MSVC dependency, works on all platforms
- **Default model:** `Xenova/bge-large-en-v1.5` via hf-hub, downloads int8 quantized ONNX (~337MB) + tokenizer on first use (cached to ~/.cache/huggingface)
- **Configurable via:**
  - `VOLT_ONNX_MODEL_DIR` — local path to directory with model.onnx + tokenizer.json
  - `EMBEDDING_MODEL` — HuggingFace model ID (default: Xenova/bge-large-en-v1.5)
- **Architecture:** Mean pooling over last_hidden_state, L2 normalization (native 1024d, no padding needed)
- **Feature flag:** `tools-local-embeddings` (now enables tract-onnx + hf-hub + tokenizers + ndarray)
- **Removed:** candle-core, candle-transformers, candle-nn dependencies

### System Prompt Bugfix (May 2026)
- **Root cause:** `build_system_prompt()` in `src/agent/prompt.rs` was defined but never called — the agent ran with no system prompt, so the LLM didn't know it was an agent with tool access
- **Fix:** Added `workspace: Option<PathBuf>` field to `Agent` struct in `loop_rs.rs`, `with_workspace()` builder, injected system prompt at start of `run()`; changed `build_system_prompt` signature from `&Path` to `Option<&Path>` for safety; all 3 `Agent::new()` call sites in `main.rs` now chain `.with_workspace(current_dir())`
- **Result:** `web_search` tool now executes when asked. Without the system prompt, the model received tool definitions but never decided to call them — agent returned empty or text-only responses instead of tool calls

### Vendor-Prefixed Model Routing Fix (May 2026)
- **Bug:** `resolve_provider()` in `src/orchestrator.rs` used `m.contains("openai")` to detect GPT models — this caught vendor-prefixed Groq-hosted models like `openai/gpt-oss-20b` and routed them to `api.openai.com` (which had no API key)
- **Fix:** Changed smart routing to only match native GPT/O names: `m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-")`. Vendor-prefixed models (`openai/`, `qwen/`, `meta-llama/`) now correctly fall through to the default Groq provider
- **Result:** `openai/gpt-oss-20b` now hits Groq API and returns responses

### Agent Loop Fallback (May 2026)
- **Fix:** When `max_iterations` exhausted, agent now returns last non-empty assistant message content (falls back to last tool result) instead of erroring. Previously returned `Err("max iterations reached without final response")` which the harness couldn't extract answers from.

### Cross-Model BFCL Results (3 cases, May 2026)
| Model | Size | Q1 (triangle area) | Q2 (factorial) | Q3 (hypotenuse) | Score |
|---|---|---|---|---|---|
| llama-3.1-8b-instant | 8B | PASS | PASS | FAIL (json_query) | 2/3 |
| openai/gpt-oss-20b | 20B | FAIL (bash) | PASS | PASS | 2/3 |
| llama-4-scout-17b | 17B | FAIL (you_research) | PASS | PASS | 2/3 |
| qwen/qwen3-32b | 32B | PASS | PASS | PASS | **3/3 = 100%** |

qwen3-32b is the only model with perfect tool selection on all 3 simple_python cases. 20B and 17B both miscall Q1 (bash/you_research instead of calculate_triangle_area). 8B fails tool selection on Q3.

### BFCL v4 Results (400-case simple_python, May 2026)
- **llama-3.1-8b-instant**: 380/400 = **95.0%** [CI: 92.6–96.8], avg 23.6s/case
- All 20 failures were Groq API schema validation errors (boolean/integer type mismatches), not wrong tool selection
- Known issue: `web_search` and `bash` built-in tools interfere with BFCL-provided function stubs (model picks them over the stubs). VOLT_MINIMAL_TOOLS for BFCL runs would eliminate this.

### 7 Architecture Improvements (May 2026)

#### Structured Output Parsing (`src/agent/tool_parser.rs`)
- `validate_tool_call()` — validates tool call arguments against JSON Schema (required fields, type checking, nested objects, enum validation)
- `validate_tool_calls()` — batch validation with error reporting
- Integrated into agent loop: validation runs BEFORE tool execution, returns error feedback to LLM for retry
- 5 unit tests covering required fields, wrong types, valid calls, nested objects, enum constraints

#### Hybrid Retrieval (BM25 + Dense RRF Fusion)
- `Bm25Scorer` — BM25+ with tunable k1=1.2, b=0.75, delta=0.5; built from corpus on each query
- `reciprocal_rank_fusion()` — combines ranked lists with k=60 constant
- Integrated into `ContextStore::search()` (5th param `query_text: Option<&str>`) and `ToolRegistry::search_tools()` (4th param)
- RRF fusion: hybrid ranking when both BM25 and cosine signals available; pure cosine fallback when no query text
- `search()` and `search_tools()` changed signature — `None` preserves backward-compatible behavior

#### Prompt Compression (`loop_rs.rs`)
- `compress_if_needed()` — selective compression preserves ALL system messages, compresses only conversation history
- Two strategies: selective (keep system, truncate conversation) and fallback (rolling truncation when system exceeds budget)
- Uses `ModelContext::estimate_tokens()` for accurate tiktoken-based token counting
- Budget = 80% of model max context; injects `[Conversation summary]` markers when truncation occurs

#### MCP Streaming + Agent-to-Agent
- `MCPTransport::WebSocket { url, headers }` variant in `models.rs`
- `MCPClient::call_tool_stream()` — SSE-based streaming for long-running tool calls
- `MCPServer::serve_http()` — axum-based HTTP server (routes: /mcp, /mcp/tools/list, /mcp/tools/call)
- Agent-to-agent tool sharing: Agent A serves tools via HTTP, Agent B discovers and calls them remotely

#### DAG Multi-Agent Orchestration (`src/orchestrator.rs`)
- `DagWorkflow` — JSON-parsed DAG definitions with `from_json()`, `topological_sort()` (Kahn's algorithm), `execution_levels()`
- `DagScheduler` — parallel level-by-level execution; `{input}`/`{node_id}` template substitution from predecessor outputs
- `Orchestrator::run_dag()` — entry point for DAG workflow execution
- `DagNode`, `DagEdge` — data structures for agent task nodes and dependency edges

#### CLI Gateway (`src/tools/cli_tools/mod.rs`)
- Two generic gateway tools (`cli_exec`, `cli_query`) replace 12 hardcoded business CLIs
- Whitelist locked to 7 binaries: task, crm, hledger, khal, vdirsyncer, qsv, himalaya — enforced via `LazyLock<HashSet>` at spawn time
- No-shell execution via `tokio::process::Command::args()`
- For CLIs with native MCP servers (himalaya-mcp, qsvmcp), `MCPTransport::Stdio` is preferred over cli_exec

#### gRPC MCP Transport (`src/mcp/grpc.rs`)
- tonic/prost-based bidirectional streaming gRPC server (215 lines)
- Implements list_tools, call_tool, call_tool_stream RPCs defined in `proto/mcp.proto`
- Client side is scaffolded (stub); use `MCPTransport::Http` for remote agent connections

#### Rust bfcl_bench Binary (`src/bin/bfcl_bench.rs`)
- Rust-native BFCL v4 benchmark runner (535 lines, 16 category mappings)
- Replaces deprecated Python `volt_bench.py` harness
- Run via `cargo run --bin bfcl_bench -- --limit 400`

#### Paper cleanup (May 2026)
- `paper/volt_arxiv_v3.html` extensively updated (12 edits) then deleted
- `paper/` directory fully gitignored
- README updated with note: official benchmarks/paper pending final validation
- All stale Python temp scripts (`script*.py`, `search*.py`, etc.) and clutter (`b64.txt`, `groq_test.json`, `temp_*.json`, `test*.txt/png/json/rs`, `volt-*.log`, `volt_sessions.db`, `run.ps1`, `package.json`, `package-lock.json`, `VOLT_IRONCLAW_PLAN.md`) deleted

### Real-World Workflow Benchmarks (`tests/real_world_benchmarks.rs`)

11 tests across 7 workflows + 3 bonus + 1 integration, all passing with `--features testutils`:

| # | Workflow | New Feature Tested |
|---|---|---|
| 1 | **Software Dev DAG** (4-node: research→code→review→report) | DAG orchestration, topological sort, execution levels |
| 2 | **Data Analysis Pipeline** (scrape→extract→transform→chart) | Pipeline tool composition |
| 3 | **Multi-Agent Research** (3× parallel agents → synthesize) | Parallel execution |
| 4 | **Tool Selection Stress** (60 tools, 50 distractors, RRF vs cosine) | Hybrid RRF retrieval (BM25 + dense) |
| 5 | **MCP Agent-to-Agent** (HTTP server + remote tool call) | HTTP MCP server, remote tool invocation |
| 6 | **Codebase Refactor** (glob→read→grep→edit→bash→final) | Multi-step tool chaining |
| 7 | **Long Context Stress** (50-turn conversation) | Prompt compression |
| **All** | Full integration (validation + RRF + DAG + compression) | All 7 features simultaneously |
| **Bonus** | BM25+ scoring benchmark | Sparse retrieval correctness |
| **Bonus** | RRF fusion benchmark | Rank fusion correctness |
| **Bonus** | Tokenizer benchmark (1000 strings) | Tokenization performance |



### Windows Build (MSVC, May 2026)
- **Root cause:** Default toolchain was `x86_64-pc-windows-gnu` (MinGW/GCC), which links against `libstdc++-6.dll`, `libgcc_s_seh-1.dll`, `libwinpthread-1.dll`. These MinGW DLLs are not present on vanilla Windows systems.
- **Fix:** Switched default toolchain to `x86_64-pc-windows-msvc` (MSVC). Requires Visual Studio Build Tools with "Desktop development with C++" workload.
- **Effect on this machine:** VS 2022 Build Tools installed via Scoop; Rustc can now use `link.exe` for MSVC builds.
- **Binary:** MSVC-built `volt.exe` is **49 MB** (vs 84.5 MB GNU), depends on standard Windows DLLs + `VCRUNTIME140.dll` (MSVC Redistributable, pre-installed on most Windows). ONNX Runtime shared libraries (`onnxruntime.dll`, `DirectML.dll`, provider plugins) are downloaded on first use to `~/.cache/ort.pyke.io/` (~50–150 MB depending on Execution Provider).
- **CI:** `.gitlab-ci.yml` Windows build job updated with vcvars64.bat setup. Requires self-hosted Windows runner with VS Build Tools.
- **GNU fallback:** `.cargo/config.toml` includes `-static -static-libstdc++` link args for `x86_64-pc-windows-gnu` target (partial static linking — gcc_s and winpthread are statically linked, but libstdc++ still resolves to DLL). GNU users should prefer MSVC instead.

### ONNX Runtime (ort) Migration (May 2026)
- **Replaced** tract-onnx (CPU-only) with ort v2.0.0-rc.12 behind `tools-local-embeddings`
- **Execution Provider chain:** OpenVINO → DirectML → CUDA → CPU — automatically selects best available hardware
- **EP detection:** DirectML=YES on Intel Core Ultra 5 135U (Arc Graphics iGPU, `DirectML.dll` loaded); OpenVINO failed (missing `onnxruntime_providers_shared.dll`); CUDA not available
- **Benchmark (BGE-small-en-v1.5, release):** Load 376ms, first inference 131ms, steady-state ~130ms per call, batch-of-5 166ms
- **MSVC CRT fix:** Added `CXXFLAGS=/MD` / `CFLAGS=/MD` to `.cargo/config.toml` — matches ort-sys prebuilt binaries (dynamic CRT `MD_DynamicRelease`); fixes `LNK2038` linker errors with `esaxx-rs` (tokenizers dep)
- **ndarray pinned** to `=0.17.1` — ort 2.0.0-rc.12 depends on `NdFloat` trait removed in 0.17.2
- **`ort::tracing` feature removed** — added ~300ms inference overhead on Windows
- **Session wrapped in `Mutex`** — `ort::Session::run()` takes `&mut self`
- **Tensor API:** `Tensor::from_array(array)` for input creation, `downcast_ref::<DynTensorValueType>()` + `try_extract_array::<f32>()` for output extraction
- **No `ort::init()` call** — environment auto-creates on first session (library-safe per ort docs)
- **`BuilderResult !Send + !Sync`** worked around via `ort_err()` helper that maps to `anyhow::Error`
- **`commit_from_file` requires `feature = "std"`** — added `std` to ort features (was missing with `default-features = false`)

### MCP Server Lifecycle (May 2026)
- **MCP protocol compliance** added: `initialize` handshake with capability declaration, `notifications/initialized` silently consumed
- **JSON-RPC 2.0 notification support:** `id` field made optional; no-response path for notifications
- **Stdout safety:** all `tracing` output already routes to stderr (`with_writer(std::io::stderr)`); `tools/list` and `tools/call` write only JSON-RPC to stdout
- **Capability declaration version:** `2024-11-05` (current MCP spec)
- **Security:** `tools/call` routes through `execute_gated()` — same permission/approval layer as internal agents

### Groq Ecosystem Integration (May 2026)
- **Audio APIs:** `transcribe()`, `translate()`, `synthesize()` in `LLMProvider` trait with default no-op impls; Groq multipart upload in `OpenAIProvider`
- **Compound Systems:** `usage_breakdown` + `executed_tools` parsing from Groq compound responses; mapped to Volt's DAG workflow architecture
- **Reasoning:** `reasoning_effort`, `reasoning_format`, `include_reasoning` fields in `LLMRequest` passthrough
- **Structured Outputs:** `ResponseFormat` enum with `JsonObject`, `JsonSchema` (strict), `Text` variants
- **Blueprints:** 19 TOML blueprints covering full Groq fleet (chat, audio, safety, compound, variants)
- **Groq API verified:** 16/16 models live at `https://api.groq.com/openai/v1`

### Environment-Aware Blueprint Routing (June 2026)
- **`get_active_providers()`** in `src/agent/router.rs` — detects live API keys (GROQ, NVIDIA, OPENAI, ANTHROPIC) and local hosts (OLLAMA, LLAMA_CPP, LITERTLM); auto-adds local fallbacks when no remote keys found
- **`filter_blueprints()`** — filters `&[AgentBlueprint]` to only those whose `model_card.provider` is in the active set
- **`route_task()`** — now filters before LLM selection with prompt: "filtered to match your active API keys"
- **3 unit tests** covering groq-only, no-remote→local, and multi-provider filtering; serialized via `ENV_MUTEX`

### NVIDIA NIM Integration (June 2026)
- **Comprehensive vendor prefix routing** — `NIM_VENDOR_PREFIXES` expanded to 27 vendors covering the full catalog from `docs.api.nvidia.com` (DeepSeek, Qwen, Google, Meta, Microsoft, Mistral, Moonshot, NVIDIA, etc.)
- **`GROQ_VENDOR_PREFIXES`** — excludes known Groq-hosted models (`openai/gpt-oss-*`, `meta-llama/*`, `canopylabs/*`) from NVIDIA catch-all
- **`is_nim_catchall_candidate()`** — any unknown vendor-prefixed model (contains `/`) routes to `integrate.api.nvidia.com/v1` when `NVIDIA_API_KEY` is available
- **Async inference polling** — 202 Accepted responses trigger `poll_async_result()`: polls `GET /{request_id}` every 2s for up to 120 cycles (4 min), handles `completed`/`failed`/timeout
- **8 NIM blueprints** in `blueprints/`: DeepSeek V4 Pro, Qwen 3.5 122B, Gemma 4 31B, GLM 5.1, Nemotron-3 Super 120B, Nemotron-3 Nano Omni, Step 3.7 Flash, MiniMax M2.7

### Riva Speech/Audio Provider (June 2026)
- **`RivaProvider`** in `src/llm/riva.rs` — implements `LLMProvider` with `transcribe()` (STT via `POST /v1/speech/recognize`) and `synthesize()` (TTS via `POST /v1/speech/synthesis`)
- **Configurable:** `sample_rate`, `language_code`, voice selection
- **Endpoint:** `https://riva.api.nvidia.com/v1` with `RIVA_API_KEY` auth

### NVIDIA Cloud Functions (June 2026)
- **3 tools** in `src/tools/nvidia_cloud_functions.rs`: `nvidia_list_functions`, `nvidia_call_function` (with async polling), `nvidia_deploy_function`
- **API:** `https://api.nvcf.nvidia.com/v2/nvcf` using `NVIDIA_API_KEY` or `NVCF_API_KEY`
- **Auto-registered** when key is present; async invocations polled automatically

### Ollama Full Integration (June 2026)
- **Native Ollama provider** `OllamaProvider` in `src/llm/ollama.rs` — uses Ollama's `/api/chat` format with native `think`/`thinking` support, tool calling, and streaming
- **Ollama web tools** in `src/tools/ollama_web_tools.rs` — `ollama_web_search` and `ollama_web_fetch` using Ollama Cloud's built-in APIs
- **Routing:** Models with Ollama's colon-tag naming (e.g., `gpt-oss:120b`, `gemma4:31b`) automatically route to Ollama Cloud when `OLLAMA_API_KEY` is set
- **`LLM_DEFAULT_PROVIDER=ollama`** routes all unmatched models to `https://api.ollama.com/v1`
- **Ollama Cloud embedding** auto-detected via `OLLAMA_API_KEY` using `embeddinggemma` at `api.ollama.com/api/embed`
- **8 Ollama blueprints:** GPT-OSS, DeepSeek V4 Pro/Flash, Gemma 4, Qwen 3.5, Nemotron-3 Super, GLM 5.1, MiniMax M2.7
- **Blueprint count: 37** (19 Groq + 8 NIM + 8 Ollama + 2 Edge)

### Full Test Suite: 198 Tests Passing
- 198 unit tests (`cargo test --lib --features testutils`) — up from 99
- 24 professional workflow tests (`tests/professional_workflows.rs`)
- 11 real-world benchmark tests (`tests/real_world_benchmarks.rs`)
- 1 program benchmark (`tests/program_bench.rs`)
- 1 BFCL pipeline test (requires `GROQ_API_KEY`, times out in CI without network)

### Blueprint Catalog: 67 Profiles
- **Groq fleet (19):** GPT-OSS 120B/20B/Safeguard, Llama 3.3 70B/3.1 8B/4 Scout, Qwen 3 32B, Whisper V3/Turbo, Orpheus EN/AR, Prompt Guard 22M/86M, Compound/Mini, + variants (vision, reasoning, MCP)
- **NVIDIA NIM (20):** DeepSeek V4 Pro/Flash, Qwen 3.5 122B, Qwen Coder 32B, QwQ 32B, Gemma 3 27B, Gemma 4 31B, GLM 5.1, Llama 3.1 8B/70B, Llama 3.3 70B, MiniMax M2.5/M2.7, Mixtral 8x22B, Nemotron-3 Super 120B/Nano Omni, Phi-4 Flash Reasoning, Phi-4 Mini, Step 3.7 Flash, Kimi K2
- **Ollama Cloud (25):** GPT-OSS, DeepSeek V3.2/V4 Pro/V4 Flash, Devstral Small 2, Gemma 4, Gemini 3 Flash, GLM 4.7/5/5.1/OCR, Kimi K2.6, MiniMax M2.1/M2.5/M2.7/M3, Nemotron Super/Nano, Qwen 3/3.5/3 Coder Next/3 Next 80B, RNJ-1
- **Edge (3):** Gemma 4 E2B Voice, Llama 3 8B Local, Allam 2 7B

### Tool Gating & System Prompt Hardening (June 2026)
- **Problem:** Models were calling broken/optional tools (`litertlm`, `cli_exec`) for simple text questions instead of answering directly
- **System prompt** (`src/agent/prompt.rs`): Added clear "WHEN TO ANSWER DIRECTLY" vs "WHEN TO USE TOOLS" guidance. Models now instructed to answer simple factual questions in plain text without calling tools
- **Tool descriptions** (`src/tools/groups/`): Added `[LOCAL ONLY]`, `[ENTERPRISE ONLY]` prefixes and "Do NOT use for..." scoping to high-risk tools (`bash`, `write`, `web_fetch`, `web_search`, `memory_append`, `cli_exec`, `cli_query`)
- **Write tool robustness** (`src/tools/write_tool.rs`): Auto-creates parent directories before writing, preventing "path not found" errors on nested paths

**New opt-in env vars (tools only register when set):**
| Env Var | Tools Gated | Description |
|---|---|---|
| `VOLT_ENABLE_LOCAL_LLM_TOOLS=1` | `litertlm`, `llamacpp`, `mtp` | Local inference binaries (LiteRT-LM, llama.cpp) |
| `VOLT_ENABLE_CLI_TOOLS=1` | `cli_exec`, `cli_query` | Enterprise CLI gateway (task, hledger, etc.) |
| `VOLT_MINIMAL_TOOLS=1` | Charts, PDF, Desktop, Browser | Minimal toolset for benchmarks |
| `VOLT_BFCL_MODE=1` | `bash`, `web_search`, `you_research`, `you_contents` | BFCL benchmark mode |

**Already gated by API key:**
| Key | Tools |
|---|---|
| `NVIDIA_API_KEY` / `NVCF_API_KEY` | `nvidia_list_functions`, `nvidia_call_function`, `nvidia_deploy_function` |
| `OLLAMA_API_KEY` | `ollama_web_search`, `ollama_web_fetch` |
| `YOUCOM_API_KEY` | `web_search`, `you_research`, `you_contents` |

**Core LLM / Routing env vars:**
| Env Var | Description |
|---|---|
| `LLM_MODEL` | Default model ID (e.g. `llama-3.1-8b-instant`, `qwen/qwen3-32b`) |
| `LLM_API_KEY` | Generic fallback API key used when provider-specific key is absent |
| `LLM_BASE_URL` | Override base URL for OpenAI-compatible providers |
| `LLM_MODEL_ROUTES` | JSON string for custom provider routing overrides |
| `LLM_DEFAULT_PROVIDER` | Default provider slug (`groq`, `ollama`, `nvidia`, `anthropic`, `openai`) |
| `OLLAMA_HOST` | Local Ollama host (e.g. `http://127.0.0.1:11434`) |
| `LLAMA_CPP_HOST` | Local llama.cpp server host |
| `LITERTLM_HOST` | Local LiteRT-LM server host |

**Embedding env vars:**
| Env Var | Description |
|---|---|
| `EMBEDDING_PROVIDER` | Provider: `nvidia`, `ollama`, `openai`, `hf`, `local`, `kimi` |
| `EMBEDDING_ENDPOINT` | Custom embedding API endpoint URL |
| `EMBEDDING_API_KEY` | Generic embedding API key (NVIDIA default) |
| `EMBEDDING_MODEL` | Model ID for embeddings (default: `Xenova/bge-large-en-v1.5`) |
| `VOLT_ONNX_MODEL_DIR` | Local path to ONNX model + tokenizer for local embedder |

**Sandbox / Security env vars:**
| Env Var | Description |
|---|---|
| `VOLT_SANDBOX_TIMEOUT_MS` | Sandbox command timeout (default: 5000) |
| `VOLT_SANDBOX_MAX_STDOUT_BYTES` | Max stdout bytes from sandbox (default: 262144) |
| `VOLT_ALLOWED_HOSTS` | Comma-separated host whitelist for `web_fetch` |
| `VOLT_COMMAND_GUARD` | Set to `false` to disable bash command guard |
| `VOLT_FAILURE_TRACKING` | Set to `false` to disable automatic tool failure tracking |
| `VOLT_FAILURE_THRESHOLD` | Integer threshold for auto-disabling failing tools |
| `VOLT_LEAK_DETECTOR` | Set to `false` to disable output leak detection |
| `VOLT_WRAP_TOOL_OUTPUT` | Control tool output wrapping in agent loop |
| `SANDBOX_SHELL` | Override shell binary for sandbox execution |

**Local LLM binary env vars:**
| Env Var | Description |
|---|---|
| `VOLT_TOOL_BIN_DIR` | Directory containing `litert_lm.exe` and `llama.exe` |

**Registry / Telemetry env vars:**
| Env Var | Description |
|---|---|
| `VOLT_REGISTRY_BASE_URL` | Volt registry base URL |
| `VOLT_REGISTRY_TOKEN` | Volt registry authentication token |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OpenTelemetry OTLP endpoint for telemetry export |

**Deprecated:**
| Env Var | Status |
|---|---|
| `KIMI_API_KEY` / `KIMI_EMBEDDING_MODEL` | Deprecated — use `EMBEDDING_API_KEY` / `EMBEDDING_MODEL` |
