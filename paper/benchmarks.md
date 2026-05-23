# Additional Benchmarks for Volt

Cost analysis for running on **Groq llama-3.1-8b-instant**
($0.05/1M input, $0.08/1M output).

## Cost Summary

| Benchmark | Cases | LLM Calls | Est. Input Tokens | Est. Output Tokens | **Cost (Groq)** |
|---|---|---|---|---|---|
| **BFCL non-live** | 1,240 | 1,240 | 2,480,000 | 124,000 | **$0.13** |
| **BFCL live** | 800 | 800 | 2,000,000 | 120,000 | **$0.11** |
| **BFCL multi-turn** | 200 | 1,000 | 4,000,000 | 300,000 | **$0.22** |
| **GAIA dev** | 165 | 1,320 | 5,280,000 | 660,000 | **$0.32** |
| **GAIA test** | 301 | 3,010 | 15,050,000 | 1,806,000 | **$0.90** |
| **Tau-Bench** | 100 | 4,000 | 12,000,000 | 1,200,000 | **$0.70** |
| **ProgramBench** | 50 | 400 | 1,200,000 | 160,000 | **$0.07** |
| **SimpleQA** | 500 | 500 | 500,000 | 50,000 | **$0.03** |
| **TOTAL** | — | — | — | — | **$2.48** |

**Key takeaway:** Groq is so cheap ($0.05/$0.08 per million tokens) that
even the most complex benchmark (GAIA test, 301 questions × 10 turns each)
costs under $1 in LLM inference. The entire BFCL suite runs for under $0.50.

**Additional costs:** Only GAIA web search may need a SerpAPI subscription
(~$50/month). BFCL live_web_search also needs SerpAPI. Everything else is
pure LLM calls.

---

## 1. BFCL Full (Extend existing harness)

**Status:** Harness already built for non-live categories
**Cost (Groq):** $0.46 (all 3 subsets)
**Est. wall time:** 10-30 min
**Docker:** No

**To extend:**
1. Add live categories to benchmark.py (`BFCL_v4_live_simple.json`, etc.)
2. Add multi-turn categories (need state management in harness)
3. Wire up full BFCL evaluator from `bfcl-eval` PyPI package (AST matching)

---

## 2. GAIA (General AI Assistants)

**Status:** Not started
**Cost (Groq):** $1.22 (dev + test)
**Est. wall time:** 2-4 hours
**Docker:** No
**Needs:** SerpAPI or Firecrawl for web search (~$50/mo)

**How to run:**
1. Accept dataset terms on HuggingFace
2. Download 165 dev + 301 test questions with attachments (PDFs, images, audio)
3. For each question, Volt agent:
   - Reads attachments via `read` tool
   - Searches web via `web_fetch` / `web_scrape`
   - Runs computations via `bash`
   - Reports final answer
4. Submit answers to leaderboard

**Volt advantage:** Multi-agent orchestration can parallelize sub-questions.
Memory system helps retain context across long multi-step chains.
RAG tool selection keeps per-turn costs low during extended reasoning.

---

## 3. Tau-Bench (Multi-turn Agent Evaluation)

**Status:** Not started
**Cost (Groq):** $0.70 (100 tasks)
**Est. wall time:** 4-8 hours
**Docker:** No (pip install tau-bench)
**Needs:** LLM for user simulator (included in cost above)

**How to run:**
1. `pip install tau-bench`
2. Implement Volt adapter that speaks tau-bench JSON protocol
3. Run eval: `tau-bench run --agent volt --model groq/llama-3.1-8b`
4. Compare against published results

**Volt advantage:** Tests multi-turn reliability. Memory/skills system
helps with policy adherence across long conversations.

---

## 4. ProgramBench (Code Puzzles)

**Status:** Not started
**Cost (Groq):** $0.07
**Est. wall time:** 15-30 min
**Docker:** No

New benchmark from the mini-SWE-agent team. Programming puzzles solved
via bash (code execution) + file I/O. Minimal overhead.

---

## 5. SimpleQA (Factuality)

**Status:** Not started
**Cost (Groq):** $0.03
**Est. wall time:** 5-10 min
**Docker:** No

Quick validation of Volt's web search + reasoning. 500 questions,
single-turn, cheap.

---

## Recommended Execution Order

| Step | Benchmark | Cost | Time | Cumulative |
|------|-----------|------|------|------------|
| 1 | **BFCL full** (extend live + multi-turn) | $0.46 | 30 min | $0.46 |
| 2 | **GAIA dev** (validation) | $0.32 | 2 h | $0.78 |
| 3 | **ProgramBench** | $0.07 | 30 min | $0.85 |
| 4 | **Tau-Bench** | $0.70 | 6 h | $1.55 |
| 5 | **GAIA test** (if dev passes) | $0.90 | 4 h | $2.45 |
| 6 | **SimpleQA** | $0.03 | 10 min | $2.48 |

**Total: ~$2.50, ~13 hours wall time** (mostly the LLM responding).
