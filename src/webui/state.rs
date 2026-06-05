use dioxus::prelude::*;
use super::commands::UiCommand;
use super::routes::Page;
use super::runtime::RuntimeHandle;

pub const SIDEBAR_WIDTH: u32 = 240;
pub const HEADER_HEIGHT: u32 = 56;
pub const COLOR_BG: &str = "#0a0a14";
pub const COLOR_PANEL: &str = "#14142a";
pub const COLOR_PANEL_HOVER: &str = "#1a1a35";
pub const COLOR_BORDER: &str = "#25254a";
pub const COLOR_TEXT: &str = "#e6e6f0";
pub const COLOR_TEXT_DIM: &str = "#9090a8";
pub const COLOR_TEXT_MUTED: &str = "#5a5a78";
pub const COLOR_ACCENT: &str = "#a855f7";
pub const COLOR_ACCENT_HOVER: &str = "#9333ea";
pub const COLOR_SUCCESS: &str = "#22c55e";
pub const COLOR_WARNING: &str = "#f59e0b";
pub const COLOR_DANGER: &str = "#ef4444";
pub const COLOR_INFO: &str = "#3b82f6";

pub const FONT_FAMILY: &str = "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, \"Helvetica Neue\", sans-serif";
pub const FONT_MONO: &str = "\"JetBrains Mono\", \"Fira Code\", Consolas, \"Courier New\", monospace";

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
    pub pending_approval: Signal<Option<super::commands::UiEvent>>,

    pub last_event_at: Signal<i64>,
    pub total_events: Signal<u64>,
    pub total_commands: Signal<u64>,
}

#[derive(Clone, Debug)]
pub enum Modal {
    NewSession { name: String },
    ImportSkill,
    InstallSkill { query: String },
    RunWorkflow { pattern: String, agents: String, tasks: String },
    ToolExecute { name: String, schema: serde_json::Value, args: String },
    About,
    Settings,
    Confirm { title: String, message: String, action: String },
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
            pending_approval: Signal::new(None),
            last_event_at: Signal::new(0),
            total_events: Signal::new(0),
            total_commands: Signal::new(0),
        }
    }
}

impl VoltState {
    pub fn toast(&mut self, level: ToastLevel, message: impl Into<String>) {
        let id = *self.toast_counter.read() + 1;
        self.toast_counter.set(id);
        let mut toasts = self.toasts.write();
        toasts.push(Toast { id, level, message: message.into() });
        if toasts.len() > 5 {
            toasts.remove(0);
        }
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
    /// Increments the command counter and spawns the actual send on the runtime.
    pub fn fire(&mut self, cmd: UiCommand) {
        let handle_opt = self.handle.read().clone();
        let count = *self.total_commands.read() + 1;
        self.total_commands.set(count);
        if let Some(handle) = handle_opt {
            dioxus::prelude::spawn(async move {
                if let Err(e) = handle.send(cmd).await {
                    tracing::warn!("Dispatch failed: {}", e);
                }
            });
        }
    }
}
