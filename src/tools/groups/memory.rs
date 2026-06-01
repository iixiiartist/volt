// Memory and persistence tools (memory_append, todo_add)
use crate::tools::registry::ToolRegistry;
use std::sync::Arc;

pub async fn register_memory_tools(registry: &Arc<ToolRegistry>) {
    registry
        .register(
            "memory_append",
            "Append to persistent memory file. Use ONLY when you learn something important about the user's preferences, project setup, or recurring patterns that should be remembered across sessions. Do NOT use for temporary conversation context or trivial information.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "description": "memory category" },
                    "content": { "type": "string", "description": "content to remember" }
                },
                "required": ["kind", "content"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let kind = args["kind"].as_str().unwrap_or("note");
                    let content = args["content"].as_str().unwrap_or("");
                    crate::tools::memory_tool::memory_append(kind, content).await
                })
            }),
        )
        .await;

    registry
        .register(
            "todo_add",
            "Add a task to the todo list",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "task description" }
                },
                "required": ["task"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let task = args["task"].as_str().unwrap_or("");
                    crate::tools::todo_tool::todo_add(task).await
                })
            }),
        )
        .await;
}
