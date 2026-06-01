# NVIDIA NIM API Model Catalog (June 2026)

Base URL: `https://integrate.api.nvidia.com/v1`  
Auth: Bearer token via `NVIDIA_API_KEY`  
API Compat: OpenAI SDK compatible (`/v1/chat/completions`)

---

## LLM Models — Full Catalog

### DeepSeek AI
| Model | Params | Context | Max Tokens | Tool Call | Vision | Reasoning | Free? |
|---|---|---|---|---|---|---|---|
| `deepseek-ai/deepseek-v4-pro` | MoE ~685B | 1M | 16384 | No | No | reasoning_effort (none/high/max) | No |
| `deepseek-ai/deepseek-v4-flash` | MoE (smaller) | 1M | 16384 | No | No | reasoning_effort (none/high/max) | — |

**Quirks:** Both use `chat_template_kwargs: {"thinking": bool}` via `extra_body`. V4 Pro default max_tokens=8192, Flash default=16384. No `tools` param in schema.

---

### Moonshot AI (Kimi)
| Model | Params | Context | Max Tokens | Tool Call | Vision | Reasoning | Free? |
|---|---|---|---|---|---|---|---|
| `moonshotai/kimi-k2-instruct` | MoE ~1T? | 128K | 16384 | **Yes** | No | No | **Yes** (deprecated) |
| `moonshotai/kimi-k2-thinking` | MoE | 128K | 16384 | — | No | **Yes** | — |

**Quirks:** K2-Instruct has full OpenAI-compatible `tools` param. Temperature range 0-1 (default 0.6). Supports frequency_penalty, presence_penalty. **Marked deprecated** on product page. K2.5 and K2.6 listed under Multimodal (vision-capable).

---

### Qwen (Alibaba)
| Model | Params | Context | Max Tokens | Tool Call | Vision | Reasoning | Free? |
|---|---|---|---|---|---|---|---|
| `qwen/qwen3.5-122b-a10b` | 122B (10B active) | 128K? | 32768 | **Yes** | No | enable_thinking | — |
| `qwen/qwen3-coder-480b-a35b-instruct` | 480B (35B active) | 128K? | — | — | No | — | — |
| `qwen/qwen3-next-80b-a3b-instruct` | 80B (3B active) | 128K | — | — | No | No | — |
| `qwen/qwen3-next-80b-a3b-thinking` | 80B (3B active) | 128K | — | — | No | **Yes** | — |
| `qwen/qwq-32b` | 32B dense | 128K | — | — | No | **Yes** | — |
| `qwen/qwen2.5-coder-32b-instruct` | 32B dense | 128K | — | — | No | No | — |
| `qwen/qwen3.5-397b-a17b` | 397B (17B active) | 128K | — | **Yes** | **Yes** | enable_thinking | — |

**Quirks:** Qwen3.5-122b uses **202 async polling** (not synchronous). `chat_template_kwargs` for thinking toggle. NVCF Asset API for large image uploads. `tools` param supported on 122B model.

---

### Google
| Model | Params | Context | Max Tokens | Tool Call | Vision | Reasoning | Free? |
|---|---|---|---|---|---|---|---|
| `google/gemma-4-31b-it` | 31B dense | 32K | 32768 | No | **Yes** | enable_thinking | No |
| `google/gemma-3-27b-it` | 27B dense | 128K | — | — | **Yes** | — | — |
| `google/gemma-3n-e2b-it` | 2B? | 128K | — | — | **Yes** | — | — |
| `google/gemma-3n-e4b-it` | 4B? | 128K | — | — | **Yes** | — | — |
| `google/gemma-2-2b-it` | 2B | 8K | — | No | No | No | — |
| `google/codegemma-7b` | 7B | 8K | — | No | No | No | — |
| `google/gemma-7b` | 7B | 8K | — | No | No | No | — |

**Quirks:** Gemma-4-31b-it supports thinking via `chat_template_kwargs: {"enable_thinking": true/false}`. Supports image input via `<img>` tags, base64, or NVCF assets. No `tools` param in schema. Free endpoint: Not available.

---

### Meta (Llama)
| Model | Params | Context | Max Tokens | Tool Call | Vision | Reasoning | Free? |
|---|---|---|---|---|---|---|---|
| `meta/llama-3.3-70b-instruct` | 70B | 128K | 4096 | **Yes** | No | No | — |
| `meta/llama-3.1-70b-instruct` | 70B | 128K | 4096 | **Yes** | No | No | — |
| `meta/llama-3.1-8b-instruct` | 8B | 128K | 4096 | **Yes** | No | No | — |
| `meta/llama-3.2-1b-instruct` | 1B | 128K | — | — | No | No | — |
| `meta/llama-3.2-3b-instruct` | 3B | 128K | — | — | No | No | — |
| `meta/llama2-70b` | 70B | 4K | — | No | No | No | — |

**Vision variants:**
| Model | Params | Context | Tool Call | Vision |
|---|---|---|---|---|
| `meta/llama-3.2-11b-vision-instruct` | 11B | 128K | — | **Yes** |
| `meta/llama-3.2-90b-vision-instruct` | 90B | 128K | — | **Yes** |
| `meta/llama-4-maverick-17b-128e-instruct` | 17B MoE | 1M? | — | **Yes** |
| `meta/llama-guard-4-12b` | 12B | — | — | **Yes** |

**Quirks:** Llama 3.1/3.3 have `tools` + `tool_choice` params, frequency_penalty, presence_penalty. Temperature default 0.2. Max_tokens capped at 4096 (small!). Llama-4-Maverick is multimodal (vision).

---

### Mistral AI
| Model | Params | Context | Max Tokens | Tool Call | Vision | Free? |
|---|---|---|---|---|---|---|
| `mistralai/mistral-large-3-675b-instruct-2512` | 675B MoE | 128K? | 8192 | No | **Yes** | — |
| `mistralai/mistral-medium-3-instruct` | — | 128K? | — | — | **Yes** | — |
| `mistralai/mistral-medium-3.5-128b` | 128B | 128K? | — | — | **Yes** | — |
| `mistralai/mistral-small-4-119b-2603` | 119B | 128K? | — | — | **Yes** | — |
| `mistralai/ministral-14b-instruct-2512` | 14B | 128K? | — | — | **Yes** | — |
| `mistralai/magistral-small-2506` | — | — | — | — | No | — |
| `mistralai/mistral-7b-instruct-v0.3` | 7B | 32K | — | — | No | — |
| `mistralai/mistral-nemotron` | 12B? | — | — | — | No | — |
| `mistralai/mixtral-8x7b-instruct` | 46B MoE | 32K | — | — | No | — |
| `mistralai/mixtral-8x22b-instruct` | 141B MoE | 64K | — | — | No | — |

**Quirks:** Large Mistral models (large-3, medium-3, small-4) are under Visual Models section — they support image input. No `tools` param visible in schema. Uses `frequency_penalty`, `presence_penalty`, `stop` params.

---

### NVIDIA
| Model | Params | Context | Max Tokens | Tool Call | Vision | Reasoning | Free? |
|---|---|---|---|---|---|---|---|
| `nvidia/nemotron-3-super-120b-a12b` | 120B (12B active) | **1M** | 32768 | **Yes** (listed) | No | reasoning_effort (none/low/high) | No |
| `nvidia/nemotron-3-nano-30b-a3b` | 30B (3B active) | — | — | — | No | — | — |
| `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning` | 30B (3B active) | — | — | — | **Yes** | **Yes** | — |
| `nvidia/nvidia-nemotron-nano-9b-v2` | 9B | — | — | — | No | — | — |
| `nvidia/llama-3.1-nemotron-nano-8b-v1` | 8B | 128K | — | — | No | — | — |
| `nvidia/llama-3.1-nemotron-ultra-253b-v1` | 253B | — | — | — | No | — | — |
| `nvidia/llama-3.3-nemotron-super-49b-v1` | 49B | 128K | — | — | No | — | — |
| `nvidia/llama-3.3-nemotron-super-49b-v1.5` | 49B | 128K | — | — | No | — | — |
| `nvidia/nvidia-nemotron-nano-9b-v2` | 9B | — | — | — | No | — | — |
| `nvidia/nemotron-mini-4b-instruct` | 4B | — | — | — | No | — | — |

**Safety models (not general LLMs):**
- `nvidia/llama-3.1-nemoguard-8b-content-safety`
- `nvidia/llama-3.1-nemoguard-8b-topic-control`
- `nvidia/llama-3.1-nemotron-safety-guard-8b-v3`
- `nvidia/nemoguard-jailbreak-detect`
- `nvidia/nemotron-content-safety-reasoning-4b`

**Vision variants:**
- `nvidia/llama-3.1-nemotron-nano-vl-8b-v1` — 8B vision-language
- `nvidia/nemotron-nano-12b-v2-vl` — 12B vision-language

**Quirks:** Nemotron-3-Super has `reasoning_effort` (none/low/high) + `reasoning_budget` (max 32768). Tool calling capability mentioned in marketing description. Stream defaults to `true` for this model. Free endpoint not available — Partner Endpoint only.

---

### OpenAI (GPT-OSS)
| Model | Params | Context | Max Tokens | Tool Call | Vision | Free? |
|---|---|---|---|---|---|---|
| `openai/gpt-oss-20b` | 20B | — | — | — | No | — |
| `openai/gpt-oss-120b` | 120B | — | — | — | No | — |

**Quirks:** These are Groq-hosted open-source GPT models, NOT hosted by OpenAI. The naming is misleading.

---

### Microsoft
| Model | Params | Context | Max Tokens | Tool Call | Vision | Reasoning |
|---|---|---|---|---|---|---|
| `microsoft/phi-4-mini-instruct` | 3.8B | 128K | — | — | No | No |
| `microsoft/phi-4-mini-flash-reasoning` | 3.8B | 128K | — | — | No | **Yes** |
| `microsoft/phi-4-multimodal-instruct` | 5.6B | 128K | — | — | **Yes** | — |

---

### Z.AI (GLM)
| Model | Params | Context | Max Tokens | Tool Call | Vision | Free? |
|---|---|---|---|---|---|---|
| `z-ai/glm5.1` | — | 128K | — | **Yes** | No | **Yes** (Free Endpoint) |
| `z-ai/glm4.7` | — | 128K | — | **Yes** | No | — |

**Quirks:** GLM-5.1 has a free endpoint. Advertised for agentic workflows, coding, long-horizon reasoning.

---

### MiniMax
| Model | Params | Context | Tool Call |
|---|---|---|---|
| `minimaxai/minimax-m2.5` | — | — | — |
| `minimaxai/minimax-m2.7` | — | 1M? | — |

---

### Others
| Model | Params | Notes |
|---|---|---|
| `abacusai/dracarys-llama-3.1-70b-instruct` | 70B | Fine-tuned Llama 3.1 70B |
| `bytedance/seed-oss-36b-instruct` | 36B | ByteDance's open model |
| `sarvamai/sarvam-m` | — | Indian languages focused |
| `stepfun-ai/step-3-5-flash` | — | StepFun flash model |
| `stepfun-ai/step-3-7-flash` | — | Vision-capable (under Visual Models) |
| `stockmark/stockmark-2-100b-instruct` | 100B | Japanese-optimized |
| `upstage/solar-10.7b-instruct` | 10.7B | Korean-optimized |

---

## Key Findings for Blueprint Building

### Models with Tool/Function Calling Support (confirmed via API schema)
1. `meta/llama-3.3-70b-instruct` — `tools` + `tool_choice`
2. `meta/llama-3.1-70b-instruct` — `tools` + `tool_choice`
3. `meta/llama-3.1-8b-instruct` — `tools` + `tool_choice`
4. `moonshotai/kimi-k2-instruct` — `tools` param
5. `qwen/qwen3.5-122b-a10b` — `tools` param
6. `nvidia/nemotron-3-super-120b-a12b` — tool calling (marketing claim)
7. `z-ai/glm5.1` — agentic claims

### Models with Vision/Multimodal Support
1. `google/gemma-4-31b-it` — img tags/base64
2. `google/gemma-3-27b-it`
3. `meta/llama-3.2-11b-vision-instruct`
4. `meta/llama-3.2-90b-vision-instruct`
5. `meta/llama-4-maverick-17b-128e-instruct`
6. `microsoft/phi-4-multimodal-instruct`
7. `mistralai/mistral-large-3-675b-instruct-2512`
8. `mistralai/mistral-small-4-119b-2603`
9. `mistralai/mistral-medium-3-instruct`
10. `mistralai/mistral-medium-3.5-128b`
11. `mistralai/ministral-14b-instruct-2512`
12. `moonshotai/kimi-k2.5`
13. `moonshotai/kimi-k2.6`
14. `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning`
15. `nvidia/nemotron-nano-12b-v2-vl`
16. `nvidia/llama-3.1-nemotron-nano-vl-8b-v1`
17. `qwen/qwen3.5-397b-a17b`
18. `stepfun-ai/step-3-7-flash`

### Models with 1M+ Context Window
1. `deepseek-ai/deepseek-v4-pro` — 1M
2. `deepseek-ai/deepseek-v4-flash` — 1M
3. `nvidia/nemotron-3-super-120b-a12b` — 1M

### Models with Free Endpoints
1. `moonshotai/kimi-k2-instruct` (deprecated)
2. `z-ai/glm5.1`
3. Some smaller models may offer free tier via playground

### Models using Async (202) Polling Pattern
1. `qwen/qwen3.5-122b-a10b` — returns 202, poll via GET
2. `google/gemma-4-31b-it` — returns 202
3. Many visual/multimodal models use NVCF async pattern

### Important Implementation Notes
- **DeepSeek V4 Pro/Flash**: Pass `reasoning_effort` as top-level param, NOT in `chat_template_kwargs`. The API docs say "Snippets translate this field into the model's `chat_template_kwargs`."
- **Nemotron-3-Super**: Has both `reasoning_effort` AND `reasoning_budget`. Default stream=true (unusual).
- **Gemma-4-31b-it**: Listed under Visual Models (has vision support) despite being text-first.
- **Llama 3.1/3.3**: Max tokens capped at 4096 on NVIDIA NIM (much lower than native 128K).
- **Mistral Large/Medium/Small**: Listed under Visual Models, not LLM section. They support images.
- **Moonshot Kimi**: K2-Instruct is marked deprecated. K2.5 and K2.6 are the current versions (vision-capable).
