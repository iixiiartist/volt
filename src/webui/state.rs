use super::commands::UiCommand;
use super::routes::Page;
use super::runtime::RuntimeHandle;
use dioxus::prelude::*;

pub const SIDEBAR_WIDTH: u32 = 240;
pub const HEADER_HEIGHT: u32 = 56;
pub const COLOR_BG: &str = "#0a0a14";
pub const COLOR_PANEL: &str = "#14142a";
pub const COLOR_PANEL_HOVER: &str = "#1a1a35";
pub const COLOR_BORDER: &str = "#25254a";
pub const COLOR_TEXT: &str = "#e6e6f0";
pub const COLOR_TEXT_DIM: &str = "#9090a8";
pub const COLOR_TEXT_MUTED: &str = "#7a7a98";
pub const COLOR_ACCENT: &str = "#a855f7";
pub const COLOR_ACCENT_HOVER: &str = "#9333ea";
pub const COLOR_SUCCESS: &str = "#22c55e";
pub const COLOR_WARNING: &str = "#f59e0b";
pub const COLOR_DANGER: &str = "#ef4444";
pub const COLOR_INFO: &str = "#3b82f6";

pub const FONT_FAMILY: &str =
    "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, \"Helvetica Neue\", sans-serif";
pub const FONT_MONO: &str =
    "\"JetBrains Mono\", \"Fira Code\", Consolas, \"Courier New\", monospace";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub level: ToastLevel,
    pub message: String,
    /// `SystemTime` ms since the epoch at creation. Used by
    /// `prune_toasts()` to drop the entry after its TTL.
    pub created_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

impl ConnectionStatus {
    pub fn color(self) -> &'static str {
        match self {
            ConnectionStatus::Disconnected => COLOR_TEXT_MUTED,
            ConnectionStatus::Connecting => COLOR_WARNING,
            ConnectionStatus::Connected => COLOR_SUCCESS,
            ConnectionStatus::Error => COLOR_DANGER,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConnectionStatus::Disconnected => "Offline",
            ConnectionStatus::Connecting => "Connecting",
            ConnectionStatus::Connected => "Online",
            ConnectionStatus::Error => "Error",
        }
    }
}

#[derive(Clone, Copy)]
pub struct VoltState {
    pub handle: Signal<Option<RuntimeHandle>>,
    pub connection: Signal<ConnectionStatus>,

    pub current_page: Signal<Page>,
    pub previous_page: Signal<Option<Page>>,

    pub model: Signal<String>,
    pub provider: Signal<String>,

    pub llm_online: Signal<bool>,
    pub db_connected: Signal<bool>,
    pub embedder_loaded: Signal<bool>,

    pub toasts: Signal<Vec<Toast>>,
    pub toast_counter: Signal<u64>,

    pub modal: Signal<Option<Modal>>,
    pub sidebar_collapsed: Signal<bool>,
    pub show_command_palette: Signal<bool>,
    pub show_trace_panel: Signal<bool>,

    pub total_events: Signal<u64>,
    pub total_commands: Signal<u64>,

    // Live data populated by events. Pages read these instead of
    // their own use_signal to stay in sync with the runtime.
    pub jobs: Signal<Vec<super::commands::JobInfo>>,
    pub routines: Signal<Vec<super::commands::RoutineInfo>>,
    pub skills: Signal<Vec<super::commands::SkillInfo>>,
    pub catalog_results: Signal<Vec<super::commands::CatalogSkillInfo>>,
    pub catalog_query: Signal<String>,
    pub mcp_servers: Signal<Vec<super::commands::McpServerInfo>>,
    pub audit_entries: Signal<Vec<super::commands::AuditEntry>>,
    pub tools: Signal<Vec<super::commands::ToolInfo>>,
    pub worktrees: Signal<Vec<super::commands::WorktreeInfo>>,
    pub workflows: Signal<Vec<super::commands::WorkflowInfo>>,
    pub models: Signal<Vec<super::commands::ModelInfo>>,
    pub doctor_report: Signal<Option<super::commands::DoctorReport>>,
    pub config: Signal<serde_json::Value>,
    pub tool_calls: Signal<Vec<super::commands::ToolCallInfo>>,

    // Chat state.
    pub chat_messages: Signal<Vec<super::commands::ChatMessage>>,
    pub chat_streaming: Signal<bool>,
    pub chat_session: Signal<Option<uuid::Uuid>>,

    /// Last message the user typed but the chat got cancelled or
    /// errored. Cleared on a successful send; restored to the
    /// textarea by the ChatPage when the runtime reports failure.
    pub last_user_draft: Signal<Option<String>>,

    // Sessions list cache (used by SessionsPage).
    pub sessions_cache: Signal<Vec<super::commands::SessionInfo>>,

    // First-run setup wizard. Populated by the `SetupNeeded` event;
    // cleared on `SetupReady`. The wizard is shown as a full-screen
    // overlay whenever this list is non-empty (or when `show_setup_wizard`
    // is true after the user dismisses but then opens it again).
    pub setup_providers: Signal<Vec<super::commands::ProviderInfo>>,
    pub show_setup_wizard: Signal<bool>,

    // Approval queue. The runtime emits an `ApprovalRequest` event
    // for every privileged tool call; we stash the request here so
    // the modal UI can render it and dispatch the user's Allow/Deny
    // response back through `UiCommand::ApprovalResponse`.
    pub pending_approvals: Signal<Vec<super::commands::ApprovalRequestInfo>>,

    /// Tracks whether the focused element is a text-entry field
    /// (input/textarea/contenteditable). The global keydown handler
    /// skips Ctrl/Cmd shortcuts when this is true so typing in
    /// the chat textarea doesn't trigger the command palette.
    pub focus_in_text_input: Signal<bool>,
}

/// Reserved for future modals. The current UI uses individual
/// boolean signals (`show_setup_wizard`, `show_command_palette`,
/// `pending_approvals`) so the `Modal` enum is unused but kept
/// in place for the next iteration that needs generic modal dispatch.
#[derive(Clone, Debug)]
pub enum Modal {
    NewSession { name: String },
    ImportSkill,
    InstallSkill { query: String },
    RunWorkflow {
        pattern: String,
        agents: String,
        tasks: String,
    },
    ToolExecute {
        name: String,
        schema: serde_json::Value,
        args: String,
    },
    About,
    Settings,
    Confirm {
        title: String,
        message: String,
        action: String,
    },
}

impl Default for VoltState {
    fn default() -> Self {
        Self {
            handle: Signal::new(None),
            connection: Signal::new(ConnectionStatus::Disconnected),
            current_page: Signal::new(Page::Dashboard),
            previous_page: Signal::new(None),
            model: Signal::new("llama-3.1-8b-instant".to_string()),
            provider: Signal::new("groq".to_string()),
            llm_online: Signal::new(false),
            db_connected: Signal::new(false),
            embedder_loaded: Signal::new(false),
            toasts: Signal::new(Vec::new()),
            toast_counter: Signal::new(0),
            modal: Signal::new(None),
            sidebar_collapsed: Signal::new(false),
            show_command_palette: Signal::new(false),
            show_trace_panel: Signal::new(false),
            total_events: Signal::new(0),
            total_commands: Signal::new(0),
            jobs: Signal::new(Vec::new()),
            routines: Signal::new(Vec::new()),
            skills: Signal::new(Vec::new()),
            catalog_results: Signal::new(Vec::new()),
            catalog_query: Signal::new(String::new()),
            mcp_servers: Signal::new(Vec::new()),
            audit_entries: Signal::new(Vec::new()),
            tools: Signal::new(Vec::new()),
            worktrees: Signal::new(Vec::new()),
            workflows: Signal::new(Vec::new()),
            models: Signal::new(Vec::new()),
            doctor_report: Signal::new(None),
            config: Signal::new(serde_json::Value::Null),
            tool_calls: Signal::new(Vec::new()),
            chat_messages: Signal::new(Vec::new()),
            chat_streaming: Signal::new(false),
            chat_session: Signal::new(None),
            last_user_draft: Signal::new(None),
            sessions_cache: Signal::new(Vec::new()),
            setup_providers: Signal::new(Vec::new()),
            show_setup_wizard: Signal::new(false),
            pending_approvals: Signal::new(Vec::new()),
            focus_in_text_input: Signal::new(false),
        }
    }
}

impl VoltState {
    pub fn toast(&mut self, level: ToastLevel, message: impl Into<String>) {
        let id = *self.toast_counter.read() + 1;
        self.toast_counter.set(id);
        let mut toasts = self.toasts.write();
        toasts.push(Toast {
            id,
            level,
            message: message.into(),
            created_at_ms: now_ms(),
        });
        if toasts.len() > 5 {
            toasts.remove(0);
        }
    }

    /// Drop toasts that are older than their TTL. Errors get 8 s;
    /// everything else gets 5 s. The `ToastContainer` calls this
    /// from a `use_future` ticker so we don't have to schedule one
    /// timer per toast.
    pub fn prune_toasts(&mut self) {
        let now = now_ms();
        let mut toasts = self.toasts.write();
        toasts.retain(|t| {
            let ttl_ms = match t.level {
                ToastLevel::Error => 8_000,
                _ => 5_000,
            };
            now.saturating_sub(t.created_at_ms) < ttl_ms
        });
    }

    pub fn dismiss_toast(&mut self, id: u64) {
        let mut toasts = self.toasts.write();
        toasts.retain(|t| t.id != id);
    }

    pub fn navigate(&mut self, page: Page) {
        let current = *self.current_page.read();
        if current != page {
            self.previous_page.set(Some(current));
            self.current_page.set(page);
        }
    }

    pub fn open_modal(&mut self, modal: Modal) {
        self.modal.set(Some(modal));
    }

    pub fn close_modal(&mut self) {
        self.modal.set(None);
    }

    pub async fn dispatch(&mut self, cmd: UiCommand) {
        let handle_opt = self.handle.read().clone();
        if let Some(handle) = handle_opt {
            let count = *self.total_commands.read() + 1;
            self.total_commands.set(count);
            if let Err(e) = handle.send(cmd).await {
                self.toast(ToastLevel::Error, format!("Dispatch failed: {}", e));
            }
        } else {
            self.toast(ToastLevel::Warning, "Runtime not ready");
        }
    }

    /// Fires a command without awaiting. Returns nothing so the closure type is `Fn`, not async.
    /// Increments the command counter and spawns the actual send on the tokio runtime
    /// (detached from the Dioxus component scope so commands aren't lost on navigation).
    pub fn fire(&mut self, cmd: UiCommand) {
        let handle_opt = self.handle.read().clone();
        let count = *self.total_commands.read() + 1;
        self.total_commands.set(count);
        if let Some(handle) = handle_opt {
            tokio::spawn(async move {
                if let Err(e) = handle.send(cmd).await {
                    // The runtime's broadcast is gone (the only way send
                    // fails). Log and move on; there's no toast path
                    // from a detached task.
                    tracing::error!("[webui] dispatch failed: {}", e);
                }
            });
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
