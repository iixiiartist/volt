use crate::context::ContextKind;
use crate::models::AgentConfig;
use std::path::Path;

pub fn build_system_prompt(config: &AgentConfig, workspace: Option<&Path>) -> String {
    // Precision mode: minimal prompt — function calling tasks need clean context
    if config.enabled_context_kinds.len() <= 2
        && config.enabled_context_kinds.contains(&ContextKind::Tool)
        && config
            .enabled_context_kinds
            .contains(&ContextKind::Artifact)
    {
        return "You are an AI agent. Use the available tools to answer questions. Call the appropriate function for each question.".into();
    }

    let mut parts = Vec::new();

    if let Some(ref sp) = config.system_prompt {
        parts.push(sp.clone());
    }

    if let Some(ws) = workspace {
        let soul_path = ws.join("SOUL.md");
        if soul_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&soul_path) {
                parts.push(format!("## Personality\n{}", content));
            }
        }

        let memory_path = ws.join("MEMORY.md");
        if memory_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&memory_path) {
                parts.push(format!("## Persistent Memory\n{}", content));
            }
        }

        let user_path = ws.join("USER.md");
        if user_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&user_path) {
                parts.push(format!("## User Profile\n{}", content));
            }
        }
    }

    parts.push(
        "You are Volt, a production-grade AI agent. You have access to tools for file system operations, shell commands, web access, and memory management. Use them step by step to accomplish the user's goals.".into()
    );

    parts.join("\n\n")
}
