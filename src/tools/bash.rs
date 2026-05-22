use crate::models::ToolResult;
use std::time::Instant;

pub fn execute_bash(command: &str) -> ToolResult {
    let started = Instant::now();
    let output = std::process::Command::new("bash")
        .arg("-lc")
        .arg(command)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output();

    let duration_ms = started.elapsed().as_millis();
    match output {
        Ok(out) => {
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
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("bash execution failed: {}", e)),
            duration_ms,
        },
    }
}
