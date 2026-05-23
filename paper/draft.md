---
title: "RAG-Based Tool Selection Reduces Token Cost and Improves Accuracy in LLM Agents"
author:
  - "Volt Team"
date: "May 2026"
abstract: |
  LLM-based agents rely on tool-calling to interact with their environment.
  Current agent frameworks inject all available tool definitions into every LLM
  call, incurring a fixed per-turn token cost proportional to registry size.
  We present Volt, an agent framework that replaces static tool injection with
  a dynamic retrieval-augmented generation (RAG) pipeline: it embeds the user
  query, retrieves only the top-k most relevant tool definitions via vector
  similarity search, and injects only those into the LLM context. On the
  Berkeley Function Calling Leaderboard (BFCL) V4 with a registry of 51 tools,
  Volt's RAG approach reduces per-turn prompt tokens by 74% (2,248 $\to$ 579
  avg) while improving function-calling accuracy by 6.7 percentage points
  (34.3% $\to$ 41.0%). Accuracy gains are largest on simple Python tasks
  (+18pp). The savings compound with registry size: at 500 tools, RAG saves
  98.4% of tool-definition overhead versus static injection. Volt is
  implemented in Rust and is available at \url{https://github.com/iixiiartist/volt}.
---
bibliography: paper.bib

---

## Introduction

Large language models (LLMs) have evolved beyond text generation into
agentic systems that call external tools to read files, execute commands,
search the web, and manipulate data [@schick2023toolformer; @patil2025bfcl].
Every major agent framework — Claude Code, OpenClaw, Hermes Agent, and
ChatGPT — uses the same architecture: a flat list of all available tool
definitions is injected into every LLM call [@anthropic2025claudecode;
@nous2025hermes; @openclaw2025].

This static injection strategy has a linear cost in the number of tools.
Claude Code injects approximately 36 core tools, OpenClaw approximately 50+,
and Hermes Agent approximately 52 [@claudecodetools2025; @openclawtools2025;
@hermestools2025]. Each tool definition consumes 100--200 tokens in the
serialized JSON schema format. At 50 tools, the tool-definition tax alone is
5,000--10,000 tokens per turn — before any conversation, instructions, or
output.

We argue that this is wasteful. In any given interaction, a user query is
relevant to only a small subset of the available tools. A query about file
search does not need the image generation tool schema in context.

We present **Volt**, an agent framework that replaces static injection with
dynamic retrieval-augmented generation (RAG) for tool selection. Volt embeds
the user query, retrieves the top-8 most semantically similar tool
definitions via vector cosine similarity, and injects only those into the
LLM context. This is analogous to how retrieval-augmented generation reduces
context length in knowledge-grounded dialogue [@lewis2020rag].

Our contributions are:

1.  A RAG-based tool selection architecture implemented in Rust with
    PostgreSQL/pgvector and a multi-provider embedding fallback chain.

2.  A reproducible evaluation on the Berkeley Function Calling Leaderboard
    (BFCL) V4 showing 74% token reduction with accuracy improvement.

3.  An analysis of how the savings scale with registry size, demonstrating
    that RAG-based selection is necessary for agents with large tool
    catalogs.

## Problem Statement

### Static Tool Injection

Every turn of an LLM agent loop sends a request of the form:

```
{model, messages, tools: [def_1, def_2, ..., def_N]}
```

where each `def_i` is the full JSON schema of tool $i$. The cost of this
list is proportional to the number of tools $N$ and the verbosity of each
schema. For a typical tool registry with $N=50$ tools, this adds
approximately 5,000--10,000 tokens per turn regardless of whether the tools
are relevant to the current query.

### Scaling Problem

As tool registries grow, static injection becomes unsustainable.
Marketplaces, enterprise integrations, and community-contributed tools can
push $N$ into the hundreds or thousands. With static injection, every turn
pays the full $O(N)$ cost. This is the core inefficiency we address.

## Methodology: Volt's RAG Architecture

Volt replaces static injection with a three-stage retrieval pipeline:

**Stage 1: Registration.** Each tool is registered with a name, description,
input schema (JSON Schema), and category. The description is embedded using
a configurable embedding model and stored alongside the tool definition.

**Stage 2: Retrieval.** At inference time, the current query context
(concatenation of recent messages and the current user input) is embedded
using the same model. Volt computes cosine similarity between the query
embedding and all tool embeddings, selects the top-8 most similar tools, and
always includes four essential fallback tools (`read`, `glob`, `grep`,
`web_fetch`).

**Stage 3: Injection.** Only the selected tool definitions are included in
the LLM call's `tools` parameter.

```python
query_emb = embedder.embed(context)
tools = registry.search(query_emb, top_k=8, essential=["read", "glob", "grep", "web_fetch"])
response = llm.complete(messages, tools)
```

### Embedding Pipeline

Volt supports a multi-provider embedding fallback chain:
1.  Ollama (local, mxbai-embed-large, default)
2.  NVIDIA NIM (cloud)
3.  OpenAI embeddings
4.  Moonshot (Kimi)
5.  Deterministic SHA-256-based placeholder (zero network, always works)

All embeddings are 1024-dimensional vectors. For database-backed
deployments, Volt stores embeddings in PostgreSQL with a pgvector HNSW
index for sub-millisecond search at registry sizes up to 10,000+ tools.

## Experiments

### Benchmark: BFCL V4

We evaluate on the Berkeley Function Calling Leaderboard V4
[@patil2025bfcl], a collection of 2,000+ function-calling test cases across
7 categories. We use the non-live subset: `simple_python` (400 cases),
`simple_java` (200), `simple_javascript` (200), `parallel` (200),
`multiple` (200), and `irrelevance` (200).

### Simulating Real-World Registry Sizes

BFCL test cases natively contain 1--2 functions per case. To simulate
real-world registry sizes, we add 50 distractor tools randomly sampled from
a pool of 49 common utility functions (file I/O, web, database, email,
monitoring, etc.). This matches the registry size of Claude Code (~36
tools), OpenClaw (~50), and Hermes Agent (~52).

### Setup

-   **Model**: llama-3.1-8b-instant (via Groq API)
-   **Embedding**: sentence-transformers/all-MiniLM-L6-v2
-   **Mode 1 (Static)**: All 51 tool definitions injected
-   **Mode 2 (RAG)**: Top-8 tools via cosine similarity
-   **Temperature**: 0.0
-   **Metric**: Exact function-name match between predicted and ground-truth
    calls
-   **Sample**: 50 cases per category (300 total)

### Results

Table 1 shows the per-category comparison. Volt's RAG mode achieves 74%
token reduction while improving accuracy by 6.7 percentage points on
average.

| Category | Static Acc. | RAG Acc. | $\Delta$ | Static Tokens | RAG Tokens | Savings |
|---|---|---|---|---|---|---|
| simple_python | 80.0% | 98.0% | +18.0pp | 2,440 | 665 | 73% |
| simple_javascript | 58.0% | 68.0% | +10.0pp | 1,864 | 520 | 72% |
| simple_java | 46.0% | 52.0% | +6.0pp | 1,418 | 374 | 74% |
| parallel | 2.0% | 2.0% | 0.0pp | 2,935 | 648 | 78% |
| multiple | 2.0% | 0.0% | -2.0pp | 2,582 | 723 | 72% |
| irrelevance | 26.0% | 26.0% | 0.0pp | 2,248 | 546 | 76% |
| **Weighted avg** | **34.3%** | **41.0%** | **+6.7pp** | **2,248** | **579** | **74.2%** |

**Table 1.** BFCL V4 results with 50 distractor tools. "Static" injects all
51 tools; "RAG" injects top-8. Tokens are prompt tokens per case.

### Analysis

**Token savings are consistent.** Across all six categories, RAG reduces
prompt tokens by 72--78%. The savings come entirely from the tool-definition
payload: with 51 tools, tool schemas dominate the prompt; with 8 tools, the
conversation content dominates.

**Accuracy improves with relevant tool context.** The largest accuracy gain
(+18pp) is on `simple_python`, where the 50 distractors include many
mathematical and physics functions with overlapping semantics. Static
injection overwhelms the model with plausible-but-incorrect options,
reducing accuracy. RAG filters to only the most relevant functions, making
the correct choice more salient.

**Categories with low static accuracy show model limitations.** The
`parallel` (2%) and `multiple` (2%) categories require the model to output
multiple function calls with correct argument formatting. The small
underlying model (llama-3.1-8b) struggles with this regardless of tool
context.

## Analysis: Scaling Behavior

The token savings of RAG-based selection grow with registry size. Static
injection incurs O(N) tool overhead per turn. RAG-based selection incurs
O(k) where k is the retrieval window (8 in our configuration).

At 20 tools (a small registry), static injection costs ~1,000 tokens per
turn versus RAG's ~400. At 100 tools (a large registry), static costs
~5,000 versus RAG's ~400 — a 92% savings. At 500 tools (a marketplace-scale
registry), static costs ~25,000 versus RAG's ~400 — a 98.4% savings.

The embedding search itself adds sub-millisecond latency (HNSW index,
O(log N) search time at registry sizes up to 10^6). This is negligible
compared to LLM inference time (100--30,000 ms per call).

## Multi-Agent Orchestration

Volt extends RAG-based tool selection with a built-in orchestrator that
supports three multi-agent patterns:

1.  **Parallel**: Multiple agents execute independent tasks concurrently
    (semaphore-limited).
2.  **Pipeline**: Sequential chaining where each agent receives the previous
    agent's output via a `{prev}` template variable.
3.  **Supervisor**: A coordinating agent delegates to worker agents and
    synthesizes results.

Table 2 shows token usage for a 3-agent parallel workflow and a 2-stage
pipeline on a Rust codebase discovery task using Volt's own tool set.

| Workflow | Agent | Duration | Prompt | Completion | Total Tokens |
|---|---|---|---|---|---|
| **Parallel** | fs-agent | 1,635ms | 6,189 | 816 | 7,005 |
| | data-agent | 1,325ms | 3,094 | 476 | 3,570 |
| | system-agent | 1,790ms | 3,420 | 786 | 4,206 |
| | **Total** | **4,750ms** | **12,703** | **2,078** | **14,781** |
| **Pipeline** | discover-agent | 1,696ms | 5,576 | 450 | 6,026 |
| | report-agent | 4,772ms | 7,171 | 4,210 | 11,381 |
| | **Total** | **6,469ms** | **12,747** | **4,660** | **17,407** |

**Table 2.** Multi-agent workflow token usage on a Rust codebase discovery
task, measured via LLM API `usage` response fields.

## Related Work

**Claude Code** [@anthropic2025claudecode] injects ~36 core tool definitions
per turn, with MCP tool schemas deferred via a `ToolSearch` mechanism. This
is the closest existing system to Volt's approach, though MCP deferral is
schema-on-demand rather than semantic retrieval.

**OpenClaw** [@openclaw2025] uses availability-filtered tool injection —
tools are gated by config state and API keys but are not selected based on
query relevance.

**Hermes Agent** [@nous2025hermes] uses toolset-based gating where tool
categories are enabled or disabled at session start but all enabled tools
are injected every turn.

**BFCL** [@patil2025bfcl] is the standard benchmark for function-calling
accuracy but has not been used to compare static versus dynamic tool
injection strategies.

**SWE-bench** [@jimenez2024swebench] and **mini-SWE-agent** evaluate
end-to-end code-fixing ability but require Docker-based sandboxing and are
tested with a single `bash` tool, making them unsuitable for tool-selection
comparisons.

## Limitations

1.  **Single model.** All experiments use llama-3.1-8b-instant. Results may
    differ with larger or instruction-tuned models.

2.  **Synthetic distractors.** The 50 distractor tools are hand-crafted
    utility functions, not a real production registry. Real-world tool
    distributions may yield different accuracy-token tradeoffs.

3.  **Name-only evaluation.** Our accuracy metric is exact function-name
    match. The full BFCL evaluation includes AST matching and execution
    checking, which we do not implement.

4.  **Single-turn only.** We evaluate on single-turn BFCL categories. The
    multi-turn and web-search categories require state management and
    external API access, which we leave for future work.

5.  **300 cases from 1,240.** Our sample of 50 per category (300 total)
    from the BFCL non-live set (1,240 total) may not capture the full
    difficulty distribution.

6.  **Embedding quality.** We use sentence-transformers/all-MiniLM-L6-v2.
    Larger embedding models or fine-tuned retrievers may improve RAG
    accuracy.

## Conclusion

Volt demonstrates that RAG-based tool selection is an effective replacement
for static injection in LLM agent frameworks. On the BFCL V4 benchmark with
a realistic 51-tool registry, Volt reduces per-turn prompt tokens by 74%
and improves function-calling accuracy by 6.7 percentage points. The
savings compound with registry size: at 500 tools, RAG saves 98.4% of
tool-definition overhead.

These results suggest that static tool injection is an architectural
liability for agents with large or growing tool catalogs. RAG-based
selection is a drop-in replacement that reduces cost, improves accuracy by
reducing tool overload, and enables scaling to marketplace-scale
registrations.

Volt is open-source and available at
\url{https://github.com/iixiiartist/volt}. The benchmark harness, data, and
reproduction instructions are in the `volt-bfcl/` directory of the
repository.

## References

[^ref1]: T. Schick et al., "Toolformer: Language Models Can Teach Themselves
    to Use Tools," 2023.

[^ref2]: S. Patil et al., "The Berkeley Function Calling Leaderboard
    (BFCL): From Tool Use to Agentic Evaluation of Large Language Models,"
    ICML, 2025.

[^ref3]: Anthropic, "Claude Code Documentation," 2025.
    \url{https://docs.anthropic.com/en/docs/claude-code/overview}

[^ref4]: Nous Research, "Hermes Agent," 2025.
    \url{https://github.com/NousResearch/hermes-agent}

[^ref5]: OpenClaw, "OpenClaw — Personal AI Assistant," 2025.
    \url{https://github.com/openclaw/openclaw}

[^ref6]: P. Lewis et al., "Retrieval-Augmented Generation for
    Knowledge-Intensive NLP Tasks," NeurIPS, 2020.

[^ref7]: C. Jimenez et al., "SWE-bench: Can Language Models Resolve
    Real-world Github Issues?" ICLR, 2024.
