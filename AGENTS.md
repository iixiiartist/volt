## Volt Project — Current State

### Tools built (all compile, all tested)
| Category | Tools | Feature Flag |
|---|---|---|
| **Screenshot** | `screenshot` | tools-screenshot |
| **Charts** | `create_bar_chart`, `create_line_chart` | built-in |
| **PDF** | `create_pdf` | tools-pdf |
| **Desktop** | `desktop_click`, `desktop_type`, `desktop_key`, `desktop_find_window` | tools-desktop |
| **Browser** | `browser_navigate`, `browser_extract`, `browser_screenshot` | tools-browser |
| **MCP client** | `MCPClient` with Bearer token auth | built-in |
| **SearchHQ MCP** | `register_searchhq_tools()` — 19 tools into ToolRegistry | built-in |

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
- ProgramBench: 25 coding puzzles
- BFCL v4: 4,241 cases across 17 categories (simple_python, parallel, multiple, live_simple, multi-turn, etc.)
- `volt-bfcl/volt_bench.py` — end-to-end test via actual Volt binary with `--load-tools`
- All run on Groq at ~$0.05–$0.59/1M tokens depending on model

### Primary Benchmark: BFCL (Berkeley Function Calling Leaderboard)
- **BFCL v3 leaderboard** (official, 23 models evaluated): qwen3-32b ranks **#2 globally at 75.7%**, behind only GLM 4.5 (76.7%). Average is 55.9%.
- Our BFCL v4 dataset (4,241 cases, 17 categories) is the next-gen version used for Volt evaluation
- GAIA removed from pipeline — it's a frontier-model benchmark (GPT-5 Mini tops at 44.8%, average 27.5%). Our Groq models (8B–120B) all scored 0% on 3-case runs. GAIA requires GPT-4-class + web tool integration far beyond what our architecture provides.

### Environment
- `.env` has GROQ_API_KEY + DATABASE_URL (not committed)
- `.env.example` for template
- Ollama needs `mxbai-embed-large` for Volt's embedding pipeline
- HuggingFace API (`HF_TOKEN`) for 384d dense embeddings (BAAI/bge-small-en-v1.5)
- SearchHQ token: generate at searchhq.setique.com/settings/mcp
- you.com API available for web search (`https://you.com/docs/welcome`)
- 98 GB free disk, 8 GB free RAM at idle (this machine)

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

### BFCL v4 Results (50-case simple_python, May 2026)
- **qwen/qwen3-32b**: 46/50 = 92.0% [CI: 81.2–96.8], avg 23.6s/case
- Full 400-case run pending; full BFCL v4 across 17 categories pending
- BFCL v3 leaderboard reference: qwen3-32b at 75.7% (#2 globally), average 55.9%
- Known issue: `web_search` and `bash` built-in tools interfere with BFCL-provided function stubs (model picks them over the stubs). VOLT_MINIMAL_TOOLS for BFCL runs would eliminate this.
