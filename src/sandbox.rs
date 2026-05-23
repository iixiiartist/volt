use crate::models::{SandboxPolicy, SandboxResult};
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

fn sandbox_cmd(program: &str, policy: &SandboxPolicy) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .kill_on_drop(true);
    if let Some(dir) = &policy.working_dir {
        cmd.current_dir(dir);
    }
    cmd
}

async fn run_sandbox_output(
    mut cmd: Command,
    timeout_ms: u64,
    max_stdout_bytes: usize,
) -> SandboxResult {
    let started = Instant::now();
    let result = timeout(Duration::from_millis(timeout_ms), cmd.output()).await;
    let duration_ms = started.elapsed().as_millis();

    match result {
        Ok(output_res) => {
            let output = match output_res {
                Ok(o) => o,
                Err(e) => {
                    return SandboxResult {
                        status: "error".to_string(),
                        stdout: String::new(),
                        stderr: format!("process error: {}", e),
                        duration_ms,
                        exit_code: None,
                        timed_out: false,
                    }
                }
            };
            let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
            truncate_to_limit(&mut stdout, max_stdout_bytes);
            truncate_to_limit(&mut stderr, max_stdout_bytes);
            SandboxResult {
                status: if output.status.success() { "ok" } else { "error" }.to_string(),
                stdout,
                stderr,
                duration_ms,
                exit_code: output.status.code(),
                timed_out: false,
            }
        }
        Err(_) => SandboxResult {
            status: "timeout".to_string(),
            stdout: String::new(),
            stderr: format!("process exceeded {}ms timeout", timeout_ms),
            duration_ms,
            exit_code: None,
            timed_out: true,
        },
    }
}

pub async fn run_command(command: &str, policy: &SandboxPolicy) -> anyhow::Result<SandboxResult> {
    let shell = std::env::var("SANDBOX_SHELL").unwrap_or_else(|_| "bash".into());
    let mut cmd = Command::new(&shell);
    cmd.arg("-lc")
        .arg(command)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .kill_on_drop(true);

    if let Some(dir) = &policy.working_dir {
        cmd.current_dir(dir);
    }

    let result = run_sandbox_output(cmd, policy.timeout_ms, policy.max_stdout_bytes).await;
    Ok(result)
}

async fn run_spawned(
    mut child: tokio::process::Child,
    stdin_data: &str,
    timeout_ms: u64,
    max_stdout_bytes: usize,
) -> SandboxResult {
    use tokio::io::AsyncWriteExt;
    let started = Instant::now();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_data.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }
    let out_res = timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await;
    let duration_ms = started.elapsed().as_millis();
    match out_res {
        Ok(Ok(output)) => {
            let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
            truncate_to_limit(&mut stdout, max_stdout_bytes);
            truncate_to_limit(&mut stderr, max_stdout_bytes);
            SandboxResult {
                status: if output.status.success() { "ok" } else { "error" }.to_string(),
                stdout,
                stderr,
                duration_ms,
                exit_code: output.status.code(),
                timed_out: false,
            }
        }
        Ok(Err(e)) => SandboxResult {
            status: "error".to_string(),
            stdout: String::new(),
            stderr: format!("process error: {}", e),
            duration_ms,
            exit_code: None,
            timed_out: false,
        },
        Err(_) => SandboxResult {
            status: "timeout".to_string(),
            stdout: String::new(),
            stderr: format!("process exceeded {}ms timeout", timeout_ms),
            duration_ms,
            exit_code: None,
            timed_out: true,
        },
    }
}

pub async fn run_command_direct(
    program: &str,
    args: &[&str],
    stdin: Option<&str>,
    policy: &SandboxPolicy,
) -> SandboxResult {
    let mut cmd = sandbox_cmd(program, policy);
    cmd.args(args);
    if let Some(input) = stdin {
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        match cmd.spawn() {
            Ok(child) => run_spawned(child, input, policy.timeout_ms, policy.max_stdout_bytes).await,
            Err(e) => SandboxResult {
                status: "error".to_string(),
                stdout: String::new(),
                stderr: format!("failed to spawn '{}': {}", program, e),
                duration_ms: 0,
                exit_code: None,
                timed_out: false,
            },
        }
    } else {
        run_sandbox_output(cmd, policy.timeout_ms, policy.max_stdout_bytes).await
    }
}

fn truncate_to_limit(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value.push_str("\n[truncated]");
}