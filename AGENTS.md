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
- GAIA: 165 validation questions
- `volt-bfcl/volt_bench.py` — end-to-end test via actual Volt binary with `--load-tools`
- All run on Groq `llama-3.1-8b-instant` at ~$0.05/1M tokens

### Paper
- `paper/draft.md` — arXiv-style, BFCL data, methodology, limitations
- `paper/benchmarks.md` — full benchmark roadmap
- `paper/tool_libraries_report.md` — Rust crate analysis

### Environment
- `.env` has GROQ_API_KEY + DATABASE_URL (not committed)
- `.env.example` for template
- Ollama needs `mxbai-embed-large` for Volt's embedding pipeline
- HuggingFace API (`HF_TOKEN`) for 384d dense embeddings (BAAI/bge-small-en-v1.5)
- SearchHQ token: generate at searchhq.setique.com/settings/mcp
- 98 GB free disk, 8 GB free RAM at idle (this machine)
