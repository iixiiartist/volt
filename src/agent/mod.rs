pub mod blueprint;
pub mod builder;
pub mod compression;
pub mod cot;
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
}
