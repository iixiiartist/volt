# Groq Integration Manual

## 1. Model Profiles

### Chat / Text Generation

| Model ID | Context | Max Output | Speed | Input $/1M | Output $/1M | Features |
|---|---|---|---|---|---|---|
| `openai/gpt-oss-120b` | 131,072 | 65,536 | ~500 tps | $0.15 | $0.60 | tools, bsearch, code, json_schema, reasoning, vision |
| `openai/gpt-oss-20b` | 131,072 | 65,536 | ~1000 tps | $0.075 | $0.30 | tools, bsearch, code, json_schema, reasoning |
| `openai/gpt-oss-safeguard-20b` | 131,072 | 65,536 | ~1000 tps | $0.075 | $0.30 | safety moderation, Harmony format |
| `llama-3.3-70b-versatile` | 131,072 | 32,768 | ~280 tps | $0.59 | $0.79 | tools, json_object |
| `llama-3.1-8b-instant` | 131,072 | 131,072 | ~560 tps | $0.05 | $0.08 | tools, json_object |
| `meta-llama/llama-4-scout-17b-16e-instruct` | 131,072 | 8,192 | ~750 tps | $0.11 | $0.34 | tools, json_schema, vision, MCP |
| `qwen/qwen3-32b` | 131,072 | 40,960 | ~400 tps | $0.29 | $0.59 | tools, json_object, reasoning |
| `groq/compound` | 131,072 | 8,192 | ~450 tps | mixed | mixed | 10 tools/req, web, code, visit, browser, wolfram |
| `groq/compound-mini` | 131,072 | 8,192 | ~450 tps | mixed | mixed | 1 tool/req, 3x lower latency |

### Audio Models

| Model ID | Type | Speed | Price | Languages |
|---|---|---|---|---|
| `whisper-large-v3` | stt | 189x | $0.111/hr | 99+ |
| `whisper-large-v3-turbo` | stt | 216x | $0.04/hr | 99+ |
| `canopylabs/orpheus-v1-english` | tts | - | $22/1M chars | EN |
| `canopylabs/orpheus-arabic-saudi` | tts | - | $40/1M chars | AR-SA |

### Content Moderation

| Model ID | Params | AUC | Use |
|---|---|---|---|
| `meta-llama/llama-prompt-guard-2-22m` | 22M | 99.5% | Lightweight injection detection |
| `meta-llama/llama-prompt-guard-2-86m` | 86M | 99.8% | Multilingual injection detection |
| `openai/gpt-oss-safeguard-20b` | 20B | - | Policy-following safety (BYO policy) |

## 2. Compound Systems ↔ Volt DAG Orchestrator

### Groq Compound System Model

```
groq/compound: Router Model (Llama 4 Scout / Llama 3.3 70B)
  → delegates to: GPT-OSS 120B (reasoning)
  → built-in tools: web_search, code_interpreter, visit_website, browser_automation, wolfram_alpha
  → up to 10 tool calls per request
```

### Volt DAG Orchestrator Mapping

Volt's existing `DagWorkflow` (Kahn-sorted topological levels with parallel execution) maps directly:

| Groq Compound Concept | Volt Implementation |
|---|---|
| Component models | `DagNode.agent.model` |
| Router/planner agent | First-level node with task template |
| Tool execution | Tool execution agents in subsequent levels |
| `{input}` / `{prev}` substitution | Template variable injection in `DagScheduler` |
| `usage_breakdown` per model | Track per-node `StepResult` metrics |
| `executed_tools` array | `StepResult.output` with tool call tracking |

### Implementation: Compound System as DAG

For native Groq Compound System support, add a new `CompoundWorkflow` that:
1. Accepts `groq/compound` model parameter
2. Sets `compound_custom.tools.enabled_tools` in request body
3. Parses `usage_breakdown` and `executed_tools` from response
4. Maps each `executed_tools[i]` to a DAG node with component model

## 3. API Implementation Plan

### Phase 3a: Extend LLMRequest with Groq-specific fields

Add to `src/models.rs` `LLMRequest`:

```rust
pub struct LLMRequest {
    // existing fields...
    
    // NEW: Groq-specific
    pub reasoning_effort: Option<String>,         // "none"|"default"|"low"|"medium"|"high"
    pub reasoning_format: Option<String>,          // "parsed"|"raw"|"hidden"
    pub include_reasoning: Option<bool>,           // GPT-OSS only
    pub response_format: Option<ResponseFormat>,   // json_schema strict mode
    pub service_tier: Option<String>,              // "auto"|"on_demand"|"flex"|"performance"
    pub search_settings: Option<Value>,            // built-in web search config
    pub compound_custom: Option<Value>,            // compound tool config
}
```

### Phase 3b: Extend LLMResponse

Add to `src/models.rs` `LLMResponse`:

```rust
pub struct LLMResponse {
    // existing fields...
    
    // NEW: Groq-specific
    pub usage_breakdown: Option<Vec<ModelUsage>>,  // compound system per-model usage
    pub executed_tools: Option<Vec<ExecutedTool>>, // compound system tool results
    pub prompt_tokens_details: Option<PromptTokensDetails>, // caching info
    pub system_fingerprint: Option<String>,
    pub x_groq: Option<Value>,                     // Groq-specific metadata
    pub queue_time: Option<f64>,
    pub total_time: Option<f64>,
}
```

### Phase 3c: Update build_request_body in openai.rs

In `src/llm/openai.rs`, the `build_request_body` function needs to:

1. Pass `reasoning_effort` if set
2. Pass `reasoning_format` if set
3. Pass `include_reasoning` if set
4. Pass `response_format` as `{"type": "json_schema", "json_schema": {...}}` 
5. Pass `service_tier` if set
6. Pass `search_settings` if set
7. Pass `compound_custom` if set
8. Handle vision: support `content` array with `image_url` objects
9. Handle streaming `reasoning` field extraction

### Phase 3d: Update parse_openai_response

Expand the response parser to extract:
- `usage_breakdown` array
- `executed_tools` array (from `choices[0].message`)
- `usage.prompt_tokens_details.cached_tokens`
- `usage.queue_time`, `usage.total_time`
- `system_fingerprint`
- `x_groq` object

### Phase 3e: Audio APIs

Add new trait methods to `LLMProvider`:

```rust
#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn complete(&self, request: &LLMRequest) -> Result<LLMResponse>;
    async fn complete_stream(&self, request: &LLMRequest, on_token: TokenCallback) -> Result<LLMResponse>;
    
    // NEW:
    async fn transcribe(&self, audio: &AudioRequest) -> Result<AudioResponse>;
    async fn translate(&self, audio: &AudioRequest) -> Result<AudioResponse>;
    async fn synthesize(&self, tts: &TtsRequest) -> Result<Vec<u8>>;
}
```

New types in `models.rs`:

```rust
pub struct AudioRequest {
    pub file_data: Vec<u8>,
    pub file_name: String,
    pub model: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub response_format: Option<String>,
    pub temperature: Option<f32>,
    pub timestamp_granularities: Option<Vec<String>>,
}

pub struct AudioResponse {
    pub text: String,
    pub x_groq: Option<Value>,
    pub segments: Option<Vec<AudioSegment>>,
    pub task: Option<String>,
    pub language: Option<String>,
    pub duration: Option<f64>,
}

pub struct TtsRequest {
    pub model: String,
    pub input: String,
    pub voice: String,
    pub response_format: Option<String>,
    pub sample_rate: Option<u32>,
    pub speed: Option<f32>,
}

pub struct ExecutedTool {
    pub tool_type: String,
    pub arguments: Value,
    pub output: Value,
    pub search_results: Option<Vec<SearchResult>>,
}

pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: f64,
}

pub struct ModelUsage {
    pub model: String,
    pub usage: Usage,
}

pub struct PromptTokensDetails {
    pub cached_tokens: Option<u64>,
}

pub struct AudioSegment {
    pub id: Option<u64>,
    pub seek: Option<u64>,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub text: String,
    pub tokens: Option<Vec<u64>>,
    pub temperature: Option<f32>,
    pub avg_logprob: Option<f64>,
    pub compression_ratio: Option<f64>,
    pub no_speech_prob: Option<f64>,
}

pub enum ResponseFormat {
    JsonObject,
    JsonSchema { name: String, strict: bool, schema: Value },
    Text,
}
```

### Phase 3f: MCP Remote Tool Integration

Add support for Groq's Remote MCP connector format in Volt's tool registry and MCP client:

```rust
// MCP connector registration - maps Groq's connector format to Volt tools
pub struct GroqMcpConnector {
    pub server_label: String,
    pub server_url: String,
    pub headers: Option<HashMap<String, String>>,
    pub connector_id: Option<String>,  // for Google Workspace connectors
    pub authorization: Option<String>,  // OAuth token for connectors
    pub require_approval: String,       // "never" | "always"
    pub allowed_tools: Option<Vec<String>>,
}
```

### Phase 3g: Batch API Support

```rust
// New endpoints for Groq Batch API
pub async fn create_batch(&self, input_file_id: &str, endpoint: &str, completion_window: &str) -> Result<Batch>;
pub async fn retrieve_batch(&self, batch_id: &str) -> Result<Batch>;
pub async fn list_batches(&self, after: Option<&str>, limit: Option<u32>) -> Result<Vec<Batch>>;
pub async fn cancel_batch(&self, batch_id: &str) -> Result<Batch>;
pub async fn upload_file(&self, data: Vec<u8>, filename: &str, purpose: &str) -> Result<FileObject>;
```

## 4. Implementation Order

1. **`src/models.rs`**: Add all new structs and enum variants listed above
2. **`src/llm/openai.rs`**: Extend `build_request_body` → extend `parse_openai_response` → extend stream parsing
3. **`src/llm/provider.rs`**: Add audio trait methods with default no-op implementations
4. **`src/tools/registry.rs`**: Add Groq built-in tool wrapper support
5. **`src/orchestrator.rs`**: Add Compound System DAG mapping

## 5. Rate Limit Integration

All models share these Developer Plan limits (extend to `Config` struct):

| Dimension | Limits Tracked |
|---|---|
| RPM | Per-model (30-60) |
| RPD | Per-model (100-14,400) |
| TPM | Per-model (1,200-70,000) |
| TPD | Per-model (3,600-500,000) |
| ASH | Audio: 7,200 |
| ASD | Audio: 28,800 |

Response headers to capture: `x-ratelimit-limit-requests`, `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-requests`, `x-ratelimit-remaining-tokens`, `x-ratelimit-reset-requests`, `x-ratelimit-reset-tokens`, `retry-after`.