# Volt Blueprint System

## What is a Blueprint?

A **blueprint** is a TOML profile that constrains agent behavior for a specific model. It sits between raw model configuration and runtime agent logic, telling Volt:

- Which model to use and which provider hosts it  
- How to format tool calls (the `format_dialect`)  
- Which model-specific quirks to compensate for  
- How many tools the model can handle per turn  
- Which core tools are available  
- Whether to override the system prompt

Blueprints are loaded at runtime from the `blueprints/` directory (or `~/.volt/blueprints/`). The orchestrator selects a blueprint based on the active model, and the agent loop applies its constraints automatically.

---

## Naming Convention

Blueprint files use **snake_case** and follow the pattern:

```
{provider}_{model_descriptor}.toml
```

Examples:

- `qwen3_32b.toml` — Groq-hosted Qwen 3 32B  
- `nim_deepseek_v4_pro.toml` — NVIDIA NIM DeepSeek V4 Pro  
- `ollama_qwen3.toml` — Ollama Cloud Qwen 3  
- `llama3_8b_local.toml` — Local Ollama Llama 3.1 8B  

The `id` field inside the TOML must match the filename stem exactly.

---

## Schema Reference

### `AgentBlueprint` (root)

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | Yes | Unique blueprint slug (matches filename). |
| `name` | string | Yes | Human-readable model name. |
| `description` | string | Yes | Short summary of the model's purpose and constraints. |
| `model_card` | `ModelCard` | Yes | Provider, model name, dialect, and quirks. |
| `scaffolding` | `ScaffoldingConfig` | Yes | Tool limits and strict-mode flag. |
| `tools` | `ToolSelection` | Yes | Lists of core and builtin tools. |
| `prompts` | `PromptOverrides` | Yes | Optional system prompt override. |

### `ModelCard`

| Field | Type | Required | Description |
|---|---|---|---|
| `model_name` | string | Yes | Provider-side model ID (e.g. `qwen/qwen3-32b`). |
| `provider` | string | Yes | Provider slug: `groq`, `nvidia`, `ollama`, `anthropic`, `openai`, `litertlm`. |
| `format_dialect` | `FormatDialect` | Yes | Tool-call serialization strategy (see below). |
| `quirks` | `Vec<ModelQuirk>` | No | Compensations to apply in the agent loop and tool parser. |

### `ScaffoldingConfig`

| Field | Type | Required | Description |
|---|---|---|---|
| `max_tools_per_turn` | `usize` (optional) | No | Hard cap on tool calls emitted in a single turn. `0` disables tool calling. |
| `strict_mode` | `bool` | Yes | When `true`, the tool parser runs in strict validation mode. |

### `ToolSelection`

| Field | Type | Required | Description |
|---|---|---|---|
| `core_tools` | `Vec<String>` | Yes | Tool names always registered for this blueprint. |
| `builtin_tools` | `Vec<String>` | No | Extra builtin tools (e.g. `ollama_web_search`). |

### `PromptOverrides`

| Field | Type | Required | Description |
|---|---|---|---|
| `system_prompt_override` | `String` (optional) | No | Replaces the default system prompt for this model. |

---

## ModelQuirk

`ModelQuirk` values are declared in the blueprint TOML as a string array, e.g.:

```toml
quirks = ["SchemaLimitTen", "ChainOfThoughtLeak"]
```

Each quirk triggers a specific compensation in the agent loop or tool parser:

| Quirk | Effect |
|---|---|
| `StringifiedBooleans` | Post-processes `"true"` / `"false"` strings into JSON booleans before validation. |
| `ChainOfThoughtLeak` | Strips conversational text outside tool-call XML/JSON tags before parsing. |
| `MultiToolParalysis` | Limits the agent to one tool call per turn; disables parallel tool dispatch. |
| `StringifiedIntegers` | Unwraps quoted integer values (e.g. `"42"` → `42`) before JSON schema validation. |
| `SchemaLimitTen` | Restricts tool retrieval to 10 tools max (for small-context models). |
| `MissingFinalAnswer` | Injects a forced-final system message when the model skips `final_answer`. |
| `NoToolCalling` | Disables tool calling entirely; model runs in pure chat mode. |
| `ReasoningEffort` | Passes `reasoning_effort` parameter in the chat template (DeepSeek-style). |
| `AsyncPolling` | Handles 202 Accepted responses by polling `GET /{request_id}` every 2s up to 4 min. |
| `MultimodalInput` | Marks the model as accepting images, video, or audio in the user message. |
| `MaxOutput4096` | Caps `max_tokens` at 4096 regardless of the model's native limit. |
| `MaxContext4096` | Caps the context window at 4096 tokens (legacy models). |
| `Deprecated` | Hides the blueprint from auto-selection; still loadable manually. |
| `ThinkingEnabled` | Enables configurable thinking/reasoning mode in the chat template. |
| `NonThinking` | Forces fast-path (non-thinking) mode for models that default to thinking. |
| `CompoundSystem` | Treats the model as a compound-system orchestrator, not a raw LLM. |
| `UsageBreakdown` | Expects and parses per-model usage breakdown from compound responses. |
| `UpTo10Tools` | Signals that the model supports up to 10 parallel tool calls per turn. |
| `BuiltinSearch` | Model has built-in web search; Volt may skip registering external search tools. |
| `BuiltinCodeInterpreter` | Model has built-in code execution; Volt may skip `bash` in some workflows. |
| `MarkdownCodeBlocks` | Strips markdown triple-backtick wrappers around tool call JSON before parsing. |

---

## FormatDialect

`FormatDialect` controls how tool calls are serialized into the prompt and how the model's output is parsed:

| Dialect | Description |
|---|---|
| `StandardXml` | `<function>` / `</function>` XML tags around JSON tool calls. |
| `GemmaNative` | `<|system|>` / `<|user|>` delimiters — native to Gemma-4 (current default). |
| `LlamaChat` | `<|begin_of_text|>…system…` — Llama chat template with tool JSON inside. |
| `OpenAiJson` | Tools as JSON objects inside the message body — OpenAI-style function calling. |
| `ClaudeXml` | `<function_calls>` / `<invoke>` XML — Claude-style tool format. |
| `ChatMlTools` | ChatML-style with `<|im_start|>` / `…` delimiters (Gemma 3 / GPT-4 class models). |

Set the dialect in the blueprint like this:

```toml
[model_card]
format_dialect = "ChatMlTools"
```

---

## How to Add a New Blueprint

1. **Pick a filename** in snake_case: `{provider}_{model}.toml`.
2. **Create the file** in the `blueprints/` directory (or `~/.volt/blueprints/` for user overrides).
3. **Fill in the root fields** (`id`, `name`, `description`).
4. **Define the `model_card`**:
   - `model_name`: the exact provider-side model string.
   - `provider`: one of the supported provider slugs.
   - `format_dialect`: pick the dialect the model expects.
   - `quirks`: list every quirk that applies (can be empty).
5. **Set `scaffolding`**:
   - `max_tools_per_turn`: recommended limit for this model.
   - `strict_mode`: `true` for high-accuracy models, `false` for fast/lazy ones.
6. **List `tools`**:
   - `core_tools`: the tool names the model should have access to.
   - `builtin_tools`: optional extras (e.g. `ollama_web_search`).
7. **Optionally override the system prompt** in `prompts.system_prompt_override`.
8. **Test** by running Volt with `LLM_MODEL={model_name}` and verifying tool calls are parsed correctly.

### Minimal Example

```toml
id = "my_custom_model"
name = "My Custom Model"
description = "Local fine-tune with LlamaChat tool format"

[model_card]
model_name = "my-org/my-model-7b"
provider = "ollama"
format_dialect = "LlamaChat"
quirks = ["StringifiedBooleans", "ChainOfThoughtLeak"]

[scaffolding]
max_tools_per_turn = 3
strict_mode = false

[tools]
core_tools = ["bash", "read", "write"]

[prompts]
system_prompt_override = "You are a helpful local assistant. Use tools when needed."
```

---

## Blueprint Overview Table

Volt ships with **56 blueprints** covering Groq, NVIDIA NIM, Ollama, and local/edge providers.

| ID | Name | Provider | Model | Dialect | Quirks |
|---|---|---|---|---|---|
| `allam_2_7b` | Allam 2 7B | groq | `allam-2-7b` | ChatMlTools | `NoToolCalling`, `MaxContext4096` |
| `gemma4_e2b_voice` | Gemma 4 E2B Voice Assistant | litertlm | `gemma-4-e2b` | ChatMlTools | `SchemaLimitTen`, `MissingFinalAnswer` |
| `gpt_oss_120b` | GPT-OSS 120B Flagship | groq | `openai/gpt-oss-120b` | ChatMlTools | `SchemaLimitTen`, `MarkdownCodeBlocks` |
| `gpt_oss_120b_reasoning` | GPT-OSS 120B Reasoning | groq | `openai/gpt-oss-120b` | ChatMlTools | `ThinkingEnabled`, `ReasoningEffort`, `SchemaLimitTen` |
| `gpt_oss_20b` | GPT-OSS 20B Fast | groq | `openai/gpt-oss-20b` | ChatMlTools | `StringifiedBooleans`, `StringifiedIntegers` |
| `gpt_oss_safeguard_20b` | GPT-OSS Safeguard 20B | groq | `openai/gpt-oss-safeguard-20b` | GemmaNative | `StringifiedBooleans`, `NoToolCalling` |
| `groq_compound` | Groq Compound System | groq | `groq/compound` | ChatMlTools | `CompoundSystem`, `UsageBreakdown`, `UpTo10Tools`, `BuiltinSearch`, `BuiltinCodeInterpreter` |
| `groq_compound_mini` | Groq Compound System Mini | groq | `groq/compound-mini` | ChatMlTools | `CompoundSystem`, `UsageBreakdown`, `MultiToolParalysis` |
| `llama_31_8b` | Llama 3.1 8B Instant | groq | `llama-3.1-8b-instant` | LlamaChat | `StringifiedBooleans`, `StringifiedIntegers`, `ChainOfThoughtLeak` |
| `llama_33_70b` | Llama 3.3 70B Versatile | groq | `llama-3.3-70b-versatile` | LlamaChat | `StringifiedBooleans` |
| `llama_4_scout` | Llama 4 Scout 17B | groq | `meta-llama/llama-4-scout-17b-16e-instruct` | LlamaChat | `MarkdownCodeBlocks`, `MultimodalInput` |
| `llama_4_scout_mcp` | Llama 4 Scout MCP | groq | `meta-llama/llama-4-scout-17b-16e-instruct` | LlamaChat | *(none)* |
| `llama_4_scout_vision` | Llama 4 Scout Vision | groq | `meta-llama/llama-4-scout-17b-16e-instruct` | LlamaChat | `MultimodalInput` |
| `llama3_8b_local` | Llama 3 8B Local Assistant | ollama | `llama-3.1-8b-instant` | LlamaChat | `StringifiedBooleans`, `StringifiedIntegers`, `ChainOfThoughtLeak` |
| `nim_deepseek_v4_flash` | NVIDIA NIM DeepSeek V4 Flash | nvidia | `deepseek-ai/deepseek-v4-flash` | ChatMlTools | `NoToolCalling`, `ReasoningEffort` |
| `nim_deepseek_v4_pro` | NVIDIA NIM DeepSeek V4 Pro | nvidia | `deepseek-ai/deepseek-v4-pro` | ChatMlTools | `NoToolCalling`, `ReasoningEffort` |
| `nim_gemma3_27b` | NVIDIA NIM Gemma 3 27B IT | nvidia | `google/gemma-3-27b-it` | ChatMlTools | `MultimodalInput`, `NoToolCalling` |
| `nim_gemma4_31b` | NVIDIA NIM Gemma 4 31B | nvidia | `google/gemma-4-31b-it` | ChatMlTools | `MissingFinalAnswer`, `MultimodalInput` |
| `nim_glm5_1` | NVIDIA NIM GLM 5.1 | nvidia | `z-ai/glm5.1` | StandardXml | `StringifiedBooleans`, `StringifiedIntegers` |
| `nim_kimi_k2` | NVIDIA NIM Kimi K2 Instruct | nvidia | `moonshotai/kimi-k2-instruct` | ChatMlTools | `SchemaLimitTen`, `Deprecated` |
| `nim_llama31_70b` | NVIDIA NIM Llama 3.1 70B Instruct | nvidia | `meta/llama-3.1-70b-instruct` | LlamaChat | `StringifiedBooleans`, `MaxOutput4096` |
| `nim_llama31_8b` | NVIDIA NIM Llama 3.1 8B Instruct | nvidia | `meta/llama-3.1-8b-instruct` | LlamaChat | `StringifiedBooleans`, `StringifiedIntegers`, `MaxOutput4096` |
| `nim_llama33_70b` | NVIDIA NIM Llama 3.3 70B Instruct | nvidia | `meta/llama-3.3-70b-instruct` | LlamaChat | `StringifiedBooleans`, `MaxOutput4096` |
| `nim_minimax_m25` | NVIDIA NIM MiniMax M2.5 | nvidia | `minimaxai/minimax-m2.5` | ChatMlTools | `NoToolCalling` |
| `nim_minimax_m27` | NVIDIA NIM MiniMax M2.7 | nvidia | `minimaxai/minimax-m2.7` | ChatMlTools | *(none)* |
| `nim_mixtral_8x22b` | NVIDIA NIM Mixtral 8x22B Instruct | nvidia | `mistralai/mixtral-8x22b-instruct` | ChatMlTools | `NoToolCalling` |
| `nim_nemotron3_nano_omni` | NVIDIA NIM Nemotron-3 Nano Omni | nvidia | `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning` | LlamaChat | `MultimodalInput` |
| `nim_nemotron3_super_120b` | NVIDIA NIM Nemotron-3 Super 120B | nvidia | `nvidia/nemotron-3-super-120b-a12b` | LlamaChat | `ChainOfThoughtLeak`, `NoToolCalling` |
| `nim_phi4_flash_reasoning` | NVIDIA NIM Phi-4 Mini Flash Reasoning | nvidia | `microsoft/phi-4-mini-flash-reasoning` | ChatMlTools | `ChainOfThoughtLeak`, `NoToolCalling` |
| `nim_phi4_mini` | NVIDIA NIM Phi-4 Mini Instruct | nvidia | `microsoft/phi-4-mini-instruct` | ChatMlTools | `NoToolCalling` |
| `nim_qwen_coder_32b` | NVIDIA NIM Qwen 2.5 Coder 32B | nvidia | `qwen/qwen2.5-coder-32b-instruct` | ChatMlTools | `NoToolCalling` |
| `nim_qwen35_122b` | NVIDIA NIM Qwen 3.5 122B | nvidia | `qwen/qwen3.5-122b-a10b` | ChatMlTools | `AsyncPolling`, `MultimodalInput` |
| `nim_qwq_32b` | NVIDIA NIM QwQ 32B | nvidia | `qwen/qwq-32b` | ChatMlTools | `ChainOfThoughtLeak`, `NoToolCalling` |
| `nim_step_37_flash` | NVIDIA NIM Step 3.7 Flash | nvidia | `stepfun-ai/step-3.7-flash` | ChatMlTools | *(none)* |
| `ollama_devstral_small_2` | Ollama Devstral Small 2 24B | ollama | `devstral-small-2:24b-cloud` | ChatMlTools | *(none)* |
| `ollama_gemma4` | Ollama Gemma 4 | ollama | `gemma4:31b` | ChatMlTools | `MissingFinalAnswer`, `MultimodalInput` |
| `ollama_gpt_oss` | Ollama GPT-OSS | ollama | `gpt-oss:120b` | ChatMlTools | `SchemaLimitTen` |
| `ollama_kimi_k2_6` | Ollama Kimi K2.6 | ollama | `kimi-k2.6:cloud` | ChatMlTools | `SchemaLimitTen`, `MultimodalInput`, `ThinkingEnabled` |
| `ollama_minimax_m2_1` | Ollama MiniMax M2.1 | ollama | `minimax-m2.1:cloud` | ChatMlTools | `NonThinking` |
| `ollama_minimax_m2_5` | Ollama MiniMax M2.5 | ollama | `minimax-m2.5:cloud` | ChatMlTools | `ThinkingEnabled` |
| `ollama_minimax_m27` | Ollama MiniMax M2.7 | ollama | `minimax-m2.7` | ChatMlTools | *(none)* |
| `ollama_minimax_m3` | Ollama MiniMax M3 | ollama | `minimax-m3:cloud` | ChatMlTools | `MultimodalInput`, `ThinkingEnabled` |
| `ollama_qwen3` | Ollama Qwen 3 | ollama | `qwen3` | ChatMlTools | `ThinkingEnabled` |
| `ollama_qwen3_5_32b` | Ollama Qwen 3.5 32B | ollama | `qwen3.5:32b-cloud` | ChatMlTools | `MultimodalInput`, `ThinkingEnabled` |
| `ollama_qwen3_coder_next` | Ollama Qwen 3 Coder Next | ollama | `qwen3-coder-next:cloud` | ChatMlTools | `NonThinking` |
| `ollama_qwen3_next_80b` | Ollama Qwen 3 Next 80B | ollama | `qwen3-next:80b-cloud` | ChatMlTools | `SchemaLimitTen`, `ThinkingEnabled` |
| `ollama_qwen35` | Ollama Qwen 3.5 | ollama | `qwen3.5:122b` | ChatMlTools | *(none)* |
| `ollama_rnj_1` | Ollama RNJ 1 8B | ollama | `rnj-1:8b-cloud` | ChatMlTools | *(none)* |
| `orpheus_arabic` | Orpheus TTS Arabic | groq | `canopylabs/orpheus-arabic-saudi` | GemmaNative | `NoToolCalling` |
| `orpheus_english` | Orpheus TTS English | groq | `canopylabs/orpheus-v1-english` | GemmaNative | `NoToolCalling` |
| `prompt_guard_22m` | Llama Prompt Guard 22M | groq | `meta-llama/llama-prompt-guard-2-22m` | GemmaNative | `NoToolCalling` |
| `prompt_guard_86m` | Llama Prompt Guard 86M | groq | `meta-llama/llama-prompt-guard-2-86m` | GemmaNative | `NoToolCalling` |
| `qwen3_32b` | Qwen 3 32B | groq | `qwen/qwen3-32b` | ChatMlTools | `SchemaLimitTen`, `ChainOfThoughtLeak` |
| `qwen3_32b_reasoning` | Qwen 3 32B Reasoning | groq | `qwen/qwen3-32b` | ChatMlTools | `ChainOfThoughtLeak`, `ThinkingEnabled` |
| `whisper_large_v3` | Whisper Large V3 | groq | `whisper-large-v3` | GemmaNative | `NoToolCalling` |
| `whisper_turbo` | Whisper Large V3 Turbo | groq | `whisper-large-v3-turbo` | GemmaNative | `NoToolCalling` |
