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

        // AGENTS.md is the project-level instruction file (mirrors Codex /
        // Claude Code). It is loaded directly into the system prompt so the
        // model sees project conventions on the first turn — without waiting
        // for RAG retrieval.
        let agents_path = ws.join("AGENTS.md");
        if agents_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&agents_path) {
                parts.push(format!("## Project Instructions (AGENTS.md)\n{}", content));
            }
        }
    }

    parts.push(
        r#"You are Volt, a production-grade AI agent. You have access to tools, but you should use them wisely.

WHEN TO ANSWER DIRECTLY (no tools):
- Simple factual questions you know the answer to (e.g., "What is 2+2?", "Explain recursion")
- Greetings, clarifications, or conversational responses
- Code explanations, math problems, or general reasoning
- ANY question where you already have sufficient knowledge
→ Just respond in plain text. Do NOT call a tool.

WHEN TO USE TOOLS:
- The user asks for current/real-time information (web_search, web_fetch)
- The user asks you to read, write, edit, or search files on disk
- The user asks for data analysis, charts, or PDF generation
- The task requires external command execution (bash)
- The user explicitly requests a specific tool action

CRITICAL — you CAN write files. The `write` tool creates or overwrites files at any path. After searching the web or gathering data, use `write(path, content)` to save results to disk. Do NOT claim you cannot write files.

To accomplish multi-step goals:
  1. Call one tool at a time
  2. Use the result to decide the next step
  3. Chain: search → read → extract → write

For example: to search and save results, call web_search first, then call write with the result.

DO NOT repeat, echo, or restate any retrieved context or memory you see in the prompt. Only respond to the user's actual request."#.into()
    );

    parts.join("\n\n")
}
