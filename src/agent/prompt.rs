use crate::context::ContextKind;
use crate::models::AgentConfig;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

/// Cap on bytes of `AGENTS.md` inlined into the system prompt. Beyond
/// this, only the first chunk is included and the rest is left to RAG
/// retrieval — the model sees the project conventions without paying
/// full cost on every turn.
const AGENTS_MD_INLINE_CAP: usize = 4 * 1024;
/// Same cap for `SOUL.md` / `MEMORY.md` / `USER.md` — these should
/// stay small.
const PERSONALITY_INLINE_CAP: usize = 2 * 1024;

/// Cached entry for a workspace file. Invalidated when the file's
/// mtime changes, so edits to SOUL.md / MEMORY.md / AGENTS.md /
/// USER.md are picked up at the next session start without paying
/// the disk-read cost on every turn.
struct CachedSnippet {
    mtime: SystemTime,
    content: String,
}

struct WorkspaceFileCache {
    soul: Mutex<Option<CachedSnippet>>,
    memory: Mutex<Option<CachedSnippet>>,
    user: Mutex<Option<CachedSnippet>>,
    agents: Mutex<Option<CachedSnippet>>,
}

thread_local! {
    static WORKSPACE_CACHE: WorkspaceFileCache = const {
        WorkspaceFileCache {
            soul: Mutex::new(None),
            memory: Mutex::new(None),
            user: Mutex::new(None),
            agents: Mutex::new(None),
        }
    };
}

/// Read `path` and return at most `max_bytes` of its content, cached
/// on the file's mtime. If the file's mtime changes, the cache is
/// invalidated and re-read. Returns `None` if the file is missing.
fn read_capped_cached(
    path: &Path,
    max_bytes: usize,
    slot: &Mutex<Option<CachedSnippet>>,
) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let content = std::fs::read_to_string(path).ok()?;

    let mut guard = slot.lock().ok()?;
    let needs_refresh = match guard.as_ref() {
        Some(c) => c.mtime != mtime,
        None => true,
    };
    if needs_refresh {
        *guard = Some(CachedSnippet {
            mtime,
            content: content.clone(),
        });
    }
    let s = guard.as_ref()?;
    if s.content.len() <= max_bytes {
        return Some(s.content.clone());
    }
    // Truncate at a char boundary to avoid splitting a UTF-8 codepoint.
    let mut end = max_bytes;
    while !s.content.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 64);
    out.push_str(&s.content[..end]);
    out.push_str("\n\n[…truncated; full text available via RAG tool…]");
    Some(out)
}

pub fn build_system_prompt(config: &AgentConfig, workspace: Option<&Path>) -> String {
    // Precision mode: minimal prompt — function calling tasks need clean context
    if config.enabled_context_kinds.len() <= 2
        && config.enabled_context_kinds.contains(&ContextKind::Tool)
        && config
            .enabled_context_kinds
            .contains(&ContextKind::Artifact)
    {
        return format!(
            "Current time: {}\n\nYou are an AI agent. Use the available tools to answer questions. Call the appropriate function for each question.",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    let mut parts = Vec::new();

    parts.push(format!(
        "Current time: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));

    if let Some(ref sp) = config.system_prompt {
        parts.push(sp.clone());
    }

    if let Some(ws) = workspace {
        // Workspace file reads are cached per-thread on the file's
        // mtime. Edits to SOUL/MEMORY/USER/AGENTS invalidate the
        // cache automatically; no per-turn disk I/O.
        WORKSPACE_CACHE.with(|cache| {
            for (label, path, cap, slot) in &[
                ("Personality", ws.join("SOUL.md"), PERSONALITY_INLINE_CAP, &cache.soul),
                ("Persistent Memory", ws.join("MEMORY.md"), PERSONALITY_INLINE_CAP, &cache.memory),
                ("User Profile", ws.join("USER.md"), PERSONALITY_INLINE_CAP, &cache.user),
            ] {
                if let Some(snippet) = read_capped_cached(path, *cap, slot) {
                    parts.push(format!("## {}\n{}", label, snippet));
                }
            }

            // AGENTS.md is the project-level instruction file. Truncate
            // it to keep prompt size bounded; longer conventions should
            // live in the RAG context store, not the system prompt.
            let agents_path = ws.join("AGENTS.md");
            if let Some(snippet) =
                read_capped_cached(&agents_path, AGENTS_MD_INLINE_CAP, &cache.agents)
            {
                parts.push(format!("## Project Instructions (AGENTS.md)\n{}", snippet));
            }
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_slot() -> std::sync::Mutex<Option<CachedSnippet>> {
        std::sync::Mutex::new(None)
    }

    #[test]
    fn read_capped_cached_returns_full_when_small() {
        let tmp = std::env::temp_dir().join("volt_prompt_small.md");
        std::fs::write(&tmp, "small content").unwrap();
        let slot = fresh_slot();
        let result = read_capped_cached(&tmp, 100, &slot).unwrap();
        assert_eq!(result, "small content");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn read_capped_cached_truncates_with_marker() {
        let tmp = std::env::temp_dir().join("volt_prompt_large.md");
        let body = "a".repeat(10_000);
        std::fs::write(&tmp, &body).unwrap();
        let slot = fresh_slot();
        let result = read_capped_cached(&tmp, 100, &slot).unwrap();
        // Truncation is at a char boundary
        assert!(result.starts_with(&"a".repeat(100)));
        // Marker tells the model there's more
        assert!(result.contains("truncated"));
        assert!(result.len() < 200);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn read_capped_cached_handles_unicode_boundary() {
        // 4-byte emoji at position 99 should not get cut in half
        let tmp = std::env::temp_dir().join("volt_prompt_unicode.md");
        let mut body = "x".repeat(98);
        body.push_str("🦀");
        body.push_str(&"y".repeat(50));
        std::fs::write(&tmp, &body).unwrap();
        let slot = fresh_slot();
        let result = read_capped_cached(&tmp, 100, &slot).unwrap();
        // Should NOT panic and should NOT include a half-emoji
        assert!(result.is_char_boundary(result.len().saturating_sub(20)));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn read_capped_cached_missing_file_returns_none() {
        let slot = fresh_slot();
        let result =
            read_capped_cached(Path::new("/nonexistent/path/that/does/not/exist"), 100, &slot);
        assert!(result.is_none());
    }

    #[test]
    fn read_capped_cached_picks_up_file_edits() {
        let tmp = std::env::temp_dir().join("volt_prompt_edit.md");
        std::fs::write(&tmp, "first content").unwrap();
        let slot = fresh_slot();
        let r1 = read_capped_cached(&tmp, 1000, &slot).unwrap();
        assert_eq!(r1, "first content");
        // Sleep briefly so mtime can change (some filesystems are
        // low-resolution; mtime equality is the cache key).
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&tmp, "second content").unwrap();
        let r2 = read_capped_cached(&tmp, 1000, &slot).unwrap();
        assert_eq!(r2, "second content", "cache should have invalidated on mtime change");
        std::fs::remove_file(&tmp).ok();
    }
}
