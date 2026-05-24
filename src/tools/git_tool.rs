use crate::models::ToolResult;
use std::time::Instant;

async fn git(args: &[&str], repo_path: &str) -> ToolResult {
    let started = Instant::now();
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .await;
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if o.status.success() {
                ToolResult { success: true, output: stdout, error: None, duration_ms: started.elapsed().as_millis() }
            } else {
                ToolResult { success: false, output: stdout, error: Some(stderr), duration_ms: started.elapsed().as_millis() }
            }
        }
        Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("git failed: {}", e)), duration_ms: started.elapsed().as_millis() },
    }
}

pub async fn git_status(repo_path: &str) -> ToolResult {
    git(&["status", "--porcelain"], repo_path).await
}

pub async fn git_diff_unstaged(repo_path: &str) -> ToolResult {
    git(&["diff"], repo_path).await
}

pub async fn git_diff_staged(repo_path: &str) -> ToolResult {
    git(&["diff", "--cached"], repo_path).await
}

pub async fn git_diff(repo_path: &str, target: &str) -> ToolResult {
    git(&["diff", target], repo_path).await
}

pub async fn git_commit(repo_path: &str, message: &str) -> ToolResult {
    git(&["commit", "-m", message], repo_path).await
}

pub async fn git_add(repo_path: &str, files: &[String]) -> ToolResult {
    let mut args = vec!["add"];
    args.extend(files.iter().map(|s| s.as_str()));
    git(&args, repo_path).await
}

pub async fn git_reset(repo_path: &str) -> ToolResult {
    git(&["reset"], repo_path).await
}

pub async fn git_log(repo_path: &str, max_count: u32) -> ToolResult {
    git(&["log", "--oneline", &format!("-{}", max_count)], repo_path).await
}

pub async fn git_create_branch(repo_path: &str, branch: &str, base: Option<&str>) -> ToolResult {
    let mut args = vec!["branch", branch];
    if let Some(b) = base { args.push(b); }
    git(&args, repo_path).await
}

pub async fn git_checkout(repo_path: &str, branch: &str) -> ToolResult {
    git(&["checkout", branch], repo_path).await
}

pub async fn git_show(repo_path: &str, revision: &str) -> ToolResult {
    git(&["show", revision], repo_path).await
}

pub async fn git_branch(repo_path: &str) -> ToolResult {
    git(&["branch"], repo_path).await
}
