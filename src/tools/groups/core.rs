// Core built-in tools (read, write, edit, bash, glob, grep, final_answer)
use crate::attenuation::TrustLevel;
use crate::models::PermissionLevel;
use crate::tools::registry::ToolRegistry;
use std::sync::Arc;

pub async fn register_core_tools(registry: &Arc<ToolRegistry>) {
    let bfcl_mode = std::env::var("VOLT_BFCL_MODE").is_ok();

    if !bfcl_mode {
        registry
            .register_with_permission(
                "bash",
                "Execute a shell command",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "shell command to run" }
                    },
                    "required": ["command"]
                }),
                "builtin",
                Arc::new(|args| {
                    Box::pin(async move {
                        let cmd = args["command"].as_str().unwrap_or("");
                        crate::tools::bash::execute_bash(cmd).await
                    })
                }),
                PermissionLevel::Prompt,
                TrustLevel::Builtin,
            )
            .await;
    }

    registry
        .register_with_permission(
            "read",
            "Read a file from disk",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "file path to read" }
                },
                "required": ["path"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    crate::tools::read_tool::read_file(path).await
                })
            }),
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;

    registry
        .register_with_permission(
            "write",
            "Write content to a file at any path on the filesystem. This is the ONLY way to save text to a file. Use this after web_search, web_fetch, bash, or any other data-gathering tool to persist results. Creates the file if it does not exist, overwrites if it does.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "file path to write to" },
                    "content": { "type": "string", "description": "content to write to the file" }
                },
                "required": ["path", "content"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let content = args["content"].as_str().unwrap_or("");
                    crate::tools::write_tool::write_file(path, content).await
                })
            }),
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;

    registry
        .register_with_permission(
            "edit",
            "Edit a file by replacing the first occurrence of old_string with new_string. Use for surgical text replacements in existing files.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "file path to edit" },
                    "old_string": { "type": "string", "description": "text to search for" },
                    "new_string": { "type": "string", "description": "replacement text" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let old_string = args["old_string"].as_str().unwrap_or("");
                    let new_string = args["new_string"].as_str().unwrap_or("");
                    crate::tools::edit::edit_file(path, old_string, new_string).await
                })
            }),
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;

    registry
        .register(
            "glob",
            "Find files matching a glob pattern",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "glob pattern" },
                    "base": { "type": "string", "description": "base directory" }
                },
                "required": ["pattern"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let pattern = args["pattern"].as_str().unwrap_or("*");
                    let base = args["base"].as_str().unwrap_or(".");
                    crate::tools::glob_tool::glob_files(pattern, base).await
                })
            }),
        )
        .await;

    registry
        .register(
            "grep",
            "Search file contents with regex",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "regex pattern" },
                    "path": { "type": "string", "description": "directory to search" }
                },
                "required": ["pattern"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let pattern = args["pattern"].as_str().unwrap_or("");
                    let path = args["path"].as_str().unwrap_or(".");
                    crate::tools::grep_tool::grep_files(pattern, path).await
                })
            }),
        )
        .await;

    registry
        .register(
            "final_answer",
            "Submit your final answer and terminate. Call this when you have determined the answer to the user's question.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "answer": {
                        "type": "string",
                        "description": "The final answer to the question"
                    }
                },
                "required": ["answer"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let answer = args["answer"].as_str().unwrap_or("");
                    crate::tools::final_answer::final_answer(answer).await
                })
            }),
        )
        .await;
}
