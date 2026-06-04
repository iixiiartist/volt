pub mod blueprint;
pub mod builder;
pub mod compression;
pub mod cot;
pub mod hooks;
pub mod model_registry;
pub mod multimodal;
pub mod preset;
pub mod prompt;
pub mod prompt_builder;
pub mod router;
pub mod run;
pub mod tool_parser;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Maximum tool output characters before truncation with a reference token.
pub(crate) const MAX_TOOL_OUTPUT_CHARS: usize = 2000;

/// Decision returned from a per-tool approval callback. The TUI can render
/// `y/N/a` as a widget; the stdin prompt just maps a character to one of
/// these variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Run the tool this once.
    AllowOnce,
    /// Run the tool this once and remember the choice for the rest of
    /// the current session (so subsequent calls of the same tool don't
    /// re-prompt).
    AllowSession,
    /// Decline the tool call.
    Deny,
}

/// Async signature of a per-tool approval callback. The callback receives
/// the tool name + arguments and returns a decision. Returning
/// `ApprovalDecision::AllowSession` flips the agent's `allow_session` flag
/// for the remainder of the turn (matches the legacy `a` short-circuit).
pub type ApprovalCallback = Arc<
    dyn Fn(&str, &serde_json::Value) -> futures::future::BoxFuture<'static, ApprovalDecision>
        + Send
        + Sync,
>;

/// An autonomous agent — holds configuration, LLM provider, tool registry, and session state.
/// Built via `Agent::new()` with chained `.with_*()` methods for context, skills, sessions, etc.
pub struct Agent {
    pub(crate) config: crate::models::AgentConfig,
    pub(crate) state: Arc<Mutex<crate::models::AgentState>>,
    pub(crate) provider: Box<dyn crate::llm::LLMProvider>,
    pub(crate) tools: Arc<crate::tools::ToolRegistry>,
    pub(crate) db: Option<sqlx::PgPool>,
    pub(crate) embedder: Option<crate::embedding::EmbeddingClient>,
    pub(crate) skills: Option<Arc<crate::skills::SkillRegistry>>,
    pub(crate) context_store: Option<Arc<crate::context::ContextStore>>,
    pub(crate) seed_channel: Option<crate::worker::SeedChannel>,
    pub(crate) cancel: Option<crate::models::CancelToken>,
    pub(crate) on_token: Option<crate::llm::provider::TokenCallback>,
    pub(crate) session_id: Option<uuid::Uuid>,
    pub(crate) sqlite_pool: Option<sqlx::SqlitePool>,
    pub(crate) workspace: Option<PathBuf>,
    pub(crate) event_bus: Option<crate::events::EventBus>,
    pub(crate) failure_tracker: Option<crate::tool_failure_tracker::ToolFailureTracker>,
    pub(crate) tool_output_buffer: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) capability_manager: Arc<crate::capability::CapabilityManager>,
    pub(crate) checkpoint_journal: Option<Arc<crate::checkpoint_journal::CheckpointJournal>>,
    /// Optional per-tool approval callback. When set, replaces the default
    /// stdin-based approval prompt. The TUI uses this to show a clickable
    /// widget instead of reading from stdin.
    pub(crate) approval_fn: Option<ApprovalCallback>,
    /// Optional hook registry. When set, runs `PreToolUse` / `PostToolUse`
    /// / `PreRun` / `PostRun` / `UserPromptSubmit` shell commands before
    /// and after tool execution. Cheap to clone — backed by `Arc` inside.
    pub(crate) hook_registry: Option<crate::agent::hooks::HookRegistry>,
}
