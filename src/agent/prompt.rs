use crate::context::ContextKind;
use crate::models::AgentConfig;
use std::path::Path;

/// Cap on bytes of `AGENTS.md` inlined into the system prompt. Beyond
/// this, only the first chunk is included and the rest is left to RAG
/// retrieval — the model sees the project conventions without paying
/// full cost on every turn.
const AGENTS_MD_INLINE_CAP: usize = 4 * 1024;
/// Same cap for `SOUL.md` / `MEMORY.md` / `USER.md` — these should
/// stay small.
const PERSONALITY_INLINE_CAP: usize = 2 * 1024;

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
        for (label, path, cap) in &[
            ("Personality", ws.join("SOUL.md"), PERSONALITY_INLINE_CAP),
            ("Persistent Memory", ws.join("MEMORY.md"), PERSONALITY_INLINE_CAP),
            ("User Profile", ws.join("USER.md"), PERSONALITY_INLINE_CAP),
        ] {
            if let Some(snippet) = read_capped(path, *cap) {
                parts.push(format!("## {}\n{}", label, snippet));
            }
        }

        // AGENTS.md is the project-level instruction file. Truncate it
        // to keep prompt size bounded; longer conventions should live
        // in the RAG context store, not the system prompt.
        let agents_path = ws.join("AGENTS.md");
        if let Some(snippet) = read_capped(&agents_path, AGENTS_MD_INLINE_CAP) {
            parts.push(format!("## Project Instructions (AGENTS.md)\n{}", snippet));
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

/// Read a file at `path` and return at most `max_bytes` of its content.
/// If truncated, appends a marker so the model knows more exists.
fn read_capped(path: &Path, max_bytes: usize) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.len() <= max_bytes {
        return Some(content);
    }
    // Truncate at a char boundary to avoid splitting a UTF-8 codepoint.
    let mut end = max_bytes;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 64);
    out.push_str(&content[..end]);
    out.push_str("\n\n[…truncated; full text available via RAG tool…]");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_capped_returns_full_when_small() {
        let tmp = std::env::temp_dir().join("volt_prompt_small.md");
        std::fs::write(&tmp, "small content").unwrap();
        let result = read_capped(&tmp, 100).unwrap();
        assert_eq!(result, "small content");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn read_capped_truncates_with_marker() {
        let tmp = std::env::temp_dir().join("volt_prompt_large.md");
        let body = "a".repeat(10_000);
        std::fs::write(&tmp, &body).unwrap();
        let result = read_capped(&tmp, 100).unwrap();
        // Truncation is at a char boundary
        assert!(result.starts_with(&"a".repeat(100)));
        // Marker tells the model there's more
        assert!(result.contains("truncated"));
        assert!(result.len() < 200);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn read_capped_handles_unicode_boundary() {
        // 4-byte emoji at position 99 should not get cut in half
        let tmp = std::env::temp_dir().join("volt_prompt_unicode.md");
        let mut body = "x".repeat(98);
        body.push_str("🦀");
        body.push_str(&"y".repeat(50));
        std::fs::write(&tmp, &body).unwrap();
        let result = read_capped(&tmp, 100).unwrap();
        // Should NOT panic and should NOT include a half-emoji
        assert!(result.is_char_boundary(result.len().saturating_sub(20)));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn read_capped_missing_file_returns_none() {
        let result = read_capped(Path::new("/nonexistent/path/that/does/not/exist"), 100);
        assert!(result.is_none());
    }
}
