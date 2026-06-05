use dioxus::prelude::*;
use super::commands::{ToolInfo, UiCommand, UiEvent};
use super::routes::Page;
use super::state::{
    ToastLevel, VoltState, COLOR_ACCENT, COLOR_BG, COLOR_BORDER, COLOR_DANGER, COLOR_INFO,
    COLOR_PANEL, COLOR_PANEL_HOVER, COLOR_SUCCESS, COLOR_TEXT, COLOR_TEXT_DIM,
    COLOR_TEXT_MUTED, COLOR_WARNING, FONT_MONO,
};

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
pub fn Panel(title: &'static str, children: Element) -> Element {
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
                }
            }
            div { style: "margin-top: 16px;",
                Panel { title: "Compliance",
                    p { style: "color: {COLOR_TEXT_DIM}; font-size: 12px; line-height: 1.6; margin: 0;",
                        "All agent actions are logged with structured tracing. Audit log is available in the "
                        "Audit section and complies with EU AI Act Art. 12 (record-keeping) and Art. 14 "
                        "(human oversight). Sensitive operations require explicit approval via the in-app "
                        "approval prompt before tool execution."
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
    let mut is_streaming = use_signal(|| false);
    let session_id = use_signal(|| None::<uuid::Uuid>);

    rsx! {
        PageHeader { title: "Chat", subtitle: "Conversational interface with the Volt agent" }
        div { style: "display: flex; flex-direction: column; height: calc(100vh - 56px - 28px - 80px);",
            div { style: "flex: 1; overflow-y: auto; padding: 24px 32px;",
                div { style: "max-width: 900px; margin: 0 auto;",
                    if session_id.read().is_none() {
                        EmptyState { icon: "\u{1F4AC}", title: "Start a conversation", description: "Type a message below to chat with the Volt agent." }
                    } else {
                        div { style: "color: {COLOR_TEXT_DIM}; text-align: center; padding: 40px;", "Messages will stream here in real time." }
                    }
                }
            }
            div { style: "padding: 16px 32px; border-top: 1px solid {COLOR_BORDER}; background-color: {COLOR_PANEL};",
                div { style: "max-width: 900px; margin: 0 auto; display: flex; gap: 12px; align-items: flex-end;",
                    textarea {
                        style: "flex: 1; padding: 12px 16px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; color: {COLOR_TEXT}; font-size: 14px; font-family: inherit; resize: none; min-height: 44px; max-height: 200px; outline: none;",
                        placeholder: "Type your message...",
                        value: "{input.read()}",
                        oninput: move |e| input.set(e.value().to_string()),
                        rows: "1",
                    }
                    button {
                        style: "padding: 8px 16px; background-color: {COLOR_ACCENT}; color: white; border: none; border-radius: 6px; font-size: 13px; font-weight: 600; cursor: pointer;",
                        onclick: move |_| {
                            let text = input.read().trim().to_string();
                            if !text.is_empty() && !*is_streaming.read() {
                                let sid = *session_id.read();
                                input.set(String::new());
                                is_streaming.set(true);
                                state.fire(UiCommand::Chat { session_id: sid, input: text });
                            }
                        },
                        "Send"
                    }
                    if *is_streaming.read() {
                        button {
                            style: "padding: 8px 16px; background-color: transparent; color: {COLOR_DANGER}; border: 1px solid {COLOR_DANGER}; border-radius: 6px; font-size: 13px; cursor: pointer;",
                            onclick: move |_| {
                                is_streaming.set(false);
                                state.fire(UiCommand::CancelChat);
                            },
                            "Cancel"
                        }
                    }
                }
                div { style: "max-width: 900px; margin: 8px auto 0 auto; display: flex; gap: 16px; font-size: 11px; color: {COLOR_TEXT_MUTED};",
                    span { "Enter to send" }
                    span { "Model: " }
                    span { style: "font-family: {FONT_MONO};", "{state.model.read()}" }
                    div { style: "flex: 1;" }
                    span { "EU AI Act Art. 12 logged" }
                }
            }
        }
    }
}

#[component]
pub fn ToolsPage() -> Element {
    let mut state: VoltState = use_context();
    let mut filter = use_signal(String::new);
    let selected = use_signal(|| None::<ToolInfo>);
    rsx! {
        PageHeader { title: "Tools", subtitle: "Live tool registry with schema browser and direct execution" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 12px; margin-bottom: 16px; align-items: center;",
                input { style: "flex: 1; max-width: 400px; padding: 8px 12px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; outline: none;",
                    placeholder: "Filter tools...",
                    value: "{filter.read()}",
                    oninput: move |e| filter.set(e.value().to_string()),
                }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListTools) }
            }
            div { style: "display: grid; grid-template-columns: 1fr 1fr; gap: 16px;",
                Panel { title: "Registered Tools",
                    div { style: "color: {COLOR_TEXT_DIM}; font-size: 13px; padding: 20px 0; text-align: center;",
                        "Tools will populate here when the runtime loads. Click Refresh to fetch the registry."
                    }
                }
                ToolDetail { selected: selected }
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
            EmptyState { icon: "\u{1F4C1}", title: "No sessions yet", description: "Start a chat to create your first session, or click 'New Session' to create one." }
        }
    }
}

#[component]
pub fn SettingsPage() -> Element {
    let mut state: VoltState = use_context();
    let initial_model = state.model.read().clone();
    let initial_provider = state.provider.read().clone();
    let mut model_input = use_signal(move || initial_model);
    let mut provider_input = use_signal(move || initial_provider);
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
                            state.model.set(model_input.read().clone());
                            state.provider.set(provider_input.read().clone());
                            state.toast(ToastLevel::Success, "Settings saved");
                        }}
                        SecondaryButton { label: "Run Doctor".to_string(), onclick: move |_| state.fire(UiCommand::RunDoctor) }
                    }
                }
            }
            div { style: "margin-top: 16px;",
                Panel { title: "Environment",
                    p { style: "margin: 0 0 8px 0; color: {COLOR_TEXT_DIM}; font-size: 12px;", "API keys are loaded from .env or system environment. Use 'volt doctor' to check status." }
                    p { style: "margin: 0; color: {COLOR_TEXT_MUTED}; font-size: 11px; font-family: {FONT_MONO};", "GROQ_API_KEY, OPENAI_API_KEY, ANTHROPIC_API_KEY, NVIDIA_API_KEY, OLLAMA_API_KEY, HF_TOKEN, YOUCOM_API_KEY" }
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
    rsx! {
        PageHeader { title: "Worktrees", subtitle: "Git worktree sessions from agent runs" }
        div { style: "padding: 24px 32px;",
            div { style: "margin-bottom: 16px;",
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListWorktrees) }
            }
            EmptyState { icon: "\u{1F33F}", title: "No worktrees", description: "Run an agent with --worktree to create an isolated worktree session." }
        }
    }
}

#[component]
pub fn JobsPage() -> Element {
    let mut state: VoltState = use_context();
    rsx! {
        PageHeader { title: "Jobs", subtitle: "Scheduled background jobs" }
        div { style: "padding: 24px 32px;",
            div { style: "margin-bottom: 16px;",
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListJobs) }
            }
            EmptyState { icon: "\u{23F0}", title: "No scheduled jobs", description: "Jobs are defined in the database with cron expressions. Use 'volt jobs' CLI to add and manage them." }
        }
    }
}

#[component]
pub fn RoutinesPage() -> Element {
    let mut state: VoltState = use_context();
    rsx! {
        PageHeader { title: "Routines", subtitle: "Event-triggered routines" }
        div { style: "padding: 24px 32px;",
            div { style: "margin-bottom: 16px;",
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListRoutines) }
            }
            EmptyState { icon: "\u{1F4A1}", title: "No routines", description: "Routines trigger on events (file changes, webhooks, schedules). Use 'volt routines' CLI to define them." }
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
    rsx! {
        PageHeader { title: "Skills", subtitle: "Reusable skill manifests from local, catalog, and imports" }
        div { style: "padding: 24px 32px;",
            div { style: "display: flex; gap: 8px; margin-bottom: 16px;",
                PrimaryButton { label: "Browse Catalog".to_string(), onclick: move |_| show_search.set(true) }
                SecondaryButton { label: "Import from File".to_string(), onclick: move |_| show_import.set(true) }
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListSkills) }
            }
            if *show_search.read() {
                Panel { title: "Catalog Search",
                    div { style: "display: flex; gap: 8px; margin-bottom: 12px;",
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
                    }
                }
            }
            EmptyState { icon: "\u{2728}", title: "No skills installed", description: "Browse the catalog to install skills, or import from a local TOML/JSON file." }
        }
    }
}

#[component]
pub fn RegistryPage() -> Element {
    let mut state: VoltState = use_context();
    rsx! {
        PageHeader { title: "MCP Registry", subtitle: "Model Context Protocol server connections" }
        div { style: "padding: 24px 32px;",
            div { style: "margin-bottom: 16px;",
                SecondaryButton { label: "Refresh".to_string(), onclick: move |_| state.fire(UiCommand::ListMcpServers) }
            }
            EmptyState { icon: "\u{1F4E6}", title: "No MCP servers", description: "MCP servers extend Volt with remote tools. Use 'volt mcp-serve' or register via .volt/mcp.toml." }
        }
    }
}

#[component]
pub fn AuditPage() -> Element {
    let mut state: VoltState = use_context();
    let mut filter_actor = use_signal(String::new);
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
                span { style: "color: {COLOR_TEXT_MUTED}; font-size: 11px;", "Persistent in .volt/audit.log + structured tracing" }
            }
            EmptyState { icon: "\u{1F50D}", title: "No audit entries", description: "As you use Volt, every action will be recorded here. Audit log is tamper-evident via append-only writes." }
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
    let pattern_val = pattern_str.read().clone();
    let agents_val = agents_str.read().clone();
    let tasks_val = tasks_str.read().clone();
    let allow_val = *allow_check.read();
    rsx! {
        PageHeader { title: "Workflows", subtitle: "DAG-based multi-agent orchestration" }
        div { style: "padding: 24px 32px;",
            Panel { title: "Run a Workflow",
                div { style: "display: flex; flex-direction: column; gap: 12px;",
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Pattern" }
                        input { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px;",
                            placeholder: "research-code-review",
                            value: "{pattern_val}",
                            oninput: move |e| pattern_str.set(e.value().to_string()),
                        }
                    }
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Agents (JSON)" }
                        input { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                            placeholder: "[agent-a, agent-b]",
                            value: "{agents_val}",
                            oninput: move |e| agents_str.set(e.value().to_string()),
                        }
                    }
                    div {
                        label { style: "display: block; font-size: 12px; color: {COLOR_TEXT_DIM}; margin-bottom: 4px;", "Tasks (JSON)" }
                        input { style: "width: 100%; padding: 8px 12px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: {FONT_MONO};",
                            placeholder: "task-1 ...",
                            value: "{tasks_val}",
                            oninput: move |e| tasks_str.set(e.value().to_string()),
                        }
                    }
                    div { style: "display: flex; align-items: center; gap: 8px;",
                        input { r#type: "checkbox", checked: "{allow_val}", onchange: move |e| allow_check.set(e.value() == "true") }
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
            div { style: "margin-top: 16px;",
                Panel { title: "Available Patterns",
                    EmptyState { icon: "\u{1F504}", title: "No workflows loaded", description: "Workflows are loaded from .volt/workflows/ as JSON DAG files. Use 'volt workflow' CLI to manage them." }
                }
            }
        }
    }
}
