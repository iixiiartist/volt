use super::Agent;
use crate::embedding::EmbeddingClient;
use crate::llm::provider::TokenCallback;
use crate::llm::LLMProvider;
use crate::models::{AgentConfig, AgentState};
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;
use crate::worker::SeedChannel;
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

impl Agent {
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }
    pub fn state(&self) -> &Arc<Mutex<AgentState>> {
        &self.state
    }
}

impl Agent {
    pub async fn new(
        config: AgentConfig,
        provider: Box<dyn LLMProvider>,
        tools: Arc<ToolRegistry>,
    ) -> Self {
        let state = AgentState {
            id: uuid::Uuid::new_v4(),
            name: config.name.clone(),
            session_id: uuid::Uuid::new_v4(),
            iteration: 0,
            context_injected: false,
            allow_session: false,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
            last_saved_message_idx: 0,
            messages: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let mgr = Arc::new(crate::capability::CapabilityManager::new());
        mgr.issue(
            crate::capability::CapabilityScope::FsRead,
            100,
            chrono::Duration::hours(24),
        )
        .await;
        mgr.issue(
            crate::capability::CapabilityScope::FsWrite,
            50,
            chrono::Duration::hours(24),
        )
        .await;
        mgr.issue(
            crate::capability::CapabilityScope::System,
            20,
            chrono::Duration::hours(24),
        )
        .await;
        mgr.issue(
            crate::capability::CapabilityScope::Network,
            200,
            chrono::Duration::hours(24),
        )
        .await;
        mgr.issue(
            crate::capability::CapabilityScope::Database,
            30,
            chrono::Duration::hours(24),
        )
        .await;
        mgr.issue(
            crate::capability::CapabilityScope::Memory,
            50,
            chrono::Duration::hours(24),
        )
        .await;
        Self {
            config,
            state: Arc::new(Mutex::new(state)),
            provider,
            tools,
            db: None,
            embedder: None,
            skills: None,
            context_store: None,
            seed_channel: None,
            cancel: None,
            on_token: None,
            session_id: None,
            sqlite_pool: None,
            workspace: None,
            event_bus: None,
            failure_tracker: None,
            tool_output_buffer: Arc::new(Mutex::new(HashMap::new())),
            checkpoint_journal: None,
            approval_fn: None,
            hook_registry: None,
            capability_manager: mgr,
        }
    }

    pub fn with_workspace(mut self, path: PathBuf) -> Self {
        self.workspace = Some(path);
        self
    }

    pub fn with_memory(mut self, db: PgPool, embedder: EmbeddingClient) -> Self {
        self.db = Some(db);
        self.embedder = Some(embedder);
        self
    }

    pub fn with_memory_embedder_only(mut self, embedder: EmbeddingClient) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn with_context(mut self, context_store: Arc<crate::context::ContextStore>) -> Self {
        self.context_store = Some(context_store);
        self
    }

    pub fn with_seed_channel(mut self, seed_channel: SeedChannel) -> Self {
        self.seed_channel = Some(seed_channel);
        self
    }

    pub fn with_cancel(mut self, cancel: crate::models::CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    pub fn with_stream(mut self, on_token: TokenCallback) -> Self {
        self.on_token = Some(on_token);
        self
    }

    pub fn with_session(mut self, session_id: uuid::Uuid, sqlite_pool: sqlx::SqlitePool) -> Self {
        self.session_id = Some(session_id);
        self.sqlite_pool = Some(sqlite_pool);
        self
    }

    pub fn with_event_bus(mut self, bus: crate::events::EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_failure_tracker(
        mut self,
        tracker: crate::tool_failure_tracker::ToolFailureTracker,
    ) -> Self {
        self.failure_tracker = Some(tracker);
        self
    }

    pub fn with_checkpoint_journal(
        mut self,
        journal: Arc<crate::checkpoint_journal::CheckpointJournal>,
    ) -> Self {
        self.checkpoint_journal = Some(journal);
        self
    }

    pub fn with_capability_manager(
        mut self,
        cap_mgr: Arc<crate::capability::CapabilityManager>,
    ) -> Self {
        self.capability_manager = cap_mgr;
        self
    }

    /// Load and merge an AgentBlueprint from a TOML file.
    /// Overrides AgentConfig fields: format_dialect, quirks, strict_mode,
    /// max_tools_per_turn, essential_tools, and system_prompt.
    pub fn with_blueprint(mut self, path: std::path::PathBuf) -> Self {
        if let Some(bp) = crate::agent::blueprint::load_blueprint(&path) {
            self.config.format_dialect = bp.model_card.format_dialect;
            self.config.quirks = bp.model_card.quirks;
            self.config.strict_mode = bp.scaffolding.strict_mode;
            self.config.max_tools_per_turn = bp.scaffolding.max_tools_per_turn;
            self.config.essential_tools = bp.tools.core_tools;
            if let Some(override_prompt) = bp.prompts.system_prompt_override {
                self.config.system_prompt = Some(override_prompt);
            }
            self.config.blueprint_path = Some(path.to_string_lossy().to_string());
        }
        self
    }

    /// Set a per-tool approval callback. When the agent needs approval for a
    /// tool call, it invokes this callback instead of reading from stdin.
    /// The TUI passes a callback that renders a clickable widget.
    pub fn with_approval(mut self, approval_fn: crate::agent::ApprovalCallback) -> Self {
        self.approval_fn = Some(approval_fn);
        self
    }

    /// Install a hook registry. Hooks are shell commands run at
    /// `PreToolUse` / `PostToolUse` / `PreRun` / `PostRun` /
    /// `UserPromptSubmit` points. They can block, modify arguments, or
    /// inject context. See `src/agent/hooks.rs` for the config format.
    pub fn with_hooks(mut self, hooks: crate::agent::hooks::HookRegistry) -> Self {
        self.hook_registry = Some(hooks);
        self
    }

    pub(super) fn is_precision_mode(&self) -> bool {
        let kinds = &self.config.enabled_context_kinds;
        kinds.len() <= 2
            && kinds.contains(&crate::context::ContextKind::Tool)
            && kinds.contains(&crate::context::ContextKind::Artifact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextKind;
    use crate::models::AgentConfig;
    use crate::test_utils::MockLLMProvider;
    use crate::tools::ToolRegistry;

    fn precision_config() -> AgentConfig {
        AgentConfig {
            name: "precision-test".into(),
            model: "test-model".into(),
            provider: "mock".into(),
            system_prompt: None,
            max_iterations: 10,
            temperature: 0.7,
            toolsets: vec!["core".into()],
            hidden: false,
            allow_all: true,
            enabled_context_kinds: vec![ContextKind::Tool, ContextKind::Artifact],
            essential_tools: vec!["read".into()],
            context_kind_quotas: std::collections::HashMap::new(),
            use_mtp: false,
            use_cot: false,
            allow_write: true,
            framework: None,
            model_variant: None,
            quantization: None,
            format_dialect: Default::default(),
            quirks: vec![],
            strict_mode: false,
            max_tools_per_turn: None,
            blueprint_path: None,
        }
    }

    fn balanced_config() -> AgentConfig {
        let mut cfg = precision_config();
        cfg.enabled_context_kinds = crate::models::default_context_kinds();
        cfg
    }

    async fn test_agent(config: AgentConfig) -> Agent {
        let provider = Box::new(MockLLMProvider::new(vec![]));
        let tools = ToolRegistry::new();
        Agent::new(config, provider, tools).await
    }

    #[tokio::test]
    async fn test_precision_mode_true() {
        let agent = test_agent(precision_config()).await;
        assert!(agent.is_precision_mode());
    }

    #[tokio::test]
    async fn test_precision_mode_false_balanced() {
        let agent = test_agent(balanced_config()).await;
        assert!(!agent.is_precision_mode());
    }

    #[tokio::test]
    async fn test_precision_mode_false_missing_artifact() {
        let mut cfg = precision_config();
        cfg.enabled_context_kinds = vec![ContextKind::Tool];
        let agent = test_agent(cfg).await;
        assert!(!agent.is_precision_mode());
    }

    #[tokio::test]
    async fn test_builder_default_state() {
        let agent = test_agent(precision_config()).await;
        let state = agent.state().lock().await;
        assert_eq!(state.iteration, 0);
        assert!(!state.allow_session);
        assert_eq!(state.total_prompt_tokens, 0);
        assert_eq!(state.total_completion_tokens, 0);
        assert_eq!(state.last_saved_message_idx, 0);
    }

    #[tokio::test]
    async fn test_builder_with_workspace() {
        let agent = test_agent(precision_config()).await;
        let path = std::path::PathBuf::from("/tmp/test");
        let agent = agent.with_workspace(path.clone());
        assert_eq!(agent.workspace, Some(path));
    }

    #[tokio::test]
    async fn test_builder_with_memory_embedder_only() {
        let agent = test_agent(precision_config()).await;
        assert!(agent.embedder.is_none());
    }

    #[tokio::test]
    async fn test_builder_with_context_store() {
        let agent = test_agent(precision_config()).await;
        let store = crate::context::ContextStore::new();
        let agent = agent.with_context(store.clone());
        assert!(agent.context_store.is_some());
    }

    #[tokio::test]
    async fn test_builder_with_cancel_token() {
        let agent = test_agent(precision_config()).await;
        let token = crate::models::CancelToken::new();
        let agent = agent.with_cancel(token);
        assert!(agent.cancel.is_some());
    }

    #[tokio::test]
    async fn test_builder_config_accessor() {
        let cfg = precision_config();
        let agent = test_agent(cfg.clone()).await;
        assert_eq!(agent.config().name, "precision-test");
        assert_eq!(agent.config().max_iterations, 10);
    }
}
