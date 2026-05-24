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

### SearchHQ MCP fixes deployed
Three bugs fixed in SearchHQ MCP server (deployed via `npx netlify deploy --build --prod`):
1. `save_clip` — removed stale `scan`/`compare` from Zod enum (features removed)
2. `add_feed` — fixed Zod schema (was copy-pasted from `generate_sandbox`)
3. `run_agent` — added structured error logging to all catch blocks

### Integration test
```rust
// Hook up 19 SearchHQ tools in Volt's ToolRegistry with RAG:
let registry = ToolRegistry::new();
volt::tools::searchhq::register_searchhq_tools(&registry, "YOUR_TOKEN").await?;
// Tools now go through Volt's embedding + cosine similarity pipeline
// Top-8 retrieved per turn — same 74% token savings as BFCL
```

### Benchmarks
- BFCL: 74% token savings, +4.8pp accuracy (verified, 470 cases, ~$0.37 total)
- ProgramBench: 25 coding puzzles, volt-bfcl/program_bench.py
- GAIA: 165 validation questions, volt-bfcl/gaia_benchmark.py
- All run on Groq llama-3.1-8b-instant for ~$0.05/1M tokens

### Paper
- `paper/draft.md` — arXiv-style, BFCL data, methodology, limitations
- `paper/benchmarks.md` — full benchmark roadmap
- `paper/tool_libraries_report.md` — Rust crate analysis

### Environment
- `.env` has GROQ_API_KEY + DATABASE_URL
- Ollama needs mxbai-embed-large for Volt's embedding pipeline
- SearchHQ token: generate at searchhq.setique.com/settings/mcp
- 98 GB free disk, 8 GB free RAM at idle (this machine)
