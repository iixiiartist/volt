# Context Kind Ablation Study

This directory contains the harness for measuring how each context kind contributes to Volt's tool-selection accuracy.

## Prerequisites

1. Build the release binary:
   ```bash
   cargo build --release
   ```

2. Set your LLM API key (Groq is cheapest for benchmarking):
   ```bash
   export GROQ_API_KEY=gsk_...
   # or
   export OPENAI_API_KEY=sk-...
   ```

3. Download BFCL test data (follow `benchmark.py` instructions if needed).

## Running the Study

```bash
python volt-bfcl/context_ablation.py \
  --category simple_python \
  --limit 50 \
  --model llama-3.1-8b-instant
```

### Arguments

| Flag | Default | Description |
|---|---|---|
| `--category` | `simple_python` | BFCL category to benchmark |
| `--limit` | `20` | Cases per configuration |
| `--model` | `llama-3.1-8b-instant` | LLM model for agent runs |
| `--configs` | `all` | Comma-separated config names (e.g. `tool_only,tool_skill`) |
| `--output` | auto | Path to write JSON results |

### Configurations Tested

| Name | Context Kinds Enabled |
|---|---|
| `tool_only` | Tool |
| `tool_skill` | Tool, Skill |
| `tool_skill_memory` | Tool, Skill, Memory |
| `tool_skill_conversation` | Tool, Skill, Conversation |
| `tool_skill_memory_conversation` | Tool, Skill, Memory, Conversation |
| `tool_skill_memory_conversation_artifact` | + Artifact |
| `all` | All 12 kinds |

Each configuration gets the same total retrieval budget (8 slots), distributed evenly across enabled kinds.

## Output

Results are written to `volt-bfcl/results/volt_ablation_YYYYMMDD_HHMMSS.json`:

```json
{
  "timestamp": "2026-05-25T20:00:00Z",
  "model": "llama-3.1-8b-instant",
  "category": "simple_python",
  "cases_per_config": 50,
  "configurations": [
    {
      "config": "tool_only",
      "kinds": "tool",
      "passed": 45,
      "failed": 5,
      "accuracy": 90.0,
      "avg_latency_sec": 2.1,
      "total_prompt_tokens": 15000,
      "total_completion_tokens": 3000,
      "details": [...]
    }
  ]
}
```

A summary table is also printed to stdout:

```
Configuration                              Acc Latency    Prompt   Complete
================================================================================
tool_only                                 90.0%      2.1s      15000       3000
tool_skill                                94.0%      2.3s      16000       3200
...
```

## Interpreting Results

- **Tool-only baseline**: Measures accuracy with just tool schemas (static injection equivalent).
- **+Skill**: Adds compiled SKILL.md manifests — typically +2-4pp for multi-step tasks.
- **+Memory**: Adds user memories — helps with personalized or repeated queries.
- **+Conversation**: Adds recent chat history — critical for multi-turn coherence.
- **+Artifact**: Adds codebase artifacts — improves programming tasks.
- **All kinds**: Full unified RAG — expected to be within 1-2pp of the best subset.

The goal is to identify the **minimum viable subset** that achieves near-full accuracy, which determines runtime cost for edge deployments.
