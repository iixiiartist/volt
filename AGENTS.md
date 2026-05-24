## Handoff for next session

### First steps
```bash
git pull
cargo build --features "tools-screenshot"  # verify new tools compile
cargo test --features testutils
```

### New tools added
| Tool | Module | Description |
|---|---|---|
| **screenshot** | `src/tools/screenshot.rs` | Capture primary monitor, base64 PNG |
| **create_bar_chart** | `src/tools/chart_tool.rs` | Bar chart from labels+values, saves HTML with Plotly.js |
| **create_line_chart** | `src/tools/chart_tool.rs` | Line chart from labels+values, saves HTML with Plotly.js |

Screenshot dependency: `windows-capture`, `image`, `base64` (feature `tools-screenshot`, default on).
Charts are pure Rust + serde_json, no extra deps.

### Tools not yet built (crate API issues on this toolchain)
- PDF creation (lopdf API changed)
- Desktop automation (enigo/uiautomation API mismatch)
- Browser automation (chromiumoxide zip conflict)

These can be added on the other machine — see `paper/tool_libraries_report.md` for specs.

### Benchmarks to run
1. **BFCL full** — extend `volt-bfcl/benchmark.py` to live + multi-turn (~$0.46 on Groq)
2. **GAIA** — implement adapter for 165 dev questions (~$0.32 on Groq)
3. **ProgramBench** — code puzzles (~$0.07)

### Paper draft at `paper/draft.md`
BFCL results: 74% token savings, +6.7pp accuracy with RAG. Ready for submission.

### Environment notes
- `.env` has working GROQ_API_KEY + DATABASE_URL
- Ollama needs `mxbai-embed-large` for Volt's embedding pipeline (pull on other machine)
- Pagefile is system-managed (will grow under compilation load)
- Docker VHDX compacted to 3 GB (was 33 GB)
- **98 GB free disk, 8 GB free RAM at idle**
