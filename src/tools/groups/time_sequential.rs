// Time and sequential thinking tools
use crate::register_tool;
use crate::tools::registry::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;

pub async fn register_time_tools(registry: &Arc<ToolRegistry>) {
    register_tool!(
        registry,
        "get_current_time",
        "Get the current time in a specific timezone.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "timezone": { "type": "string", "description": "IANA timezone (e.g. 'America/New_York', 'UTC', 'Asia/Tokyo')" }
            },
            "required": ["timezone"]
        }),
        "utilities",
        |args: Value| async move {
            let tz = args["timezone"].as_str().unwrap_or("UTC");
            crate::tools::time_tool::get_current_time(tz).await
        }
    );

    register_tool!(
        registry,
        "convert_time",
        "Convert time between timezones.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "timezone": { "type": "string", "description": "source IANA timezone" },
                "timezone_to": { "type": "string", "description": "target IANA timezone" }
            },
            "required": ["timezone", "timezone_to"]
        }),
        "utilities",
        |args: Value| async move {
            let from = args["timezone"].as_str().unwrap_or("UTC");
            let to = args["timezone_to"].as_str().unwrap_or("UTC");
            crate::tools::time_tool::convert_time(from, to).await
        }
    );
}

pub async fn register_sequential_tools(registry: &Arc<ToolRegistry>) {
    register_tool!(
        registry,
        "sequentialthinking",
        "A detailed tool for dynamic and reflective problem-solving through structured thoughts. Use when the task requires careful reasoning, multi-step analysis, or exploring alternative solutions.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "thought": { "type": "string", "description": "your current thought or reasoning step" },
                "next_thought_needed": { "type": "boolean", "description": "whether another thought step is needed" },
                "branch_id": { "type": "string", "description": "optional branch ID to explore alternative reasoning paths" },
                "branch_from_thought": { "type": "number", "description": "optional thought number to branch from" }
            },
            "required": ["thought", "next_thought_needed"]
        }),
        "reasoning",
        |args: Value| async move {
            let thought = args["thought"].as_str().unwrap_or("");
            let next = args["next_thought_needed"].as_bool().unwrap_or(true);
            let branch_id = args["branch_id"].as_str();
            let branch_from = args["branch_from_thought"].as_u64().map(|n| n as u32);
            crate::tools::sequential_thinking::sequentialthinking(thought, next, branch_id, branch_from).await
        }
    );
}
