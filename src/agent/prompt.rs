use crate::models::AgentConfig;
use std::path::Path;

pub fn build_system_prompt(config: &AgentConfig, workspace: &Path) -> String {
    let mut parts = Vec::new();

    if let Some(ref sp) = config.system_prompt {
        parts.push(sp.clone());
    }

    let soul_path = workspace.join("SOUL.md");
    if soul_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&soul_path) {
            parts.push(format!("## Personality\n{}", content));
        }
    }

    let memory_path = workspace.join("MEMORY.md");
    if memory_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&memory_path) {
            parts.push(format!("## Persistent Memory\n{}", content));
        }
    }

    let user_path = workspace.join("USER.md");
    if user_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&user_path) {
            parts.push(format!("## User Profile\n{}", content));
        }
    }

    parts.push(
        "You are Volt, a production-grade AI agent. You have access to tools for file system operations, shell commands, web access, and memory management. Use them step by step to accomplish the user's goals.".into()
    );

    parts.join("\n\n")
}
