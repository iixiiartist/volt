# Additional Benchmarks for Volt

Research findings on benchmarks that can run with Volt's existing tool set
(no Docker, no infrastructure beyond what Volt already has).

## Candidate Benchmarks

### 1. GAIA (General AI Assistants) — RECOMMENDED

**URL:** https://huggingface.co/datasets/gaia-benchmark/GAIA
**Tasks:** 466 total (165 validation + 301 test), 3 difficulty levels
**Docker:** No
**Tools needed:** web_fetch, web_scrape, bash (code execution), read (PDF/txt), write
**Est. API cost:** $80-200 for 165-dev set
**Volt fit:** Excellent. Maps directly to existing tools. Tests multi-step reasoning.

**How to run:**
1. Download the GAIA dataset (gated, requires HuggingFace login)
2. For each question, Volt agent uses web_fetch/search to find info, bash to compute, read to parse attachments
3. Answer is a short string — easy to evaluate
4. Compare against published leaderboard scores

**Volt advantage:** Multi-agent orchestration can parallelize sub-questions. RAG tool selection reduces token overhead across many tool calls.

---

### 2. BFCL V4 Full (Live + Multi-turn) — ALREADY PARTIALLY BUILT

**URL:** https://github.com/ShishirPatil/gorilla/tree/main/berkeley-function-call-leaderboard
**Tasks:** 2,000+ (already built for non-live subset)
**Docker:** No
**Tools needed:** None for non-live (just LLM calls); SerpAPI for web_search category
**Est. API cost:** $30-60 for full 2,000-case eval
**Volt fit:** Already benchmarked on non-live. Extend to live + multi-turn categories.

**How to extend:**
1. Add `live_simple`, `live_parallel`, `live_multiple`, `live_irrelevance`, `live_relevance` categories
2. Add multi-turn categories (need state management)
3. Full AST/execution evaluation (need bfcl-eval package)

---

### 3. τ³-bench (Tau-Bench) — MULTI-TURN, NO DOCKER

**URL:** https://github.com/sierra-research/tau2-bench
**Tasks:** ~100-200 per domain (airline, retail, telecom, banking)
**Docker:** No (pip install)
**Tools needed:** Domain-specific API tools, user simulator (LLM)
**Est. API cost:** $150-400 full eval (agent + user simulator)
**Volt fit:** Tests multi-turn reliability, policy adherence, long sessions.

**How to run:**
1. `pip install tau-bench`
2. Implement Volt agent adapter that speaks tau-bench's JSON protocol
3. Run eval with `tau-bench run --agent volt`
4. Compares against published results (GPT-4, Claude)

**Volt advantage:** Memory/skills system helps with long sessions. RAG keeps multi-turn costs low.

---

### 4. ProgramBench — NEW SWE-BENCH VARIANT

**URL:** https://mini-swe-agent.com/latest/usage/programbench/
**Tasks:** Programming puzzles (from mini-SWE-agent paper)
**Docker:** No (standalone)
**Tools needed:** bash (code execution), read, write
**Est. API cost:** $20-50
**Volt fit:** Tests code generation and execution. Minimal overhead with RAG.

---

### 5. SimpleQA / Factuality Benchmarks — QUICK TO RUN

**URL:** https://github.com/openai/simple-evals
**Tasks:** 100-500 questions
**Docker:** No
**Tools needed:** web_fetch (for search), maybe bash
**Est. API cost:** $5-20
**Volt fit:** Quick validation. Tests Volt's web search + reasoning capabilities.

---

## Recommended Path

| Order | Benchmark | Est. Cost | Time | Why |
|-------|-----------|-----------|------|-----|
| 1 | **BFCL full (remaining categories)** | $30-60 | 1-2h | Already have the harness; extend coverage |
| 2 | **GAIA validation set** | $80-200 | 4-8h | Highest impact, tests real tool use, no Docker |
| 3 | **Tau-Bench** | $150-400 | 8-16h | Multi-turn, policy adherence, high signal |
| 4 | **ProgramBench** | $20-50 | 1-2h | Code tasks, complements GAIA |
| 5 | **SimpleQA** | $5-20 | 30min | Quick factuality check |

Total est. cost for all: **$285-730**
