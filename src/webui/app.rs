use dioxus::prelude::*;
use super::commands::{UiCommand, UiEvent};
use super::state::{ConnectionStatus, ToastLevel, VoltState};

#[component]
pub fn App() -> Element {
    let mut state = VoltState::default();
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
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            let cur = *state.total_events.read();
                            state.total_events.set(cur + 1);
                            handle_event(&mut state, event).await;
                        }
                        Err(_) => break,
                    }
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
        UiEvent::ChatComplete { final_text, tokens_used, duration_ms } => {
            state.toast(ToastLevel::Success, format!("Done ({} tokens, {}ms)", tokens_used, duration_ms));
            let _ = final_text;
        }
        UiEvent::ChatError { message } => {
            state.toast(ToastLevel::Error, format!("Chat error: {}", message));
        }
        UiEvent::ChatCancelled => {
            state.toast(ToastLevel::Warning, "Chat cancelled");
        }
        UiEvent::ApprovalRequest { request_id, tool_name, args } => {
            state.pending_approval.set(Some(UiEvent::ApprovalRequest { request_id, tool_name, args }));
            state.toast(ToastLevel::Warning, "Tool approval required");
        }
        UiEvent::Error { source, message } => {
            state.toast(ToastLevel::Error, format!("{}: {}", source, message));
        }
        _ => {}
    }
}
