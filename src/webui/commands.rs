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
    Chat { session_id: Option<Uuid>, input: String },

    /// Abort the in-flight chat turn, if any.
    CancelChat,

    /// Request a snapshot of the tool registry.
    ListTools,

    /// Invoke a tool directly without going through the agent.
    ExecuteTool { name: String, args: serde_json::Value },

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

    /// List routines.
    ListRoutines,

    /// List installed skills.
    ListSkills,

    /// Search the remote skill catalog.
    SearchCatalogSkills { query: String },

    /// Install a skill from the catalog.
    InstallSkill { name: String },

    /// Import a skill from a local file path.
    ImportSkill { path: String },

    /// List registered MCP servers.
    ListMcpServers,

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

    /// Scheduled jobs snapshot.
    JobsListed { jobs: Vec<JobInfo> },

    /// Routines snapshot.
    RoutinesListed { routines: Vec<RoutineInfo> },

    /// Installed skills snapshot.
    SkillsListed { skills: Vec<SkillInfo> },

    /// Catalog skill search results.
    CatalogResults {
        query: String,
        skills: Vec<CatalogSkillInfo>,
    },

    /// A skill was installed.
    SkillInstalled { name: String },

    /// MCP servers snapshot.
    McpServersListed { servers: Vec<McpServerInfo> },

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

    /// Generic transport-level or command-handler error.
    Error { source: String, message: String },
}

// =============================================================================
// Supporting DTOs
// =============================================================================

/// Snapshot entry describing a single tool in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub category: String,
    /// One of `"allow"`, `"prompt"`, `"deny"`.
    pub permission: String,
    pub schema: serde_json::Value,
    pub enabled: bool,
}

/// A single message in a chat session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    /// One of `"user"`, `"assistant"`, `"tool"`, `"system"`.
    pub role: String,
    pub content: String,
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
}

/// A routine (event-triggered automation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineInfo {
    pub id: String,
    pub name: String,
    pub trigger: String,
    pub last_run: Option<DateTime<Utc>>,
    pub enabled: bool,
}

/// An installed skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub installed_at: DateTime<Utc>,
    /// One of `"local"`, `"catalog"`, `"imported"`.
    pub source: String,
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
    /// One of `"stdio"`, `"http"`, `"websocket"`, `"grpc"`.
    pub transport: String,
    /// One of `"connected"`, `"disconnected"`, `"error"`.
    pub status: String,
    pub tools_count: u32,
    pub endpoint: String,
}

/// A single audit-log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    /// One of `"user"`, `"agent"`, `"tool"`.
    pub actor: String,
    /// One of `"chat"`, `"tool_call"`, `"config_change"`, `"approval"`.
    pub action: String,
    /// Target of the action (tool name, model id, etc.).
    pub target: String,
    /// One of `"ok"`, `"denied"`, `"error"`.
    pub result: String,
    pub detail: serde_json::Value,
    pub session_id: Option<Uuid>,
}
