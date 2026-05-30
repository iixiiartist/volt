// Git tools group
use crate::tools::registry::ToolRegistry;
use std::sync::Arc;

pub async fn register_git_tools(registry: &Arc<ToolRegistry>) {
    registry.register("git_status", "Show the working tree status (porcelain format).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_status(repo).await
    }))).await;

    registry.register("git_diff_unstaged", "Show unstaged changes in the working directory.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_diff_unstaged(repo).await
    }))).await;

    registry.register("git_diff_staged", "Show staged changes (diff --cached).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_diff_staged(repo).await
    }))).await;

    registry.register("git_diff", "Show differences between branches or commits.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "target": { "type": "string", "description": "branch, commit, or range to diff against" }
        },
        "required": ["target"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let target = args["target"].as_str().unwrap_or("HEAD");
        crate::tools::git_tool::git_diff(repo, target).await
    }))).await;

    registry.register("git_commit", "Record changes to the repository.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "message": { "type": "string", "description": "commit message" }
        },
        "required": ["message"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let msg = args["message"].as_str().unwrap_or("");
        crate::tools::git_tool::git_commit(repo, msg).await
    }))).await;

    registry.register("git_add", "Add file contents to the staging area.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "files": { "type": "array", "items": { "type": "string" }, "description": "files to stage" }
        },
        "required": ["files"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let files: Vec<String> = args["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        crate::tools::git_tool::git_add(repo, &files).await
    }))).await;

    registry.register("git_reset", "Unstage all staged changes.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_reset(repo).await
    }))).await;

    registry.register("git_log", "Show commit logs (oneline format).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "max_count": { "type": "number", "description": "maximum number of commits to show (default: 20)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let count = args["max_count"].as_u64().unwrap_or(20) as u32;
        crate::tools::git_tool::git_log(repo, count).await
    }))).await;

    registry.register("git_create_branch", "Create a new branch.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "branch": { "type": "string", "description": "name of the new branch" },
            "base": { "type": "string", "description": "optional base branch or commit" }
        },
        "required": ["branch"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let branch = args["branch"].as_str().unwrap_or("");
        let base = args["base"].as_str();
        crate::tools::git_tool::git_create_branch(repo, branch, base).await
    }))).await;

    registry.register("git_checkout", "Switch branches.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "branch": { "type": "string", "description": "branch to switch to" }
        },
        "required": ["branch"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let branch = args["branch"].as_str().unwrap_or("");
        crate::tools::git_tool::git_checkout(repo, branch).await
    }))).await;

    registry.register("git_show", "Show the contents of a commit.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "revision": { "type": "string", "description": "revision (commit hash, branch, tag)" }
        },
        "required": ["revision"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let rev = args["revision"].as_str().unwrap_or("HEAD");
        crate::tools::git_tool::git_show(repo, rev).await
    }))).await;

    registry.register("git_branch", "List git branches.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_branch(repo).await
    }))).await;
}
