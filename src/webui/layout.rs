use super::commands::UiCommand;
use super::routes::Page;
use super::state::{
    Toast, ToastLevel, VoltState, COLOR_ACCENT, COLOR_BG, COLOR_BORDER, COLOR_DANGER, COLOR_INFO,
    COLOR_PANEL, COLOR_PANEL_HOVER, COLOR_SUCCESS, COLOR_TEXT, COLOR_TEXT_DIM, COLOR_TEXT_MUTED,
    COLOR_WARNING, SIDEBAR_WIDTH,
};
use dioxus::prelude::*;

#[component]
pub fn AppLayout() -> Element {
    let mut state: VoltState = use_context();
    let current = *state.current_page.read();
    let _collapsed = *state.sidebar_collapsed.read();
    let show_trace = *state.show_trace_panel.read();
    let show_palette = *state.show_command_palette.read();
    let connection = *state.connection.read();
    // Prune stale toasts every 500 ms. Cheap, and replaces
    // per-toast timers (which can't hold a `VoltState` reference
    // because they're spawned from `&mut self`).
    {
        use dioxus::prelude::use_future;
        use std::time::Duration;
        let _ = use_future(move || async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(500));
            loop {
                ticker.tick().await;
                state.prune_toasts();
            }
        });
    }
    let show_error_banner = matches!(
        connection,
        super::state::ConnectionStatus::Error | super::state::ConnectionStatus::Disconnected
    );

    rsx! {
        div {
            style: "display: flex; height: 100vh; width: 100vw; background-color: {COLOR_BG}; color: {COLOR_TEXT}; font-family: -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif; overflow: hidden;",
            tabindex: "0",
            onkeydown: move |e| {
                use dioxus::prelude::Key as KbdKey;
                let key = e.key();
                let ctrl_or_meta = e.modifiers().meta() || e.modifiers().ctrl();
                // Escape is always handled, even in text fields.
                if key == KbdKey::Escape {
                    if !state.pending_approvals.read().is_empty() {
                        // Deny the front-of-queue request on Escape.
                        let req_id = {
                            let mut q = state.pending_approvals.write();
                            if q.is_empty() {
                                None
                            } else {
                                Some(q.remove(0).request_id)
                            }
                        };
                        if let Some(id) = req_id {
                            state.fire(UiCommand::ApprovalResponse {
                                request_id: id,
                                allow: false,
                                allow_session: false,
                            });
                        }
                        return;
                    }
                    if *state.show_command_palette.read() {
                        state.show_command_palette.set(false);
                        return;
                    }
                    if *state.show_trace_panel.read() {
                        state.show_trace_panel.set(false);
                        return;
                    }
                    if *state.show_setup_wizard.read() {
                        state.show_setup_wizard.set(false);
                        return;
                    }
                }
                // Ctrl/Cmd shortcuts are suppressed while typing
                // in a text input so they don't fire mid-keystroke.
                // (Each text input sets the signal on focus and
                // clears it on blur — see `mark_text_focus`.)
                if ctrl_or_meta && !*state.focus_in_text_input.read() {
                    if key == KbdKey::Character("k".to_string())
                        || key == KbdKey::Character("K".to_string())
                    {
                        let cur = *state.show_command_palette.read();
                        state.show_command_palette.set(!cur);
                    } else if key == KbdKey::Character(".".to_string()) {
                        let cur = *state.show_trace_panel.read();
                        state.show_trace_panel.set(!cur);
                    }
                }
            },
            Sidebar {}
            div {
                style: "flex: 1; display: flex; flex-direction: column; min-width: 0; overflow: hidden;",
                if show_error_banner {
                    div { style: "padding: 8px 16px; background-color: rgba(239, 68, 68, 0.15); border-bottom: 1px solid {COLOR_DANGER}; color: {COLOR_DANGER}; font-size: 12px; display: flex; align-items: center; gap: 12px;",
                        span { "\u{26A0}" }
                        span { "Runtime is offline. Some commands will fail. Check ~/.volt/logs/webui.log for details." }
                    }
                }
                Header {}
                div {
                    style: "flex: 1; display: flex; flex-direction: row; min-height: 0;",
                    div {
                        style: "flex: 1; overflow-y: auto; background-color: {COLOR_BG};",
                        { super::pages::render_page(current) }
                    }
                    if show_trace {
                        TracePanel {}
                    }
                }
                StatusBar {}
            }
            ToastContainer {}
            if show_palette {
                CommandPalette {}
            }
            // The first-run setup wizard. Rendered last so it sits on
            // top of every other layer; full-screen overlay with
            // opaque backdrop.
            super::setup_wizard::SetupWizard {}
            ApprovalModal {}
        }
    }
}

#[component]
fn ApprovalModal() -> Element {
    let mut state: VoltState = use_context();
    let pending = state.pending_approvals.read().clone();
    if pending.is_empty() {
        return rsx! { div {} };
    }
    // Always show the oldest pending request; the rest stay in the
    // queue and the next one will appear as soon as the user
    // answers.
    let req = pending[0].clone();
    let rest_len = pending.len() - 1;
    let args_text = serde_json::to_string_pretty(&req.args).unwrap_or_default();
    rsx! {
        div {
            style: "position: fixed; inset: 0; background-color: rgba(0,0,0,0.7); z-index: 1500; display: flex; align-items: center; justify-content: center; padding: 24px;",
            div {
                style: "width: 520px; max-width: 100%; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_WARNING}; border-radius: 12px; box-shadow: 0 24px 48px rgba(0,0,0,0.5);",
                div { style: "padding: 18px 22px; border-bottom: 1px solid {COLOR_BORDER}; display: flex; align-items: center; gap: 12px;",
                    div { style: "width: 32px; height: 32px; background-color: rgba(245,158,11,0.2); border-radius: 8px; display: flex; align-items: center; justify-content: center; color: {COLOR_WARNING}; font-size: 18px;",
                        "\u{26A0}"
                    }
                    div {
                        h3 { style: "margin: 0; font-size: 16px; color: {COLOR_TEXT};", "Tool Approval Required" }
                        p { style: "margin: 2px 0 0 0; font-size: 12px; color: {COLOR_TEXT_DIM};",
                            "The agent wants to invoke a privileged tool."
                        }
                    }
                }
                div { style: "padding: 16px 22px;",
                    div { style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: {COLOR_TEXT_MUTED}; margin-bottom: 4px;",
                        "Tool"
                    }
                    div { style: "font-family: monospace; font-size: 14px; color: {COLOR_TEXT}; margin-bottom: 12px;",
                        "{req.tool_name}"
                    }
                    div { style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: {COLOR_TEXT_MUTED}; margin-bottom: 4px;",
                        "Arguments"
                    }
                    pre { style: "background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; padding: 10px; font-size: 11px; color: {COLOR_TEXT_DIM}; overflow-x: auto; max-height: 180px; margin: 0;",
                        "{args_text}"
                    }
                    if rest_len > 0 {
                        p { style: "margin: 12px 0 0 0; font-size: 11px; color: {COLOR_TEXT_MUTED};",
                            "Plus {rest_len} more pending request(s)."
                        }
                    }
                }
                div { style: "padding: 14px 22px; border-top: 1px solid {COLOR_BORDER}; display: flex; gap: 8px; justify-content: flex-end;",
                    // Each button captures the *current* request_id
                    // and dispatches the response. The queue removal
                    // happens *inside* the click closure so it runs
                    // at click time, not at render time.
                    button {
                        style: "padding: 8px 14px; background-color: transparent; border: 1px solid {COLOR_DANGER}; color: {COLOR_DANGER}; border-radius: 6px; cursor: pointer; font-size: 13px;",
                        onclick: {
                            let req_id = req.request_id;
                            move |_| {
                                let mut q = state.pending_approvals.write();
                                if !q.is_empty() && q[0].request_id == req_id {
                                    q.remove(0);
                                }
                                drop(q);
                                state.fire(UiCommand::ApprovalResponse {
                                    request_id: req_id,
                                    allow: false,
                                    allow_session: false,
                                });
                            }
                        },
                        "Deny"
                    }
                    button {
                        style: "padding: 8px 14px; background-color: transparent; border: 1px solid {COLOR_BORDER}; color: {COLOR_TEXT}; border-radius: 6px; cursor: pointer; font-size: 13px;",
                        onclick: {
                            let req_id = req.request_id;
                            move |_| {
                                let mut q = state.pending_approvals.write();
                                if !q.is_empty() && q[0].request_id == req_id {
                                    q.remove(0);
                                }
                                drop(q);
                                state.fire(UiCommand::ApprovalResponse {
                                    request_id: req_id,
                                    allow: true,
                                    allow_session: false,
                                });
                            }
                        },
                        "Allow Once"
                    }
                    button {
                        style: "padding: 8px 14px; background-color: {COLOR_ACCENT}; border: none; color: white; border-radius: 6px; cursor: pointer; font-size: 13px; font-weight: 600;",
                        onclick: {
                            let req_id = req.request_id;
                            move |_| {
                                let mut q = state.pending_approvals.write();
                                if !q.is_empty() && q[0].request_id == req_id {
                                    q.remove(0);
                                }
                                drop(q);
                                state.fire(UiCommand::ApprovalResponse {
                                    request_id: req_id,
                                    allow: true,
                                    allow_session: true,
                                });
                            }
                        },
                        "Allow for Session"
                    }
                }
            }
        }
    }
}

#[component]
fn Sidebar() -> Element {
    let mut state: VoltState = use_context();
    let current = *state.current_page.read();
    let collapsed = *state.sidebar_collapsed.read();
    let width = if collapsed { 64u32 } else { SIDEBAR_WIDTH };

    rsx! {
        div {
            style: "width: {width}px; min-width: {width}px; background-color: {COLOR_PANEL}; border-right: 1px solid {COLOR_BORDER}; display: flex; flex-direction: column; transition: width 0.15s ease;",
            div {
                style: "padding: 16px; display: flex; align-items: center; justify-content: space-between; border-bottom: 1px solid {COLOR_BORDER};",
                if !collapsed {
                    div {
                        style: "display: flex; align-items: center; gap: 8px;",
                        div { style: "width: 28px; height: 28px; background: linear-gradient(135deg, #a855f7, #3b82f6); border-radius: 6px; display: flex; align-items: center; justify-content: center; font-weight: 700; font-size: 14px; color: white;", "V" }
                        div { style: "font-size: 16px; font-weight: 700; color: {COLOR_TEXT};", "Volt" }
                    }
                } else {
                    div { style: "width: 28px; height: 28px; background: linear-gradient(135deg, #a855f7, #3b82f6); border-radius: 6px; display: flex; align-items: center; justify-content: center; font-weight: 700; font-size: 14px; color: white; margin: 0 auto;", "V" }
                }
                button {
                    style: "background: transparent; border: none; color: {COLOR_TEXT_DIM}; cursor: pointer; padding: 4px; font-size: 14px;",
                    title: if collapsed { "Expand sidebar" } else { "Collapse sidebar" },
                    aria_label: if collapsed { "Expand sidebar" } else { "Collapse sidebar" },
                    onclick: move |_| {
                        let cur = *state.sidebar_collapsed.read();
                        state.sidebar_collapsed.set(!cur);
                    },
                    if collapsed { ">" } else { "<" }
                }
            }
            nav {
                style: "flex: 1; overflow-y: auto; padding: 8px 0;",
                NavItem { page: Page::Dashboard, label: "Dashboard", current: current }
                NavItem { page: Page::Chat, label: "Chat", current: current }
                NavItem { page: Page::Tools, label: "Tools", current: current }
                NavItem { page: Page::Sessions, label: "Sessions", current: current }
                NavItem { page: Page::Settings, label: "Settings", current: current }
                div { style: "margin: 8px 12px; height: 1px; background-color: {COLOR_BORDER};" }
                NavItem { page: Page::Workflows, label: "Workflows", current: current }
                NavItem { page: Page::Worktrees, label: "Worktrees", current: current }
                NavItem { page: Page::Jobs, label: "Jobs", current: current }
                NavItem { page: Page::Routines, label: "Routines", current: current }
                NavItem { page: Page::Skills, label: "Skills", current: current }
                NavItem { page: Page::Registry, label: "Registry", current: current }
                NavItem { page: Page::Audit, label: "Audit", current: current }
            }
            div {
                style: "padding: 12px; border-top: 1px solid {COLOR_BORDER}; font-size: 11px;",
                ConnectionIndicator { collapsed: collapsed }
            }
        }
    }
}

#[component]
fn NavItem(page: Page, label: &'static str, current: Page) -> Element {
    let mut state: VoltState = use_context();
    let is_active = current == page;
    let bg = if is_active {
        "background-color: rgba(168, 85, 247, 0.15); border-left: 3px solid #a855f7;"
    } else {
        "border-left: 3px solid transparent;"
    };
    let color = if is_active {
        COLOR_TEXT
    } else {
        COLOR_TEXT_DIM
    };
    let icon = page.icon();
    rsx! {
        div {
            style: "{bg} padding: 10px 16px; font-size: 13px; cursor: pointer; color: {color}; display: flex; align-items: center; gap: 12px;",
            onclick: move |_| state.navigate(page),
            div { style: "font-size: 16px; width: 20px; text-align: center;", "{icon}" }
            if !*state.sidebar_collapsed.read() {
                span { "{label}" }
            }
        }
    }
}

#[component]
fn ConnectionIndicator(collapsed: bool) -> Element {
    let state: VoltState = use_context();
    let conn = *state.connection.read();
    let color = conn.color();
    let label = conn.label();
    rsx! {
        div {
            style: "display: flex; align-items: center; gap: 8px;",
            div { style: "width: 8px; height: 8px; border-radius: 50%; background-color: {color}; box-shadow: 0 0 4px {color};" }
            if !collapsed {
                span { style: "color: {COLOR_TEXT_DIM};", "{label}" }
            }
        }
    }
}

/// Returns the platform-specific keyboard shortcut label for
/// the command palette button. `⌘K` on macOS, `Ctrl K` elsewhere.
fn platform_shortcut_label() -> String {
    if cfg!(target_os = "macos") {
        "\u{2318}K".to_string()
    } else {
        "Ctrl K".to_string()
    }
}

#[component]
fn Header() -> Element {
    let mut state: VoltState = use_context();
    let current = *state.current_page.read();
    let model = state.model.read().clone();
    let show_palette = *state.show_command_palette.read();
    let show_trace = *state.show_trace_panel.read();
    // Resolve the active session name for the chat page header.
    let session_name: String = {
        let active_id = *state.chat_session.read();
        if current == Page::Chat {
            state
                .sessions_cache
                .read()
                .iter()
                .find(|s| Some(s.id) == active_id)
                .map(|s| s.name.clone())
                .unwrap_or_default()
        } else {
            String::new()
        }
    };
    rsx! {
        div {
            style: "height: 56px; padding: 0 20px; border-bottom: 1px solid {COLOR_BORDER}; background-color: {COLOR_PANEL}; display: flex; align-items: center; justify-content: space-between;",
            div {
                style: "display: flex; align-items: center; gap: 12px;",
                h1 { style: "margin: 0; font-size: 16px; font-weight: 600; color: {COLOR_TEXT};", "{current.title()}" }
                if current == Page::Chat {
                    span { style: "color: {COLOR_TEXT_MUTED}; font-size: 12px;", "\u{00B7}" }
                    if !session_name.is_empty() {
                        span { style: "color: {COLOR_TEXT}; font-size: 12px;", "{session_name}" }
                        span { style: "color: {COLOR_TEXT_MUTED}; font-size: 12px;", "\u{00B7}" }
                    }
                    span { style: "color: {COLOR_TEXT_DIM}; font-size: 12px; font-family: monospace;", "{model}" }
                    // "New Chat" button — always visible on the
                    // chat page, fires CreateSession with a
                    // timestamped name and navigates to chat.
                    button {
                        style: "padding: 4px 10px; background-color: {COLOR_ACCENT}; border: none; color: white; border-radius: 4px; cursor: pointer; font-size: 11px; font-weight: 600;",
                        onclick: move |_| {
                            let name = format!(
                                "Chat {}",
                                chrono::Utc::now().format("%Y-%m-%d %H:%M")
                            );
                            state.fire(UiCommand::CreateSession { name });
                            state.navigate(Page::Chat);
                        },
                        "+ New Chat"
                    }
                }
            }
            div { style: "display: flex; align-items: center; gap: 8px;",
                HeaderButton { label: platform_shortcut_label(), active: show_palette, action: HeaderAction::TogglePalette }
                HeaderButton { label: "Trace".to_string(), active: show_trace, action: HeaderAction::ToggleTrace }
                HeaderButton { label: "Doctor".to_string(), active: false, action: HeaderAction::RunDoctor }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HeaderAction {
    TogglePalette,
    ToggleTrace,
    RunDoctor,
}

#[component]
fn HeaderButton(label: String, active: bool, action: HeaderAction) -> Element {
    let mut state: VoltState = use_context();
    let bg = if active {
        "background-color: rgba(168,85,247,0.2); color: #a855f7; border-color: #a855f7;"
    } else {
        "background-color: transparent; border: 1px solid #25254a; color: #9090a8;"
    };
    rsx! {
        button {
            style: "{bg} padding: 6px 12px; border-radius: 6px; cursor: pointer; font-size: 12px;",
            onclick: move |_| match action {
                HeaderAction::TogglePalette => {
                    let cur = *state.show_command_palette.read();
                    state.show_command_palette.set(!cur);
                }
                HeaderAction::ToggleTrace => {
                    let cur = *state.show_trace_panel.read();
                    state.show_trace_panel.set(!cur);
                }
                HeaderAction::RunDoctor => { state.fire(UiCommand::RunDoctor); },
            },
            "{label}"
        }
    }
}

#[component]
fn StatusBar() -> Element {
    let state: VoltState = use_context();
    let llm_color = if *state.llm_online.read() {
        COLOR_SUCCESS
    } else {
        COLOR_DANGER
    };
    let db_color = if *state.db_connected.read() {
        COLOR_SUCCESS
    } else {
        COLOR_DANGER
    };
    let emb_color = if *state.embedder_loaded.read() {
        COLOR_SUCCESS
    } else {
        COLOR_WARNING
    };
    let events = *state.total_events.read();
    let cmds = *state.total_commands.read();
    rsx! {
        div {
            style: "height: 28px; padding: 0 16px; background-color: {COLOR_PANEL}; border-top: 1px solid {COLOR_BORDER}; display: flex; align-items: center; gap: 16px; font-size: 11px; color: {COLOR_TEXT_DIM};",
            StatusPill { label: "LLM", color: llm_color }
            StatusPill { label: "DB", color: db_color }
            StatusPill { label: "Embedder", color: emb_color }
            span { style: "color: {COLOR_TEXT_MUTED};", "Events: {events}" }
            span { style: "color: {COLOR_TEXT_MUTED};", "Cmds: {cmds}" }
            div { style: "flex: 1;" }
            span { style: "color: {COLOR_TEXT_MUTED};", "EU AI Act Art. 12 audit logging active" }
        }
    }
}

#[component]
fn StatusPill(label: &'static str, color: &'static str) -> Element {
    rsx! {
        div { style: "display: flex; align-items: center; gap: 4px;",
            div { style: "width: 6px; height: 6px; border-radius: 50%; background-color: {color};" }
            span { "{label}" }
        }
    }
}

#[component]
fn TracePanel() -> Element {
    let mut state: VoltState = use_context();
    let calls = state.tool_calls.read().clone();
    rsx! {
        div {
            style: "width: 380px; min-width: 380px; background-color: {COLOR_PANEL}; border-left: 1px solid {COLOR_BORDER}; display: flex; flex-direction: column;",
            div {
                style: "padding: 12px 16px; border-bottom: 1px solid {COLOR_BORDER}; display: flex; align-items: center; justify-content: space-between;",
                span { style: "font-size: 13px; font-weight: 600; color: {COLOR_TEXT};", "Trace" }
                div { style: "display: flex; gap: 8px; align-items: center;",
                    span { style: "font-size: 11px; color: {COLOR_TEXT_MUTED};", "{calls.len()} calls" }
                    button {
                        style: "background: transparent; border: none; color: {COLOR_TEXT_DIM}; cursor: pointer; font-size: 12px;",
                        onclick: move |_| state.tool_calls.set(Vec::new()),
                        "Clear"
                    }
                    button {
                        style: "background: transparent; border: none; color: {COLOR_TEXT_DIM}; cursor: pointer;",
                        onclick: move |_| state.show_trace_panel.set(false),
                        "\u{2715}"
                    }
                }
            }
            div { style: "flex: 1; overflow-y: auto; padding: 8px;",
                if calls.is_empty() {
                    div { style: "color: {COLOR_TEXT_MUTED}; padding: 16px; text-align: center; font-size: 12px;",
                        "No tool calls yet. Run a chat turn to see the trace."
                    }
                } else {
                    div { style: "display: flex; flex-direction: column; gap: 6px;",
                        for c in calls.iter().rev() {
                            div { style: "padding: 8px 10px; background-color: {COLOR_PANEL_HOVER}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; font-family: monospace; font-size: 11px;",
                                div { style: "display: flex; align-items: center; gap: 6px; margin-bottom: 4px;",
                                    span { style: "color: {COLOR_ACCENT}; font-weight: 600;",
                                        "{c.name}"
                                    }
                                    if let Some(_err) = &c.error {
                                        span { style: "margin-left: auto; color: {COLOR_DANGER}; font-size: 10px; padding: 1px 4px; background-color: rgba(239,68,68,0.1); border-radius: 3px;",
                                            "error"
                                        }
                                    } else if c.result.is_some() {
                                        span { style: "margin-left: auto; color: {COLOR_SUCCESS}; font-size: 10px; padding: 1px 4px; background-color: rgba(34,197,94,0.1); border-radius: 3px;",
                                            "ok"
                                        }
                                    } else {
                                        span { style: "margin-left: auto; color: {COLOR_WARNING}; font-size: 10px; padding: 1px 4px; background-color: rgba(245,158,11,0.1); border-radius: 3px;",
                                            "running"
                                        }
                                    }
                                }
                                div { style: "color: {COLOR_TEXT_DIM}; word-break: break-all; max-height: 60px; overflow: hidden;",
                                    "{serde_json::to_string(&c.args).unwrap_or_default()}"
                                }
                                if let Some(r) = &c.result {
                                    div { style: "margin-top: 4px; color: {COLOR_TEXT}; max-height: 80px; overflow-y: auto; word-break: break-all;",
                                        "{serde_json::to_string(r).unwrap_or_default()}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ToastContainer() -> Element {
    let state: VoltState = use_context();
    let toasts = state.toasts.read().clone();
    if toasts.is_empty() {
        return rsx! { div {} };
    }
    rsx! {
        div { style: "position: fixed; bottom: 48px; right: 16px; display: flex; flex-direction: column; gap: 8px; z-index: 1000; max-width: 400px;",
            for toast in toasts.iter() { ToastItem { toast: toast.clone() } }
        }
    }
}

#[component]
fn ToastItem(toast: Toast) -> Element {
    let mut state: VoltState = use_context();
    let (border, icon) = match toast.level {
        ToastLevel::Info => (COLOR_INFO, "\u{2139}"),
        ToastLevel::Success => (COLOR_SUCCESS, "\u{2713}"),
        ToastLevel::Warning => (COLOR_WARNING, "\u{26A0}"),
        ToastLevel::Error => (COLOR_DANGER, "\u{2717}"),
    };
    let id = toast.id;
    rsx! {
        div { style: "padding: 12px 16px; background-color: {COLOR_PANEL}; border-left: 4px solid {border}; border-radius: 6px; display: flex; align-items: center; gap: 12px; box-shadow: 0 4px 12px rgba(0,0,0,0.4);",
            span { style: "color: {border}; font-size: 16px;", "{icon}" }
            span { style: "flex: 1; color: {COLOR_TEXT}; font-size: 13px;", "{toast.message}" }
            button { style: "background: transparent; border: none; color: {COLOR_TEXT_DIM}; cursor: pointer;",
                onclick: move |_| state.dismiss_toast(id),
                "\u{2715}"
            }
        }
    }
}

#[component]
fn CommandPalette() -> Element {
    let mut state: VoltState = use_context();
    let mut query = use_signal(String::new);
    rsx! {
        div { style: "position: fixed; inset: 0; background-color: rgba(0,0,0,0.5); z-index: 950; display: flex; align-items: flex-start; justify-content: center; padding-top: 100px;",
            onclick: move |_| state.show_command_palette.set(false),
            div { style: "background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; width: 600px; max-height: 500px; overflow: hidden; display: flex; flex-direction: column;",
                onclick: move |e| e.stop_propagation(),
                input { style: "padding: 16px; background-color: transparent; border: none; border-bottom: 1px solid {COLOR_BORDER}; color: {COLOR_TEXT}; font-size: 14px; outline: none;",
                    placeholder: "Type a command or page name...",
                    value: "{query.read()}",
                    oninput: move |e| query.set(e.value().to_string()),
                }
                div { style: "flex: 1; overflow-y: auto; padding: 8px;",
                    PaletteItem { label: "Go to Dashboard", page: Page::Dashboard }
                    PaletteItem { label: "Go to Chat", page: Page::Chat }
                    PaletteItem { label: "Go to Tools", page: Page::Tools }
                    PaletteItem { label: "Go to Sessions", page: Page::Sessions }
                    PaletteItem { label: "Go to Settings", page: Page::Settings }
                    PaletteItem { label: "Go to Audit", page: Page::Audit }
                    div { style: "padding: 8px 12px; color: {COLOR_TEXT_DIM}; font-size: 12px; cursor: pointer;",
                        onclick: move |_| {
                            state.fire(UiCommand::RunDoctor);
                            state.show_command_palette.set(false);
                        },
                        "Run doctor"
                    }
                }
            }
        }
    }
}

#[component]
fn PaletteItem(label: &'static str, page: Page) -> Element {
    let mut state: VoltState = use_context();
    rsx! {
        div { style: "padding: 8px 12px; color: {COLOR_TEXT}; font-size: 13px; cursor: pointer; border-radius: 4px;",
            onclick: move |_| {
                state.navigate(page);
                state.show_command_palette.set(false);
            },
            "{label}"
        }
    }
}
