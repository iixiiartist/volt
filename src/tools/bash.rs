use crate::models::ToolResult;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const BASH_TIMEOUT_SECS: u64 = 120;

fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

fn build_shell_command(user_command: &str) -> Command {
    if is_windows() {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/c").arg(user_command);
        cmd
    } else {
        let mut cmd = Command::new("bash");
        cmd.arg("-lc").arg(user_command);
        cmd.env_clear().env("PATH", "/usr/bin:/bin");
        cmd
    }
}

pub async fn execute_bash(command: &str) -> ToolResult {
    let started = Instant::now();
    let output = timeout(Duration::from_secs(BASH_TIMEOUT_SECS), async {
        build_shell_command(command).output().await
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
                output: if success {
                    stdout
                } else {
                    format!("stderr: {}", stderr)
                },
                error: if success { None } else { Some(stderr) },
                duration_ms,
            }
        }
        Ok(Err(e)) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("shell execution failed: {}", e)),
            duration_ms,
        },
        Err(_) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("command timed out after {}s", BASH_TIMEOUT_SECS)),
            duration_ms,
        },
    }
}
