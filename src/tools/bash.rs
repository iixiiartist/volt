use crate::models::ToolResult;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const BASH_TIMEOUT_SECS: u64 = 120;

pub async fn execute_bash(command: &str) -> ToolResult {
    let started = Instant::now();
    let output = timeout(Duration::from_secs(BASH_TIMEOUT_SECS), async {
        Command::new("bash")
            .arg("-lc")
            .arg(command)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .await
    })
    .await;

    let duration_ms = started.elapsed().as_millis();
    match output {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let success = out.status.success();
            ToolResult {
                success,
                output: if success { stdout } else { format!("stderr: {}", stderr) },
                error: if success { None } else { Some(stderr) },
                duration_ms,
            }
        }
        Ok(Err(e)) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("bash execution failed: {}", e)),
            duration_ms,
        },
        Err(_) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("bash command timed out after {}s", BASH_TIMEOUT_SECS)),
            duration_ms,
        },
    }
}
