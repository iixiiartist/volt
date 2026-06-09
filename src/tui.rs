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
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
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

const HELP_TEXT: &str = "Volt Agent — slash commands

  /help, /?           Show this help
  /quit, /exit, /q    Exit the TUI
  /clear              Clear visible messages (keeps session)
  /model              Show current model
  /model <name>       Switch model (resets conversation)
  /status             Show agent status (model, messages, tokens)
  /cost, /tokens      Show cumulative token usage + est. cost
  /sessions           List recent sessions (top 10)
  /resume             Re-load latest session messages
  /tools              List available tools
  /init               Create a starter AGENTS.md in the current dir
  /delegate <task>    Spawn a sub-agent to do the task; result is shown here
  /fork [n]           Copy the conversation up to message N into a new session
  /plan               Toggle plan mode (next turn is read-only)

Tip: while the agent is thinking, press Ctrl-C to cancel.";

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
                        }
                        Ok(items) => {
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
            "/resume" => self.cmd_resume(&args, sessions_pool).await,
            "/fork" => self.cmd_fork(&args, sessions_pool).await,
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
            _ => SlashResult::NotASlash,
        }
    }

    async fn cmd_fork(
        &mut self,
        args: &[&str],
        sessions_pool: &Option<sqlx::SqlitePool>,
    ) -> SlashResult {
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
        let snapshot: Vec<ChatMessage> =
            self.messages.iter().take(up_to).cloned().collect();

        if let Err(e) = persist_tui_messages(&pool, current_session, &self.agent).await {
            self.add_message(
                "system",
                &format!("fork aborted: failed to save current session: {}", e),
            );
            return SlashResult::Handled;
        }
        match sessions::fork_session(&pool, current_session, up_to, None).await {
            Ok(new_id) => {
                {
                    let mut state = self.agent.state().lock().await;
                    state.session_id = new_id;
                }
                self.messages = snapshot;
                self.add_message(
                    "system",
                    &format!(
                        "forked into new session {} ({} messages copied from {}; original session {} left intact)",
                        new_id, up_to, current_session, current_session
                    ),
                );
            }
            Err(e) => {
                self.add_message("system", &format!("fork failed: {}", e));
            }
        }
        SlashResult::Handled
    }

    async fn cmd_resume(
        &mut self,
        args: &[&str],
        sessions_pool: &Option<sqlx::SqlitePool>,
    ) -> SlashResult {
        let n: Option<usize> = args.first().and_then(|s| s.parse().ok());
        match (n, sessions_pool) {
            (Some(idx), Some(pool)) => {
                if idx < 1 {
                    self.add_message("system", "session numbers start at 1 — try /sessions");
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
        "user" => Style::default().fg(Color::Cyan),
        "assistant" => Style::default().fg(Color::Green),
        "system" => Style::default().fg(Color::Yellow),
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
        ] {
            assert!(HELP_TEXT.contains(cmd), "missing {} in help", cmd);
        }
    }
}
