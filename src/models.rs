use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ─── Agent types ──────────────────────────────────────────────

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Arc<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_result: Option<String>,
    pub tool_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub category: String,
}

// ─── LLM types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub model: String,
    pub messages: Vec<LLMMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMMessage {
    pub role: String,
    pub content: Arc<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: Arc<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMProviderConfig {
    pub name: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub models: Vec<String>,
    pub priority: u32,
}

// ─── Memory types ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub agent_name: String,
    pub title: String,
    pub message_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub session_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u128,
}

// ─── MCP types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerConfig {
    pub name: String,
    pub transport: MCPTransport,
    pub tools: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
}

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

// ─── Existing types ───────────────────────────────────────────

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRelationshipSpec {
    pub child_tool_name: String,
    pub relationship_type: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryFetchOptions {
    pub pkg_id: String,
    pub registry_base_url: String,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionResult {
    pub tool_name: String,
    pub source_sha256: String,
    pub verified: bool,
    pub execution_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub accepted: bool,
    pub language: String,
    pub denied_patterns: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub working_dir: Option<String>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    Allow,
    Prompt,
}

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
