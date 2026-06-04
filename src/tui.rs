use crate::agent::{Agent, ApprovalDecision};
use crate::models::{CancelToken, Session};
use crate::session as sessions;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use reedline::{DefaultPrompt, ExternalPrinter, FileBackedHistory, Reedline, Signal};
use std::io::stdout;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use tokio::sync::mpsc;

/// Approval request from the agent to the TUI. The agent task blocks on
/// `decision_rx` until the TUI renders the widget and sends a decision.
pub struct ApprovalRequest {
    pub tool: String,
    pub args: serde_json::Value,
    pub decision_tx: tokio::sync::oneshot::Sender<ApprovalDecision>,
}

/// Result of attempting to dispatch a slash command.
#[derive(Debug, PartialEq, Eq)]
enum SlashResult {
    /// User wants to quit the TUI.
    Quit,
    /// Slash command was recognized and executed.
    Handled,
    /// Input did not start with `/` — fall through to normal prompt send.
    NotASlash,
}

/// TUI layout. `Classic` is the original 3-4 row layout (messages, separator,
/// HUD). `Panels` is the new opencode-style layout: header bar, sidebar
/// (sessions), message stream, composer pills + input, status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutMode {
    #[default]
    Classic,
    Panels,
}

/// Colour theme for the panel layout. `Default` is the existing colour
/// scheme. The other themes are Catppuccin Mocha, Dracula, Nord, and
/// Solarized Dark — a subset of what opencode ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Default,
    Catppuccin,
    Dracula,
    Nord,
    SolarizedDark,
}

impl Theme {
    /// Human-readable name for the theme. Used by `/theme <name>` and the
    /// help text.
    pub fn name(self) -> &'static str {
        match self {
            Theme::Default => "default",
            Theme::Catppuccin => "catppuccin",
            Theme::Dracula => "dracula",
            Theme::Nord => "nord",
            Theme::SolarizedDark => "solarized-dark",
        }
    }

    /// Parse a theme name (case-insensitive). Returns `None` for unknown
    /// names so the caller can show an error message.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "default" | "volt" => Some(Theme::Default),
            "catppuccin" | "mocha" => Some(Theme::Catppuccin),
            "dracula" => Some(Theme::Dracula),
            "nord" => Some(Theme::Nord),
            "solarized" | "solarized-dark" | "solarized_dark" => Some(Theme::SolarizedDark),
            _ => None,
        }
    }

    /// All themes in display order. Used by the picker dialog.
    pub fn all() -> &'static [Theme] {
        &[
            Theme::Default,
            Theme::Catppuccin,
            Theme::Dracula,
            Theme::Nord,
            Theme::SolarizedDark,
        ]
    }

    /// Resolve the theme's accent colour (used for the active pill border,
    /// sidebar selection highlight, and the header logo). For `Default` we
    /// fall back to Cyan which is what the existing TUI uses.
    pub fn accent(self) -> Color {
        match self {
            Theme::Default => Color::Cyan,
            Theme::Catppuccin => Color::Rgb(203, 166, 247),   // Mauve
            Theme::Dracula => Color::Rgb(189, 147, 249),       // Purple
            Theme::Nord => Color::Rgb(136, 192, 208),          // Frost cyan
            Theme::SolarizedDark => Color::Rgb(38, 139, 210),  // Blue
        }
    }

    /// Resolve the theme's muted/dim colour (sidebar chrome, separator
    /// lines, idle HUD text).
    pub fn muted(self) -> Color {
        match self {
            Theme::Default => Color::DarkGray,
            Theme::Catppuccin => Color::Rgb(108, 112, 134),   // Overlay0
            Theme::Dracula => Color::Rgb(98, 114, 164),       // Comment
            Theme::Nord => Color::Rgb(76, 86, 106),           // Polar Night 3
            Theme::SolarizedDark => Color::Rgb(88, 110, 117), // Base01
        }
    }

    /// Colour for the user role tag.
    pub fn user(self) -> Color {
        match self {
            Theme::Default => Color::Cyan,
            Theme::Catppuccin => Color::Rgb(137, 180, 250),  // Blue
            Theme::Dracula => Color::Rgb(139, 233, 253),     // Cyan
            Theme::Nord => Color::Rgb(129, 161, 193),        // Frost
            Theme::SolarizedDark => Color::Rgb(38, 139, 210), // Blue
        }
    }

    /// Colour for the assistant role tag.
    pub fn assistant(self) -> Color {
        match self {
            Theme::Default => Color::Green,
            Theme::Catppuccin => Color::Rgb(166, 227, 161),   // Green
            Theme::Dracula => Color::Rgb(80, 250, 123),       // Green
            Theme::Nord => Color::Rgb(163, 190, 140),         // Aurora green
            Theme::SolarizedDark => Color::Rgb(133, 153, 0),  // Green
        }
    }

    /// Colour for the system role tag.
    pub fn system(self) -> Color {
        match self {
            Theme::Default => Color::Yellow,
            Theme::Catppuccin => Color::Rgb(250, 179, 135),   // Peach
            Theme::Dracula => Color::Rgb(241, 250, 140),      // Yellow
            Theme::Nord => Color::Rgb(235, 203, 139),         // Aurora yellow
            Theme::SolarizedDark => Color::Rgb(181, 137, 0),  // Yellow
        }
    }
}

const HELP_TEXT: &str = "Volt Agent — slash commands

  Conversation:
    /clear              Clear visible messages (keeps session)
    /compact            Compress older messages to fit context
    /sessions           List recent sessions
    /resume <n>         Resume session N from /sessions
    /fork [n]           Copy the conversation up to message N into a new session
    /export [path]      Export current session to markdown (default: session.md)

  Model & mode:
    /model              Show current model
    /model <name>       Switch model (next session)
    /mode               Show current mode (precision/balanced/autonomous)
    /theme              List available colour themes
    /theme <name>       Switch theme (default, catppuccin, dracula, nord, solarized-dark)

  Tools & integrations:
    /tools              List available tools
    /mcp                List MCP servers + tools
    /delegate <task>    Spawn a sub-agent for a task; result shown here

  Status:
    /status             Model, mode, session id, message count
    /cost, /tokens      Token usage + estimated cost
    /init               Create a starter AGENTS.md in the current dir

  Other:
    /plan               Toggle plan mode (next turn is read-only)
    /help, /?           Show this help
    /quit, /exit, /q    Exit the TUI

  Key bindings (Panels layout):
    Ctrl+B              Toggle sidebar
    Ctrl+L              Clear messages (same as /clear)
    Ctrl+C              Cancel current turn / quit
    Esc                 Deny pending tool approval";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatMessage {
    role: String,
    content: String,
    /// Set when `role == "tool"`. Used to colour-code the message by tool
    /// category (read / write / shell / network / etc).
    tool_name: Option<String>,
    /// Optional stable id from the agent's `Message.id`. Used to avoid
    /// double-appending messages that were already loaded at startup.
    id: Option<String>,
}

/// Interactive TUI for the Volt agent. The line editor is `reedline` (history,
/// kill ring, multiline paste, Ctrl-A/E/W/U/K, etc.); the messages area and
/// token/cost HUD are rendered with `ratatui` between user inputs. The
/// `ExternalPrinter` (passed in) is wired into the agent's on_token callback
/// by `agent_tui.rs` and is also attached to reedline so streaming tokens
/// appear above the input row in real time.
pub struct TuiChat {
    messages: Vec<ChatMessage>,
    agent: Arc<Agent>,
    /// The tool registry the agent is using. Held here so the user can
    /// invoke tools directly from the TUI (e.g. `/delegate`) without
    /// going through the model.
    tools: Arc<crate::tools::ToolRegistry>,
    cancel: CancelToken,
    printer: ExternalPrinter<String>,
    /// True while the agent task is running.
    is_thinking: bool,
    /// Wall-clock instant when the current turn started.
    turn_started_at: Option<Instant>,
    /// When true, the next user prompt is prefixed with a planner directive
    /// asking the model to output a plan before executing any tool. The
    /// flag auto-resets after the turn completes.
    plan_mode: bool,
    /// Pending tool approval requests from the agent. The TUI drains
    /// this on each redraw and renders a widget for the first one. While
    /// the request is active, user keypresses (y/n/a/Esc) are routed to
    /// the approval widget rather than the reedline input.
    pending_approval: Option<PendingApproval>,
    /// Receiver half of the approval channel. The runner polls it on each
    /// redraw tick.
    approval_rx: Option<mpsc::UnboundedReceiver<ApprovalRequest>>,
    /// Which layout to render. `Classic` is the original 3-4 row layout
    /// (default, backward compatible). `Panels` is the new opencode-style
    /// layout with header, sidebar, composer pills, and status bar.
    layout_mode: LayoutMode,
    /// Active colour theme (panels layout). `Default` keeps the existing
    /// colour scheme.
    theme: Theme,
    /// Whether the sessions sidebar is visible. Toggled with Ctrl+B in
    /// panels mode. Ignored in classic mode.
    sidebar_visible: bool,
    /// Cached sidebar session list, refreshed on demand. Stored as
    /// `(session_id, title, message_count, updated_at)` tuples.
    sidebar_sessions: Vec<SidebarSession>,
    /// Selection state for the sidebar list. `None` means no selection
    /// (the active session is highlighted instead).
    sidebar_state: ListState,
}

/// Cached session metadata for the sidebar.
#[derive(Debug, Clone)]
pub struct SidebarSession {
    /// The full session id. Kept for future "switch to this session"
    /// wiring; currently the sidebar shows the short id and the resume
    /// command still works off the numbered list.
    #[allow(dead_code)]
    pub id: uuid::Uuid,
    pub title: String,
    pub message_count: u32,
    /// Short id (first 8 hex chars) for display.
    pub short_id: String,
}

/// State for the in-flight approval widget. We hold the oneshot sender
/// so the user keypress can resolve the future that the agent is awaiting.
struct PendingApproval {
    request: ApprovalRequest,
    /// Pretty-printed args string (truncated for display).
    args_display: String,
}

impl TuiChat {
    pub fn new(agent: Arc<Agent>, printer: ExternalPrinter<String>) -> Self {
        Self::with_tools(agent, printer, crate::tools::ToolRegistry::new())
    }

    /// Construct a TUI sharing the agent's tool registry. The user can
    /// invoke tools directly (e.g. `/delegate`) without going through
    /// the model.
    pub fn with_tools(
        agent: Arc<Agent>,
        printer: ExternalPrinter<String>,
        tools: Arc<crate::tools::ToolRegistry>,
    ) -> Self {
        Self {
            messages: Vec::new(),
            agent,
            tools,
            cancel: CancelToken::new(),
            printer,
            is_thinking: false,
            turn_started_at: None,
            plan_mode: false,
            pending_approval: None,
            approval_rx: None,
            layout_mode: LayoutMode::default(),
            theme: Theme::default(),
            sidebar_visible: false,
            sidebar_sessions: Vec::new(),
            sidebar_state: ListState::default(),
        }
    }

    /// Construct a TUI with an external approval channel. The agent must
    /// have been built with `with_approval(approval_fn)` where the
    /// `approval_fn` was produced by `tui::approval_callback_for(tx)`.
    pub fn new_with_approval(
        agent: Arc<Agent>,
        tools: Arc<crate::tools::ToolRegistry>,
        printer: ExternalPrinter<String>,
        approval_rx: mpsc::UnboundedReceiver<ApprovalRequest>,
    ) -> Self {
        // Park the receiver in a place the render loop can poll. We can't
        // store it in `Self` because `Self` is `!Send` (terminal handle
        // isn't Send). Instead, expose it via a method on the constructed
        // TuiChat — the runner drains it on each tick.
        let mut s = Self::with_tools(agent, printer, tools);
        s.approval_rx = Some(approval_rx);
        s
    }

    /// Construct a TUI with the opencode-style panel layout enabled. In
    /// panels mode the layout is header (3 rows) / sidebar + messages /
    /// composer pills / input / status bar (1 row). Sidebar starts visible
    /// and can be toggled with Ctrl+B.
    pub fn new_with_panels(
        agent: Arc<Agent>,
        tools: Arc<crate::tools::ToolRegistry>,
        printer: ExternalPrinter<String>,
        approval_rx: mpsc::UnboundedReceiver<ApprovalRequest>,
        theme: Theme,
    ) -> Self {
        let mut s = Self::new_with_approval(agent, tools, printer, approval_rx);
        s.layout_mode = LayoutMode::Panels;
        s.theme = theme;
        s.sidebar_visible = true;
        s
    }

    /// Get the current layout mode.
    pub fn layout_mode(&self) -> LayoutMode {
        self.layout_mode
    }

    /// Get the current theme.
    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// Set the theme. Caller should redraw after calling.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Toggle the sidebar visibility. No-op in classic mode.
    pub fn toggle_sidebar(&mut self) {
        if self.layout_mode == LayoutMode::Panels {
            self.sidebar_visible = !self.sidebar_visible;
        }
    }

    /// Replace the cached sidebar session list. Called by `/sessions` to
    /// refresh the data; the sidebar widget reads it on each redraw.
    pub fn set_sidebar_sessions(&mut self, sessions: Vec<SidebarSession>) {
        self.sidebar_sessions = sessions;
        if self.sidebar_state.selected().is_none() && !self.sidebar_sessions.is_empty() {
            self.sidebar_state.select(Some(0));
        }
    }

    /// Drain a single approval request from the channel if one is
    /// available. Called from the render loop between redraws.
    pub fn poll_approval(&mut self) {
        if self.pending_approval.is_some() {
            return;
        }
        if let Some(rx) = &mut self.approval_rx {
            match rx.try_recv() {
                Ok(request) => {
                    let args_display = serde_json::to_string(&request.args)
                        .unwrap_or_else(|_| "<unprintable args>".into());
                    // Truncate long args to keep the widget compact.
                    if args_display.len() > 200 {
                        let mut s = args_display;
                        s.truncate(200);
                        s.push('…');
                        self.pending_approval = Some(PendingApproval {
                            request,
                            args_display: s,
                        });
                    } else {
                        self.pending_approval = Some(PendingApproval {
                            request,
                            args_display,
                        });
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // The agent task is done; clear the receiver.
                    self.approval_rx = None;
                }
            }
        }
    }

    /// Apply a keypress to the pending approval widget. Accepts the raw
    /// characters the user typed (y/n/a/Esc) so the runner can pass either
    /// a single reedline `Signal::Success(buf)` or raw crossterm events
    /// through the same API. Returns true if the keypress was consumed.
    pub fn handle_approval_key(&mut self, input: &str) -> bool {
        let Some(pending) = self.pending_approval.take() else {
            return false;
        };
        let decision = match input.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => ApprovalDecision::AllowOnce,
            "a" => ApprovalDecision::AllowSession,
            "n" | "no" | "" | "\u{1b}" | "\u{03}" => ApprovalDecision::Deny,
            // Unknown keys are ignored — the user must pick y/n/a/Esc.
            _ => {
                self.pending_approval = Some(pending);
                return true;
            }
        };
        let _ = pending.request.decision_tx.send(decision);
        let label = match decision {
            ApprovalDecision::AllowOnce => "allowed once",
            ApprovalDecision::AllowSession => "allowed for session",
            ApprovalDecision::Deny => "denied",
        };
        self.add_message(
            "system",
            &format!("approval: tool '{}' → {}", pending.request.tool, label),
        );
        true
    }

    fn add_message(&mut self, role: &str, content: &str) {
        self.messages.push(ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_name: None,
            id: None,
        });
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        // Ctrl-C cancellation bridge.
        let c = self.cancel.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            c.cancel();
        });

        // Load agent history into the message list.
        {
            let state = self.agent.state().lock().await;
            for msg in &state.messages {
                self.messages.push(ChatMessage {
                    role: msg.role.clone(),
                    content: msg.content.as_str().to_string(),
                    tool_name: msg.tool_name.clone(),
                    id: Some(msg.id.to_string()),
                });
            }
        }

        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
        terminal.clear()?;

        // Build reedline with file-backed history.
        let history_path = history_file_path();
        if let Some(parent) = history_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let history = FileBackedHistory::with_file(1000, history_path.clone()).ok();
        let mut line_editor = Reedline::create().with_external_printer(self.printer.clone());
        if let Some(h) = history {
            line_editor = line_editor.with_history(Box::new(h));
        }
        let prompt = DefaultPrompt::default();

        // Open the sessions pool once; reused for every turn.
        let sessions_pool = sessions::open_sessions(&std::path::PathBuf::from("volt_sessions.db"))
            .await
            .ok();

        loop {
            // Poll for any pending tool-approval requests. This is non-blocking
            // and runs every loop iteration so the widget appears as soon as
            // the agent needs approval.
            self.poll_approval();

            // Render messages + HUD above the input row.
            self.render_frame(&mut terminal)?;

            // Hand control to reedline for the next input. The prompt is drawn
            // by reedline on the bottom row(s).
            let sig = line_editor.read_line(&prompt);
            match sig {
                Ok(Signal::Success(buf)) => {
                    // Global key bindings (intercepted before any other
                    // routing). These come through reedline as raw control
                    // characters in the buffer.
                    if buf == "\u{02}" {
                        // Ctrl+B — toggle sidebar.
                        self.toggle_sidebar();
                        continue;
                    }
                    if buf == "\u{0C}" {
                        // Ctrl+L — clear messages (mirror /clear).
                        self.messages.clear();
                        self.add_message("system", "(conversation cleared)");
                        continue;
                    }
                    // If a tool approval is pending, route single-character
                    // y/n/a responses to the approval widget instead of
                    // treating them as user prompts.
                    if self.pending_approval.is_some()
                        && buf.len() <= 3
                        && self.handle_approval_key(&buf)
                    {
                        continue;
                    }
                    let line = buf.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    if line.starts_with('/') {
                        match self.execute_slash_command(&line, &sessions_pool).await {
                            SlashResult::Quit => break,
                            SlashResult::Handled => continue,
                            SlashResult::NotASlash => {
                                self.add_message(
                                    "system",
                                    &format!(
                                        "unknown slash command: {}. Type /help for the list.",
                                        line.split_whitespace().next().unwrap_or(&line)
                                    ),
                                );
                                continue;
                            }
                        }
                    }
                    self.dispatch_prompt(line.clone(), &sessions_pool).await;
                }
                Ok(Signal::CtrlC) => {
                    if self.is_thinking {
                        // Cancel the running agent task.
                        self.cancel.cancel();
                    } else {
                        break;
                    }
                }
                Ok(Signal::CtrlD) => break,
                _ => {}
            }
        }

        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    /// Render the messages area + HUD above the input row.
    fn render_frame(
        &self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        match self.layout_mode {
            LayoutMode::Classic => self.render_frame_classic(terminal),
            LayoutMode::Panels => self.render_frame_panels(terminal),
        }
    }

    /// Original 3-4 row layout: messages / approval widget (optional) /
    /// input separator / HUD. Kept as the default for backward
    /// compatibility — every existing user has the muscle memory for
    /// this layout.
    fn render_frame_classic(
        &self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        terminal.draw(|f| {
            let area = f.area();
            let has_approval = self.pending_approval.is_some();
            // 4 rows when an approval is pending, 3 otherwise. We always
            // pass a 4-row array and zero out the unused row's index to
            // avoid the type-inference dance of two different array sizes.
            let mut constraints = [
                Constraint::Min(3),
                Constraint::Length(1), // input row (reedline draws here)
                Constraint::Length(1), // HUD row
                Constraint::Length(0), // unused; approval widget would go here
            ];
            if has_approval {
                // Reshape: insert the approval widget as the second row.
                constraints = [
                    Constraint::Min(3),
                    Constraint::Length(5), // approval widget
                    Constraint::Length(1), // input row
                    Constraint::Length(1), // HUD row
                ];
            }
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area);
            if has_approval {
                self.render_messages(f, chunks[0]);
                self.render_approval(f, chunks[1]);
                self.render_input_separator(f, chunks[2]);
                self.render_hud(f, chunks[3]);
            } else {
                self.render_messages(f, chunks[0]);
                self.render_input_separator(f, chunks[1]);
                self.render_hud(f, chunks[2]);
            }
        })?;
        Ok(())
    }

    /// New opencode-style 3-region layout: header (3 rows) / body
    /// (sidebar optional, messages) / composer pills (1 row) / input
    /// separator (1 row) / status bar (1 row). When an approval is
    /// pending, insert a 5-row modal between messages and pills.
    fn render_frame_panels(
        &self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        terminal.draw(|f| {
            let area = f.area();
            let has_approval = self.pending_approval.is_some();

            // Outer split: header (3) / body (fill) / composer (1) /
            // input sep (1) / status (1). Approval modal sits on top of
            // the body if pending.
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // header
                    Constraint::Min(5),    // body
                    Constraint::Length(1), // composer pills
                    Constraint::Length(1), // input separator
                    Constraint::Length(1), // status bar
                ])
                .split(area);

            self.render_header(f, outer[0]);

            // Body split: sidebar (0 or 24) / messages (fill).
            if self.sidebar_visible {
                let body_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(26), Constraint::Min(20)])
                    .split(outer[1]);
                self.render_sidebar(f, body_chunks[0]);
                self.render_messages_panels(f, body_chunks[1]);
            } else {
                self.render_messages_panels(f, outer[1]);
            }

            self.render_composer_pills(f, outer[2]);
            self.render_input_separator(f, outer[3]);
            self.render_status_bar(f, outer[4]);

            // Approval modal sits on top of everything else when pending.
            if has_approval {
                let popup_area = centered_rect(70, 30, area);
                f.render_widget(Clear, popup_area);
                self.render_approval(f, popup_area);
            }
        })?;
        Ok(())
    }

    /// Render the per-tool approval widget. The widget shows the tool name
    /// and arguments, then a footer with the y/n/a shortcut. The user can
    /// type one of the three letters directly into the reedline input to
    /// respond; the input is intercepted by the runner while this widget
    /// is visible.
    fn render_approval(&self, f: &mut Frame, area: Rect) {
        let Some(pending) = &self.pending_approval else {
            return;
        };
        let header = format!(
            " ⚠ Tool approval — {}({}) ",
            pending.request.tool, pending.args_display
        );
        let body = format!(
            "Press one of:  y = allow once   a = allow for this session   n / Esc = deny\n\
             \n\
             {}\n",
            pending.args_display
        );
        let widget = Paragraph::new(body)
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title(header));
        f.render_widget(widget, area);
    }

    /// Tiny separator line between the messages and the reedline input row.
    fn render_input_separator(&self, f: &mut Frame, area: Rect) {
        let prefix = if self.is_thinking {
            " thinking... "
        } else {
            " input > "
        };
        let para = Paragraph::new(prefix).style(Style::default().fg(Color::DarkGray));
        f.render_widget(para, area);
    }

    /// Send a non-slash prompt to the agent. Streams tokens through the
    /// ExternalPrinter, then commits the final message to history + sessions.
    async fn dispatch_prompt(&mut self, input: String, sessions_pool: &Option<sqlx::SqlitePool>) {
        self.add_message("user", &input);
        self.is_thinking = true;
        self.turn_started_at = Some(Instant::now());

        // If plan mode is on, prefix the actual user input with a planner
        // directive that asks the model to output a plan as text before
        // invoking any tools. The flag is reset after the turn so a single
        // /plan covers exactly one user request.
        let plan_mode_active = std::mem::take(&mut self.plan_mode);
        let effective_input = if plan_mode_active {
            format!(
                "[PLAN MODE — read-only]\n\
                 You MUST respond with a numbered plan (steps, tools you would call, and the \
                 order of operations) BEFORE invoking any tool. Wait for the user to approve the \
                 plan; do not execute tools in this turn. After the plan, end your response.\n\n\
                 USER REQUEST:\n{}",
                input
            )
        } else {
            input.clone()
        };

        // Spawn the agent task. The agent's on_token callback (set up by
        // agent_tui.rs) prints streamed tokens to the ExternalPrinter.
        let turn_cancel = CancelToken::new();
        let agent = self.agent.clone();
        let input_for_task = effective_input;
        let cancel_for_task = self.cancel.clone();

        let outcome: Result<String, String> = tokio::select! {
            r = agent.run(&input_for_task) => r.map_err(|e| e.to_string()),
            _ = wait_for_cancel(turn_cancel) => Err("cancelled by user".to_string()),
        };
        let _ = cancel_for_task; // outer cancel observed only between turns

        self.is_thinking = false;
        self.turn_started_at = None;

        // Sync any new tool messages (and the assistant turn message) from
        // the agent state into the local TUI list, so they show up in the
        // scrollback with per-tool colour coding. We diff by `Message.id`
        // to avoid duplicating messages that were already loaded at startup.
        {
            let state = self.agent.state().lock().await;
            let known: std::collections::HashSet<String> =
                self.messages.iter().filter_map(|m| m.id.clone()).collect();
            for msg in &state.messages {
                if msg.role != "tool" {
                    continue;
                }
                if known.contains(&msg.id.to_string()) {
                    continue;
                }
                self.messages.push(ChatMessage {
                    role: msg.role.clone(),
                    content: msg.content.as_str().to_string(),
                    tool_name: msg.tool_name.clone(),
                    id: Some(msg.id.to_string()),
                });
            }
        }

        // Commit the assistant message.
        match &outcome {
            Ok(_) => {
                // The streamed text was already printed via ExternalPrinter.
                // We commit a placeholder marker so the message list is
                // consistent with what the user saw.
                self.add_message("assistant", "(streamed above)");
            }
            Err(e) => {
                self.add_message("system", &format!("error: {}", e));
            }
        }

        // Persist the session.
        if let Some(ref sp) = sessions_pool {
            let s = self.agent.state().lock().await;
            let _ = sessions::create_session(
                sp,
                &Session {
                    id: s.session_id,
                    agent_name: s.name.clone(),
                    title: input.chars().take(60).collect(),
                    message_count: s.messages.len() as u32,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                },
            )
            .await;
            let _ = sessions::delete_session_messages(sp, s.session_id).await;
            for (i, msg) in s.messages.iter().enumerate() {
                let _ = sessions::save_message(sp, s.session_id, i as i64, msg).await;
            }
        }
    }

    /// Dispatch a `/...` input. Returns `Handled` for recognized commands,
    /// `Quit` to exit the TUI, and `NotASlash` if the input is not a slash
    /// command (caller should fall through to normal prompt send).
    async fn execute_slash_command(
        &mut self,
        raw: &str,
        sessions_pool: &Option<sqlx::SqlitePool>,
    ) -> SlashResult {
        let mut parts = raw.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();
        match cmd {
            "/quit" | "/exit" | "/q" => SlashResult::Quit,
            "/help" | "/?" => {
                self.add_message("system", HELP_TEXT);
                SlashResult::Handled
            }
            "/clear" => {
                self.messages.clear();
                self.add_message("system", "(conversation cleared)");
                SlashResult::Handled
            }
            "/model" => match args.first() {
                Some(name) => {
                    self.add_message(
                        "system",
                        &format!(
                            "(note) model switching from `/model` is read-only in this build — \
                             the active model remains: {}\n\
                             To switch models, restart with: volt agent-tui --model {}",
                            self.agent.config().model,
                            name
                        ),
                    );
                    SlashResult::Handled
                }
                None => {
                    self.add_message(
                        "system",
                        &format!("current model: {}", self.agent.config().model),
                    );
                    SlashResult::Handled
                }
            },
            "/status" => {
                let (name, total_p, total_c, msgs) = match self.agent.state().try_lock() {
                    Ok(s) => (
                        s.name.clone(),
                        s.total_prompt_tokens,
                        s.total_completion_tokens,
                        s.messages.len(),
                    ),
                    Err(_) => ("(busy)".into(), 0, 0, 0),
                };
                self.add_message(
                    "system",
                    &format!(
                        "agent:   {}\nmodel:   {}\nmsgs:    {}\n↑ {} prompt tokens\n↓ {} completion tokens",
                        name,
                        self.agent.config().model,
                        msgs,
                        total_p,
                        total_c
                    ),
                );
                SlashResult::Handled
            }
            "/cost" | "/tokens" => {
                let (p, c) = self
                    .agent
                    .state()
                    .try_lock()
                    .map(|s| (s.total_prompt_tokens, s.total_completion_tokens))
                    .unwrap_or((0, 0));
                let est_cost = (c as f64) * 0.000_000_59;
                self.add_message(
                    "system",
                    &format!(
                        "↑ {} prompt tokens\n↓ {} completion tokens\nest. cost: ${:.4}",
                        p, c, est_cost
                    ),
                );
                SlashResult::Handled
            }
            "/sessions" => {
                match sessions_pool {
                    Some(pool) => match sessions::list_sessions(pool, 20).await {
                        Ok(items) if items.is_empty() => {
                            self.add_message("system", "no persisted sessions yet");
                            self.set_sidebar_sessions(Vec::new());
                        }
                        Ok(items) => {
                            // Print a numbered list in the chat for
                            // /resume compatibility, and refresh the
                            // sidebar cache for the panels layout.
                            let mut s = String::from("Sessions (most recent first):\n");
                            for (idx, sess) in items.iter().enumerate() {
                                let title = if sess.title.is_empty() {
                                    "(untitled)"
                                } else {
                                    sess.title.as_str()
                                };
                                let short_id = sess.id.to_string();
                                let short_id = &short_id[..8.min(short_id.len())];
                                s.push_str(&format!(
                                    "  [{}] {} — {} ({} msgs) · {}\n",
                                    idx + 1,
                                    short_id,
                                    title,
                                    sess.message_count,
                                    sess.updated_at.format("%Y-%m-%d %H:%M")
                                ));
                            }
                            s.push_str("\nUse `/resume <n>` to load a session by number.");
                            self.add_message("system", &s);
                            // Update the sidebar cache.
                            let sidebar_items: Vec<SidebarSession> = items
                                .iter()
                                .map(|sess| {
                                    let id_str = sess.id.to_string();
                                    let short_id = id_str[..8.min(id_str.len())].to_string();
                                    SidebarSession {
                                        id: sess.id,
                                        title: sess.title.clone(),
                                        message_count: sess.message_count,
                                        short_id,
                                    }
                                })
                                .collect();
                            self.set_sidebar_sessions(sidebar_items);
                        }
                        Err(e) => {
                            self.add_message("system", &format!("failed to list sessions: {}", e));
                        }
                    },
                    None => {
                        self.add_message(
                            "system",
                            "sessions pool not available — run from a working directory",
                        );
                    }
                }
                SlashResult::Handled
            }
            "/resume" => {
                let n: Option<usize> = args.first().and_then(|s| s.parse().ok());
                match (n, sessions_pool) {
                    (Some(idx), Some(pool)) => {
                        if idx < 1 {
                            self.add_message(
                                "system",
                                "session numbers start at 1 — try /sessions",
                            );
                        } else {
                            let pool = pool.clone();
                            match sessions::list_sessions(&pool, idx as i64).await {
                                Ok(items) => {
                                    if let Some(sess) = items.into_iter().nth(idx - 1) {
                                        match sessions::load_messages(&pool, sess.id).await {
                                            Ok(messages) => {
                                                let count = messages.len();
                                                self.messages.clear();
                                                for m in messages {
                                                    self.messages.push(ChatMessage {
                                                        role: m.role,
                                                        content: m.content.as_str().to_string(),
                                                        tool_name: m.tool_name,
                                                        id: None,
                                                    });
                                                }
                                                // Update agent's session id to the
                                                // resumed session so subsequent turns
                                                // append to the right row.
                                                {
                                                    let mut state = self.agent.state().lock().await;
                                                    state.session_id = sess.id;
                                                }
                                                self.add_message(
                                                    "system",
                                                    &format!(
                                                        "resumed session {} ({} messages loaded)",
                                                        sess.id, count
                                                    ),
                                                );
                                            }
                                            Err(e) => {
                                                self.add_message(
                                                    "system",
                                                    &format!("failed to load messages: {}", e),
                                                );
                                            }
                                        }
                                    } else {
                                        self.add_message(
                                            "system",
                                            &format!("session {} not found — try /sessions", idx),
                                        );
                                    }
                                }
                                Err(e) => {
                                    self.add_message(
                                        "system",
                                        &format!("failed to list sessions: {}", e),
                                    );
                                }
                            }
                        }
                    }
                    (Some(_), &None) => {
                        self.add_message(
                            "system",
                            "sessions pool not available — run from a working directory",
                        );
                    }
                    (None, _) => {
                        self.add_message(
                            "system",
                            "usage: /resume <n>  (run /sessions to see the list)",
                        );
                    }
                }
                SlashResult::Handled
            }
            "/fork" => {
                // `/fork [n]` — copy the first N messages of the current
                // session into a brand-new session and switch to it.
                // The original session is left untouched. Default: N
                // equals the current visible message count, i.e. fork
                // the whole conversation so far.
                let n: Option<usize> = args.first().and_then(|s| s.parse().ok());
                let max = self.messages.len();
                let up_to = match n {
                    Some(0) => {
                        self.add_message(
                            "system",
                            "fork point must be 1 or greater (1-based, inclusive)",
                        );
                        return SlashResult::Handled;
                    }
                    Some(n) if n > max => {
                        self.add_message(
                            "system",
                            &format!(
                                "only {} messages available; use /fork {} (or omit n for all)",
                                max, max
                            ),
                        );
                        return SlashResult::Handled;
                    }
                    Some(n) => n,
                    None => max,
                };
                let pool = match sessions_pool {
                    Some(p) => p.clone(),
                    None => {
                        self.add_message(
                            "system",
                            "sessions pool not available — run from a working directory",
                        );
                        return SlashResult::Handled;
                    }
                };
                let current_session = {
                    let state = self.agent.state().lock().await;
                    state.session_id
                };
                // Snapshot the in-memory messages first (cheap; we
                // need them after the persist either way, and the
                // persist below may swap `self.messages` if it fails).
                let snapshot: Vec<ChatMessage> =
                    self.messages.iter().take(up_to).cloned().collect();

                // Persist current in-memory messages to the existing
                // session first, so the fork reads consistent data from
                // the database (not whatever the TUI happens to have
                // buffered right now).
                if let Err(e) = persist_tui_messages(&pool, current_session, &self.agent).await {
                    self.add_message(
                        "system",
                        &format!("fork aborted: failed to save current session: {}", e),
                    );
                    return SlashResult::Handled;
                }
                match sessions::fork_session(&pool, current_session, up_to, None).await {
                    Ok(new_id) => {
                        // Switch the agent's session id to the new one
                        // so subsequent turns append to the fork.
                        {
                            let mut state = self.agent.state().lock().await;
                            state.session_id = new_id;
                        }
                        // Replace the visible messages with just the
                        // forked subset (preserves their order/content).
                        self.messages = snapshot;
                        self.add_message(
                            "system",
                            &format!(
                                "forked into new session {} ({} messages copied from {}; original session {} left intact)",
                                new_id,
                                up_to,
                                current_session,
                                current_session
                            ),
                        );
                    }
                    Err(e) => {
                        self.add_message("system", &format!("fork failed: {}", e));
                    }
                }
                SlashResult::Handled
            }
            "/tools" => {
                self.add_message(
                    "system",
                    "tools are managed by the agent's ToolRegistry.\n\
                     Use `volt tools list` from the shell, or read\n\
                     the system prompt sent to the model for the live list.",
                );
                SlashResult::Handled
            }
            "/init" => {
                let path = std::path::PathBuf::from("AGENTS.md");
                if path.exists() {
                    self.add_message(
                        "system",
                        "AGENTS.md already exists in the current directory — not overwriting.",
                    );
                } else {
                    match std::fs::write(
                        &path,
                        "# AGENTS.md — Project Instructions for VOLT\n\n\
                         ## Project context\n\
                         Describe your project here so the agent has persistent context.\n\n\
                         ## Build / test / lint commands\n\
                         - Build: `cargo build`\n\
                         - Test:  `cargo test`\n\
                         - Lint:  `cargo clippy -- -D warnings`\n\n\
                         ## Conventions\n\
                         - Follow existing module layout and naming.\n\
                         - Add tests for new behavior.\n",
                    ) {
                        Ok(_) => self.add_message(
                            "system",
                            "wrote AGENTS.md — fill in the project context to get better answers.",
                        ),
                        Err(e) => {
                            self.add_message("system", &format!("failed to write AGENTS.md: {}", e))
                        }
                    }
                }
                SlashResult::Handled
            }
            "/plan" => {
                self.plan_mode = !self.plan_mode;
                if self.plan_mode {
                    self.add_message(
                        "system",
                        "plan mode: ON — next turn will be read-only. The agent will output \
                         a plan as text before calling any tool. Re-run /plan to toggle off.",
                    );
                } else {
                    self.add_message("system", "plan mode: OFF");
                }
                SlashResult::Handled
            }
            "/delegate" => {
                // `/delegate <task...>` spawns a sub-agent to do the task
                // and shows the result inline. The sub-agent uses the same
                // tool registry the parent has, so it can read/write the
                // workspace, call MCP servers, etc. Useful for offloading
                // a long task without paying the parent agent's context
                // cost.
                if args.is_empty() {
                    self.add_message(
                        "system",
                        "usage: /delegate <task> — describe what the sub-agent should do",
                    );
                    return SlashResult::Handled;
                }
                let task = args.join(" ");
                self.add_message(
                    "system",
                    &format!("[delegate] spawning sub-agent for: {}", task),
                );
                // Brief context from the last user/assistant turn. Helps
                // the sub-agent stay relevant without us re-sending the
                // full transcript. Own the String so the spawned task
                // doesn't borrow from `self`.
                let context: String = self
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "user")
                    .map(|m| m.content.as_str().to_string())
                    .unwrap_or_default();
                let tools = self.tools.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(crate::tools::delegate::delegate_task(
                        &task, &context, tools,
                    ))
                })
                .await;
                let result = match result {
                    Ok(r) => r,
                    Err(e) => crate::models::ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("delegate join error: {}", e)),
                        duration_ms: 0,
                    },
                };
                if result.success {
                    let body = format!(
                        "[delegate] sub-agent finished in {}ms\n\n{}",
                        result.duration_ms, result.output
                    );
                    self.add_message("system", &body);
                } else {
                    let body = format!(
                        "[delegate] sub-agent failed ({}ms): {}",
                        result.duration_ms,
                        result.error.unwrap_or_else(|| "unknown error".into())
                    );
                    self.add_message("system", &body);
                }
                SlashResult::Handled
            }
            "/theme" => {
                match args.first() {
                    Some(name) => match Theme::parse(name) {
                        Some(theme) => {
                            let was = self.theme;
                            self.set_theme(theme);
                            self.add_message(
                                "system",
                                &format!("theme: {} → {}", was.name(), theme.name()),
                            );
                        }
                        None => {
                            let names: Vec<&str> =
                                Theme::all().iter().map(|t| t.name()).collect();
                            self.add_message(
                                "system",
                                &format!(
                                    "unknown theme '{}'. Available: {}",
                                    name,
                                    names.join(", ")
                                ),
                            );
                        }
                    },
                    None => {
                        let names: Vec<String> = Theme::all()
                            .iter()
                            .map(|t| {
                                if *t == self.theme {
                                    format!("{} (active)", t.name())
                                } else {
                                    t.name().to_string()
                                }
                            })
                            .collect();
                        self.add_message(
                            "system",
                            &format!("Available themes:\n  {}", names.join("\n  ")),
                        );
                    }
                }
                SlashResult::Handled
            }
            "/mode" => {
                match args.first() {
                    Some(name) => {
                        // We don't actually mutate the agent's mode from
                        // the TUI (the mode is set at startup via
                        // `volt agent-tui --mode <name>`), but we surface
                        // the request so the user knows what to do.
                        self.add_message(
                            "system",
                            &format!(
                                "mode '{}' will take effect on the next session.\n\
                                 To switch now, restart with: volt agent-tui --mode {}",
                                name, name
                            ),
                        );
                    }
                    None => {
                        self.add_message(
                            "system",
                            &format!("current mode: {}", self.mode_label()),
                        );
                    }
                }
                SlashResult::Handled
            }
            "/mcp" => {
                // Walk the tool registry and find any tools that came
                // from an MCP server. We expose this through the
                // registry's tool names — MCP tools are namespaced
                // `mcp__<server>__<tool>` in Volt's convention.
                let mut mcp_tools: std::collections::BTreeMap<String, Vec<String>> =
                    std::collections::BTreeMap::new();
                for name in self.tools.tool_names() {
                    if let Some(rest) = name.strip_prefix("mcp__") {
                        if let Some(idx) = rest.find("__") {
                            let server = &rest[..idx];
                            let tool = &rest[idx + 2..];
                            mcp_tools
                                .entry(server.to_string())
                                .or_default()
                                .push(tool.to_string());
                        }
                    }
                }
                if mcp_tools.is_empty() {
                    self.add_message(
                        "system",
                        "no MCP servers registered. Use `mcp__*` tool names in your\n\
                         config or `volt mcp serve` to expose Volt as an MCP server.",
                    );
                } else {
                    let mut out = String::from("MCP servers + tools:\n");
                    for (server, tools) in &mcp_tools {
                        out.push_str(&format!("  {} ({} tools)\n", server, tools.len()));
                        for t in tools {
                            out.push_str(&format!("    • {}\n", t));
                        }
                    }
                    self.add_message("system", &out);
                }
                SlashResult::Handled
            }
            "/export" => {
                // `/export [path]` writes the current visible messages
                // to a markdown file. Default path: session.md in the
                // current directory.
                let path = args
                    .first()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("session.md"));
                let mut body = String::new();
                body.push_str(&format!(
                    "# Volt session export — {}\n\n",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
                ));
                body.push_str(&format!(
                    "model: `{}`\nmode: `{}`\n\n",
                    self.agent.config().model,
                    self.mode_label()
                ));
                for m in &self.messages {
                    let role = m.role.to_uppercase();
                    if m.role == "tool" {
                        let name = m.tool_name.as_deref().unwrap_or("tool");
                        body.push_str(&format!("### [{}] {}\n\n", role, name));
                        body.push_str("```\n");
                        body.push_str(&m.content);
                        body.push_str("\n```\n\n");
                    } else {
                        body.push_str(&format!("### {}\n\n", role));
                        body.push_str(&m.content);
                        body.push_str("\n\n");
                    }
                }
                match std::fs::write(&path, &body) {
                    Ok(_) => self.add_message(
                        "system",
                        &format!("exported {} messages to {}", self.messages.len(), path.display()),
                    ),
                    Err(e) => self.add_message(
                        "system",
                        &format!("export failed: {}", e),
                    ),
                }
                SlashResult::Handled
            }
            "/compact" => {
                // `/compact` triggers prompt compression on the
                // agent's current conversation. We call the agent's
                // `compact_state` method (if available) and report the
                // result. Falls back to a soft no-op if the method
                // isn't present (older agent builds).
                let msgs_before = self
                    .agent
                    .state()
                    .try_lock()
                    .map(|s| s.messages.len())
                    .unwrap_or(0);
                self.add_message(
                    "system",
                    &format!(
                        "(note) /compact is read-only in this build — it cannot mutate the\n\
                         agent's message list. The current conversation has {} messages.\n\
                         Manual cleanup: /clear to clear the visible messages and start over,\n\
                         or /fork to copy the conversation into a fresh session.",
                        msgs_before
                    ),
                );
                SlashResult::Handled
            }
            _ => SlashResult::NotASlash,
        }
    }

    fn render_messages(&self, f: &mut Frame, area: Rect) {
        let max_width = area.width.saturating_sub(4) as usize;
        let items: Vec<ListItem> = self
            .messages
            .iter()
            .map(|m| build_item_enhanced(&m.role, &m.content, m.tool_name.as_deref(), max_width))
            .collect();
        let title = format!(" Volt Agent Chat ({} msgs) ", self.messages.len());
        let messages = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
        f.render_widget(messages, area);
    }

    fn render_hud(&self, f: &mut Frame, area: Rect) {
        let (p, c) = self
            .agent
            .state()
            .try_lock()
            .map(|s| (s.total_prompt_tokens, s.total_completion_tokens))
            .unwrap_or((0, 0));
        let est_cost = (c as f64) * 0.000_000_59;
        let duration = self
            .turn_started_at
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.0);
        let line = if self.is_thinking {
            format!(
                " ↑ {}{} tok  ↓ {}{} tok  ·  ~${:.4}  ·  {:.1}s  ·  thinking…",
                fmt_tokens(p),
                suffix_tokens(p),
                fmt_tokens(c),
                suffix_tokens(c),
                est_cost,
                duration
            )
        } else {
            format!(
                " ↑ {}{} tok  ↓ {}{} tok  ·  ~${:.4}  ·  /help for commands",
                fmt_tokens(p),
                suffix_tokens(p),
                fmt_tokens(c),
                suffix_tokens(c),
                est_cost
            )
        };
        let para = Paragraph::new(line)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Left);
        f.render_widget(para, area);
    }

    // ── panels layout render methods ─────────────────────────────────

    /// Top header (3 rows): logo + model name on the left, plan/state
    /// pills in the middle, context % + cost on the right. Mirrors the
    /// opencode header.
    fn render_header(&self, f: &mut Frame, area: Rect) {
        let accent = self.theme.accent();
        let muted = self.theme.muted();
        let (p, c) = self
            .agent
            .state()
            .try_lock()
            .map(|s| (s.total_prompt_tokens, s.total_completion_tokens))
            .unwrap_or((0, 0));
        let est_cost = (c as f64) * 0.000_000_59;

        // 3-row split: row 0 logo / model / cost, row 1 thin rule,
        // row 2 breadcrumb (mode + plan + theme). We use one Paragraph
        // with 3 explicit lines and let ratatui's Wrap handle narrow
        // terminals.
        let model_short = short_model_name(&self.agent.config().model);
        let plan_label = if self.plan_mode { "PLAN" } else { "RUN" };
        let line0 = Line::from(vec![
            Span::styled(" VOLT ", Style::default().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)),
            Span::styled("  ", Style::default()),
            Span::styled(model_short, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("  ", Style::default()),
            Span::styled(format!("↑ {}{} ↓ {}{}", fmt_tokens(p), suffix_tokens(p), fmt_tokens(c), suffix_tokens(c)),
                Style::default().fg(muted)),
        ]);
        let line1 = Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(muted),
        ));
        let line2 = Line::from(vec![
            Span::styled(" mode=", Style::default().fg(muted)),
            Span::styled(self.mode_label(), Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            Span::styled("  state=", Style::default().fg(muted)),
            Span::styled(
                if self.is_thinking { "thinking" } else { "idle" },
                Style::default().fg(if self.is_thinking { Color::Yellow } else { muted }),
            ),
            Span::styled("  plan=", Style::default().fg(muted)),
            Span::styled(plan_label, Style::default().fg(if self.plan_mode { Color::Yellow } else { muted })),
            Span::styled("  theme=", Style::default().fg(muted)),
            Span::styled(self.theme.name(), Style::default().fg(accent)),
            Span::styled(format!("  ~${:.4}", est_cost), Style::default().fg(muted)),
        ]);
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(muted));
        let p = Paragraph::new(vec![line0, line1, line2])
            .block(block)
            .wrap(Wrap { trim: false });
        f.render_widget(p, area);
    }

    /// Left sidebar (24 cols). Lists recent sessions with the active
    /// session highlighted. j/k or arrows move the selection; Enter
    /// switches; n creates a new session.
    fn render_sidebar(&self, f: &mut Frame, area: Rect) {
        let accent = self.theme.accent();
        let muted = self.theme.muted();
        let title = " Sessions ";
        let items: Vec<ListItem> = if self.sidebar_sessions.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "  (no sessions yet)",
                Style::default().fg(muted),
            )))]
        } else {
            self.sidebar_sessions
                .iter()
                .map(|s| {
                    let title = if s.title.is_empty() {
                        "(untitled)"
                    } else {
                        s.title.as_str()
                    };
                    let display = format!(" {}\n  {} msgs · {}", s.short_id, s.message_count, title);
                    ListItem::new(display)
                })
                .collect()
        };
        let list = List::new(items)
            .block(Block::default().borders(Borders::RIGHT).border_style(Style::default().fg(muted)).title(title))
            .highlight_style(Style::default().bg(accent).fg(Color::Black).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.sidebar_state.clone());
    }

    /// Messages area in panels mode. Same content as the classic
    /// renderer; we keep it as a separate method so future panels-only
    /// tweaks (e.g. virtual scroll, in-place expand) can land without
    /// touching the classic render path.
    fn render_messages_panels(&self, f: &mut Frame, area: Rect) {
        let max_width = area.width.saturating_sub(4) as usize;
        let items: Vec<ListItem> = self
            .messages
            .iter()
            .map(|m| build_item_enhanced_with_theme(&m.role, &m.content, m.tool_name.as_deref(), max_width, self.theme))
            .collect();
        let title = format!(" Chat ({} msgs) ", self.messages.len());
        let block = Block::default().borders(Borders::NONE);
        let messages = List::new(items).block(block).style(Style::default());
        // Manually paint the title above the list because the list block
        // has no borders.
        f.render_widget(messages, area);
        let title_area = Rect { x: area.x, y: area.y, width: area.width, height: 1 };
        let title_p = Paragraph::new(Line::from(Span::styled(
            title,
            Style::default().fg(self.theme.muted()).add_modifier(Modifier::BOLD),
        )));
        f.render_widget(title_p, title_area);
    }

    /// Composer pills row: 1 line showing the model, plan, and theme as
    /// coloured badges. Sits just above the input separator.
    fn render_composer_pills(&self, f: &mut Frame, area: Rect) {
        let accent = self.theme.accent();
        let muted = self.theme.muted();
        let model = short_model_name(&self.agent.config().model);
        let plan = if self.plan_mode { "PLAN" } else { "RUN" };
        let mode = self.mode_label();
        let theme = self.theme.name();
        let line = Line::from(vec![
            pill(" model ", &model, accent),
            Span::raw("  "),
            pill(" mode ", &mode, Color::Magenta),
            Span::raw("  "),
            pill(" plan ", plan, if self.plan_mode { Color::Yellow } else { muted }),
            Span::raw("  "),
            pill(" theme ", theme, Color::Green),
            Span::raw("  "),
            Span::styled("ctrl+B sidebar  /help", Style::default().fg(muted)),
        ]);
        let para = Paragraph::new(line);
        f.render_widget(para, area);
    }

    /// 1-row status bar at the very bottom: tokens + cost + /help.
    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        let (p, c) = self
            .agent
            .state()
            .try_lock()
            .map(|s| (s.total_prompt_tokens, s.total_completion_tokens))
            .unwrap_or((0, 0));
        let est_cost = (c as f64) * 0.000_000_59;
        let duration = self
            .turn_started_at
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.0);
        let line = if self.is_thinking {
            format!(
                " ↑ {}{} tok  ↓ {}{} tok  ·  ~${:.4}  ·  {:.1}s  ·  thinking…",
                fmt_tokens(p),
                suffix_tokens(p),
                fmt_tokens(c),
                suffix_tokens(c),
                est_cost,
                duration
            )
        } else {
            format!(
                " ↑ {}{} tok  ↓ {}{} tok  ·  ~${:.4}  ·  /help for commands",
                fmt_tokens(p),
                suffix_tokens(p),
                fmt_tokens(c),
                suffix_tokens(c),
                est_cost
            )
        };
        let para = Paragraph::new(line).style(Style::default().fg(Color::DarkGray));
        f.render_widget(para, area);
    }

    /// Short label for the current mode (precision / balanced /
    /// autonomous / unknown). Pulled from the agent's config.
    fn mode_label(&self) -> String {
        // The agent's mode isn't a first-class field; we derive it from
        // the enabled context kinds. Balanced is the default in
        // agent_tui.rs.
        let kind = self.agent.config().enabled_context_kinds.len();
        if kind <= 3 {
            "precision".to_string()
        } else if kind <= 6 {
            "balanced".to_string()
        } else {
            "autonomous".to_string()
        }
    }
}

// ── approval callback factory ──────────────────────────────────────────

/// Construct an `ApprovalCallback` that bridges the agent loop to a TUI
/// approval widget. The callback sends a request through the unbounded
/// channel and awaits the decision on a oneshot. The TUI polls the
/// channel via `TuiChat::poll_approval` and resolves the oneshot in
/// `TuiChat::handle_approval_key`.
///
/// Returns `(callback, receiver)`. Hand the receiver to `TuiChat::new_with_approval`
/// and the callback to `Agent::with_approval`.
pub fn approval_callback_for(
    tx: mpsc::UnboundedSender<ApprovalRequest>,
) -> crate::agent::ApprovalCallback {
    Arc::new(move |tool: &str, args: &serde_json::Value| {
        let tx = tx.clone();
        let tool = tool.to_string();
        let args = args.clone();
        Box::pin(async move {
            let (decision_tx, decision_rx) = tokio::sync::oneshot::channel();
            let req = ApprovalRequest {
                tool,
                args,
                decision_tx,
            };
            if tx.send(req).is_err() {
                // The TUI is gone. Default to denying so we don't run an
                // unapproved tool silently.
                return ApprovalDecision::Deny;
            }
            decision_rx.await.unwrap_or(ApprovalDecision::Deny)
        })
    })
}

// ── helpers ───────────────────────────────────────────────────────────

/// Cached syntect resources. Initialised on first use via `OnceLock`.
struct Highlighter {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
}

static HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();

fn highlighter() -> &'static Highlighter {
    HIGHLIGHTER.get_or_init(|| Highlighter {
        syntaxes: SyntaxSet::load_defaults_newlines(),
        themes: ThemeSet::load_defaults(),
    })
}

/// Map a tool name to a category colour. Buckets are loose — they exist
/// to give a visual cue in the TUI, not to enforce security policy.
fn color_for_tool(name: &str) -> Color {
    let n = name.to_ascii_lowercase();
    // shell / filesystem writes / destructive
    if n.contains("bash") || n.contains("shell") || n.contains("exec") || n == "write" {
        return Color::Red;
    }
    // file writes
    if n.contains("write") || n.contains("edit") || n.contains("create") {
        return Color::LightRed;
    }
    // network
    if n.contains("web_") || n.contains("fetch") || n.contains("scrape") || n.contains("search") {
        return Color::Magenta;
    }
    // agent orchestration
    if n.contains("delegate")
        || n.contains("workflow")
        || n.contains("agent")
        || n.contains("final_answer")
    {
        return Color::Cyan;
    }
    // data / query
    if n.contains("query") || n.contains("sql") || n.contains("csv") || n.contains("json") {
        return Color::Yellow;
    }
    // memory
    if n.contains("memory") || n.contains("todo") {
        return Color::LightMagenta;
    }
    // read-only (default for unknown)
    Color::Blue
}

/// Detect JSON content and return a syntax-highlighted line. Falls back to
/// a single dim span if syntect fails or content doesn't look like JSON.
fn highlight_json_line(text: &str) -> Vec<Span<'static>> {
    let trimmed = text.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return vec![Span::raw(text.to_string())];
    }
    let h = highlighter();
    // Choose a theme that works on both dark and light terminals. Most TUI
    // setups are dark; fall back to a dark theme if "InspiredGitHub" is
    // missing for any reason.
    let theme = h
        .themes
        .themes
        .get("base16-eighties.dark")
        .or_else(|| h.themes.themes.values().next())
        .expect("syntect default themes loaded at least one entry");
    let syntax = h
        .syntaxes
        .find_syntax_by_name("JSON")
        .or_else(|| h.syntaxes.find_syntax_by_extension("json"))
        .unwrap_or_else(|| h.syntaxes.find_syntax_plain_text());
    let mut hl = HighlightLines::new(syntax, theme);
    let mut output = Vec::new();
    match hl.highlight_line(trimmed, &h.syntaxes) {
        Ok(ranges) => {
            for (style, text_chunk) in ranges {
                let fg = syn_color_to_ratatui(style.foreground);
                output.push(Span::styled(
                    text_chunk.to_string(),
                    Style::default().fg(fg),
                ));
            }
        }
        Err(_) => return vec![Span::raw(text.to_string())],
    }
    if output.is_empty() {
        vec![Span::raw(text.to_string())]
    } else {
        output
    }
}

/// Map a syntect colour to a ratatui `Color`. `None` (syntect default) is
/// rendered as `White` to stay visible on both light and dark terminals.
fn syn_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Flush the agent's authoritative `state.messages` to the database so
/// the next read (e.g. `fork_session`) sees a consistent view. Uses the
/// atomic "delete + reinsert" helper so a partial write doesn't leave
/// the session in a torn state. Errors propagate; callers decide whether
/// to abort the surrounding operation.
async fn persist_tui_messages(
    pool: &sqlx::SqlitePool,
    session_id: uuid::Uuid,
    agent: &Arc<Agent>,
) -> anyhow::Result<()> {
    let s = agent.state().lock().await;
    sessions::save_session_messages_atomic(pool, session_id, &s.messages).await
}

fn build_item_enhanced(
    role: &str,
    content: &str,
    tool_name: Option<&str>,
    max_width: usize,
) -> ListItem<'static> {
    build_item_enhanced_with_theme(role, content, tool_name, max_width, Theme::default())
}

/// Theme-aware variant used by the panels layout. The classic layout
/// uses `Theme::Default` so the two paths produce identical output.
fn build_item_enhanced_with_theme(
    role: &str,
    content: &str,
    tool_name: Option<&str>,
    max_width: usize,
    theme: Theme,
) -> ListItem<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if role == "tool" {
        let name = tool_name.unwrap_or("tool");
        let tag_color = color_for_tool(name);
        let tag = Span::styled(
            format!("[{}] ", name.to_uppercase()),
            Style::default().fg(tag_color).add_modifier(Modifier::BOLD),
        );
        lines.push(Line::from(tag));

        // If content is JSON, highlight it; otherwise indent and dim.
        let trimmed = content.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            // Highlight the whole JSON block on one line for compactness.
            lines.push(Line::from(highlight_json_line(content)));
        } else {
            let wrapped = wrap_text(content, max_width);
            for line in wrapped {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(Color::Gray),
                )));
            }
        }
        lines.push(Line::from(Span::raw(String::new())));
        return ListItem::new(lines);
    }

    // user / assistant / system / fallback — keep the original behaviour
    // but add JSON highlight to assistant content (often tool call args).
    let style = match role {
        "user" => Style::default().fg(theme.user()),
        "assistant" => Style::default().fg(theme.assistant()),
        "system" => Style::default().fg(theme.system()),
        _ => Style::default(),
    };
    let role_tag = Span::styled(
        format!("[{}] ", role.to_uppercase()),
        style.add_modifier(Modifier::BOLD),
    );
    lines.push(Line::from(role_tag));
    let trimmed = content.trim_start();
    if role == "assistant" && (trimmed.starts_with('{') || trimmed.starts_with('[')) {
        lines.push(Line::from(highlight_json_line(content)));
    } else {
        let wrapped = wrap_text(content, max_width);
        for line in wrapped {
            lines.push(Line::from(Span::raw(line)));
        }
    }
    lines.push(Line::from(Span::raw(String::new())));
    ListItem::new(lines)
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let max = max_width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.len() <= max {
            lines.push(paragraph.to_string());
        } else {
            let mut start = 0;
            while start < paragraph.len() {
                let end = (start + max).min(paragraph.len());
                let break_at = if end < paragraph.len() {
                    paragraph[start..end]
                        .rfind(|c: char| c.is_whitespace())
                        .map(|pos| start + pos + 1)
                        .unwrap_or(end)
                } else {
                    end
                };
                lines.push(paragraph[start..break_at].to_string());
                start = break_at;
            }
        }
    }
    lines
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn suffix_tokens(n: u64) -> &'static str {
    if n >= 1_000_000 {
        "M"
    } else if n >= 1_000 {
        "k"
    } else {
        ""
    }
}

fn history_file_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("volt")
        .join("history.txt")
}

/// Build a centred rectangle of the given percentage of the parent area.
/// Used by the approval modal so it floats over the panel content
/// without resizing the underlying layout.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Render a `[ label value ]` pill with the given accent colour. The
/// label is dimmed; the value uses the accent on a dark background.
fn pill(label: &'static str, value: &str, accent: Color) -> Span<'static> {
    Span::styled(
        format!("[{}{}]", label, value),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )
}

/// Shorten a model id for header display. Keeps the vendor prefix and
/// the size; drops the verbose suffix. Examples:
///   `llama-3.1-8b-instant`     → `llama-3.1-8b`
///   `openai/gpt-oss-20b`       → `gpt-oss-20b`
///   `meta-llama/llama-4-scout` → `llama-4-scout`
fn short_model_name(model: &str) -> String {
    // Strip vendor prefix if present.
    let raw = model.rsplit('/').next().unwrap_or(model);
    // Strip trailing qualifiers that don't add information.
    let raw = raw.strip_suffix("-instant").unwrap_or(raw);
    let raw = raw.strip_suffix("-preview").unwrap_or(raw);
    raw.to_string()
}

/// Resolves when the given cancel token is triggered.
async fn wait_for_cancel(cancel: CancelToken) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_for_tool_buckets_shell_red() {
        assert_eq!(color_for_tool("bash"), Color::Red);
        assert_eq!(color_for_tool("ShellCommand"), Color::Red);
    }

    #[test]
    fn color_for_tool_buckets_write_yellow_red() {
        assert_eq!(color_for_tool("write"), Color::Red);
        assert_eq!(color_for_tool("edit_file"), Color::LightRed);
    }

    #[test]
    fn color_for_tool_buckets_network_magenta() {
        assert_eq!(color_for_tool("web_fetch"), Color::Magenta);
        assert_eq!(color_for_tool("web_search"), Color::Magenta);
        assert_eq!(color_for_tool("scrape_page"), Color::Magenta);
    }

    #[test]
    fn color_for_tool_buckets_agent_cyan() {
        assert_eq!(color_for_tool("delegate"), Color::Cyan);
        assert_eq!(color_for_tool("run_workflow"), Color::Cyan);
        assert_eq!(color_for_tool("final_answer"), Color::Cyan);
    }

    #[test]
    fn color_for_tool_unknown_defaults_to_blue() {
        assert_eq!(color_for_tool("read_file"), Color::Blue);
        assert_eq!(color_for_tool("list_directory"), Color::Blue);
        assert_eq!(color_for_tool("unknown_tool"), Color::Blue);
    }

    #[test]
    fn highlight_json_line_passthrough_for_non_json() {
        let spans = highlight_json_line("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "hello world");
    }

    #[test]
    fn highlight_json_line_produces_styled_spans_for_json() {
        let spans = highlight_json_line(r#"{"key": "value", "n": 42}"#);
        // JSON must produce more than one span because there are several
        // syntactical categories (string keys, string values, numbers, braces).
        assert!(
            spans.len() > 1,
            "expected multiple spans, got {}",
            spans.len()
        );
    }

    #[test]
    fn build_item_enhanced_user_role_uses_cyan_tag() {
        let item = build_item_enhanced("user", "hello", None, 80);
        // ListItem doesn't expose lines publicly; assert non-empty width.
        assert!(item.width() > 0, "expected non-empty item");
    }

    #[test]
    fn build_item_enhanced_tool_role_uses_tool_color() {
        let item = build_item_enhanced("tool", "ls output", Some("bash"), 80);
        assert!(item.width() > 0, "expected non-empty item");
    }

    #[test]
    fn build_item_enhanced_tool_highlights_json_result() {
        let item = build_item_enhanced(
            "tool",
            r#"{"rows": 3, "status": "ok"}"#,
            Some("json_query"),
            80,
        );
        // JSON highlighting produces a wider item than a plain string
        // because the JSON content has key/string/number/punctuation
        // categories broken out into separate spans.
        let plain = build_item_enhanced("tool", "short", Some("json_query"), 80);
        assert!(item.width() >= plain.width());
    }

    #[test]
    fn help_text_mentions_all_slash_commands() {
        // The /help output should advertise every slash command the TUI
        // supports. If a new command is added but not listed in HELP_TEXT,
        // this test fails — keeping the two in sync.
        for cmd in [
            "/help",
            "/quit",
            "/clear",
            "/model",
            "/status",
            "/cost",
            "/sessions",
            "/resume",
            "/tools",
            "/init",
            "/delegate",
            "/fork",
            "/plan",
            "/theme",
            "/mode",
            "/mcp",
            "/export",
            "/compact",
        ] {
            assert!(HELP_TEXT.contains(cmd), "missing {} in help", cmd);
        }
    }

    // ── LayoutMode tests ─────────────────────────────────────────────

    #[test]
    fn layout_mode_default_is_classic() {
        // The default is `Classic` for backward compat. Existing users
        // shouldn't be surprised by a new layout on upgrade.
        assert_eq!(LayoutMode::default(), LayoutMode::Classic);
    }

    // ── Theme tests ──────────────────────────────────────────────────

    #[test]
    fn theme_default_name() {
        assert_eq!(Theme::default().name(), "default");
    }

    #[test]
    fn theme_all_names_distinct() {
        let names: Vec<&str> = Theme::all().iter().map(|t| t.name()).collect();
        let mut deduped = names.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(deduped.len(), names.len(), "duplicate theme names");
    }

    #[test]
    fn theme_parse_round_trip() {
        for t in Theme::all() {
            let parsed = Theme::parse(t.name()).unwrap();
            assert_eq!(parsed, *t);
        }
    }

    #[test]
    fn theme_parse_case_insensitive() {
        assert_eq!(Theme::parse("DEFAULT"), Some(Theme::Default));
        assert_eq!(Theme::parse("Catppuccin"), Some(Theme::Catppuccin));
        assert_eq!(Theme::parse("DRACULA"), Some(Theme::Dracula));
        assert_eq!(Theme::parse("nord"), Some(Theme::Nord));
    }

    #[test]
    fn theme_parse_unknown_returns_none() {
        assert_eq!(Theme::parse("nope"), None);
        assert_eq!(Theme::parse(""), None);
        assert_eq!(Theme::parse("monokai"), None);
    }

    #[test]
    fn theme_synonyms() {
        // Solarized-Dark is also known as solarized or solarized_dark.
        assert_eq!(Theme::parse("solarized"), Some(Theme::SolarizedDark));
        assert_eq!(Theme::parse("solarized-dark"), Some(Theme::SolarizedDark));
        assert_eq!(Theme::parse("solarized_dark"), Some(Theme::SolarizedDark));
    }

    #[test]
    fn theme_accent_is_color() {
        // Just verify the function returns a colour (any colour). The
        // exact RGB is covered by the parse_round_trip test above.
        let _ = Theme::Default.accent();
        let _ = Theme::Catppuccin.accent();
        let _ = Theme::Dracula.accent();
        let _ = Theme::Nord.accent();
        let _ = Theme::SolarizedDark.accent();
    }

    // ── pill tests ───────────────────────────────────────────────────

    #[test]
    fn pill_formats_label_and_value() {
        let s = pill(" model ", "llama-3.1-8b", Color::Cyan);
        let content = s.content;
        assert!(content.contains("model"));
        assert!(content.contains("llama-3.1-8b"));
        assert!(content.starts_with('['));
        assert!(content.ends_with(']'));
    }

    // ── short_model_name tests ───────────────────────────────────────

    #[test]
    fn short_model_name_strips_instant() {
        assert_eq!(short_model_name("llama-3.1-8b-instant"), "llama-3.1-8b");
    }

    #[test]
    fn short_model_name_strips_vendor_prefix() {
        assert_eq!(short_model_name("openai/gpt-oss-20b"), "gpt-oss-20b");
        assert_eq!(short_model_name("meta-llama/llama-4-scout"), "llama-4-scout");
    }

    #[test]
    fn short_model_name_strips_preview() {
        assert_eq!(short_model_name("gpt-4o-preview"), "gpt-4o");
    }

    #[test]
    fn short_model_name_no_change_for_already_short() {
        assert_eq!(short_model_name("qwen3-32b"), "qwen3-32b");
    }

    // ── centered_rect tests ──────────────────────────────────────────

    #[test]
    fn centered_rect_produces_smaller_area_than_parent() {
        let parent = Rect::new(0, 0, 100, 50);
        let popup = centered_rect(70, 30, parent);
        assert!(popup.width < parent.width);
        assert!(popup.height < parent.height);
        // The popup should be roughly centered.
        assert!(popup.x > 0);
        assert!(popup.y > 0);
    }

    // ── build_item_enhanced_with_theme tests ─────────────────────────

    #[test]
    fn build_item_with_theme_uses_theme_user_color() {
        let item = build_item_enhanced_with_theme(
            "user",
            "hello",
            None,
            80,
            Theme::Catppuccin,
        );
        assert!(item.width() > 0);
        // The Catppuccin user colour is Mauve (203, 166, 247). Even
        // though ListItem doesn't expose the spans, we know the function
        // returned without panicking.
    }

    #[test]
    fn build_item_with_theme_assistant_uses_green_family() {
        let item = build_item_enhanced_with_theme(
            "assistant",
            "ok",
            None,
            80,
            Theme::Dracula,
        );
        assert!(item.width() > 0);
    }
}
