use crate::models::{SandboxPolicy, SandboxResult};
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

pub async fn run_command(command: &str, policy: &SandboxPolicy) -> anyhow::Result<SandboxResult> {
    let started = Instant::now();
    let mut cmd = Command::new("bash");
    cmd.arg("-lc")
        .arg(command)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .kill_on_drop(true);

    if let Some(dir) = &policy.working_dir {
        cmd.current_dir(dir);
    }

    let result = timeout(Duration::from_millis(policy.timeout_ms), cmd.output()).await;
    let duration_ms = started.elapsed().as_millis();

    match result {
        Ok(output_res) => {
            let output = output_res?;
            let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
            truncate_to_limit(&mut stdout, policy.max_stdout_bytes);
            truncate_to_limit(&mut stderr, policy.max_stdout_bytes);
            Ok(SandboxResult {
                status: if output.status.success() { "ok" } else { "error" }.to_string(),
                stdout,
                stderr,
                duration_ms,
                exit_code: output.status.code(),
                timed_out: false,
            })
        }
        Err(_) => Ok(SandboxResult {
            status: "timeout".to_string(),
            stdout: String::new(),
            stderr: format!("process exceeded {}ms timeout", policy.timeout_ms),
            duration_ms,
            exit_code: None,
            timed_out: true,
        }),
    }
}

fn truncate_to_limit(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    value.truncate(max_bytes);
    value.push_str("\n[truncated]");
}