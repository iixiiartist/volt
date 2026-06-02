use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ─── Agent types ──────────────────────────────────────────────

/// Configuration for an agent instance — model, iteration limits, toolsets, context kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub model: String,
    pub provider: String,
    pub system_prompt: Option<String>,
    pub max_iterations: u32,
    pub temperature: f32,
    pub toolsets: Vec<String>,
    pub hidden: bool,
    #[serde(default)]
    pub allow_all: bool,
    /// Which context kinds to retrieve during agent runs.
    /// Defaults to all 12 kinds. Set to a subset for ablation studies.
    #[serde(default = "default_context_kinds")]
    pub enabled_context_kinds: Vec<crate::context::ContextKind>,
    /// Tools always force-included regardless of semantic retrieval score.
    /// Defaults to core file/web tools. Set to empty for pure RAG only.
    #[serde(default = "default_essential_tools")]
    pub essential_tools: Vec<String>,
    /// Per-kind quota overrides for the unified context store.
    /// If empty, the hardcoded defaults in ContextKind::quota() are used.
    /// Keys not present fall back to defaults. Set to experiment with retrieval budgets.
    #[serde(default)]
    pub context_kind_quotas: std::collections::HashMap<crate::context::ContextKind, usize>,
    #[serde(default)]
    pub use_mtp: bool,
    #[serde(default)]
    pub use_cot: bool,
    #[serde(default)]
    pub allow_write: bool,
    pub framework: Option<String>,
    pub model_variant: Option<String>,
    pub quantization: Option<String>,
    /// Format dialect for prompt building (defaults to GemmaNative).
    #[serde(default)]
    pub format_dialect: crate::agent::blueprint::FormatDialect,
    /// Model quirks requiring compensation interceptors (StringifiedBooleans, ChainOfThoughtLeak).
    #[serde(default)]
    pub quirks: Vec<crate::agent::blueprint::ModelQuirk>,
    /// When true, filter ContextKind::Tool and ContextKind::Skill from RAG retrieval,
    /// only using the explicitly listed essential/core tools.
    #[serde(default)]
    pub strict_mode: bool,
    /// Maximum tool calls allowed per LLM turn (None = unlimited).
    #[serde(default)]
    pub max_tools_per_turn: Option<usize>,
    /// Path to the loaded blueprint file, if any.
    #[serde(default)]
    pub blueprint_path: Option<String>,
}

pub fn default_context_kinds() -> Vec<crate::context::ContextKind> {
    use crate::context::ContextKind;
    vec![
        ContextKind::Tool,
        ContextKind::Skill,
        ContextKind::Memory,
        ContextKind::Conversation,
        ContextKind::AgentRun,
        ContextKind::Artifact,
        ContextKind::SystemPrompt,
        ContextKind::FewShot,
        ContextKind::Policy,
        ContextKind::Permission,
        ContextKind::Security,
        ContextKind::MCPConfig,
    ]
}

/// Core tools always force-included for safety and basic operation.
pub fn default_essential_tools() -> Vec<String> {
    vec![
        "read".into(),
        "glob".into(),
        "grep".into(),
        "web_fetch".into(),
    ]
}

/// Runtime state for an agent during a session — iteration count, messages, token usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub id: Uuid,
    pub name: String,
    pub session_id: Uuid,
    pub iteration: u32,
    pub messages: Vec<Message>,
    pub context_injected: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub allow_session: bool,
    #[serde(default)]
    pub total_prompt_tokens: u64,
    #[serde(default)]
    pub total_completion_tokens: u64,
    /// High-water mark: index of the last message persisted to SQLite.
    /// Enables delta-based saves so we don't re-write the entire history
    /// on every iteration turn (fixes the O(N) write amplification bug).
    #[serde(default)]
    pub last_saved_message_idx: usize,
}

/// A single message in the agent's conversation history (user, assistant, system, or tool result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(default = "default_message_id")]
    pub id: Uuid,
    #[serde(default)]
    pub parent_message_id: Option<Uuid>,
    pub role: String,
    pub content: Arc<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_result: Option<String>,
    pub tool_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn default_message_id() -> Uuid {
    Uuid::nil()
}

impl Message {
    /// Create a new message with an auto-generated id and no parent.
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            parent_message_id: None,
            role: role.into(),
            content: Arc::new(content.into()),
            tool_calls: None,
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        }
    }

    /// Chain a parent message so the DAG topology is preserved.
    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent_message_id = Some(parent_id);
        self
    }

    /// Chain a parent message from an Option (convenience for builders).
    pub fn with_parent_option(mut self, parent_id: Option<Uuid>) -> Self {
        self.parent_message_id = parent_id;
        self
    }

    /// Extract the last message id from a slice to use as parent.
    pub fn last_id(messages: &[Self]) -> Option<Uuid> {
        messages.last().map(|m| m.id)
    }
}

/// Linearize a slice of messages into a topological order by their parent chain.
///
/// In the common non-branching case every message's parent is the previous
/// message, so the returned order is identical to the input order.
///
/// In a branching conversation (parallel agent outputs merged into a
/// supervisor), messages may share parents. This function resolves the DAG
/// by walking from each leaf up through its parent chain, producing a
/// breadth-first level ordering that guarantees every parent appears before
/// its children.
pub fn linearize_messages(messages: &[Message]) -> Vec<&Message> {
    if messages.is_empty() {
        return Vec::new();
    }

    // Build a lookup: id -> &Message
    // Build a lookup: id -> &Message (used in branching resolution)
    let _by_id: std::collections::HashMap<Uuid, &Message> =
        messages.iter().map(|m| (m.id, m)).collect();

    // Find root messages (no parent, or parent not in set) and leaf messages
    let ids: std::collections::HashSet<Uuid> = messages.iter().map(|m| m.id).collect();
    let roots: Vec<&Message> = messages
        .iter()
        .filter(|m| m.parent_message_id.is_none_or(|p| !ids.contains(&p)))
        .collect();

    // BFS level-ordering: start from roots
    let mut result: Vec<&Message> = Vec::with_capacity(messages.len());
    let mut visited: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    let mut queue: Vec<&Message> = roots;

    while let Some(msg) = queue.pop() {
        if visited.contains(&msg.id) {
            continue;
        }
        visited.insert(msg.id);
        result.push(msg);

        // Find direct children
        for child in messages.iter() {
            if child.parent_message_id == Some(msg.id) && !visited.contains(&child.id) {
                queue.push(child);
            }
        }
    }

    // Append any messages not reachable from roots (shouldn't happen, but be safe)
    for msg in messages {
        if !visited.contains(&msg.id) {
            result.push(msg);
        }
    }

    result
}

/// A tool call issued by the LLM — name, arguments, and a unique call ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Schema definition for a tool registered in the tool registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub category: String,
}

// ─── LLM types ────────────────────────────────────────────────

/// Structured output mode for Groq/OpenAI response_format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseFormat {
    JsonObject,
    JsonSchema {
        name: String,
        #[serde(default)]
        strict: bool,
        schema: Value,
    },
    Text,
}

impl ResponseFormat {
    pub fn to_request_value(&self) -> Value {
        match self {
            ResponseFormat::JsonObject => serde_json::json!({"type": "json_object"}),
            ResponseFormat::JsonSchema {
                name,
                strict,
                schema,
            } => serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": name,
                    "strict": strict,
                    "schema": schema,
                }
            }),
            ResponseFormat::Text => serde_json::json!({"type": "text"}),
        }
    }
}

/// Request payload sent to the LLM — model, messages, tools, temperature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LLMRequest {
    pub model: String,
    pub messages: Vec<LLMMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub stream: bool,
    // Groq-specific:
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub reasoning_format: Option<String>,
    #[serde(default)]
    pub include_reasoning: Option<bool>,
    #[serde(default)]
    pub response_format: Option<ResponseFormat>,
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub search_settings: Option<Value>,
    #[serde(default)]
    pub compound_custom: Option<Value>,
    /// When true, use native structured outputs (json_schema response_format)
    /// instead of traditional tool calling. Guarantees schema conformance.
    #[serde(default)]
    pub strict_mode: bool,
}

/// A single message in the LLM request format (role + content + optional tool call info).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMMessage {
    pub role: String,
    pub content: Arc<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

/// Response from the LLM — content, tool calls, token usage, finish reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: Arc<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
    // Groq-specific
    #[serde(default)]
    pub usage_breakdown: Option<Vec<ModelUsage>>,
    #[serde(default)]
    pub executed_tools: Option<Vec<ExecutedTool>>,
    #[serde(default)]
    pub system_fingerprint: Option<String>,
    #[serde(default)]
    pub x_groq: Option<Value>,
}

/// Token usage stats for a single LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub queue_time: Option<f64>,
    #[serde(default)]
    pub total_time: Option<f64>,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

/// Details about prompt token usage (e.g. cached tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u64>,
}

/// Per-model usage breakdown from Compound System responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub usage: Usage,
}

/// A tool executed by a Compound System — type, arguments, output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedTool {
    pub tool_type: String,
    pub arguments: Value,
    pub output: Value,
    pub search_results: Option<Vec<SearchResult>>,
}

/// A search result from Groq's built-in web search tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: f64,
}

/// Configuration for an LLM provider — name, API key, base URL, supported models.
#[derive(Clone, Serialize, Deserialize)]
pub struct LLMProviderConfig {
    pub name: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub models: Vec<String>,
    pub priority: u32,
}

impl std::fmt::Debug for LLMProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LLMProviderConfig")
            .field("name", &self.name)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("base_url", &self.base_url)
            .field("models", &self.models)
            .field("priority", &self.priority)
            .finish()
    }
}

// ─── Memory types ─────────────────────────────────────────────

/// A conversation session — ID, agent name, title, and message count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub agent_name: String,
    pub title: String,
    pub message_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A persistent memory entry — content, embedding vector, session ID, and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub session_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// An audit record of a tool execution — agent name, tool, status, tokens, timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub id: i64,
    pub tool_id: Option<i32>,
    pub tool_name: String,
    pub input: Value,
    pub output: Value,
    pub status: String,
    pub error: Option<String>,
    pub duration_ms: i32,
    pub created_at: DateTime<Utc>,
    pub execution_id: Uuid,
}

// ─── Tool execution types ─────────────────────────────────────

/// Result of executing a tool — success flag, output text, optional error, and duration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u128,
}

// ─── MCP types ────────────────────────────────────────────────

/// Configuration for an MCP (Model Context Protocol) server — name, transport, tools, env vars.
#[derive(Clone, Serialize, Deserialize)]
pub struct MCPServerConfig {
    pub name: String,
    pub transport: MCPTransport,
    pub tools: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
}

impl std::fmt::Debug for MCPServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_env = self.env.as_ref().map(|m| {
            m.keys()
                .map(|k| (k.clone(), "***".to_string()))
                .collect::<HashMap<_, _>>()
        });
        f.debug_struct("MCPServerConfig")
            .field("name", &self.name)
            .field("transport", &self.transport)
            .field("tools", &self.tools)
            .field("env", &redacted_env)
            .finish()
    }
}

/// Transport mechanism for an MCP server — HTTP with URL and optional headers, or Stdio.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MCPTransport {
    #[serde(rename = "stdio")]
    Stdio { command: String, args: Vec<String> },
    #[serde(rename = "http")]
    Http {
        url: String,
        headers: Option<HashMap<String, String>>,
    },
}

// ─── Groq Audio types ─────────────────────────────────────────

/// Request for Groq's speech-to-text (transcription/translation) endpoints.
#[derive(Debug, Clone)]
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

/// Response from Groq's speech-to-text endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioResponse {
    pub text: String,
    #[serde(default)]
    pub x_groq: Option<Value>,
    #[serde(default)]
    pub segments: Option<Vec<AudioSegment>>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
}

/// Request for Groq's text-to-speech endpoint.
#[derive(Debug, Clone)]
pub struct TtsRequest {
    pub model: String,
    pub input: String,
    pub voice: String,
    pub response_format: Option<String>,
    pub sample_rate: Option<u32>,
    pub speed: Option<f32>,
}

/// A segment from a verbose_json transcription response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSegment {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub seek: Option<u64>,
    #[serde(default)]
    pub start: Option<f64>,
    #[serde(default)]
    pub end: Option<f64>,
    pub text: String,
    #[serde(default)]
    pub tokens: Option<Vec<u64>>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub avg_logprob: Option<f64>,
    #[serde(default)]
    pub compression_ratio: Option<f64>,
    #[serde(default)]
    pub no_speech_prob: Option<f64>,
}

/// Groq Remote MCP connector — maps Groq's MCP connector format to Volt tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroqMcpConnector {
    pub server_label: String,
    pub server_url: String,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub connector_id: Option<String>,
    #[serde(default)]
    pub authorization: Option<String>,
    #[serde(default)]
    pub require_approval: String,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
}

/// A manifest describing an agent registry entry — name, version, tools, dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryManifest {
    pub tool_name: String,
    pub description: String,
    pub language: String,
    pub source_code: String,
    #[serde(default = "default_parameter_schema")]
    pub parameter_schema: Value,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub source_sha256: Option<String>,
    #[serde(default)]
    pub relationships: Vec<AssetRelationshipSpec>,
    #[serde(default)]
    pub metadata: Value,
}

/// A relationship between two registry assets — source, target, and relationship type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRelationshipSpec {
    pub child_tool_name: String,
    pub relationship_type: String,
}

/// A tool registered in the agent tool registry — ID, name, description, language, verification status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTool {
    pub id: i32,
    pub tool_name: String,
    pub description: String,
    pub language: String,
    pub is_marketplace_verified: bool,
    pub source_sha256: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Options for fetching from a registry — URL, auth token, timeout, cache settings.
#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryFetchOptions {
    pub pkg_id: String,
    pub registry_base_url: String,
    pub auth_token: Option<String>,
}

impl std::fmt::Debug for RegistryFetchOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryFetchOptions")
            .field("pkg_id", &self.pkg_id)
            .field("registry_base_url", &self.registry_base_url)
            .field("auth_token", &self.auth_token.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Result of provisioning a tool from a registry — success flag, manifest, and any warnings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionResult {
    pub tool_name: String,
    pub source_sha256: String,
    pub verified: bool,
    pub execution_id: Uuid,
}

/// Report from validating a tool manifest — valid flag, errors, warnings, and SHA-256 hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub accepted: bool,
    pub language: String,
    pub denied_patterns: Vec<String>,
    pub warnings: Vec<String>,
}

/// Sandbox execution policy — allowed commands, network access, timeout, and resource limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub working_dir: Option<String>,
}

/// Result of a sandboxed command execution — exit code, stdout, stderr, and duration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub status: String,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

fn default_parameter_schema() -> Value {
    serde_json::json!({ "type": "object" })
}

/// Permission level for a tool — Allow (auto-execute) or Prompt (requires human approval).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    Allow,
    Prompt,
    ReadOnly,
    Blocked,
}

/// A cancellable token for cooperative task shutdown. Clone to share across tasks.
#[derive(Debug, Clone)]
pub struct CancelToken(pub std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )))
    }

    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
/// Per-model context window sizing. Maps model names to their token limits for compression.
pub struct ModelContext {
    pub model: String,
    pub max_tokens: u32,
    pub max_context_tokens: u32,
}

impl ModelContext {
    pub fn for_model(model: &str) -> Self {
        let model_lower = model.to_lowercase();
        let max_context = if model_lower.contains("gemma")
            || model_lower.contains("phi")
            || model_lower.contains("qwen")
        {
            if model_lower.contains("qwen3") || model_lower.contains("qwen2.5") {
                131072
            } else {
                8192
            }
        } else if model_lower.contains("claude-sonnet-4")
            || model_lower.contains("claude-4")
            || model_lower.contains("claude-3-5-sonnet")
            || model_lower.contains("claude-3.5")
            || model_lower.contains("claude")
        {
            200000
        } else if model_lower.contains("claude-3") {
            100000
        } else if model_lower.contains("gpt-4o-mini")
            || model_lower.contains("gpt-4o")
            || model_lower.contains("gpt-4.1")
            || model_lower.contains("gpt-4")
        {
            128000
        } else if model_lower.contains("o1") || model_lower.contains("o3") {
            200000
        } else if model_lower.contains("gpt-3.5") {
            16385
        } else if model_lower.contains("deepseek") {
            65536
        } else if model_lower.contains("mistral") {
            if model_lower.contains("large") {
                131072
            } else {
                32768
            }
        } else if model_lower.contains("gemini") {
            100000
        } else if model_lower.contains("llama-3.3")
            || model_lower.contains("llama-3.2")
            || model_lower.contains("llama-3.1")
            || model_lower.contains("llama-4")
        {
            131072
        } else if model_lower.contains("llama-3") {
            8192
        } else {
            32768
        };
        Self {
            model: model.to_string(),
            max_tokens: 4096,
            max_context_tokens: max_context,
        }
    }

    pub fn estimate_tokens(text: &str) -> u32 {
        // Try accurate tokenization via tiktoken-rs (cl100k_base for GPT/Llama models)
        // Falls back to chars/3 heuristic if tokenizer unavailable
        static TOKENIZER: std::sync::OnceLock<Option<tiktoken_rs::CoreBPE>> =
            std::sync::OnceLock::new();
        let bpe = TOKENIZER.get_or_init(|| tiktoken_rs::cl100k_base().ok());
        if let Some(bpe) = bpe {
            bpe.encode_ordinary(text).len() as u32
        } else {
            (text.len() / 3).max(1) as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_short() {
        assert_eq!(ModelContext::estimate_tokens("abc"), 1);
    }

    #[test]
    fn test_estimate_tokens_long() {
        let s = "a".repeat(300);
        let tokens = ModelContext::estimate_tokens(&s);
        // tiktoken-rs cl100k_base: repeated chars tokenize efficiently (~38)
        // Fallback heuristic: chars/3 = 100
        assert!(tokens <= 100, "expected <=100, got {}", tokens);
        assert!(tokens > 0, "expected >0, got {}", tokens);
    }

    #[test]
    fn test_cancel_token() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_cancel_token_clone() {
        let t1 = CancelToken::new();
        let t2 = t1.clone();
        assert!(!t1.is_cancelled());
        assert!(!t2.is_cancelled());
        t1.cancel();
        assert!(t2.is_cancelled());
    }

    #[test]
    fn test_agent_state_default() {
        let state = AgentState {
            id: Uuid::new_v4(),
            name: "test".into(),
            session_id: Uuid::new_v4(),
            iteration: 0,
            messages: vec![],
            context_injected: false,
            allow_session: false,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
            last_saved_message_idx: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert_eq!(state.name, "test");
        assert!(!state.context_injected);
    }

    #[test]
    fn test_model_context_claude() {
        let ctx = ModelContext::for_model("claude-3-5-sonnet");
        assert_eq!(ctx.max_context_tokens, 200000);
    }

    #[test]
    fn test_model_context_gpt4() {
        let ctx = ModelContext::for_model("gpt-4o");
        assert_eq!(ctx.max_context_tokens, 128000);
    }

    #[test]
    fn test_model_context_default() {
        let ctx = ModelContext::for_model("unknown-model");
        assert_eq!(ctx.max_context_tokens, 32768);
    }
}
