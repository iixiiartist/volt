use super::commands::{UiCommand, UiEvent};
use super::state::{ConnectionStatus, ToastLevel, VoltState};
use dioxus::prelude::*;

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
            Ok(handle) => {
                state.handle.set(Some(handle.clone()));
                state.connection.set(ConnectionStatus::Connected);
                state.llm_online.set(true);
                state.db_connected.set(true);
                state.embedder_loaded.set(true);
                state.toast(ToastLevel::Success, "Runtime connected");
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
        UiEvent::ChatComplete {
            final_text,
            tokens_used,
            duration_ms,
        } => {
            state.toast(
                ToastLevel::Success,
                format!("Done ({} tokens, {}ms)", tokens_used, duration_ms),
            );
            let _ = final_text;
        }
        UiEvent::ChatError { message } => {
            state.toast(ToastLevel::Error, format!("Chat error: {}", message));
        }
        UiEvent::ChatCancelled => {
            state.toast(ToastLevel::Warning, "Chat cancelled");
        }
        UiEvent::ApprovalRequest {
            request_id,
            tool_name,
            args,
        } => {
            state.pending_approval.set(Some(UiEvent::ApprovalRequest {
                request_id,
                tool_name,
                args,
            }));
            state.toast(ToastLevel::Warning, "Tool approval required");
        }
        UiEvent::Error { source, message } => {
            state.toast(ToastLevel::Error, format!("{}: {}", source, message));
        }
        UiEvent::JobCreated { id } => {
            state.toast(ToastLevel::Success, format!("Job created ({})", &id[..8.min(id.len())]));
        }
        UiEvent::JobUpdated { id, state: job_state } => {
            state.toast(
                ToastLevel::Info,
                format!("Job {} → {}", &id[..8.min(id.len())], job_state),
            );
            state.fire(UiCommand::ListJobs);
        }
        UiEvent::RoutineUpdated { id, enabled } => {
            state.toast(
                ToastLevel::Success,
                format!("Routine {} {}", &id[..8.min(id.len())], if enabled { "enabled" } else { "disabled" }),
            );
            state.fire(UiCommand::ListRoutines);
        }
        UiEvent::RoutineDeleted { id } => {
            state.toast(ToastLevel::Info, format!("Routine {} deleted", &id[..8.min(id.len())]));
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
        _ => {}
    }
}
