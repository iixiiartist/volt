use super::commands::{ChatMessage, ChatRole, UiCommand, UiEvent};
use super::state::{ConnectionStatus, ToastLevel, VoltState};
use dioxus::prelude::*;
use uuid::Uuid;

/// Truncate a UUID/ID string to its first 8 characters, never panicking on
/// shorter inputs. Centralizes the format used in toast/UI messages.
pub(crate) fn short_id(id: &str) -> &str {
    &id[..8.min(id.len())]
}

#[component]
pub fn App() -> Element {
    let state = VoltState::default();
    use_context_provider(|| state);
    rsx! { Bootstrap {} }
}

#[component]
fn Bootstrap() -> Element {
    let mut state: VoltState = use_context();

    use_future(move || async move {
        state.connection.set(ConnectionStatus::Connecting);
        match super::runtime::Runtime::start().await {
            Ok(start_result) => {
                let handle = start_result.handle;
                let setup_providers = start_result.setup_providers;
                let needs_setup = !setup_providers.is_empty();
                state.handle.set(Some(handle.clone()));
                state.connection.set(ConnectionStatus::Connected);
                state.llm_online.set(!needs_setup);
                state.db_connected.set(true);
                state.embedder_loaded.set(true);
                if needs_setup {
                    state.setup_providers.set(setup_providers);
                    state.show_setup_wizard.set(true);
                    state.toast(
                        ToastLevel::Warning,
                        "Welcome! Pick an LLM provider to get started.",
                    );
                } else {
                    state.toast(ToastLevel::Success, "Runtime connected");
                }
                // Subscribe for subsequent events (chat, errors, etc).
                // SetupNeeded has already been shown from the start
                // result so we don't need to wait for a broadcast.
                let mut rx = handle.subscribe();
                while let Ok(event) = rx.recv().await {
                    let cur = *state.total_events.read();
                    state.total_events.set(cur + 1);
                    handle_event(&mut state, event).await;
                }
            }
            Err(e) => {
                state.connection.set(ConnectionStatus::Error);
                state.toast(ToastLevel::Error, format!("Failed to start runtime: {}", e));
            }
        }
    });

    rsx! { super::layout::AppLayout {} }
}

async fn handle_event(state: &mut VoltState, event: UiEvent) {
    match event {
        UiEvent::Pong => {}
        UiEvent::ChatStarted { session_id } => {
            state.chat_session.set(Some(session_id));
            state.chat_streaming.set(true);
            // Reset per-chat state: tool-call list and any
            // pending approval requests for the previous turn
            // shouldn't bleed into the new turn.
            state.tool_calls.set(Vec::new());
            state.pending_approvals.set(Vec::new());
        }
        UiEvent::ChatChunk { content } => {
            append_assistant_chunk(state, &content);
        }
        UiEvent::ChatComplete { final_text, tokens_used, duration_ms } => {
            state.chat_streaming.set(false);
            finalize_assistant_message(state, &final_text);
            state.toast(
                ToastLevel::Success,
                format!("Done ({} tokens, {}ms)", tokens_used, duration_ms),
            );
        }
        UiEvent::ChatError { message } => {
            state.chat_streaming.set(false);
            state.toast(ToastLevel::Error, format!("Chat error: {}", message));
        }
        UiEvent::ChatCancelled => {
            state.chat_streaming.set(false);
            state.toast(ToastLevel::Warning, "Chat cancelled");
        }
        UiEvent::ApprovalRequest {
            request_id,
            tool_name,
            args,
        } => {
            // Push onto the queue so the modal UI can render a
            // proper Allow/Deny prompt. Also surface a brief toast
            // so the user notices even if the modal is not yet
            // visible (e.g. they navigated away).
            let mut queue = state.pending_approvals.read().clone();
            queue.push(super::commands::ApprovalRequestInfo {
                request_id,
                tool_name: tool_name.clone(),
                args: args.clone(),
            });
            state.pending_approvals.set(queue);
            let args_preview = args
                .as_object()
                .map(|m| {
                    m.iter()
                        .take(2)
                        .map(|(k, v)| {
                            let v = v.to_string();
                            if v.len() > 40 {
                                format!("{}={}…", k, &v[..40])
                            } else {
                                format!("{}={}", k, v)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            state.toast(
                ToastLevel::Warning,
                format!(
                    "Approval needed: {} ({}) — req {}",
                    tool_name,
                    args_preview,
                    short_id(&request_id.to_string())
                ),
            );
        }
        UiEvent::Error { source, message } => {
            state.toast(ToastLevel::Error, format!("{}: {}", source, message));
        }
        UiEvent::JobCreated { id } => {
            state.toast(ToastLevel::Success, format!("Job created ({})", short_id(&id)));
            state.fire(UiCommand::ListJobs);
        }
        UiEvent::JobUpdated { id, state: job_state } => {
            state.toast(
                ToastLevel::Info,
                format!("Job {} \u{2192} {}", short_id(&id), job_state),
            );
            state.fire(UiCommand::ListJobs);
        }
        UiEvent::RoutineUpdated { id, enabled } => {
            state.toast(
                ToastLevel::Success,
                format!("Routine {} {}", short_id(&id), if enabled { "enabled" } else { "disabled" }),
            );
            state.fire(UiCommand::ListRoutines);
        }
        UiEvent::RoutineDeleted { id } => {
            state.toast(ToastLevel::Info, format!("Routine {} deleted", short_id(&id)));
            state.fire(UiCommand::ListRoutines);
        }
        UiEvent::SkillInstalled { name } => {
            state.toast(ToastLevel::Success, format!("Skill installed: {}", name));
            state.fire(UiCommand::ListSkills);
        }
        UiEvent::SkillUninstalled { name } => {
            state.toast(ToastLevel::Info, format!("Skill uninstalled: {}", name));
            state.fire(UiCommand::ListSkills);
        }
        UiEvent::McpServerRegistered { name } => {
            state.toast(ToastLevel::Success, format!("MCP server registered: {}", name));
            state.fire(UiCommand::ListMcpServers);
        }
        UiEvent::WorkflowCompleted { pattern, .. } => {
            state.toast(ToastLevel::Success, format!("Workflow {} done", pattern));
        }
        UiEvent::WorkflowFailed { pattern, error, .. } => {
            state.toast(
                ToastLevel::Error,
                format!("Workflow {} failed: {}", pattern, error),
            );
        }
        UiEvent::JobsListed { jobs } => {
            state.jobs.set(jobs);
        }
        UiEvent::RoutinesListed { routines } => {
            state.routines.set(routines);
        }
        UiEvent::SkillsListed { skills } => {
            state.skills.set(skills);
        }
        UiEvent::CatalogResults { query, skills } => {
            state.catalog_query.set(query);
            state.catalog_results.set(skills);
        }
        UiEvent::McpServersListed { servers } => {
            state.mcp_servers.set(servers);
        }
        UiEvent::AuditLog { entries } => {
            state.audit_entries.set(entries);
        }
        UiEvent::ToolsListed { tools } => {
            state.tools.set(tools);
        }
        UiEvent::WorktreesListed { worktrees } => {
            state.worktrees.set(worktrees);
        }
        UiEvent::WorkflowsListed { workflows } => {
            state.workflows.set(workflows);
        }
        UiEvent::CanvasWorkflowsListed { workflows } => {
            state.canvas_workflows.set(workflows);
        }
        UiEvent::CanvasWorkflowLoaded { name, graph_json } => {
            state.canvas_loaded_name.set(Some(name));
            state.canvas_graph_json.set(graph_json);
        }
        UiEvent::CanvasWorkflowSaved { name, path } => {
            state.toast(
                ToastLevel::Success,
                format!("Saved {} ({})", name, short_id(&path)),
            );
        }
        UiEvent::CanvasWorkflowDeleted { name } => {
            state.toast(ToastLevel::Info, format!("Deleted {}", name));
            // If the deleted workflow is the one currently loaded, clear it.
            let current = state.canvas_loaded_name.peek().clone();
            if current.as_deref() == Some(name.as_str()) {
                state.canvas_loaded_name.set(None);
                state.canvas_graph_json.set(String::new());
            }
        }
        UiEvent::ModelsListed { models } => {
            state.models.set(models);
        }
        UiEvent::ConfigLoaded { config } => {
            state.config.set(config);
        }
        UiEvent::ConfigUpdated => {
            state.toast(ToastLevel::Success, "Config updated");
        }
        UiEvent::DoctorCompleted { report } => {
            state.doctor_report.set(Some(report));
            state.toast(ToastLevel::Success, "Doctor finished");
        }
        UiEvent::ToolCallStart { id, name, args } => {
            let mut calls = state.tool_calls.read().clone();
            calls.push(super::commands::ToolCallInfo {
                id,
                name,
                args,
                result: None,
                error: None,
                duration_ms: None,
            });
            state.tool_calls.set(calls);
        }
        UiEvent::ToolCallEnd { id, result, error } => {
            let mut calls = state.tool_calls.read().clone();
            if let Some(call) = calls.iter_mut().find(|c| c.id == id) {
                call.result = Some(result);
                call.error = error;
            } else {
                // Tool-call came in without a Start (e.g. tool executed
                // outside the chat pipeline) — append it so it still
                // appears in the trace.
                calls.push(super::commands::ToolCallInfo {
                    id,
                    name: "(unknown)".into(),
                    args: serde_json::Value::Null,
                    result: Some(result),
                    error,
                    duration_ms: None,
                });
            }
            state.tool_calls.set(calls);
        }
        UiEvent::WorkflowStarted { pattern, run_id } => {
            state.toast(
                ToastLevel::Info,
                format!("Workflow {} started ({})", pattern, short_id(&run_id)),
            );
        }
        UiEvent::SessionCreated { id } => {
            state.chat_session.set(Some(id));
            state.fire(UiCommand::ListSessions);
        }
        UiEvent::SessionLoaded { id, messages } => {
            state.chat_session.set(Some(id));
            state.chat_messages.set(messages);
        }
        UiEvent::SessionDeleted { id } => {
            if *state.chat_session.read() == Some(id) {
                state.chat_session.set(None);
                state.chat_messages.set(Vec::new());
            }
            state.fire(UiCommand::ListSessions);
        }
        UiEvent::SessionsListed { sessions } => {
            state.sessions_cache.set(sessions);
        }
        UiEvent::SetupNeeded { providers } => {
            tracing::warn!(
                "[webui-ui] received SetupNeeded event with {} providers",
                providers.len()
            );
            state.setup_providers.set(providers);
            state.show_setup_wizard.set(true);
            state.llm_online.set(false);
            state.toast(
                ToastLevel::Warning,
                "Welcome! Pick an LLM provider to get started.",
            );
        }
        UiEvent::SetupReady { provider, model } => {
            tracing::warn!(
                "[webui-ui] received SetupReady event for {}:{}",
                provider,
                model
            );
            state.setup_providers.set(Vec::new());
            state.show_setup_wizard.set(false);
            state.llm_online.set(true);
            state.model.set(model.clone());
            state.provider.set(provider.clone());
            state.toast(
                ToastLevel::Success,
                format!("Connected to {} ({})", provider, model),
            );
        }
    }
}

/// Append a streamed chunk to the in-progress assistant bubble, or
/// start a new one. The in-progress bubble is identified by
/// `id == Uuid::nil()` so `ChatComplete` can finalize it later.
fn append_assistant_chunk(state: &mut VoltState, content: &str) {
    let mut msgs = state.chat_messages.read().clone();
    if let Some(last) = msgs
        .last_mut()
        .filter(|m| m.role == ChatRole::Assistant && m.id.is_nil())
    {
        last.content.push_str(content);
    } else {
        msgs.push(ChatMessage {
            timestamp: chrono::Utc::now(),
            role: ChatRole::Assistant,
            content: content.into(),
            ..Default::default()
        });
    }
    state.chat_messages.set(msgs);
}

/// Finalize the in-progress assistant bubble with the model's full
/// final text. If no streaming happened, push a fresh completed message.
fn finalize_assistant_message(state: &mut VoltState, final_text: &str) {
    let mut msgs = state.chat_messages.read().clone();
    if let Some(last) = msgs
        .last_mut()
        .filter(|m| m.role == ChatRole::Assistant && m.id.is_nil())
    {
        last.content = final_text.into();
        last.id = Uuid::new_v4();
    } else {
        msgs.push(ChatMessage {
            id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            role: ChatRole::Assistant,
            content: final_text.into(),
            ..Default::default()
        });
    }
    state.chat_messages.set(msgs);
}
