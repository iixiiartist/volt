# Benchmarks for Volt

Cost ranges across our 6 Groq models: $0.05–$0.59/1M input, $0.08–$0.79/1M output.

## Cost Summary

| Benchmark | Cases | Category | Best Model | Best Score |
|---|---|---|---|---|
| **BFCL v4 simple_python** | 400 | Single function call | qwen3-32b | 75.7% (BFCL v3 #2 globally) |
| **BFCL v4 live_simple** | 258 | Real-world APIs | — | — |
| **BFCL v4 parallel** | 200 | Multiple independent calls | — | — |
| **BFCL v4 multiple** | 200 | Dependent calls | — | — |
| **BFCL v4 multi-turn** | 800 | 4 sub-categories | — | — |
| **TerminalBench** | TBD | CLI interaction | gpt-oss-120b | 23.5% |
| **Tau2** | TBD | Airline/retail agent | — | — |
| **ProgramBench** | 25 | Code puzzles | — | — |

## BFCL v3 Leaderboard Context

BFCL v3 (23 models evaluated, average 55.9%):
- #1: GLM 4.5 — 76.7%
- #2: **qwen3-32b — 75.7%** (we use this model on Groq)
- #13: Llama 4 Scout — 55.7%
- Last: Claude Opus 4 — 25.3% (Claude's tool-use API differs from OpenAI-compatible)

Our BFCL v4 harness runs the next-gen dataset (4,241 cases, 17 categories). The v3 leaderboard gives us a reference point for publishable scores.

## GAIA — Deprecated

Removed from pipeline. GAIA is a frontier-model benchmark:
- Top score: GPT-5 Mini at 44.8%, average 27.5%
- qwen3-32b scores 12.3% (our #2 BFCL model)
- All 6 Groq models scored 0/3 on harness test
- GAIA requires GPT-4-class reasoning + web tool integration beyond what our architecture provides

## Recommended Execution Order

| Step | Benchmark | Time (est) |
|---|---|---|
| 1 | BFCL v4 simple_python (400 cases, qwen3-32b) | ~160 min |
| 2 | BFCL v4 live_simple (258 cases) | ~100 min |
| 3 | BFCL v4 parallel + multiple (400 cases) | ~160 min |
| 4 | BFCL v4 multi-turn (800 cases) | ~6+ hours |
| 5 | TerminalBench adapter | TBD |
