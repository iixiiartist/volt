use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub id: Uuid,
    pub name: String,
    pub session_id: Uuid,
    pub iteration: u32,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
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
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
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
    Http { url: String, headers: Option<HashMap<String, String>> },
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

impl CancelToken {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
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
        let max_context = if model.contains("gemma") || model.contains("phi") || model.contains("qwen") {
            8192
        } else if model.contains("claude-3-5-sonnet") {
            200000
        } else if model.contains("claude-3") {
            100000
        } else if model.contains("gpt-4") {
            128000
        } else {
            4096
        };
        Self {
            model: model.to_string(),
            max_tokens: 4096,
            max_context_tokens: max_context,
        }
    }

    pub fn estimate_tokens(text: &str) -> u32 {
        (text.len() / 4) as u32
    }
}