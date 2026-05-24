# Rust Tool Libraries for Volt — Feasibility Report

**Date:** 2026-05-23
**Machine:** Dell Latitude 7350, Intel Core Ultra 5 135U (14-core + NPU), 15.5 GB RAM, 7.1 GB free disk

---

## 1. Web Automation

| Crate | Downloads | Stage | Description | Feasibility |
|---|---|---|---|---|
| **chromiumoxide** | 1.9M | Mature | Chrome DevTools Protocol — control headless Chrome. Navigate, click, screenshot, extract, execute JS. | ✅ **HIGH.** Add `--tool browser_navigate`, `browser_click`, `browser_screenshot`. Needs Chrome installed (~200 MB disk, acceptable). |
| **spider** | 2.0M | Mature | Web crawler. Concurrent crawling, follows links, renders JS (via chrome). | ✅ **HIGH.** Add as advanced `web_crawl` tool. Complements existing `web_fetch`/`web_scrape`. |
| **headless_chrome** | 342K | Mature | Alternative CDP client. Simpler API than chromiumoxide. | ✅ **MEDIUM.** Evaluate which API is cleaner for agent tool wrapping. |
| **scraper** | — | Already in Volt | CSS selector HTML parsing (`src/tools/scrape_tool.rs`). | Already done. |

**Disk impact:** ~3-5 MB per crate + Chrome browser (~200 MB optional, can use system Chrome).
**RAM impact:** ~50-100 MB during browser automation (Chrome process).
**Feasibility: ✅ HIGH — can build and test on this machine.**

---

## 2. Computer / Desktop Automation

| Crate | Downloads | Stage | Description | Feasibility |
|---|---|---|---|---|
| **uiautomation** | 276K | Mature | Windows UI Automation — find windows, buttons, text fields by name/type. Click, type, read. | ✅ **HIGH.** Windows-only but we're on Windows. Add `--tool desktop_click`, `desktop_type`, `desktop_find`. |
| **terminator-rs** | 54K | Active | Cross-platform desktop GUI automation (Playwright-style). Newer, broader platform support. | ⚠️ **MEDIUM.** Newer crate. Evaluate stability. If solid, better than uiautomation for cross-platform. |
| **windows-capture** | 286K | Mature | Fast Windows screen capture. Capture specific windows or full screen. | ✅ **HIGH.** Add `--tool screenshot` (replaces shelling out to external tools). |
| **enigo** | 1.5M | Mature | Cross-platform mouse/keyboard simulation. Move mouse, click, type. | ✅ **HIGH.** Simple, well-tested. Add `--tool mouse_move`, `mouse_click`, `keyboard_type`. |

**Disk impact:** ~2-5 MB per crate.
**RAM impact:** ~10-30 MB during automation.
**Feasibility: ✅ HIGH — particularly strong on Windows. uiautomation + enigo + windows-capture together give full desktop control.**

---

## 3. Artifact Creation (PDF, Charts, Diagrams, HTML)

| Crate | Downloads | Stage | Description | Feasibility |
|---|---|---|---|---|
| **lopdf** | 8.1M | Very Mature | PDF creation and manipulation. Generate reports, invoices, documents. | ✅ **HIGH.** Add `--tool create_pdf`, `edit_pdf`, `merge_pdfs`. Essential for agent productivity. |
| **plotly** | 3.5M | Mature | Generate Plotly.js interactive charts as HTML. Line, bar, scatter, heatmap, etc. | ✅ **HIGH.** Add `--tool create_chart`. Output interactive HTML the agent can share. |
| **selkie-rs** | 7.5K | New | Mermaid diagram renderer (flowcharts, sequence diagrams, Gantt). | ⚠️ **MEDIUM.** New, but Mermaid is widely used. Could render architecture diagrams. |
| **ariel-rs** | 94 | Very New | Rust port of Mermaid JS. Headless SVG generation. | ⚠️ **LOW.** Too new. Watch for maturity. |
| **markdown** | 6.6M | Very Mature | CommonMark compliant parser + AST. Convert markdown to HTML. | ✅ **HIGH.** Add `--tool markdown_to_html`. |
| **pdfium-render** | 1.1M | Mature | Render PDF pages to images. Extract text, annotations. | ✅ **HIGH.** Add `--tool pdf_render_page`, `pdf_extract_text`. Read PDFs visually. |

**Disk impact:** ~3-10 MB per crate (lopdf is small; pdfium-render bundles a C++ library, ~20 MB).
**RAM impact:** ~30-50 MB during PDF/chart generation.
**Feasibility: ✅ HIGH — lopdf and plotly are very mature and straightforward to integrate.**

---

## 4. Design / Image / Media Tools

| Crate | Downloads | Stage | Description | Feasibility |
|---|---|---|---|---|
| **image** | — | Very Mature | Image loading, resizing, cropping, filtering, format conversion. De facto standard. | ✅ **HIGH.** Add `--tool image_resize`, `image_convert`, `image_filter`. |
| **resvg** | — | Mature | SVG rendering to PNG. Renders SVG files without a browser. | ✅ **HIGH.** Combine with ariel-rs/selkie-rs to generate diagrams as PNG. |
| **rustmotion** | 102 | Very New | Render motion design videos from JSON. No browser, no Node. Single binary. | ❌ **LOW.** Too new. Could be interesting later. |

**Disk impact:** ~2-8 MB per crate.
**RAM impact:** ~20-50 MB during image processing.
**Feasibility: ✅ HIGH — image crate is trivial to add.**

---

## 5. Summary — Priority Implementation Order

| Priority | Tool | Crate | Agent Tool Name | Effort | Value |
|---|---|---|---|---|---|
| 1 | **Screenshot** | windows-capture | `screenshot` | 1 day | 📸 Capture screen/windows |
| 2 | **PDF creation** | lopdf | `create_pdf`, `edit_pdf` | 1-2 days | 📄 Generate reports |
| 3 | **Charts** | plotly | `create_chart` | 1 day | 📊 Visualize data |
| 4 | **Desktop automation** | uiautomation + enigo | `desktop_find`, `desktop_click`, `desktop_type` | 2-3 days | 🖱️ Control any app |
| 5 | **Browser automation** | chromiumoxide | `browser_navigate`, `browser_click`, `browser_extract` | 2-3 days | 🌐 Full browser control |
| 6 | **Image processing** | image | `image_convert`, `image_resize` | 0.5 day | 🖼️ Image manipulation |
| 7 | **PDF rendering** | pdfium-render | `pdf_render`, `pdf_extract_text` | 1 day | 📖 Read PDFs visually |

**Total effort:** ~10-14 days for a complete tool suite.
**Total disk impact:** ~50-80 MB (crates) + ~200 MB (optional Chrome) = **~280 MB max**.
**RAM at runtime:** ~100-200 MB when all tools loaded (most are lazy/on-demand).

---

## 6. Machine Feasibility Assessment

| Resource | Current | Needed | Verdict |
|---|---|---|---|
| **Disk free** | 7.1 GB | ~300 MB for new crates + build artifacts | ✅ **Fine** |
| **RAM free** | 8 GB | ~100-200 MB at runtime | ✅ **Fine** |
| **CPU** | Ultra 5 135U (14-core) | Compilation time only | ✅ **Fine** |
| **NPU** | Intel AI Boost available | Not directly used by these crates | ⏸️ NPU not needed for these tasks |
| **Compile time** | — | Adding 5-10 dependencies, incremental builds | ⚠️ First build will be slow (~30-60 min) |

**Verdict: This machine can handle all proposed libraries.** The key constraint is disk (7.1 GB free) and compile time. Once compiled, runtime overhead is minimal.

---

## 7. Recommended Package Structure

Since you mentioned an "add-on" pattern, I recommend:

```
volt/
  src/
    tools/
      browser/       # chromiumoxide-based tools
      desktop/       # uiautomation + enigo tools
      media/         # image + resvg tools
      pdf/           # lopdf + pdfium-render tools
      chart/         # plotly tools
```

Each category gets its own module behind a Cargo feature flag:

```toml
[features]
default = ["tools-browser", "tools-desktop", "tools-media", "tools-pdf", "tools-chart"]
tools-browser = ["chromiumoxide"]
tools-desktop = ["uiautomation", "enigo"]
tools-media = ["image", "resvg"]
tools-pdf = ["lopdf", "pdfium-render"]
tools-chart = ["plotly"]
```

This way the agent can be compiled with only the tools needed for a given deployment, keeping the binary small.
