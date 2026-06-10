use crate::attenuation::TrustLevel;
use crate::models::PermissionLevel;
use crate::register_tool_with_permission;
use crate::tools::registry::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;

pub async fn register_git_tools(registry: &Arc<ToolRegistry>) {
    register_tool_with_permission!(
        registry,
        "git_query",
        "Run a read-only git command. Accepts any git subcommand string like 'status --porcelain', 'log --oneline -10', 'diff', 'show HEAD', 'branch -a'. Do NOT use for commands that modify the repo.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
                "command": { "type": "string", "description": "git subcommand and args, e.g. 'status --porcelain', 'log --oneline -5', 'diff --cached'" }
            },
            "required": ["command"]
        }),
        "git",
        |args: Value| async move {
            let repo = args["repo_path"].as_str().unwrap_or(".");
            let cmd = args["command"].as_str().unwrap_or("");
            crate::tools::git_tool::git_query(repo, cmd).await
        },
        PermissionLevel::Allow,
        TrustLevel::Builtin
    );

    register_tool_with_permission!(
        registry,
        "git_mutate",
        "Run a git command that modifies the repository. Accepts any git subcommand string like 'commit -m msg', 'add file.rs', 'checkout main', 'branch new-feature', 'reset'. Use ONLY when the user asks you to make changes.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
                "command": { "type": "string", "description": "git subcommand and args, e.g. 'commit -m fix bug', 'add src/main.rs', 'checkout main'" }
            },
            "required": ["command"]
        }),
        "git",
        |args: Value| async move {
            let repo = args["repo_path"].as_str().unwrap_or(".");
            let cmd = args["command"].as_str().unwrap_or("");
            crate::tools::git_tool::git_mutate(repo, cmd).await
        },
        PermissionLevel::Prompt,
        TrustLevel::Builtin
    );
}
