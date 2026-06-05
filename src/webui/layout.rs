use super::commands::UiCommand;
use super::routes::Page;
use super::state::{
    Toast, ToastLevel, VoltState, COLOR_BG, COLOR_BORDER, COLOR_DANGER, COLOR_INFO, COLOR_PANEL,
    COLOR_SUCCESS, COLOR_TEXT, COLOR_TEXT_DIM, COLOR_TEXT_MUTED, COLOR_WARNING, SIDEBAR_WIDTH,
};
use dioxus::prelude::*;

#[component]
pub fn AppLayout() -> Element {
    let state: VoltState = use_context();
    let current = *state.current_page.read();
    let _collapsed = *state.sidebar_collapsed.read();
    let show_trace = *state.show_trace_panel.read();
    let show_palette = *state.show_command_palette.read();

    rsx! {
        div {
            style: "display: flex; height: 100vh; width: 100vw; background-color: {COLOR_BG}; color: {COLOR_TEXT}; font-family: -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif; overflow: hidden;",
            Sidebar {}
            div {
                style: "flex: 1; display: flex; flex-direction: column; min-width: 0; overflow: hidden;",
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
                NavItem { page: Page::Workflows, label: "Workflows", current: current }
                NavItem { page: Page::Worktrees, label: "Worktrees", current: current }
                NavItem { page: Page::Jobs, label: "Jobs", current: current }
                NavItem { page: Page::Routines, label: "Routines", current: current }
                NavItem { page: Page::Skills, label: "Skills", current: current }
                NavItem { page: Page::Registry, label: "Registry", current: current }
                NavItem { page: Page::Audit, label: "Audit", current: current }
                NavItem { page: Page::Settings, label: "Settings", current: current }
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

#[component]
fn Header() -> Element {
    let state: VoltState = use_context();
    let current = *state.current_page.read();
    let model = state.model.read().clone();
    let show_palette = *state.show_command_palette.read();
    let show_trace = *state.show_trace_panel.read();
    rsx! {
        div {
            style: "height: 56px; padding: 0 20px; border-bottom: 1px solid {COLOR_BORDER}; background-color: {COLOR_PANEL}; display: flex; align-items: center; justify-content: space-between;",
            div {
                style: "display: flex; align-items: center; gap: 12px;",
                h1 { style: "margin: 0; font-size: 16px; font-weight: 600; color: {COLOR_TEXT};", "{current.title()}" }
                if current == Page::Chat {
                    span { style: "color: {COLOR_TEXT_MUTED}; font-size: 12px;", "\u{00B7}" }
                    span { style: "color: {COLOR_TEXT_DIM}; font-size: 12px; font-family: monospace;", "{model}" }
                }
            }
            div { style: "display: flex; align-items: center; gap: 8px;",
                HeaderButton { label: "Cmd K".to_string(), active: show_palette, action: HeaderAction::TogglePalette }
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
    rsx! {
        div {
            style: "width: 380px; min-width: 380px; background-color: {COLOR_PANEL}; border-left: 1px solid {COLOR_BORDER}; display: flex; flex-direction: column;",
            div {
                style: "padding: 12px 16px; border-bottom: 1px solid {COLOR_BORDER}; display: flex; align-items: center; justify-content: space-between;",
                span { style: "font-size: 13px; font-weight: 600; color: {COLOR_TEXT};", "Trace" }
                button {
                    style: "background: transparent; border: none; color: {COLOR_TEXT_DIM}; cursor: pointer;",
                    onclick: move |_| state.show_trace_panel.set(false),
                    "\u{2715}"
                }
            }
            div { style: "flex: 1; overflow-y: auto; padding: 8px; font-family: monospace; font-size: 11px;",
                div { style: "color: {COLOR_TEXT_MUTED}; padding: 16px; text-align: center;", "Trace stream will appear here." }
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
