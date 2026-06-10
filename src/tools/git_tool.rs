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
                ToolResult {
                    success: true,
                    output: stdout,
                    error: None,
                    duration_ms: started.elapsed().as_millis(),
                }
            } else {
                ToolResult {
                    success: false,
                    output: stdout,
                    error: Some(stderr),
                    duration_ms: started.elapsed().as_millis(),
                }
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("git failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn git_query(repo_path: &str, command: &str) -> ToolResult {
    let args: Vec<&str> = command.split_whitespace().collect();
    git(&args, repo_path).await
}

pub async fn git_mutate(repo_path: &str, command: &str) -> ToolResult {
    let args: Vec<&str> = command.split_whitespace().collect();
    git(&args, repo_path).await
}
