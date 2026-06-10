// `use_resource` returns a future we intentionally don't await —
// we just want it to run on first mount of each page.
#![allow(clippy::let_underscore_future)]

use super::canvas::{CanvasInspector, CanvasQuickAdd, WorkflowCanvas};
use super::commands::{AuditResult, ChatRole, ToolInfo, UiCommand};
use super::routes::Page;
use super::state::{
    ToastLevel, VoltState, COLOR_ACCENT, COLOR_BG, COLOR_BORDER, COLOR_DANGER, COLOR_INFO,
    COLOR_PANEL, COLOR_PANEL_HOVER, COLOR_SUCCESS, COLOR_TEXT, COLOR_TEXT_DIM, COLOR_TEXT_MUTED,
    COLOR_WARNING, FONT_MONO,
};
use dioxus::prelude::*;

pub fn render_page(page: Page) -> Element {
    match page {
        Page::Dashboard => rsx! { DashboardPage {} },
        Page::Chat => rsx! { ChatPage {} },
        Page::Tools => rsx! { ToolsPage {} },
        Page::Sessions => rsx! { SessionsPage {} },
        Page::Settings => rsx! { SettingsPage {} },
        Page::Workflows => rsx! { WorkflowsPage {} },
        Page::Worktrees => rsx! { WorktreesPage {} },
        Page::Jobs => rsx! { JobsPage {} },
        Page::Routines => rsx! { RoutinesPage {} },
        Page::Skills => rsx! { SkillsPage {} },
        Page::Registry => rsx! { RegistryPage {} },
        Page::Audit => rsx! { AuditPage {} },
    }
}

#[component]
pub fn PageHeader(title: &'static str, subtitle: &'static str) -> Element {
    rsx! {
        div { style: "padding: 24px 32px 16px 32px; border-bottom: 1px solid {COLOR_BORDER}; background-color: {COLOR_PANEL};",
            h1 { style: "margin: 0 0 4px 0; font-size: 24px; font-weight: 700; color: {COLOR_TEXT};", "{title}" }
            p { style: "margin: 0; color: {COLOR_TEXT_DIM}; font-size: 13px;", "{subtitle}" }
        }
    }
}

#[component]
pub fn EmptyState(icon: &'static str, title: &'static str, description: &'static str) -> Element {
    rsx! {
        div { style: "display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 80px 32px; color: {COLOR_TEXT_DIM}; text-align: center;",
            div { style: "font-size: 48px; margin-bottom: 16px; opacity: 0.4;", "{icon}" }
            h2 { style: "margin: 0 0 8px 0; font-size: 18px; color: {COLOR_TEXT};", "{title}" }
            p { style: "margin: 0; font-size: 13px; max-width: 400px;", "{description}" }
        }
    }
}

#[component]
pub fn PrimaryButton(label: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { style: "padding: 8px 16px; background-color: {COLOR_ACCENT}; color: white; border: none; border-radius: 6px; font-size: 13px; font-weight: 600; cursor: pointer;",
            onclick: move |e| onclick.call(e),
            "{label}"
        }
    }
}

#[component]
pub fn SecondaryButton(label: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { style: "padding: 8px 16px; background-color: transparent; color: {COLOR_TEXT}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; font-size: 13px; cursor: pointer;",
            onclick: move |e| onclick.call(e),
            "{label}"
        }
    }
}

#[component]
pub fn DangerButton(label: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { style: "padding: 8px 16px; background-color: transparent; color: {COLOR_DANGER}; border: 1px solid {COLOR_DANGER}; border-radius: 6px; font-size: 13px; cursor: pointer;",
            onclick: move |e| onclick.call(e),
            "{label}"
        }
    }
}

#[component]
pub fn LoadingState(message: &'static str) -> Element {
    rsx! { div { style: "display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 32px; color: {COLOR_TEXT_DIM};",
        p { style: "margin: 0; font-size: 13px;", "{message}" }
    } }
}

#[component]
pub fn Panel(title: String, children: Element) -> Element {
    rsx! {
        div { style: "background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; overflow: hidden;",
            div { style: "padding: 12px 16px; border-bottom: 1px solid {COLOR_BORDER};",
                h3 { style: "margin: 0; font-size: 13px; font-weight: 600; color: {COLOR_TEXT};", "{title}" }
            }
            div { style: "padding: 16px;", {children} }
        }
    }
}

#[component]
pub fn StatusRow(label: &'static str, online: bool) -> Element {
    let color = if online { COLOR_SUCCESS } else { COLOR_DANGER };
    let text = if online { "Online" } else { "Offline" };
    rsx! { div { style: "display: flex; align-items: center; justify-content: space-between; padding: 6px 0; font-size: 13px;",
        span { style: "color: {COLOR_TEXT_DIM};", "{label}" }
        div { style: "display: flex; align-items: center; gap: 6px;",
            div { style: "width: 8px; height: 8px; border-radius: 50%; background-color: {color};" }
            span { style: "color: {color}; font-size: 12px;", "{text}" }
        }
    } }
}

#[component]
fn StatCard(label: &'static str, value: String, sub: String, color: String) -> Element {
    rsx! { div { style: "padding: 20px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 8px;",
        div { style: "font-size: 11px; color: {COLOR_TEXT_DIM}; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 8px;", "{label}" }
        div { style: "font-size: 22px; font-weight: 700; color: {color}; font-family: {FONT_MONO}; word-break: break-all;", "{value}" }
        div { style: "font-size: 11px; color: {COLOR_TEXT_MUTED}; margin-top: 4px;", "{sub}" }
    } }
}

#[component]
pub fn DashboardPage() -> Element {
    let mut state: VoltState = use_context();
    let conn_label = state.connection.read().label().to_string();
    let conn_color = state.connection.read().color().to_string();
    let model_name = state.model.read().clone();
    let provider_name = state.provider.read().clone();
    let events = *state.total_events.read();
    let cmds = *state.total_commands.read();
    rsx! {
        PageHeader { title: "Dashboard", subtitle: "Volt runtime health and recent activity" }
        div { style: "padding: 24px 32px;",
            div { style: "display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 16px; margin-bottom: 24px;",
                StatCard { label: "Active Model", value: model_name, sub: provider_name, color: COLOR_ACCENT.to_string() }
                StatCard { label: "Connection", value: conn_label, sub: "Runtime bridge".to_string(), color: conn_color }
                StatCard { label: "Events Processed", value: events.to_string(), sub: "Total".to_string(), color: COLOR_INFO.to_string() }
                StatCard { label: "Commands Sent", value: cmds.to_string(), sub: "From UI".to_string(), color: COLOR_SUCCESS.to_string() }
            }
            div { style: "display: grid; grid-template-columns: 1fr 1fr; gap: 16px;",
                Panel { title: "Quick Actions",
                    div { style: "display: grid; grid-template-columns: 1fr 1fr; gap: 8px;",
                        PrimaryButton { label: "New Chat".to_string(), onclick: move |_| state.navigate(Page::Chat) }
                        PrimaryButton { label: "Run Doctor".to_string(), onclick: move |_| state.fire(UiCommand::RunDoctor) }
                        SecondaryButton { label: "Browse Tools".to_string(), onclick: move |_| state.fire(UiCommand::ListTools) }
                        SecondaryButton { label: "View Sessions".to_string(), onclick: move |_| state.fire(UiCommand::ListSessions) }
                    }
                }
                Panel { title: "System Status",
                    StatusRow { label: "LLM Provider", online: *state.llm_online.read() }
                    StatusRow { label: "Database", online: *state.db_connected.read() }
                    StatusRow { label: "Embedder", online: *state.embedder_loaded.read() }
                    StatusRow { label: "Context Store", online: state.doctor_report.read().as_ref().and_then(|r| r.context_entries.as_ref()).map(|c| !c.is_empty()).unwrap_or(false) }
                }
            }
            div { style: "margin-top: 16px;",
                Panel { title: "Compliance",
                    p { style: "color: {COLOR_TEXT_DIM}; font-size: 12px; line-height: 1.6; margin: 0;",
                        "Agent actions are logged with structured tracing and persisted to PostgreSQL "
                        "(append-only, EU AI Act Art. 12). The 3-kind context store (Tool, Memory, "
                        "Conversation) is auto-seeded from workspace files and tool definitions. "
                        "Sensitive operations require explicit approval via the in-app prompt "
                        "(Art. 14 human oversight). Audit log is available in the Audit section."
                    }
                }
            }
        }
    }
}

#[component]
pub fn ChatPage() -> Element {
    let mut state: VoltState = use_context();
    let mut input = use_signal(String::new);
    let streaming = *state.chat_streaming.read();
    // If the runtime reports cancel/error and we've stashed the
    // user's draft, restore it to the textarea and clear the
    // stash. We do this in a `use_effect` so the effect runs after
    // the streaming flag flips to false.
    {
        use dioxus::prelude::use_effect;
        use_effect(move || {
            let streaming_now = *state.chat_streaming.read();
            if !streaming_now {
                // Take the draft out atomically: read, clone, set
                // None in a separate statement so the read guard
                // is dropped before the set.
                let draft = state.last_user_draft.read().clone();
                if let Some(text) = draft {
                    state.last_user_draft.set(None);
                    input.set(text);
                }
            }
        });
    }
    // Auto-scroll the message container to the bottom whenever a
    // new message arrives or streaming flips. Uses `document::eval`
    // because Dioxus doesn't expose element refs directly. This
    // is a best-effort sticky scroll: it always yanks the user
    // to the latest message, which is fine for the chat UX
    // (long chats can add a "Jump to bottom" pill later).
    {
        use dioxus::document::eval;
        use dioxus::prelude::use_effect;
        use_effect(move || {
            // Re-run when the message count or streaming flag changes.
            let _ = state.chat_messages.read().len();
            let _ = *state.chat_streaming.read();
            eval(
                r#"
                (() => {
                    const el = document.getElementById('volt-chat-messages-bottom');
                    if (el) el.scrollIntoView({ behavior: 'auto', block: 'end' });
                })()
                "#,
            );
        });
    }
    rsx! {
        PageHeader { title: "Chat", subtitle: "Conversational interface with the Volt agent" }
        div { style: "display: flex; flex-direction: column; height: calc(100vh - 56px - 28px - 80px);",
            div { style: "flex: 1; overflow-y: auto; padding: 24px 32px;",
                div { style: "max-width: 900px; margin: 0 auto; display: flex; flex-direction: column; gap: 16px;",
                    {
                        let msgs = state.chat_messages.read();
                        let messages = msgs.clone();
                        if messages.is_empty() {
                            rsx! { EmptyState { icon: "\u{1F4AC}", title: "Start a conversation", description: "Type a message below to chat with the Volt agent. The conversation is persisted to SQLite and replayed next time you load this session." } }
                        } else {
                            rsx! {
                                for m in messages.iter() {
                                    ChatBubble { role: m.role, content: m.content.clone() }
                                }
                            }
                        }
                    }
                    if *state.chat_streaming.read() {
                        div { style: "color: {COLOR_TEXT_DIM}; font-style: italic; font-size: 13px; padding: 0 8px;",
                            "▌ streaming..."
                        }
                    }
                    // Sentinel element the auto-scroll `use_effect`
                    // targets. Always rendered last so we can
                    // `scrollIntoView` it to land at the bottom.
                    div { id: "volt-chat-messages-bottom", style: "height: 1px;" }
                }
            }
            div { style: "padding: 16px 32px; border-top: 1px solid {COLOR_BORDER}; background-color: {COLOR_PANEL};",
                div { style: "max-width: 900px; margin: 0 auto; display: flex; gap: 12px; align-items: flex-end;",
                    textarea {
                        style: "flex: 1; padding: 12px 16px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; color: {COLOR_TEXT}; font-size: 14px; font-family: inherit; resize: none; min-height: 44px; max-height: 200px; outline: none;",
                        placeholder: if streaming { "Waiting for response..." } else { "Type your message..." },
                        value: "{input.read()}",
                        disabled: streaming,
                        oninput: move |e| input.set(e.value().to_string()),
                        onfocus: move |_| state.focus_in_text_input.set(true),
                        onblur: move |_| state.focus_in_text_input.set(false),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter && !e.modifiers().shift() {
                                e.prevent_default();
                                let text = input.read().trim().to_string();
                                if !text.is_empty() {
                                    send_chat_message(&mut state, input, text);
                                }
                            }
                        },
                        rows: "1",
                    }
                    if streaming {
                        button {
                            style: "padding: 8px 16px; background-color: {COLOR_DANGER}; color: white; border: none; border-radius: 6px; font-size: 13px; font-weight: 600; cursor: pointer;",
                            onclick: move |_| state.fire(UiCommand::CancelChat),
                            "Cancel"
                        }
                    } else {
                        button {
                            style: "padding: 8px 16px; background-color: {COLOR_ACCENT}; color: white; border: none; border-radius: 6px; font-size: 13px; font-weight: 600; cursor: pointer;",
                            disabled: input.read().trim().is_empty(),
                            onclick: move |_| {
                                let text = input.read().trim().to_string();
                                if !text.is_empty() {
                                    send_chat_message(&mut state, input, text);
                                }
                            },
                            "Send"
                        }
                    }
                }
                div { style: "max-width: 900px; margin: 8px auto 0 auto; display: flex; gap: 16px; font-size: 11px; color: {COLOR_TEXT_MUTED};",
                    span { "Enter to send, Shift+Enter for newline" }
                    span { "Model: " }
                    span { style: "font-family: {FONT_MONO};", "{state.model.read()}" }
                    div { style: "flex: 1;" }
                    span { "EU AI Act Art. 12 logged" }
                }
            }
        }
    }
}

/// Push a user message into the chat history and dispatch the
/// `Chat` command. Centralised so the textarea-onkeydown path and
/// the Send button share the exact same behaviour. We keep a copy
/// of the typed text in `last_user_draft` so the cancel path can
/// restore it instead of leaving the textarea empty.
fn send_chat_message(state: &mut VoltState, mut input: Signal<String>, text: String) {
    if *state.chat_streaming.read() {
        return;
    }
    let sid = *state.chat_session.read();
    // Stash the input in case the chat is cancelled or fails; the
    // ChatCancelled/ChatError handlers in `app.rs` pop it back.
    state.last_user_draft.set(Some(text.clone()));
    input.set(String::new());
    state.chat_streaming.set(true);
    state.chat_messages.write().push(super::commands::ChatMessage {
        id: uuid::Uuid::new_v4(),
        role: super::commands::ChatRole::User,
        content: text.clone(),
        tool_calls: Vec::new(),
        timestamp: chrono::Utc::now(),
    });
    state.fire(UiCommand::Chat { session_id: sid, input: text });
}

/// Pop the stashed user input back into the chat textarea, if any.

#[component]
fn ChatBubble(role: ChatRole, content: String) -> Element {
    let is_user = role == ChatRole::User;
    let is_tool = role == ChatRole::Tool;
    let bg = if is_user { COLOR_PANEL_HOVER } else { COLOR_PANEL };
    let label = if is_user { "You" } else if is_tool { "Tool" } else { "Volt" };
    let label_color = if is_user { COLOR_ACCENT } else if is_tool { COLOR_INFO } else { COLOR_SUCCESS };
    rsx! {
        div { style: "display: flex; flex-direction: column; gap: 4px;",
            span { style: "color: {label_color}; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em;",
                "{label}"
            }
            div { style: "background-color: {bg}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; padding: 12px 16px; color: {COLOR_TEXT}; font-size: 14px; line-height: 1.6; white-space: pre-wrap; word-wrap: break-word;",
                "{content}"
            }
        }
    }
}

#[component]
pub fn ToolsPage() -> Element {
    let mut state: VoltState = use_context();
    let mut filter = use_signal(String::new);
    let selected = use_signal(|| None::<ToolInfo>);
    let mut show_all = use_signal(|| false);
    {
        // Auto-load on first mount.
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListTools);
        });
    }
    rsx! {
        PageHeader { title: "Tools", subtitle: "Live tool registry with schema browser and direct execution" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 12px; margin-bottom: 16px; align-items: center;",
                input { style: "flex: 1; max-width: 400px; padding: 8px 12px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; outline: none;",
                    placeholder: "Filter tools...",
                    value: "{filter.read()}",
                    oninput: move |e| filter.set(e.value().to_string()),
                    onfocus: move |_| state.focus_in_text_input.set(true),
                    onblur: move |_| state.focus_in_text_input.set(false),
                }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListTools) }
                label { style: "display: flex; align-items: center; gap: 6px; font-size: 12px; color: {COLOR_TEXT_DIM}; cursor: pointer; white-space: nowrap;",
                    input { r#type: "checkbox", style: "accent-color: {COLOR_ACCENT};",
                        checked: "{show_all()}",
                        oninput: move |_| {
                        let next = !*show_all.read();
                        show_all.set(next);
                    },
                    }
                    {format_args!("Show all disabled ({})", state.tools.read().len())}
                }
            }
            div { style: "display: grid; grid-template-columns: 1fr 1fr; gap: 16px;",
                ToolList { filter: filter, selected: selected, show_all: show_all }
                ToolDetail { selected: selected }
            }
        }
    }
}

#[component]
fn ToolList(filter: Signal<String>, selected: Signal<Option<ToolInfo>>, show_all: Signal<bool>) -> Element {
    let state: VoltState = use_context();
    let tools = state.tools.read().clone();
    let needle = filter.read().to_lowercase();
    let all = *show_all.read();
    let active_count = tools.iter().filter(|t| t.enabled).count();
    let visible: Vec<_> = tools
        .iter()
        .filter(|t| {
            if !all && !t.enabled { return false; }
            needle.is_empty()
                || t.name.to_lowercase().contains(&needle)
                || t.description.to_lowercase().contains(&needle)
                || t.category.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect();
    let title = if all {
        format!("All Tools ({})", tools.len())
    } else {
        format!("Active Tools ({})", active_count)
    };
    rsx! {
        Panel { title: title,
            if tools.is_empty() {
                div { style: "color: {COLOR_TEXT_DIM}; font-size: 13px; padding: 20px 0; text-align: center;",
                    "No tools registered yet. Click Refresh to fetch the registry."
                }
            } else if visible.is_empty() {
                div { style: "color: {COLOR_TEXT_DIM}; font-size: 13px; padding: 20px 0; text-align: center;",
                    "No tools match this filter."
                }
            } else {
                div { style: "display: flex; flex-direction: column; gap: 4px; max-height: 600px; overflow-y: auto;",
                    for t in visible.iter() {
                        ToolRow { tool: t.clone(), selected: selected }
                    }
                }
            }
        }
    }
}

#[component]
fn ToolRow(tool: ToolInfo, selected: Signal<Option<ToolInfo>>) -> Element {
    let is_selected = selected.read().as_ref().map(|s| s.name == tool.name).unwrap_or(false);
    let bg = if is_selected { COLOR_PANEL_HOVER } else { COLOR_PANEL };
    let perm_color = match tool.permission {
        super::commands::ToolPermission::Allow => COLOR_SUCCESS,
        super::commands::ToolPermission::Prompt => COLOR_WARNING,
        super::commands::ToolPermission::Deny => COLOR_DANGER,
    };
    let opacity = if tool.enabled { "1" } else { "0.5" };
    rsx! {
        div {
            style: "padding: 10px 12px; background-color: {bg}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; cursor: pointer; display: flex; flex-direction: column; gap: 4px; opacity: {opacity};",
            onclick: move |_| selected.set(Some(tool.clone())),
            div { style: "display: flex; align-items: center; gap: 8px;",
                span { style: "font-family: monospace; font-size: 13px; color: {COLOR_TEXT}; font-weight: 600; flex: 1; word-break: break-all;",
                    "{tool.name}"
                }
                if !tool.enabled {
                    span { style: "font-size: 10px; padding: 2px 6px; border-radius: 3px; background-color: rgba(239,68,68,0.15); color: {COLOR_DANGER}; text-transform: uppercase;",
                        "BLOCKED"
                    }
                }
                span { style: "font-size: 10px; padding: 2px 6px; border-radius: 3px; background-color: rgba(168,85,247,0.15); color: {perm_color}; text-transform: uppercase;",
                    "{tool.permission}"
                }
            }
            div { style: "font-size: 11px; color: {COLOR_TEXT_DIM}; line-height: 1.4;",
                "{tool.description}"
            }
        }
    }
}

#[component]
fn ToolDetail(selected: Signal<Option<ToolInfo>>) -> Element {
    let sel = selected.read().clone();
    rsx! {
        Panel { title: "Tool Detail",
            if let Some(tool) = sel {
                div {
                    h2 { style: "margin: 0 0 8px 0; color: {COLOR_TEXT}; font-size: 18px; font-family: {FONT_MONO};", "{tool.name}" }
                    p { style: "margin: 0 0 12px 0; color: {COLOR_TEXT_DIM}; font-size: 13px;", "{tool.description}" }
                    div { style: "margin-bottom: 12px;",
                        span { style: "display: inline-block; padding: 2px 8px; background-color: {COLOR_PANEL_HOVER}; border-radius: 4px; font-size: 11px; color: {COLOR_TEXT_DIM}; margin-right: 8px;", "{tool.category}" }
                        span { style: "display: inline-block; padding: 2px 8px; background-color: rgba(168,85,247,0.15); border-radius: 4px; font-size: 11px; color: {COLOR_ACCENT};", "{tool.permission}" }
                    }
                    h3 { style: "margin: 16px 0 8px 0; font-size: 13px; color: {COLOR_TEXT};", "Schema" }
                    pre { style: "background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; padding: 12px; font-size: 11px; color: {COLOR_TEXT_DIM}; overflow-x: auto; max-height: 300px;",
                        code { "{serde_json::to_string_pretty(&tool.schema).unwrap_or_default()}" }
                    }
                }
            } else {
                p { style: "color: {COLOR_TEXT_DIM}; font-size: 13px; text-align: center; padding: 40px 0;", "Select a tool to see details." }
            }
        }
    }
}

#[component]
pub fn SessionsPage() -> Element {
    let mut state: VoltState = use_context();
    let mut new_name = use_signal(String::new);
    let mut show_new = use_signal(|| false);
    {
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListSessions);
        });
    }
    rsx! {
        PageHeader { title: "Sessions", subtitle: "Conversation history with load, fork, and delete" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 12px; margin-bottom: 16px; align-items: center;",
                PrimaryButton { label: "+ New Session".to_string(), onclick: move |_| show_new.set(true) }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListSessions) }
            }
            if *show_new.read() {
                div { style: "background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; padding: 16px; margin-bottom: 16px;",
                    h3 { style: "margin: 0 0 12px 0; color: {COLOR_TEXT}; font-size: 14px;", "Create new session" }
                    div { style: "display: flex; gap: 8px;",
                        input { style: "flex: 1; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; outline: none;",
                            placeholder: "Session name...",
                            value: "{new_name.read()}",
                            oninput: move |e| new_name.set(e.value().to_string()),
                        }
                        PrimaryButton { label: "Create".to_string(), onclick: move |_| {
                            let name = new_name.read().clone();
                            if !name.is_empty() {
                                state.fire(UiCommand::CreateSession { name });
                                new_name.set(String::new());
                                show_new.set(false);
                            }
                        }}
                        SecondaryButton { label: "Cancel".to_string(), onclick: move |_| show_new.set(false) }
                    }
                }
            }
            SessionsList {}
        }
    }
}

#[component]
fn SessionsList() -> Element {
    // `state.sessions_cache` is already populated by the global event
    // loop in `app.rs` whenever `SessionsListed` arrives. Reading it
    // here gives us a reactive view of the latest snapshot without
    // opening a second broadcast subscriber (which would race the
    // global one and waste memory).
    let mut state: VoltState = use_context();
    let mut confirm_delete: Signal<Option<uuid::Uuid>> = use_signal(|| None);
    let cached = state.sessions_cache.read().clone();
    if cached.is_empty() {
        rsx! { EmptyState { icon: "\u{1F4C1}", title: "No sessions yet", description: "Start a chat to create your first session, or click 'New Session' to create one." } }
    } else {
        rsx! {
            div { style: "display: flex; flex-direction: column; gap: 8px;",
                for s in cached.iter() {
                    Panel { title: s.name.clone(),
                        div { style: "display: flex; gap: 16px; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 8px; flex-wrap: wrap;",
                            span { "ID: " }
                            span { style: "font-family: {FONT_MONO}; color: {COLOR_TEXT};", "{&s.id.to_string()[..8]}" }
                            span { "•" }
                            span { "Created: {s.created_at}" }
                            span { "•" }
                            span { "Updated: {s.updated_at}" }
                            span { "•" }
                            span { "{s.message_count} messages" }
                        }
                        div { style: "display: flex; gap: 8px;",
                            PrimaryButton { label: "Load".to_string(), onclick: {
                                let id = s.id;
                                move |_| {
                                    state.fire(UiCommand::LoadSession { id });
                                    state.navigate(super::routes::Page::Chat);
                                }
                            }}
                            SecondaryButton { label: "Fork".to_string(), onclick: {
                                let id = s.id;
                                move |_| state.fire(UiCommand::ForkSession { id })
                            }}
                            DangerButton {
                                label: if *confirm_delete.read() == Some(s.id) { "Confirm?" } else { "Delete".to_string() },
                                onclick: {
                                    let id = s.id;
                                    move |_| {
                                        if *confirm_delete.read() == Some(id) {
                                            state.fire(UiCommand::DeleteSession { id });
                                            confirm_delete.set(None);
                                        } else {
                                            confirm_delete.set(Some(id));
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
}

#[component]
pub fn SettingsPage() -> Element {
    let mut state: VoltState = use_context();
    {
        // Auto-load the current runtime config + model registry on mount
        let _ = use_resource(move || async move {
            state.fire(UiCommand::GetConfig);
            state.fire(UiCommand::ListModels);
            state.fire(UiCommand::RunDoctor);
        });
    }
    let initial_model = state.model.read().clone();
    let initial_provider = state.provider.read().clone();
    let mut model_input = use_signal(move || initial_model);
    let mut provider_input = use_signal(move || initial_provider);
    let models = state.models.read().clone();
    let doctor = state.doctor_report.read().clone();
    rsx! {
        PageHeader { title: "Settings", subtitle: "Configure LLM provider, model, and runtime options" }
        div { style: "padding: 24px 32px; max-width: 800px;",
            Panel { title: "LLM Configuration",
                div { style: "display: flex; flex-direction: column; gap: 16px;",
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Provider" }
                        select { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            value: "{provider_input.read()}",
                            onchange: move |e| provider_input.set(e.value().to_string()),
                            option { value: "groq", "Groq" }
                            option { value: "openai", "OpenAI" }
                            option { value: "anthropic", "Anthropic" }
                            option { value: "ollama", "Ollama" }
                            option { value: "nvidia", "NVIDIA NIM" }
                        }
                    }
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Model" }
                        input { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                            value: "{model_input.read()}",
                            oninput: move |e| model_input.set(e.value().to_string()),
                        }
                    }
                    div { style: "display: flex; gap: 8px;",
                        PrimaryButton { label: "Save".to_string(), onclick: move |_| {
                            let m = model_input.read().clone();
                            let p = provider_input.read().clone();
                            state.model.set(m.clone());
                            state.provider.set(p.clone());
                            // Persist to the runtime via UpdateConfig
                            let patch = serde_json::json!({
                                "default_model": m,
                                "default_provider": p,
                            });
                            state.fire(UiCommand::UpdateConfig { patch });
                            // Re-fetch config + models so the UI
                            // reflects the new value rather than
                            // looking like a no-op.
                            state.fire(UiCommand::GetConfig);
                            state.fire(UiCommand::ListModels);
                            state.toast(ToastLevel::Success, "Settings saved");
                        }}
                        SecondaryButton { label: "Run Doctor".to_string(), onclick: move |_| state.fire(UiCommand::RunDoctor) }
                    }
                }
            }
            div { style: "margin-top: 16px;",
                Panel { title: format!("Available Models ({})", models.len()),
                    if models.is_empty() {
                        p { style: "margin: 0; color: {COLOR_TEXT_DIM}; font-size: 12px;", "No models discovered yet. Make sure your LLM API keys are set, then click Refresh below." }
                    } else {
                        div { style: "display: flex; flex-direction: column; gap: 4px; max-height: 240px; overflow-y: auto;",
                            for m in models.iter() {
                                div {
                                    style: "padding: 8px 12px; background-color: {COLOR_PANEL_HOVER}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; display: flex; align-items: center; gap: 12px; cursor: pointer;",
                                    onclick: {
                                        let mid = m.id.clone();
                                        let pid = m.provider.clone();
                                        move |_| {
                                            model_input.set(mid.clone());
                                            provider_input.set(pid.clone());
                                        }
                                    },
                                    div { style: "flex: 1; min-width: 0;",
                                        div { style: "font-family: monospace; font-size: 12px; color: {COLOR_TEXT}; word-break: break-all;",
                                            "{m.id}"
                                        }
                                        div { style: "font-size: 11px; color: {COLOR_TEXT_DIM};",
                                            "Provider: {m.provider} \u{00B7} {m.context_window} tokens"
                                        }
                                    }
                                    if m.available {
                                        span { style: "color: {COLOR_SUCCESS}; font-size: 11px; padding: 2px 6px; background-color: rgba(34,197,94,0.1); border-radius: 3px;",
                                            "available"
                                        }
                                    } else {
                                        span { style: "color: {COLOR_WARNING}; font-size: 11px; padding: 2px 6px; background-color: rgba(245,158,11,0.1); border-radius: 3px;",
                                            "no key"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { style: "display: flex; gap: 8px; margin-top: 12px;",
                        SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListModels) }
                    }
                }
            }
            div { style: "margin-top: 16px;",
                Panel { title: "API Keys",
                    p { style: "margin: 0 0 12px 0; color: {COLOR_TEXT_DIM}; font-size: 12px;",
                        "Paste keys directly here. They are written to your .env and the runtime picks them up on the next request. \
                         Placeholder values (like 'your_*_here') are rejected."
                    }
                    div { style: "display: flex; flex-direction: column; gap: 8px;",
                        ApiKeyRow { slug: "groq".to_string(), display_name: "Groq".to_string() }
                        ApiKeyRow { slug: "nvidia".to_string(), display_name: "NVIDIA NIM".to_string() }
                        ApiKeyRow { slug: "openai".to_string(), display_name: "OpenAI".to_string() }
                        ApiKeyRow { slug: "anthropic".to_string(), display_name: "Anthropic".to_string() }
                        ApiKeyRow { slug: "ollama".to_string(), display_name: "Ollama Cloud".to_string() }
                        ApiKeyRow { slug: "moonshot".to_string(), display_name: "Moonshot / Kimi".to_string() }
                    }
                }
            }
            div { style: "margin-top: 16px;",
                Panel { title: "Environment",
                    p { style: "margin: 0 0 8px 0; color: {COLOR_TEXT_DIM}; font-size: 12px;", "API keys are loaded from .env or system environment. Use 'volt doctor' to check status." }
                    p { style: "margin: 0 0 12px 0; color: {COLOR_TEXT_MUTED}; font-size: 11px; font-family: {FONT_MONO};", "GROQ_API_KEY, OPENAI_API_KEY, ANTHROPIC_API_KEY, NVIDIA_API_KEY, OLLAMA_API_KEY, HF_TOKEN, YOUCOM_API_KEY" }
                    div { style: "display: flex; gap: 8px;",
                        SecondaryButton {
                            label: "Run API Key Setup".to_string(),
                            onclick: move |_| {
                                // Reopen the wizard. If the runtime is
                                // already configured the wizard will
                                // still let the user change provider.
                                let providers = vec![
                                    crate::webui::commands::ProviderInfo {
                                        slug: "groq".into(),
                                        label: "Groq \u{2014} fast cloud inference (free tier)".into(),
                                        env_var: crate::config::provider_env_var("groq"),
                                        default_model: crate::config::default_model_for_provider("groq").into(),
                                    },
                                    crate::webui::commands::ProviderInfo {
                                        slug: "openai".into(),
                                        label: "OpenAI \u{2014} GPT-4o, GPT-4o-mini".into(),
                                        env_var: crate::config::provider_env_var("openai"),
                                        default_model: crate::config::default_model_for_provider("openai").into(),
                                    },
                                    crate::webui::commands::ProviderInfo {
                                        slug: "anthropic".into(),
                                        label: "Anthropic \u{2014} Claude Sonnet 4.5".into(),
                                        env_var: crate::config::provider_env_var("anthropic"),
                                        default_model: crate::config::default_model_for_provider("anthropic").into(),
                                    },
                                    crate::webui::commands::ProviderInfo {
                                        slug: "nvidia".into(),
                                        label: "NVIDIA NIM \u{2014} hosted open models".into(),
                                        env_var: crate::config::provider_env_var("nvidia"),
                                        default_model: crate::config::default_model_for_provider("nvidia").into(),
                                    },
                                    crate::webui::commands::ProviderInfo {
                                        slug: "ollama".into(),
                                        label: "Ollama \u{2014} local or cloud (OLLAMA_API_KEY)".into(),
                                        env_var: crate::config::provider_env_var("ollama"),
                                        default_model: crate::config::default_model_for_provider("ollama").into(),
                                    },
                                ];
                                state.setup_providers.set(providers);
                                state.show_setup_wizard.set(true);
                            }
                        }
                        SecondaryButton { label: "Run Doctor".to_string(), onclick: move |_| state.fire(UiCommand::RunDoctor) }
                    }
                }
            }
            if let Some(report) = doctor {
                div { style: "margin-top: 16px;",
                    Panel { title: "Doctor Report",
                        div { style: "display: grid; grid-template-columns: 1fr 1fr; gap: 8px; font-size: 12px;",
                            span { style: "color: {COLOR_TEXT_DIM};", "OS" }
                            span { style: "color: {COLOR_TEXT}; font-family: monospace;", "{report.os} ({report.arch})" }
                            span { style: "color: {COLOR_TEXT_DIM};", "Rust channel" }
                            span { style: "color: {COLOR_TEXT}; font-family: monospace;", "{report.rust_channel}" }
                            span { style: "color: {COLOR_TEXT_DIM};", "Database" }
                            span { style: "color: {COLOR_TEXT}; font-family: monospace;", "{report.database}" }
                            span { style: "color: {COLOR_TEXT_DIM};", "Embedder" }
                            span { style: "color: {COLOR_TEXT}; font-family: monospace;", "{report.embedder_provider} / {report.embedder_model}" }
                            span { style: "color: {COLOR_TEXT_DIM};", "Disk free" }
                            span { style: "color: {COLOR_TEXT};", "{report.disk_free_gb:.1} GB" }
                            if let Some(ref ctx) = report.context_entries {
                                span { style: "color: {COLOR_TEXT_DIM};", "Context Store" }
                                span { style: "color: {COLOR_TEXT}; font-family: monospace;",
                                    {ctx.iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<_>>().join(", ")}
                                }
                            }
                        }
                        div { style: "margin-top: 12px;",
                            div { style: "font-size: 11px; color: {COLOR_TEXT_MUTED}; text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 6px;",
                                "API Keys"
                            }
                            div { style: "display: flex; flex-direction: column; gap: 3px;",
                                for k in report.api_keys.iter() {
                                    div { style: "display: flex; justify-content: space-between; padding: 4px 8px; background-color: {COLOR_PANEL_HOVER}; border-radius: 4px; font-family: monospace; font-size: 11px;",
                                        span { style: "color: {COLOR_TEXT};", "{k.name}" }
                                        span { style: "color: {COLOR_TEXT_DIM};", "{k.masked}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { style: "margin-top: 16px;",
                Panel { title: "Permissions",
                    p { style: "margin: 0; color: {COLOR_TEXT_DIM}; font-size: 12px;", "Read-only tools are auto-allowed. Write, network, and system tools require explicit per-call approval via the in-app prompt. All approvals are recorded in the audit log." }
                }
            }
        }
    }
}

#[component]
pub fn WorktreesPage() -> Element {
    let mut state: VoltState = use_context();
    {
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListWorktrees);
        });
    }
    let worktrees = state.worktrees.read().clone();
    rsx! {
        PageHeader { title: "Worktrees", subtitle: "Git worktree sessions from agent runs" }
        div { style: "padding: 24px 32px;",
            div { style: "margin-bottom: 16px;",
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListWorktrees) }
            }
            if worktrees.is_empty() {
                EmptyState { icon: "\u{1F33F}", title: "No worktrees", description: "Run an agent with --worktree to create an isolated worktree session." }
            } else {
                div { style: "display: flex; flex-direction: column; gap: 8px;",
                    for w in worktrees.iter() {
                        Panel { title: format!("{} (+{} commits)", w.branch, w.commits_ahead),
                            div { style: "display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: {COLOR_TEXT_DIM};",
                                span { "Path: " }
                                span { style: "font-family: monospace; color: {COLOR_TEXT}; word-break: break-all;", "{w.path}" }
                                span { "Session: " }
                                span { style: "font-family: monospace; color: {COLOR_TEXT};", "{w.session_id}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn JobsPage() -> Element {
    let mut state: VoltState = use_context();
    let mut new_desc = use_signal(String::new);
    let mut show_create = use_signal(|| false);
    {
        // Auto-load on first mount.
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListJobs);
        });
    }
    rsx! {
        PageHeader { title: "Jobs", subtitle: "Scheduled background jobs (Postgres)" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 8px; margin-bottom: 16px;",
                PrimaryButton { label: "New Job".to_string(), onclick: move |_| show_create.set(true) }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListJobs) }
            }
            if *show_create.read() {
                Panel { title: "Create Job",
                    div { style: "display: flex; gap: 8px; margin-bottom: 12px;",
                        input { style: "flex: 1; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            placeholder: "Job description...",
                            value: "{new_desc.read()}",
                            oninput: move |e| new_desc.set(e.value().to_string()),
                        }
                        PrimaryButton { label: "Create".to_string(), onclick: move |_| {
                            let d = new_desc.read().clone();
                            if !d.is_empty() {
                                state.fire(UiCommand::CreateJob { description: d });
                                new_desc.set(String::new());
                                show_create.set(false);
                            }
                        }}
                        SecondaryButton { label: "Cancel".to_string(), onclick: move |_| show_create.set(false) }
                    }
                }
            }
            {
                let jobs = state.jobs.read();
                if jobs.is_empty() {
                    rsx! { EmptyState { icon: "\u{23F0}", title: "No scheduled jobs", description: "Create one above or use 'volt jobs add' from the CLI. Jobs run when the daemon is active." } }
                } else {
                    rsx! {
                        div { style: "display: flex; flex-direction: column; gap: 8px;",
                            for j in jobs.iter() {
                                Panel { title: format!("{} ({})", j.name, j.last_status),
                                    div { style: "display: flex; gap: 16px; font-size: 12px; color: {COLOR_TEXT_DIM};",
                                        span { "ID: " }, span { style: "font-family: {FONT_MONO}; color: {COLOR_TEXT};", "{crate::webui::app::short_id(&j.id)}" }
                                        span { "•" }
                                        span { "Attempts: {j.attempt_count}" }
                                        if let Some(w) = &j.worker_id { span { "•" } span { "Worker: {w}" } }
                                        span { "•" }
                                        span { "Created: {j.created_at}" }
                                    }
                                    div { style: "display: flex; gap: 8px; margin-top: 12px;",
                                        PrimaryButton { label: "Start".to_string(), onclick: {
                                            let id_str = j.id.clone();
                                            move |_| {
                                                if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                                                    state.fire(UiCommand::StartJob { id: uuid, worker_id: Some("webui".into()) });
                                                }
                                            }
                                        }}
                                        SecondaryButton { label: "Complete".to_string(), onclick: {
                                            let id_str = j.id.clone();
                                            move |_| {
                                                if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                                                    state.fire(UiCommand::CompleteJob { id: uuid, output: "completed from webui".into() });
                                                }
                                            }
                                        }}
                                        DangerButton { label: "Fail\u{2026}".to_string(), onclick: {
                                            let id_str = j.id.clone();
                                            let prompt = format!(
                                                "Fail job {} ({}): reason?",
                                                &id_str[..8.min(id_str.len())],
                                                j.name,
                                            );
                                            // Native browser confirm() is
                                            // unavailable in the Dioxus
                                            // webview; the inline two-step
                                            // pattern from SessionsList is
                                            // the right answer. We
                                            // short-circuit to a toast
                                            // that prompts for the reason.
                                            move |_| {
                                                if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                                                    state.toast(
                                                        ToastLevel::Warning,
                                                        format!(
                                                            "{} Reply with `volt jobs fail {} <reason>` to record the failure.",
                                                            prompt, uuid,
                                                        ),
                                                    );
                                                }
                                            }
                                        }}
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
pub fn RoutinesPage() -> Element {
    let mut state: VoltState = use_context();
    let mut new_name = use_signal(String::new);
    let mut new_prompt = use_signal(String::new);
    let mut new_cron = use_signal(String::new);
    let mut show_create = use_signal(|| false);
    {
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListRoutines);
        });
    }
    rsx! {
        PageHeader { title: "Routines", subtitle: "Event-triggered routines (Postgres)" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 8px; margin-bottom: 16px;",
                PrimaryButton { label: "New Routine".to_string(), onclick: move |_| show_create.set(true) }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListRoutines) }
            }
            if *show_create.read() {
                Panel { title: "Create Routine",
                    div { style: "display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px;",
                        input { style: "padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            placeholder: "Name (e.g. daily-cleanup)",
                            value: "{new_name.read()}",
                            oninput: move |e| new_name.set(e.value().to_string()),
                        }
                        input { style: "padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            placeholder: "Action prompt (what the agent does)",
                            value: "{new_prompt.read()}",
                            oninput: move |e| new_prompt.set(e.value().to_string()),
                        }
                        input { style: "padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                            placeholder: "Cron expression (optional, e.g. 0 9 * * *)",
                            value: "{new_cron.read()}",
                            oninput: move |e| new_cron.set(e.value().to_string()),
                        }
                        div { style: "display: flex; gap: 8px;",
                            PrimaryButton { label: "Create".to_string(), onclick: move |_| {
                                let n = new_name.read().clone();
                                let p = new_prompt.read().clone();
                                let c = new_cron.read().clone();
                                if !n.is_empty() && !p.is_empty() {
                                    state.fire(UiCommand::CreateRoutine {
                                        name: n,
                                        action_prompt: p,
                                        cron: if c.is_empty() { None } else { Some(c) },
                                        trigger_type: Some("cron".into()),
                                    });
                                    new_name.set(String::new());
                                    new_prompt.set(String::new());
                                    new_cron.set(String::new());
                                    show_create.set(false);
                                }
                            }}
                            SecondaryButton { label: "Cancel".to_string(), onclick: move |_| show_create.set(false) }
                        }
                    }
                }
            }
            {
                let routines = state.routines.read();
                if routines.is_empty() {
                    rsx! { EmptyState { icon: "\u{1F4A1}", title: "No routines", description: "Create one above. Routines trigger on cron events or external webhooks." } }
                } else {
                    rsx! {
                        div { style: "display: flex; flex-direction: column; gap: 8px;",
                            for r in routines.iter() {
                                Panel { title: format!("{} ({})", r.name, if r.enabled { "enabled" } else { "disabled" }),
                                    div { style: "font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 8px;",
                                        span { "Trigger: {r.trigger}" }
                                        span { " • ID: " }
                                        span { style: "font-family: {FONT_MONO};", "{crate::webui::app::short_id(&r.id)}" }
                                    }
                                    div { style: "font-size: 13px; color: {COLOR_TEXT}; margin-bottom: 12px;",
                                        "{r.action_prompt}"
                                    }
                                    div { style: "display: flex; gap: 8px;",
                                        if r.enabled {
                                            SecondaryButton { label: "Disable".to_string(), onclick: {
                                                let id_str = r.id.clone();
                                                move |_| {
                                                    if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                                                        state.fire(UiCommand::ToggleRoutine { id: uuid, enabled: false });
                                                    }
                                                }
                                            }}
                                        } else {
                                            PrimaryButton { label: "Enable".to_string(), onclick: {
                                                let id_str = r.id.clone();
                                                move |_| {
                                                    if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                                                        state.fire(UiCommand::ToggleRoutine { id: uuid, enabled: true });
                                                    }
                                                }
                                            }}
                                        }
                                        DangerButton { label: "Delete".to_string(), onclick: {
                                            let id_str = r.id.clone();
                                            move |_| {
                                                if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                                                    state.fire(UiCommand::DeleteRoutine { id: uuid });
                                                }
                                            }
                                        }}
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
pub fn SkillsPage() -> Element {
    let mut state: VoltState = use_context();
    let mut search_query = use_signal(String::new);
    let mut show_search = use_signal(|| false);
    let mut show_import = use_signal(|| false);
    let mut import_path = use_signal(String::new);
    {
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListSkills);
        });
    }
    rsx! {
        PageHeader { title: "Skills", subtitle: "Reusable skill manifests (local, catalog, imports)" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 8px; margin-bottom: 16px;",
                PrimaryButton { label: "Browse Catalog".to_string(), onclick: move |_| show_search.set(true) }
                SecondaryButton { label: "Import from File".to_string(), onclick: move |_| show_import.set(true) }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListSkills) }
            }
            if *show_search.read() {
                Panel { title: "Catalog Search",
                    div { style: "display: flex; gap: 8px; margin-bottom: 12px; align-items: center;",
                        input { style: "flex: 1; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            placeholder: "Search skills...",
                            value: "{search_query.read()}",
                            oninput: move |e| search_query.set(e.value().to_string()),
                        }
                        PrimaryButton { label: "Search".to_string(), onclick: move |_| {
                            let q = search_query.read().clone();
                            if !q.is_empty() {
                                state.fire(UiCommand::SearchCatalogSkills { query: q });
                            }
                        }}
                        SecondaryButton { label: "Close".to_string(), onclick: move |_| {
                            // Clear stale results so reopening
                            // doesn't show the previous query.
                            state.catalog_results.set(Vec::new());
                            state.catalog_query.set(String::new());
                            search_query.set(String::new());
                            show_search.set(false);
                        }}
                    }
                    {
                        let results = state.catalog_results.read();
                        if !results.is_empty() {
                            rsx! {
                                div { style: "display: flex; flex-direction: column; gap: 6px; margin-top: 8px;",
                                    for c in results.iter() {
                                        div { style: "display: flex; align-items: center; gap: 12px; padding: 8px 12px; background-color: {COLOR_PANEL}; border-radius: 6px;",
                                            div { style: "flex: 1;",
                                                div { style: "color: {COLOR_TEXT}; font-size: 13px;", "{c.name}" }
                                                div { style: "color: {COLOR_TEXT_DIM}; font-size: 11px;", "{c.description}" }
                                            }
                                            PrimaryButton { label: "Install".to_string(), onclick: {
                                                let n = c.name.clone();
                                                move |_| state.fire(UiCommand::InstallSkill { name: n.clone() })
                                            }}
                                        }
                                    }
                                }
                            }
                        } else if !state.catalog_query.read().is_empty() {
                            rsx! { div { style: "color: {COLOR_TEXT_DIM}; font-size: 12px; margin-top: 8px;", "No matches." } }
                        } else {
                            rsx! { div { style: "color: {COLOR_TEXT_DIM}; font-size: 12px; margin-top: 8px;", "Type a query and press Search." } }
                        }
                    }
                }
            }
            if *show_import.read() {
                Panel { title: "Import Skill",
                    div { style: "display: flex; gap: 8px; margin-bottom: 12px;",
                        input { style: "flex: 1; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                            placeholder: "/path/to/skill.toml",
                            value: "{import_path.read()}",
                            oninput: move |e| import_path.set(e.value().to_string()),
                        }
                        PrimaryButton { label: "Import".to_string(), onclick: move |_| {
                            let p = import_path.read().clone();
                            if !p.is_empty() {
                                state.fire(UiCommand::ImportSkill { path: p });
                                import_path.set(String::new());
                                show_import.set(false);
                            }
                        }}
                        SecondaryButton { label: "Cancel".to_string(), onclick: move |_| show_import.set(false) }
                    }
                }
            }
            {
                let skills = state.skills.read();
                if skills.is_empty() {
                    rsx! { EmptyState { icon: "\u{2728}", title: "No skills installed", description: "Browse the catalog to install skills, or import from a local TOML/JSON file." } }
                } else {
                    rsx! {
                        div { style: "display: flex; flex-direction: column; gap: 6px;",
                            for s in skills.iter() {
                                Panel { title: format!("{} ({})", s.name, s.source),
                                    div { style: "font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;",
                                        "v{s.version} • installed {s.installed_at}"
                                    }
                                    div { style: "font-size: 13px; color: {COLOR_TEXT}; margin-bottom: 8px;",
                                        "{s.description}"
                                    }
                                    DangerButton { label: "Uninstall".to_string(), onclick: {
                                        let n = s.name.clone();
                                        move |_| state.fire(UiCommand::UninstallSkill { name: n.clone() })
                                    }}
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
pub fn RegistryPage() -> Element {
    let mut state: VoltState = use_context();
    let mut new_name = use_signal(String::new);
    let mut new_transport = use_signal(|| "stdio".to_string());
    let mut new_command = use_signal(String::new);
    let mut new_url = use_signal(String::new);
    let mut show_create = use_signal(|| false);
    {
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListMcpServers);
        });
    }
    rsx! {
        PageHeader { title: "MCP Registry", subtitle: "Model Context Protocol server connections (~/.volt/mcp_servers.json)" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 8px; margin-bottom: 16px;",
                PrimaryButton { label: "Register Server".to_string(), onclick: move |_| show_create.set(true) }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListMcpServers) }
            }
            if *show_create.read() {
                Panel { title: "Register MCP Server",
                    div { style: "display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px;",
                        input { style: "padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            placeholder: "Server name",
                            value: "{new_name.read()}",
                            oninput: move |e| new_name.set(e.value().to_string()),
                        }
                        select { style: "padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            value: "{new_transport.read()}",
                            onchange: move |e| new_transport.set(e.value().to_string()),
                            option { value: "stdio", "stdio" }
                            option { value: "http", "http" }
                            option { value: "websocket", "websocket" }
                            option { value: "grpc", "grpc" }
                        }
                        if new_transport.read().as_str() == "stdio" {
                            input { style: "padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                                placeholder: "Command (e.g. himalaya-mcp --stdio)",
                                value: "{new_command.read()}",
                                oninput: move |e| new_command.set(e.value().to_string()),
                            }
                        } else {
                            input { style: "padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                                placeholder: "URL (e.g. https://mcp.example.com)",
                                value: "{new_url.read()}",
                                oninput: move |e| new_url.set(e.value().to_string()),
                            }
                        }
                        div { style: "display: flex; gap: 8px;",
                            PrimaryButton { label: "Register".to_string(), onclick: move |_| {
                                let n = new_name.read().clone();
                                let t = new_transport.read().clone();
                                let c = new_command.read().clone();
                                let u = new_url.read().clone();
                                if !n.is_empty() {
                                    state.fire(UiCommand::RegisterMcpServer {
                                        name: n,
                                        transport: t,
                                        command: if c.is_empty() { None } else { Some(c) },
                                        url: if u.is_empty() { None } else { Some(u) },
                                    });
                                    new_name.set(String::new());
                                    new_command.set(String::new());
                                    new_url.set(String::new());
                                    show_create.set(false);
                                }
                            }}
                            SecondaryButton { label: "Cancel".to_string(), onclick: move |_| show_create.set(false) }
                        }
                    }
                }
            }
            {
                let servers = state.mcp_servers.read();
                if servers.is_empty() {
                    rsx! { EmptyState { icon: "\u{1F4E6}", title: "No MCP servers", description: "Register one above. Stdio spawns a process, http/websocket/grpc connect to a remote endpoint." } }
                } else {
                    rsx! {
                        div { style: "display: flex; flex-direction: column; gap: 6px;",
                            for s in servers.iter() {
                                Panel { title: format!("{} ({})", s.name, s.transport),
                                    div { style: "font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;",
                                        "Status: {s.status} • Tools: {s.tools_count}"
                                    }
                                    div { style: "font-size: 13px; color: {COLOR_TEXT}; font-family: {FONT_MONO};",
                                        "{s.endpoint}"
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
pub fn AuditPage() -> Element {
    let mut state: VoltState = use_context();
    let mut filter_actor = use_signal(String::new);
    {
        let _ = use_resource(move || async move {
            state.fire(UiCommand::GetAuditLog { limit: 200 });
        });
    }
    rsx! {
        PageHeader { title: "Audit Log", subtitle: "EU AI Act Art. 12 compliance: all agent actions, tool calls, and approvals" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 12px; margin-bottom: 16px; align-items: center;",
                input { style: "max-width: 300px; padding: 8px 12px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; outline: none;",
                    placeholder: "Filter by actor...",
                    value: "{filter_actor.read()}",
                    oninput: move |e| filter_actor.set(e.value().to_string()),
                }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::GetAuditLog { limit: 200 }) }
                div { style: "flex: 1;" }
                span { style: "color: {COLOR_TEXT_MUTED}; font-size: 11px;", "Append-only PostgreSQL + in-memory ring buffer (EU AI Act Art. 12)" }
            }
            {
                let entries = state.audit_entries.read();
                let filter = filter_actor.read().to_lowercase();
                let filtered: Vec<_> = entries
                    .iter()
                    .filter(|e| {
                        filter.is_empty() || e.actor.to_string().to_lowercase().contains(&filter)
                    })
                    .collect();
                if filtered.is_empty() {
                    rsx! { EmptyState { icon: "\u{1F50D}", title: "No audit entries", description: "As you use Volt, every action will be recorded here. Audit log is tamper-evident via append-only writes." } }
                } else {
                    rsx! {
                        div { style: "display: flex; flex-direction: column; gap: 4px;",
                            for e in filtered {
                                {
                                    let ts = e.timestamp.format("%H:%M:%S").to_string();
                                    let result_color = if e.result == AuditResult::Ok { COLOR_SUCCESS } else { COLOR_DANGER };
                                    rsx! {
                                        div { style: "padding: 8px 12px; background-color: {COLOR_PANEL}; border-radius: 4px; display: flex; gap: 12px; align-items: center; font-size: 12px;",
                                            span { style: "color: {COLOR_TEXT_MUTED}; min-width: 130px; font-family: {FONT_MONO};", "{ts}" }
                                            span { style: "color: {COLOR_ACCENT}; min-width: 80px;", "{e.actor}" }
                                            span { style: "color: {COLOR_TEXT}; min-width: 120px;", "{e.action}" }
                                            span { style: "color: {COLOR_TEXT_DIM};", "{e.target}" }
                                            span { style: "margin-left: auto; color: {result_color};", "{e.result}" }
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
}

#[component]
pub fn WorkflowsPage() -> Element {
    let mut state: VoltState = use_context();
    let mut pattern_str = use_signal(String::new);
    let mut agents_str = use_signal(String::new);
    let mut tasks_str = use_signal(String::new);
    let mut allow_check = use_signal(|| false);
    let mut new_workflow_name = use_signal(String::new);
    {
        let _ = use_resource(move || async move {
            state.fire(UiCommand::ListWorkflows);
            state.fire(UiCommand::ListCanvasWorkflows);
        });
    }
    let workflows = state.workflows.read().clone();
    let canvas_workflows = state.canvas_workflows.read().clone();
    let loaded_name = state.canvas_loaded_name.read().clone();
    rsx! {
        PageHeader { title: "Workflows", subtitle: "DAG-based multi-agent orchestration" }
        div { style: "padding: 24px 32px; display: flex; flex-direction: column; gap: 16px;",
            // Visual editor panel — primary surface.
            Panel { title: format!("Visual Editor{}", loaded_name.as_deref().map(|n| format!(" — {}", n)).unwrap_or_default()),
                div { style: "display: flex; flex-direction: column; gap: 12px;",
                    // Top toolbar: file ops + quick-add.
                    div { style: "display: flex; gap: 8px; align-items: center; flex-wrap: wrap;",
                        input { style: "flex: 1; min-width: 200px; padding: 6px 10px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; color: {COLOR_TEXT}; font-size: 12px;",
                            placeholder: "Workflow name (e.g. research-pipeline)",
                            value: "{new_workflow_name.read()}",
                            oninput: move |e| new_workflow_name.set(e.value().to_string()),
                        }
                        button { style: "padding: 6px 12px; background-color: {COLOR_PANEL_HOVER}; color: {COLOR_TEXT}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; font-size: 12px; cursor: pointer;",
                            onclick: move |_| {
                                let name = new_workflow_name.read().trim().to_string();
                                if !name.is_empty() {
                                    state.fire(UiCommand::NewCanvasWorkflow { name });
                                }
                            },
                            "New"
                        }
                        button { style: "padding: 6px 12px; background-color: {COLOR_PANEL_HOVER}; color: {COLOR_TEXT}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; font-size: 12px; cursor: pointer;",
                            onclick: move |_| {
                                let name = loaded_name.clone().unwrap_or_default();
                                let json = state.canvas_graph_json.read().clone();
                                if !name.is_empty() && !json.is_empty() {
                                    state.fire(UiCommand::SaveCanvasWorkflow { name, graph_json: json });
                                }
                            },
                            "Save"
                        }
                        CanvasQuickAdd {}
                    }
                    // Side-by-side: canvas + side panel.
                    div { style: "display: grid; grid-template-columns: 1fr 280px; gap: 12px;",
                        WorkflowCanvas {}
                        div { style: "display: flex; flex-direction: column; gap: 8px;",
                            CanvasInspector {}
                            // Saved workflows list.
                            div { style: "padding: 12px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px;",
                                div { style: "font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 8px;",
                                    "Saved workflows ({canvas_workflows.len()})"
                                }
                                if canvas_workflows.is_empty() {
                                    div { style: "font-size: 11px; color: {COLOR_TEXT_MUTED};", "No saved workflows yet" }
                                } else {
                                    div { style: "display: flex; flex-direction: column; gap: 4px; max-height: 200px; overflow-y: auto;",
                                        for w in canvas_workflows.iter() {
                                            div { style: "padding: 6px 8px; background-color: {COLOR_PANEL_HOVER}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; cursor: pointer; display: flex; justify-content: space-between; align-items: center;",
                                                div { style: "flex: 1; min-width: 0;",
                                                    onclick: {
                                                        let n = w.name.clone();
                                                        move |_| state.fire(UiCommand::LoadCanvasWorkflow { name: n.clone() })
                                                    },
                                                    div { style: "font-size: 12px; color: {COLOR_TEXT}; font-family: {FONT_MONO}; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;",
                                                        "{w.name}"
                                                    }
                                                    div { style: "font-size: 10px; color: {COLOR_TEXT_MUTED};", "{w.node_count} nodes · {w.edge_count} edges" }
                                                }
                                                button { style: "padding: 2px 6px; background-color: transparent; color: {COLOR_DANGER}; border: 1px solid {COLOR_DANGER}; border-radius: 3px; font-size: 10px; cursor: pointer;",
                                                    onclick: {
                                                        let n = w.name.clone();
                                                        move |_| state.fire(UiCommand::DeleteCanvasWorkflow { name: n.clone() })
                                                    },
                                                    "Delete"
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
            // Existing text-based pattern runner — kept for quick ad-hoc runs.
            Panel { title: "Quick Run (text)",
                div { style: "display: flex; flex-direction: column; gap: 12px;",
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Pattern" }
                        input { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            placeholder: "research-code-review",
                            value: "{pattern_str.read()}",
                            oninput: move |e| pattern_str.set(e.value().to_string()),
                        }
                    }
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Agents (JSON)" }
                        input { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                            placeholder: "[agent-a, agent-b]",
                            value: "{agents_str.read()}",
                            oninput: move |e| agents_str.set(e.value().to_string()),
                        }
                    }
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Tasks (JSON)" }
                        input { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                            placeholder: "task-1 ...",
                            value: "{tasks_str.read()}",
                            oninput: move |e| tasks_str.set(e.value().to_string()),
                        }
                    }
                    div { style: "display: flex; align-items: center; gap: 8px;",
                        input { r#type: "checkbox", checked: "{*allow_check.read()}", onchange: move |e| allow_check.set(e.checked()) }
                        label { style: "font-size: 13px; color: {COLOR_TEXT_DIM};", "Allow all tool calls without prompting" }
                    }
                    PrimaryButton { label: "Run Workflow".to_string(), onclick: move |_| {
                        let p = pattern_str.read().clone();
                        if !p.is_empty() {
                            let ag = agents_str.read().clone();
                            let tk = tasks_str.read().clone();
                            let al = *allow_check.read();
                            state.fire(UiCommand::RunWorkflow { pattern: p, agents: Some(ag), tasks: Some(tk), allow: al });
                        }
                    }}
                }
            }
            Panel { title: format!("Available Patterns ({})", workflows.len()),
                if workflows.is_empty() {
                    EmptyState { icon: "\u{1F504}", title: "No workflows loaded", description: "Workflows are loaded from .volt/workflows/ as JSON DAG files. Use 'volt workflow' CLI to manage them." }
                } else {
                    div { style: "display: flex; flex-direction: column; gap: 6px;",
                        for w in workflows.iter() {
                            div {
                                style: "padding: 10px 12px; background-color: {COLOR_PANEL_HOVER}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; cursor: pointer;",
                                onclick: {
                                    let pat = w.pattern.clone();
                                    move |_| pattern_str.set(pat.clone())
                                },
                                div { style: "display: flex; align-items: center; gap: 8px; margin-bottom: 4px;",
                                    span { style: "font-family: monospace; font-size: 13px; color: {COLOR_TEXT}; font-weight: 600;", "{w.name}" }
                                    span { style: "font-size: 11px; padding: 2px 6px; border-radius: 3px; background-color: rgba(168,85,247,0.15); color: {COLOR_ACCENT}; text-transform: uppercase;",
                                        "{w.pattern}"
                                    }
                                }
                                div { style: "font-size: 12px; color: {COLOR_TEXT_DIM}; line-height: 1.4;",
                                    "{w.description}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A single row in the API Keys settings panel: provider name, current
/// masked key, and a text input that calls `SubmitApiKey` on save. Used
/// inside `SettingsPage`.
#[component]
fn ApiKeyRow(slug: String, display_name: String) -> Element {
    let mut state: VoltState = use_context();
    let mut input = use_signal(String::new);
    let mut show_input = use_signal(|| false);
    let doctor = state.doctor_report.read().clone();
    let env_var = format!("{}_API_KEY", slug.to_uppercase());
    let current = doctor
        .as_ref()
        .and_then(|r| r.api_keys.iter().find(|k| k.name == env_var).cloned());
    let masked = current
        .as_ref()
        .map(|k| k.masked.clone())
        .unwrap_or_default();
    let is_set = !masked.is_empty() && masked != "***" && !masked.to_lowercase().contains("not set");
    rsx! {
        div { style: "display: grid; grid-template-columns: 160px 1fr auto; gap: 12px; align-items: center; padding: 10px 12px; background-color: {COLOR_PANEL_HOVER}; border-radius: 6px;",
            div { style: "font-size: 13px; color: {COLOR_TEXT};",
                "{display_name}"
                div { style: "font-size: 10px; color: {COLOR_TEXT_MUTED}; font-family: monospace; margin-top: 2px;",
                    "{env_var}"
                }
            }
            div { style: "font-family: monospace; font-size: 12px; color: {COLOR_TEXT_DIM}; word-break: break-all;",
                if is_set {
                    "{masked}"
                } else {
                    span { style: "color: {COLOR_WARNING}; font-style: italic;", "not set — click Add to configure" }
                }
            }
            div { style: "display: flex; gap: 6px; align-items: center;",
                if *show_input.read() {
                    input { style: "padding: 6px 10px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; color: {COLOR_TEXT}; font-size: 12px; font-family: monospace; width: 220px;",
                        placeholder: "paste key…",
                        r#type: "password",
                        value: "{input.read()}",
                        oninput: move |e| input.set(e.value().to_string()),
                    }
                    PrimaryButton { label: "Save".to_string(), onclick: {
                        let s = slug.clone();
                        let v = input.read().trim().to_string();
                        move |_| {
                            if v.is_empty() { return; }
                            state.fire(UiCommand::SubmitApiKey {
                                provider: s.clone(),
                                api_key: v.clone(),
                                model: crate::config::default_model_for_provider(&s).to_string(),
                            });
                            state.toast(ToastLevel::Success, format!("{} saved", env_var));
                            input.set(String::new());
                            show_input.set(false);
                            state.fire(UiCommand::RunDoctor);
                        }
                    }}
                    SecondaryButton { label: "Cancel".to_string(), onclick: move |_| {
                        input.set(String::new());
                        show_input.set(false);
                    }}
                } else {
                    SecondaryButton { label: { if is_set { "Replace".to_string() } else { "Add".to_string() } }, onclick: move |_| show_input.set(true) }
                }
            }
        }
    }
}
