//! Message types for UI <-> Runtime communication.
//!
//! `UiCommand` is sent from the Dioxus web UI to the runtime task that owns
//! the `Agent`, tool registry, session store, etc.  `UiEvent` flows back,
//! streaming chat chunks, tool-call lifecycle events, snapshots, and errors.
//!
//! All types are `Serialize + Deserialize` so they can travel across the
//! runtime bridge (channel, WebSocket, or in-process queue) using JSON.
//! The runtime-side handlers are not part of this module; only the wire
//! schema is defined here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// UiCommand
// =============================================================================

/// Commands the UI sends to the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    /// Run a chat turn. If `session_id` is `None` the runtime creates a new session.
    Chat {
        session_id: Option<Uuid>,
        input: String,
    },

    /// Abort the in-flight chat turn, if any.
    CancelChat,

    /// Request a snapshot of the tool registry.
    ListTools,

    /// Invoke a tool directly without going through the agent.
    ExecuteTool {
        name: String,
        args: serde_json::Value,
    },

    /// Request the list of stored sessions.
    ListSessions,

    /// Load the messages of a specific session.
    LoadSession { id: Uuid },

    /// Create a new session with the given display name.
    CreateSession { name: String },

    /// Fork an existing session into a new one.
    ForkSession { id: Uuid },

    /// Delete a session.
    DeleteSession { id: Uuid },

    /// Request the available model registry.
    ListModels,

    /// Request the current effective config as JSON.
    GetConfig,

    /// Apply a partial config patch (JSON Merge Patch semantics).
    UpdateConfig { patch: serde_json::Value },

    /// Run the doctor health-check.
    RunDoctor,

    /// List git worktrees.
    ListWorktrees,

    /// Diff stat for the given branch.
    WorktreeStatus { branch: String },

    /// Merge a branch into the current worktree.
    WorktreeMerge { branch: String },

    /// Clean up a worktree.
    WorktreeClean { branch: String },

    /// List available workflow patterns.
    ListWorkflows,

    /// Run a workflow. `agents` and `tasks` are optional YAML/JSON spec strings.
    RunWorkflow {
        pattern: String,
        agents: Option<String>,
        tasks: Option<String>,
        allow: bool,
    },

    /// List scheduled jobs.
    ListJobs,

    /// Create a new job row.
    CreateJob { description: String },

    /// Mark a job in-progress.
    StartJob { id: Uuid, worker_id: Option<String> },

    /// Mark a job complete.
    CompleteJob { id: Uuid, output: String },

    /// Mark a job failed.
    FailJob { id: Uuid, error: String },

    /// List routines.
    ListRoutines,

    /// Toggle a routine enabled/disabled.
    ToggleRoutine { id: Uuid, enabled: bool },

    /// Create a new routine.
    CreateRoutine {
        name: String,
        action_prompt: String,
        cron: Option<String>,
        trigger_type: Option<String>,
    },

    /// Delete a routine.
    DeleteRoutine { id: Uuid },

    /// List installed skills.
    ListSkills,

    /// Search the remote skill catalog.
    SearchCatalogSkills { query: String },

    /// Install a skill from the catalog.
    InstallSkill { name: String },

    /// Import a skill from a local file path.
    ImportSkill { path: String },

    /// Uninstall (delete) a skill by name.
    UninstallSkill { name: String },

    /// List registered MCP servers.
    ListMcpServers,

    /// Register a new MCP server.
    RegisterMcpServer {
        name: String,
        transport: String,
        command: Option<String>,
        url: Option<String>,
    },

    /// Request the most recent audit-log entries.
    GetAuditLog { limit: u32 },

    /// User response to an `ApprovalRequest` event.
    ApprovalResponse {
        request_id: Uuid,
        allow: bool,
        allow_session: bool,
    },

    /// Liveness probe.
    Ping,

    /// Persist a newly-entered API key. The runtime writes the value
    /// to `volt_home()/.env` (so it survives restarts), sets it in the
    /// process environment, and rebuilds the LLM provider. Emitted
    /// back to the UI as a `SetupReady` event on success.
    SubmitApiKey {
        /// Provider slug, e.g. "groq", "openai", "anthropic", "nvidia",
        /// "ollama". The runtime maps this to the correct env var name.
        provider: String,
        /// The API key value. Ignored for providers with no key (e.g.
        /// local Ollama).
        api_key: String,
        /// Default model to use for this provider (e.g. "llama-3.1-8b-instant").
        model: String,
    },
}

// =============================================================================
// UiEvent
// =============================================================================

/// Events the runtime emits to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    /// Chat turn started; echoes the session that owns it.
    ChatStarted { session_id: Uuid },

    /// Streaming text chunk from the assistant.
    ChatChunk { content: String },

    /// A tool call has started executing.
    ToolCallStart {
        id: String,
        name: String,
        args: serde_json::Value,
    },

    /// A tool call has finished executing.
    ToolCallEnd {
        id: String,
        result: serde_json::Value,
        error: Option<String>,
    },

    /// Chat turn finished successfully.
    ChatComplete {
        #[serde(rename = "final")]
        final_text: String,
        tokens_used: u32,
        duration_ms: u64,
    },

    /// Chat turn failed.
    ChatError { message: String },

    /// Chat turn was cancelled by the user.
    ChatCancelled,

    /// Snapshot of the tool registry.
    ToolsListed { tools: Vec<ToolInfo> },

    /// Snapshot of stored sessions.
    SessionsListed { sessions: Vec<SessionInfo> },

    /// A session was loaded with its full message history.
    SessionLoaded {
        id: Uuid,
        messages: Vec<ChatMessage>,
    },

    /// A new session was created.
    SessionCreated { id: Uuid },

    /// A session was deleted.
    SessionDeleted { id: Uuid },

    /// Snapshot of the model registry.
    ModelsListed { models: Vec<ModelInfo> },

    /// Current effective config.
    ConfigLoaded { config: serde_json::Value },

    /// Config patch was applied successfully.
    ConfigUpdated,

    /// Doctor health-check completed.
    DoctorCompleted { report: DoctorReport },

    /// Worktree list snapshot.
    WorktreesListed { worktrees: Vec<WorktreeInfo> },

    /// Workflow list snapshot.
    WorkflowsListed { workflows: Vec<WorkflowInfo> },

    /// A workflow run has started.
    WorkflowStarted { pattern: String, run_id: String },

    /// A workflow run completed successfully.
    WorkflowCompleted { pattern: String, run_id: String },

    /// A workflow run failed.
    WorkflowFailed {
        pattern: String,
        run_id: String,
        error: String,
    },

    /// Scheduled jobs snapshot.
    JobsListed { jobs: Vec<JobInfo> },

    /// A new job was created.
    JobCreated { id: String },

    /// A job state changed (e.g. InProgress -> Completed).
    JobUpdated { id: String, state: String },

    /// Routines snapshot.
    RoutinesListed { routines: Vec<RoutineInfo> },

    /// A routine was toggled or created.
    RoutineUpdated { id: String, enabled: bool },

    /// A routine was deleted.
    RoutineDeleted { id: String },

    /// Installed skills snapshot.
    SkillsListed { skills: Vec<SkillInfo> },

    /// Catalog skill search results.
    CatalogResults {
        query: String,
        skills: Vec<CatalogSkillInfo>,
    },

    /// A skill was installed.
    SkillInstalled { name: String },

    /// A skill was uninstalled.
    SkillUninstalled { name: String },

    /// MCP servers snapshot.
    McpServersListed { servers: Vec<McpServerInfo> },

    /// An MCP server was registered.
    McpServerRegistered { name: String },

    /// Audit-log entries (most recent first).
    AuditLog { entries: Vec<AuditEntry> },

    /// A tool needs user approval before executing.
    ApprovalRequest {
        request_id: Uuid,
        tool_name: String,
        args: serde_json::Value,
    },

    /// Pong response to `UiCommand::Ping`.
    Pong,

    /// The runtime started but no LLM API key is configured. The UI
    /// should show a setup wizard and let the user enter credentials.
    /// Includes the current env-var search paths so the UI can tell
    /// the user which key names are accepted.
    SetupNeeded {
        providers: Vec<ProviderInfo>,
    },

    /// The runtime accepted a new API key, persisted it, and rebuilt
    /// the LLM provider successfully. The UI should close the wizard.
    SetupReady { provider: String, model: String },

    /// Generic transport-level or command-handler error.
    Error { source: String, message: String },
}

/// One entry in the provider list shown by the setup wizard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub slug: String,
    pub label: String,
    /// Env var that the runtime will read (e.g. "GROQ_API_KEY").
    /// `None` for local providers that don't need a key.
    pub env_var: Option<String>,
    /// Default model id for this provider.
    pub default_model: String,
}

/// UI-side record of an `ApprovalRequest` event. Stored in
/// `VoltState::pending_approvals` so the modal can render all
/// outstanding requests and the user can answer each one in turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestInfo {
    pub request_id: uuid::Uuid,
    pub tool_name: String,
    pub args: serde_json::Value,
}

// =============================================================================
// Supporting DTOs
// =============================================================================

/// Role of a chat-message author. Wire-level values stay lowercase so
/// the runtime can keep emitting `"user"` / `"assistant"` strings
/// without conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    #[default]
    User,
    Assistant,
    Tool,
    System,
}

/// Whether the runtime is allowed to auto-run a tool, must prompt the
/// user, or is blocked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPermission {
    #[default]
    Allow,
    Prompt,
    Deny,
}

/// A single transport for an MCP server connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    #[default]
    Stdio,
    Http,
    Websocket,
    Grpc,
}

/// Connection state of a registered MCP server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpStatus {
    #[default]
    Disconnected,
    Connected,
    Error,
}

/// Who initiated an audit-logged action.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditActor {
    #[default]
    User,
    Agent,
    Tool,
}

/// Category of audited action.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    #[default]
    Chat,
    ToolCall,
    ConfigChange,
    Approval,
}

/// Outcome of an audited action.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditResult {
    #[default]
    Ok,
    Denied,
    Error,
}

/// Origin of an installed skill.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    #[default]
    Local,
    Catalog,
    Imported,
}

impl std::fmt::Display for ChatRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            ChatRole::Tool => "tool",
            ChatRole::System => "system",
        })
    }
}

impl std::fmt::Display for ToolPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ToolPermission::Allow => "allow",
            ToolPermission::Prompt => "prompt",
            ToolPermission::Deny => "deny",
        })
    }
}

impl std::fmt::Display for McpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            McpTransport::Stdio => "stdio",
            McpTransport::Http => "http",
            McpTransport::Websocket => "websocket",
            McpTransport::Grpc => "grpc",
        })
    }
}

impl std::fmt::Display for McpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            McpStatus::Connected => "connected",
            McpStatus::Disconnected => "disconnected",
            McpStatus::Error => "error",
        })
    }
}

impl std::fmt::Display for AuditActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AuditActor::User => "user",
            AuditActor::Agent => "agent",
            AuditActor::Tool => "tool",
        })
    }
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AuditAction::Chat => "chat",
            AuditAction::ToolCall => "tool_call",
            AuditAction::ConfigChange => "config_change",
            AuditAction::Approval => "approval",
        })
    }
}

impl std::fmt::Display for AuditResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AuditResult::Ok => "ok",
            AuditResult::Denied => "denied",
            AuditResult::Error => "error",
        })
    }
}

impl std::fmt::Display for SkillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SkillSource::Local => "local",
            SkillSource::Catalog => "catalog",
            SkillSource::Imported => "imported",
        })
    }
}

/// Snapshot entry describing a single tool in the registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub category: String,
    pub permission: ToolPermission,
    pub schema: serde_json::Value,
    pub enabled: bool,
}

/// A single message in a chat session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub role: ChatRole,
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallInfo>,
    pub timestamp: DateTime<Utc>,
}

/// Record of a tool invocation attached to an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Summary metadata for a stored session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u32,
    pub tokens_used: u32,
}

/// One entry in the model registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub display_name: String,
    pub context_window: u32,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub available: bool,
}

/// Aggregate result of `volt doctor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub os: String,
    pub arch: String,
    pub rust_channel: String,
    pub api_keys: Vec<ApiKeyStatus>,
    /// One of `"ok"`, `"unreachable"`, `"not configured"`.
    pub database: String,
    pub embedder_provider: String,
    pub embedder_model: String,
    pub disk_free_gb: f32,
    pub permissions_default: String,
    pub recent_failures: u32,
    pub workspace_files: Vec<WorkspaceFileStatus>,
}

/// Status of a single API key in the environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyStatus {
    pub name: String,
    pub present: bool,
    /// Last 4 chars of the key, or `"(not set)"` if absent.
    pub masked: String,
}

/// Status of a single workspace file (e.g. `AGENTS.md`, `MEMORY.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFileStatus {
    pub name: String,
    pub present: bool,
    pub bytes: u64,
}

/// A single git worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub path: String,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub commits_ahead: u32,
}

/// A workflow pattern available to the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    pub name: String,
    pub description: String,
    pub pattern: String,
    pub agents: Vec<String>,
}

/// A scheduled job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub last_run: Option<DateTime<Utc>>,
    pub last_status: String,
    pub next_run: Option<DateTime<Utc>>,
    pub attempt_count: i32,
    pub worker_id: Option<String>,
    pub output: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A routine (event-triggered automation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineInfo {
    pub id: String,
    pub name: String,
    pub trigger: String,
    pub last_run: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub next_run: Option<DateTime<Utc>>,
    pub action_prompt: String,
}

/// An installed skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub installed_at: DateTime<Utc>,
    pub source: SkillSource,
}

/// A skill returned from catalog search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogSkillInfo {
    pub name: String,
    pub description: String,
    pub author: String,
    pub downloads: u32,
}

/// A registered MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub transport: McpTransport,
    pub status: McpStatus,
    pub tools_count: u32,
    pub endpoint: String,
}

/// A single audit-log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub actor: AuditActor,
    pub action: AuditAction,
    /// Target of the action (tool name, model id, etc.).
    pub target: String,
    pub result: AuditResult,
    pub detail: serde_json::Value,
    pub session_id: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_chat_chunk() {
        let e = UiEvent::ChatChunk {
            content: "hi".into(),
        };
        let s = serde_json::to_string(&e).unwrap();
        let v: UiEvent = serde_json::from_str(&s).unwrap();
        match v {
            UiEvent::ChatChunk { content } => assert_eq!(content, "hi"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_install_skill() {
        let c = UiCommand::InstallSkill { name: "weather".into() };
        let s = serde_json::to_string(&c).unwrap();
        let v: UiCommand = serde_json::from_str(&s).unwrap();
        match v {
            UiCommand::InstallSkill { name } => assert_eq!(name, "weather"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_create_job() {
        let c = UiCommand::CreateJob { description: "build the thing".into() };
        let s = serde_json::to_string(&c).unwrap();
        let v: UiCommand = serde_json::from_str(&s).unwrap();
        match v {
            UiCommand::CreateJob { description } => {
                assert_eq!(description, "build the thing")
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_toggle_routine() {
        let id = Uuid::new_v4();
        let c = UiCommand::ToggleRoutine { id, enabled: false };
        let s = serde_json::to_string(&c).unwrap();
        let v: UiCommand = serde_json::from_str(&s).unwrap();
        match v {
            UiCommand::ToggleRoutine { id: i, enabled } => {
                assert_eq!(i, id);
                assert!(!enabled);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_register_mcp_server() {
        let c = UiCommand::RegisterMcpServer {
            name: "himalaya".into(),
            transport: "stdio".into(),
            command: Some("himalaya-mcp --stdio".into()),
            url: None,
        };
        let s = serde_json::to_string(&c).unwrap();
        let v: UiCommand = serde_json::from_str(&s).unwrap();
        match v {
            UiCommand::RegisterMcpServer { name, transport, command, url } => {
                assert_eq!(name, "himalaya");
                assert_eq!(transport, "stdio");
                assert_eq!(command.as_deref(), Some("himalaya-mcp --stdio"));
                assert!(url.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_chat_complete_final_renamed() {
        let e = UiEvent::ChatComplete {
            final_text: "done".into(),
            tokens_used: 42,
            duration_ms: 123,
        };
        let s = serde_json::to_string(&e).unwrap();
        // Must serialize as "final", not "final_text"
        assert!(s.contains("\"final\":\"done\""), "got {}", s);
        let v: UiEvent = serde_json::from_str(&s).unwrap();
        match v {
            UiEvent::ChatComplete { final_text, tokens_used, duration_ms } => {
                assert_eq!(final_text, "done");
                assert_eq!(tokens_used, 42);
                assert_eq!(duration_ms, 123);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn job_info_serializes_with_all_fields() {
        let j = JobInfo {
            id: "abc-123".into(),
            name: "build".into(),
            schedule: String::new(),
            last_run: None,
            last_status: "Pending".into(),
            next_run: None,
            attempt_count: 0,
            worker_id: None,
            output: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let s = serde_json::to_string(&j).unwrap();
        let v: JobInfo = serde_json::from_str(&s).unwrap();
        assert_eq!(v.id, "abc-123");
        assert_eq!(v.attempt_count, 0);
    }

    #[test]
    fn workflow_completed_round_trip() {
        let e = UiEvent::WorkflowCompleted {
            pattern: "dag".into(),
            run_id: "run-1".into(),
        };
        let s = serde_json::to_string(&e).unwrap();
        let v: UiEvent = serde_json::from_str(&s).unwrap();
        match v {
            UiEvent::WorkflowCompleted { pattern, run_id } => {
                assert_eq!(pattern, "dag");
                assert_eq!(run_id, "run-1");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn audit_entry_round_trip() {
        let a = AuditEntry {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            actor: AuditActor::User,
            action: AuditAction::ToolCall,
            target: "bash".into(),
            result: AuditResult::Ok,
            detail: json!({"exit_code": 0}),
            session_id: Some(Uuid::new_v4()),
        };
        let s = serde_json::to_string(&a).unwrap();
        let v: AuditEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(v.actor, AuditActor::User);
        assert_eq!(v.action, AuditAction::ToolCall);
        assert_eq!(v.result, AuditResult::Ok);
    }

    #[test]
    fn chat_role_serializes_lowercase() {
        // Wire format must stay lowercase so the runtime can keep
        // emitting plain strings without conversion.
        let s = serde_json::to_string(&ChatRole::Assistant).unwrap();
        assert_eq!(s, "\"assistant\"");
        let s = serde_json::to_string(&AuditAction::ConfigChange).unwrap();
        assert_eq!(s, "\"config_change\"");
    }
}
