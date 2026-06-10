# vLLM Deployment Guide

A runbook for the ops team responsible for running vLLM as the inference
backend for Volt. Follow these steps in order. Total expected time:
2-4 hours for a working single-node deployment, longer for a
production-grade multi-node cluster.

## TL;DR

```bash
# 1. Install vLLM
pip install vllm

# 2. Start the server (single GPU, single model)
vllm serve meta-llama/Llama-3.3-70B-Instruct \
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json \
    --chat-template examples/tool_chat_template_llama3.1_json.jinja

# 3. Tell Volt where vLLM is
export VLLM_HOST=http://vllm.internal:8000

# 4. Verify
curl http://vllm.internal:8000/v1/models
```

## 1. Hardware Sizing

vLLM's VRAM requirements scale with model size and quantization. A
single H100 (80GB) or A100 (80GB) is the standard target for serious
deployments. Two or more GPUs are required for models that don't fit
on one card.

| Model | Quantization | Min VRAM | Recommended GPU |
|---|---|---|---|
| Llama 3.1 8B Instruct | FP16 | 16 GB | 1x A10 / 1x L4 |
| Llama 3.1 8B Instruct | INT8 / AWQ | 8 GB | 1x L4 / 1x RTX 4090 |
| Llama 3.3 70B Instruct | FP16 | 140 GB | 2x A100-80GB / 1x H200 |
| Llama 3.3 70B Instruct | AWQ-INT4 | 40 GB | 1x A100-80GB / 1x H100 |
| Qwen 3 32B | FP16 | 64 GB | 1x A100-80GB / 1x H100 |
| Qwen 2.5 Coder 32B | FP16 | 64 GB | 1x A100-80GB / 1x H100 |
| BGE-large-en-v1.5 (embedder) | FP32 | 4 GB | bundled on any GPU |
| Qwen 2.5 VL 7B (vision) | FP16 | 16 GB | 1x A10 / 1x L4 |
| Whisper Large V3 (STT) | FP16 | 12 GB | 1x L4 |

For a 70B-class supervisor + 8B classifier + 32B coder on a single
node, you need **at least 4x A100-80GB** (or 2x H100-80GB with
tensor parallelism) for full FP16. With INT4 quantization, 1x
A100-80GB fits the supervisor alone, and the smaller models can run
alongside.

## 2. Model Selection

The default `volt.models.toml` (shipped with Volt) maps roles to:

- **supervisor**: `meta-llama/Llama-3.3-70B-Instruct` — the smart
  model. ~70B parameters, ~40GB VRAM at AWQ-INT4, the workhorse
  for any non-trivial reasoning.
- **classifier**: `meta-llama/Llama-3.1-8B-Instant` — the fast
  model. 8B parameters, ~16GB VRAM at FP16 or ~8GB at INT8. Used
  for routing, classification, and tool-call argument generation
  where latency matters more than depth.
- **coder**: `Qwen/Qwen2.5-Coder-32B-Instruct` — the code
  specialist. 32B parameters, ~64GB VRAM at FP16. Best
  open-source code model at this size as of early 2026.
- **summarizer**: `meta-llama/Llama-3.1-8B-Instant` — mid-sized
  generalist for intermediate condensation.

### License notes

Before deploying any model, confirm the license terms are compatible
with your organization's policies. Notable points:

- **Llama 3.x**: Meta's license. Free for most uses but requires
  accepting Meta's Acceptable Use Policy. Not allowed if your
  product has >700M monthly active users (special license required).
- **Qwen 3 / Qwen 2.5 Coder**: Apache 2.0 (Qwen 3) and Apache 2.0
  (Qwen 2.5 Coder). Permissive, commercial use OK.
- **DeepSeek V3 / R1**: MIT license for the weights. Country-of-origin
  export controls may apply depending on your jurisdiction.
- **BGE / BGE-reranker**: MIT license. Permissive.
- **Whisper**: MIT license. Permissive.

### Weights procurement

Download weights from Hugging Face. Verify the SHA-256 hash of the
downloaded files against the publisher's manifest before loading.
vLLM does not (yet) verify weights automatically. For a higher
assurance posture, mirror the weights to an internal registry
(S3 + manifest, or a private HF instance) and serve from there.

## 3. vLLM Install

```bash
# In a fresh Python 3.10+ venv
pip install vllm

# Verify the install
vllm --version
```

For production, pin to a specific vLLM version (e.g. `vllm==0.10.0`)
in your requirements file. vLLM ships breaking changes between
minor versions; pinning avoids surprise upgrades.

## 4. Starting the Server

### Single-model, single-GPU

```bash
vllm serve meta-llama/Llama-3.3-70B-Instruct \
    --host 0.0.0.0 \
    --port 8000 \
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json \
    --chat-template examples/tool_chat_template_llama3.1_json.jinja \
    --max-model-len 8192 \
    --gpu-memory-utilization 0.92
```

### Multi-model, single-server (recommended for multi-model workflows)

vLLM V1 supports serving multiple models in one process via
`--served-model-name`. The same HTTP endpoint then accepts any of
the served names. The role registry in `volt.models.toml` resolves
each role to a specific served name.

```bash
vllm serve \
    --served-model-name meta-llama/Llama-3.3-70B-Instruct meta-llama/Llama-3.1-8B-Instruct Qwen/Qwen2.5-Coder-32B-Instruct BAAI/bge-large-en-v1.5 \
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json \
    --task embed \
    ...
```

This is the deployment mode Volt's `volt.models.toml` is designed
for. One vLLM process, four served names, one endpoint.

### Multi-GPU (tensor parallelism for large models)

```bash
vllm serve meta-llama/Llama-3.3-70B-Instruct \
    --tensor-parallel-size 2 \
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json
```

### Production hardening flags

```bash
vllm serve ... \
    --api-key "$VLLM_API_KEY" \      # optional; Volt will send as Bearer if set
    --ssl-keyfile /etc/ssl/vllm.key \ # TLS for in-transit encryption
    --ssl-certfile /etc/ssl/vllm.crt \
    --max-num-seqs 256 \              # bound concurrent requests
    --max-model-len 8192              # bound context length (memory ceiling)
    --enforce-eager                   # disable CUDA graphs if stability > throughput
```

The `--api-key` flag is the simplest form of client authentication.
Volt's `VllmProvider` will send the same key as a Bearer token
(operator sets `VLLM_HOST=http://...` and provides the key in the
volt config or via the WebUI's provider settings).

## 5. Volt Configuration

Set these env vars on the Volt process (or in `.env`):

```bash
# Required: where vLLM is reachable from Volt
export VLLM_HOST=http://vllm.internal:8000
# Or use the more general override:
export LLM_BASE_URL=http://vllm.internal:8000/v1

# Optional: auth (only if vLLM was started with --api-key)
export VLLM_API_KEY=secret-from-vllm-server

# Optional: edit the role -> model map
# Default: ~/.volt/volt.models.toml
# Override:
export VOLT_MODELS_CONFIG=/etc/volt/volt.models.toml

# Optional: restrict which providers prod workflows can use
export VOLT_PROD_PROVIDER_ALLOWLIST=vllm,ollama_local

# Optional: enable cloud providers (Groq, OpenAI, etc.) for dev only
# DO NOT SET THIS IN PROD
# export VOLT_ENABLE_CLOUD_PROVIDERS=1
```

The `volt.models.toml` file (created on first volt run) maps role
names to model IDs. Edit it to match the model names served by your
vLLM instance:

```toml
[roles.supervisor]
model = "meta-llama/Llama-3.3-70B-Instruct"
temperature = 0.3
max_tokens = 4096

[roles.classifier]
model = "meta-llama/Llama-3.1-8B-Instant"
temperature = 0.0
max_tokens = 512

[roles.coder]
model = "Qwen/Qwen2.5-Coder-32B-Instruct"
temperature = 0.1
max_tokens = 4096
```

## 6. Verification

After the vLLM server is up and Volt is configured:

### 6.1 vLLM is reachable

```bash
curl -s http://vllm.internal:8000/v1/models | jq
```

Should return a list of served model names. If you see `[]`, vLLM
hasn't finished loading the model yet (large models can take 60s
on first boot).

### 6.2 Volt sees vLLM as active

```bash
volt doctor
```

The doctor output should list `vllm` as an active provider. If it
shows `inactive`, check `VLLM_HOST` is set and the URL is reachable
from the volt process.

### 6.3 Volt can route a chat to vLLM

```bash
volt agent-run --input "say hello in spanish" --model meta-llama/Llama-3.1-8B-Instant --print
```

Should return a Spanish greeting. The model name must match one of
the `--served-model-name` values from your vLLM start command.

### 6.4 Tool calling works

```bash
volt agent-run \
    --input "what's in /tmp" \
    --model meta-llama/Llama-3.3-70B-Instruct \
    --print
```

If the agent calls the `bash` tool and the tool's output is
correct, the vLLM tool-calling path is working. If the model
responds with text instead of a tool call, the
`--tool-call-parser` is wrong for the model — consult the vLLM
tool-calling docs for the correct parser name for your model.

### 6.5 Role resolution works

Create a workflow file with `role: "supervisor"` and a node. Save
it. Open the WebUI's Workflows page, load the file, click Run.
The audit log should show:

> node `<id>` resolved to `meta-llama/Llama-3.3-70B-Instruct` via role `supervisor`

If the audit shows `Literal` instead of `Role`, the role registry
didn't load the config — check `~/.volt/volt.models.toml` exists
and is valid TOML.

### 6.6 Prod allowlist refuses cloud models

Create a workflow with `environment: "prod"` and a node that
names `groq/llama-3.1-70b-versatile`. Run it. Should fail with:

> workflow environment=prod refused: node '...' uses model 'groq/llama-3.1-70b-versatile' which resolves to provider 'groq' (not in allowlist ...)

If the workflow runs instead, `VOLT_PROD_PROVIDER_ALLOWLIST` is set
to include `groq` (or unset and the env classification is
mis-routing). Verify the env var and the allowlist contents.

## 7. Failure Modes

| Symptom | Likely cause | Fix |
|---|---|---|
| vLLM is up but volt can't reach it | Network ACL, wrong port | `curl http://vllm.internal:8000/v1/models` from the volt host |
| Tool calls return text instead of executing | Wrong `--tool-call-parser` | See [vLLM tool-calling docs](https://docs.vllm.ai/en/latest/features/tool_calling/) for your model |
| Model never finishes loading | OOM, GPU mismatch | Check vLLM logs; reduce `--max-model-len` |
| Workflow fails with "no active provider" | `VLLM_HOST` unset or vLLM is down | `volt doctor` |
| Prod workflow succeeds when it should fail | Allowlist too permissive | `echo $VOLT_PROD_PROVIDER_ALLOWLIST` and verify |
| Slow first request after idle | Cold start / model re-load | Pre-warm with a dummy call after vLLM starts |
| vLLM OOM under load | `--gpu-memory-utilization` too high, or KV cache too small | Lower `--gpu-memory-utilization` to 0.85; add `--max-num-seqs 64` |
| Streaming chunks arrive in bursts | `enable-prefix-caching` interaction | Acceptable; not a bug |

## 8. Observability

vLLM exposes Prometheus metrics at `/metrics` on the same port as
the chat API. Key metrics to alert on:

- `vllm:gpu_cache_usage_perc` — KV cache fill. >0.9 means new
  requests will be queued or rejected.
- `vllm:num_requests_running` — active request count. Track
  against your concurrency budget.
- `vllm:request_latency_seconds` (histogram) — end-to-end
  latency. p99 should be under your SLA threshold.
- `vllm:prompt_tokens_total` / `vllm:generation_tokens_total` —
  throughput counters. Cross-reference with the audit log to
  reconcile per-workflow token usage.

Volt's existing OpenTelemetry pipeline (`tracing-opentelemetry`)
emits spans for each LLM call. Wire vLLM's metrics to the same
backend (e.g. Prometheus + Grafana, or Datadog) for a unified view.

## 9. Multi-Model Setup (Recommended)

The cleanest production setup is **one vLLM process, multiple
served models**. vLLM V1 supports this; the role registry in Volt
maps each role to a specific served name.

```bash
vllm serve \
    --served-model-name \
        meta-llama/Llama-3.3-70B-Instruct \
        meta-llama/Llama-3.1-8B-Instant \
        Qwen/Qwen2.5-Coder-32B-Instruct \
        BAAI/bge-large-en-v1.5 \
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json \
    --port 8000
```

Then the role registry (`volt.models.toml`) maps each role to one
of these served names. A single workflow can route to multiple
models, all served by the same vLLM process.

For the embedding model (BGE), vLLM exposes a different endpoint
(`/v1/embeddings`) — Volt's embedding pipeline already supports
this via the `EMBEDDING_ENDPOINT` env var if you want Volt to hit
vLLM for embeddings instead of the local ONNX embedder.

## 10. Integration Test (Pending)

The vLLM provider in Volt has been validated against the OpenAI
spec that vLLM commits to, but has **not yet been run against a
live vLLM server** as of this writing. The integration test is
the next step.

```rust
// In tests/integration/vllm.rs (future, gated on env var)
#[tokio::test]
#[ignore] // run with: VLLM_INTEGRATION_URL=http://... cargo test -- --ignored
async fn vllm_provider_completes_a_chat() {
    let url = std::env::var("VLLM_INTEGRATION_URL").unwrap();
    let provider = VllmProvider::new(String::new(), url);
    let request = LLMRequest { /* ... */ };
    let response = provider.complete(&request).await.unwrap();
    assert!(!response.content.as_str().is_empty());
}
```

This test lands when a vLLM deployment is available. Until then,
treat vLLM-tagged workflows as `environment: dev|staging` only.

## 11. Common Configuration Recipes

### Dev laptop (single 4090, 24GB VRAM)

```bash
# Run one model at a time
vllm serve meta-llama/Llama-3.1-8B-Instruct \
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json \
    --max-model-len 4096 \
    --gpu-memory-utilization 0.85
export VLLM_HOST=http://localhost:8000
```

The 8B model fits comfortably. Use `Qwen/Qwen2.5-Coder-3B-Instruct`
or similar small models for the coder role in `volt.models.toml`.

### Single-node production (4x A100-80GB)

```bash
vllm serve \
    --served-model-name \
        meta-llama/Llama-3.3-70B-Instruct \
        meta-llama/Llama-3.1-8B-Instruct \
        Qwen/Qwen2.5-Coder-32B-Instruct \
        BAAI/bge-large-en-v1.5 \
    --tensor-parallel-size 2 \  # 70B needs 2 GPUs
    --enable-auto-tool-choice \
    --tool-call-parser llama3_json \
    --api-key "$VLLM_API_KEY" \
    --max-num-seqs 128 \
    --max-model-len 8192
```

Three models fit on 4 GPUs: 70B on GPUs 0-1 (tensor parallel), 32B
on GPU 2, 8B + embedding on GPU 3. Tune based on actual memory
usage.

### Multi-node production (8x H100 per node, 2+ nodes)

Out of scope for this guide. Use vLLM's
[disaggregated serving](https://docs.vllm.ai/en/latest/serving/disagg%5Fdeployment/)
or Kubernetes with vLLM-operator. The Volt side doesn't change —
it talks to the load-balanced HTTP endpoint.

## 12. What To Tell The Compliance Team

When audit asks "where does the prompt data go," the answer for a
vLLM-backed Volt deployment is:

> Prompts and completions stay on the host where vLLM runs. They
> are processed by the open-source model weights in GPU memory. No
> third-party API receives the data. The Volt process logs request
> IDs and token counts (not the prompt text) to the audit log. The
> vLLM server, if configured with `--api-key`, requires Bearer-token
> auth on every request. The `volt.models.toml` file is the only
> place where model selection is configured; it is checked into
> version control and reviewed via the same PR process as
> application code.
