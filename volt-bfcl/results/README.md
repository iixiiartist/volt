# Volt Benchmark Results

## BFCL simple_python (400 cases, full sweep)

| Configuration | Context Kinds | Passed | Accuracy | Avg Latency |
|---|---|---|---|---|
| `tool_only` (baseline) | tool | 324/400 | **81.0%** | 13.3s |
| `tool_skill_memory_conversation_artifact` | tool,skill,memory,conversation,artifact | 330/400 | **82.5%** | 14.3s |
| Delta | +artifact | +6 | **+1.5pp** | +1.0s |

### Ablation sweep (50 cases per config)

| Configuration | Accuracy | vs Baseline |
|---|---|---|
| `tool_only` | 80.0% | — |
| `tool_skill` | 82.0% | +2.0pp |
| `tool_skill_memory` | 76.0% | -4.0pp |
| `tool_skill_conversation` | 76.0% | -4.0pp |
| `tool_skill_memory_conversation` | 82.0% | +2.0pp |
| `tool_skill_memory_conversation_artifact` | **86.0%** | **+6.0pp** |
| `all` (12 kinds) | 82.0% | +2.0pp |

Key observations:
- **Artifact context** provides the strongest individual lift (+6pp on 50-case, +1.5pp on 400-case)
- Memory/conversation alone hurt for single-turn BFCL (no prior context to retrieve)
- Adding all 12 context kinds regresses — selective beats exhaustive
- With `VOLT_MINIMAL_TOOLS=1`, average latency drops from >120s to ~13s per case

## ProgramBench (25 cases)

- Accuracy: **92%** (23/25)
- Model: llama-3.1-8b-instant

## Multi-turn Episodic Memory (3 sequences, 6 turns)

- Math recall: base=10 remembered across turns ✓
- Code artifact: double.py remembered across turns ✓
- Factorial chain: 5! = 120 remembered across turns ✓

## Methodology

All BFCL runs use:
- Model: `llama-3.1-8b-instant` (Groq)
- `EMBEDDING_PROVIDER=none` (deterministic fallback)
- `VOLT_MINIMAL_TOOLS=1` (~16 essential tools only)
- Release binary build
- BFCL v4 simple_python test set
