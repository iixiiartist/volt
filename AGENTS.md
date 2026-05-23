For the next OpenCode session on the clean machine:

## Current state

1. All changes committed and pushed to main (75a2bbb → 97dd244)
2. git pull, then check paper/ and volt-bfcl/ for context
3. cargo test --features testutils to verify everything builds

## What was in progress

- BFCL benchmark harness at volt-bfcl/benchmark.py — works, run with --limit N for quick tests
- Rust pipeline test at tests/bfcl_pipeline.rs — compiles, Ollama embeddings failed (OOM on this machine)
- Paper draft at paper/draft.md — arXiv-style, needs data from remaining BFCL categories + GAIA
- Token tracking added to AgentState and Orchestrator StepResult

## This machine's limitations

- 16GB RAM, GNU toolchain on Windows — sqlx-core and tokio nearly OOM during compile
- Ollama model loaded but can't allocate for embeddings (334M BERT model OOMs)
- BFCL JS/Java categories have TypeScript type annotations that need schema normalization (already handled in benchmark.py's _fix_parameters)

## Next steps (prioritized)

1. git pull, cargo test --features testutils to verify
2. Extend volt-bfcl/benchmark.py to cover BFCL live + multi-turn categories
3. Implement GAIA benchmark adapter (most impactful for paper)
4. cargo run to test the agent interactively with Groq

## Important: environment

- .env has GROQ_API_KEY and DATABASE_URL
- PowerShell session has a stale GROQ_API_KEY — the Python benchmark's .env override handles this
- For cargo test to work with the Rust pipeline test, need Ollama running with enough RAM for mxbai-embed-large

— saved from previous session, 2026-05-23
