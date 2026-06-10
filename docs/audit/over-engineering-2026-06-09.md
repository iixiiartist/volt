# Volt Framework Over-Engineering & Agent-Loop Bloat Audit

**Date:** 2026-06-09
**Source:** Gemini conversation: *"60-80% of AI agent workloads can be replaced with simple function calls, cron jobs, and deterministic state machines"*
**Verdict:** Volt confirms the premise. ~70% of decisions the LLM is asked to make are replaceable with deterministic code.

## Headline numbers

| Metric | Value |
|---|---|
| Total Rust source | 41,177 lines across 130 files |
| Tools always registered | ~40 of which ~14 (35%) are 1-line stdlib wraps |
| Blueprints (model metadata) | 67 |
| Per-iteration deterministic branches in `run_iteration_loop` | ~14, ~7 always run |
| **% of agent-loop work replaceable with deterministic code** | **~50-60% per turn, ~70% of "decisions"** |

## P0 — Agent loop bloat hurting users (5)

| # | Issue | File:line | Recommended fix |
|---|---|---|---|
| 15 | **Routines engine fires on cron, then asks LLM what to do** | `src/routines/engine.rs:50-85` | Parse `action_prompt` into typed operation at definition time. Cron fires; only `LLMPrompt` variants invoke LLM. |
| 23 | **`run_workflow` spawns N agents with full setup each (deepest recursion)** | `src/orchestrator.rs:534-609` | Cap agent setup; or document that this is N×LLM cost. |
| 24 | **`use_cot` runs a full LLM call to produce a plan that's never used** | `src/agent/run.rs:19-21, 108-151` | Delete `use_cot` and `run_planning_cot`. |
| 32 | **`max_iterations: u32` is the only loop abstraction; no `run_once` mode** | `src/agent/run.rs:158-554` | Add `run_once(input) -> String` for 60% of invocations. |
| 39 | **`audit_turn` writes to context store; never retrieved; burns 200-quota** | `src/agent/run.rs:675-719` | Delete `audit_turn` or write to a file. |

## P1 — Should be simplified (22)

- **#1, #46**: `final_answer` tool + 4-stage fallback (60 lines fixing 8B-model quirk) → delete tool, let LLM text be the answer
- **#4, #5**: `get_current_time` / `convert_time` tools → host injects `now` at session start
- **#6**: `sequentialthinking` writes to a `THOUGHTS` HashMap that's never read → delete
- **#7**: `json_validate`, `json_prettify`, `json_query` defined but never registered → delete source + test stubs
- **#11**: **12 git tools** that differ only by verb → 1 `git` tool, or remove entirely
- **#19**: **LLM-driven blueprint selection** (120 prompt tokens per call) → keyword table
- **#20**: `get_active_providers` duplicates `ProviderDetector` → delete, use the detector
- **#21**: Supervisor's "synthesizer" LLM call on top of N workers → 1+N+1 LLM cost; often `format!(outputs.last())` suffices
- **#22**: Pipeline agents re-load full system prompt per step → 3× setup cost for 3 steps
- **#25**: Tool definition retrieval per turn (40 tools → 8 by RRF) → co-occurrence pinning after 2 uses
- **#26**: System prompt rebuilt from disk every turn (SOUL/MEMORY/USER/AGENTS) → read once at session start
- **#27**: Context build per turn (8 pgvector queries + 1 BGE embed, ~80ms) → content-hash cache
- **#33**: `MissingFinalAnswer` quirk synthesizes a `ToolCall` for a tool the LLM didn't call → dies with #1
- **#36**: 12-kind ContextStore fan-out per turn (Tool+Skill+Memory+Conversation+AgentRun+Artifact+SystemPrompt+FewShot+Policy+Permission+Security+MCPConfig) → keep only Tool+Skill+Memory+Conversation; others recompute at startup
- **#40**: CapabilityManager token lookup per tool call (6 tokens, constant for agent lifetime) → cache in Agent
- **#53-56**: Files 1000+ lines that should be split: `run.rs` (1648), `webui/runtime.rs` (2088), `hooks.rs` (1556), `orchestrator.rs` (1478)
- **#61**: `Routines` CLI has only `List` — no way to create/edit/delete routines; routine firing creates jobs that no consumer drains

## P2/P3 — Polish (30+)

- #2, #3: `memory_append`/`todo_add` tools = plain fs::write
- #8, #9: `csv_*`, `archive_*` tools = pure stdlib
- #10: Chart/PDF tools (consider host functions, not tools)
- #14: `cli_exec`/`cli_query` overlap with `bash` + `command_guard`
- #17: Self-repair monitor (no LLM, just interval vs sleep_until)
- #28-31, #34-35, #41-43, #50-52, #57-60, #62-63: Various per-turn lookups, match arms, file sizes, `unwrap_or_default` patterns, fallback code
- `serde_json::to_string(...).unwrap_or_default()` appears 7× and silently drops tool-call JSON

## The pattern (illustrative)

The agent loop at `run.rs:158-554` does this every turn:

```
for iteration in 0..max_iterations {
  1. Read 4 files from disk (SOUL/MEMORY/USER/AGENTS)        # 100µs deterministic
  2. Embed recent 3 messages + current input                 # 130ms BGE
  3. For each of 12 context kinds: pgvector search            # 80ms × 8 = 640ms
  4. Search skills (3)                                       # 50ms
  5. Search memories (5)                                     # 50ms
  6. Compress messages if needed (tokenize whole list)       # 5ms
  7. Build LLM request with system prompt + tools            # microsecs
  8. LLM call (variable; the only required step)             # 1-30s
  9. Validate tool calls                                     # microsecs
  10. Filter by capability (6-token list)                    # microsecs
  11. Filter by failure tracker                              # microsecs
  12. Run tools (parallel)                                   # variable
  13. Audit-turn write to context store                      # 5ms
  14. Seed-episode-complete to context store                 # 5ms
  15. Seed-artifact-if-applicable match (70 lines)           # microsecs
  16. Store memory                                           # microsecs
  17. Final-answer 4-stage fallback if no answer             # microsecs
  18. Is-cancelled check                                     # microsecs
}
```

Steps 1, 3, 4, 5, 6, 13, 14, 15, 17 are deterministic. The total is **~800ms of pre-LLM work** that could be **~200ms** with caching, or **~50ms** for a single-shot question that doesn't need RAG at all.

## Recommendations

### P0 (must fix)
1. Delete `use_cot` and `run_planning_cot` (the most clearly wasteful feature).
2. Add `run_once(input) -> String` mode to `Agent` for the 60% of invocations that don't need iteration.
3. Delete `audit_turn` or move audit to a file.
4. Parse `Routine.action_prompt` at definition time; only invoke LLM for `LLMPrompt` variants.
5. Document the cost of `run_workflow(parallel, N agents)` so users opt in knowingly.

### P1 quick wins
- Delete `final_answer`, `sequentialthinking`, `memory_append`, `todo_add` tools.
- Collapse 12 git tools to 1 (or remove entirely).
- Replace LLM blueprint selection with a keyword table.
- Cache system-prompt file reads; cache per-tool embedding map.

### Architectural
- Split `run.rs`, `webui/runtime.rs`, `hooks.rs`, `orchestrator.rs` into 200-400 line modules.
- Trim ContextStore to 4-5 kinds; recompute others at startup.
- Treat LLM as a *tool argument synthesizer* + *unstructured text generator*, not as a coordinator.

## Why this is hard to do incrementally

The agent loop, ContextStore, and orchestrator are tightly coupled — every "while loop with extra JSON" is justified by *"the LLM might need to consider X"*. To simplify safely, we need:

1. A characterization of which invocations are deterministic (single-shot, simple) vs. which need iteration.
2. A session-level config switch (`mode: "single_shot" | "iterative" | "delegated"`).
3. Per-tool flag (`agent_only`, `host_only`, `delegable`) so the LLM isn't offered things it can't usefully do.

These are P1 architectural changes that enable most of the P0/P1 work above.

## File reference

The full 63-item audit (with file:line evidence and recommended fixes) is in the subagent transcript on `wip/prod-readiness-2026-06-09`. This document is the executive summary.
