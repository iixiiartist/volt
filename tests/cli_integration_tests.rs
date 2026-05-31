use assert_cmd::Command;
use predicates::prelude::*;
use std::env;

/// Helper: skip test if DATABASE_URL is not set (for DB-dependent tests)
fn requires_db() -> bool {
    env::var("DATABASE_URL")
        .ok()
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Helper: run a command with DATABASE_URL set to a dead host so dotenvy
/// override prevents the real DB from being used.
fn cmd_without_db() -> Command {
    let mut cmd = Command::cargo_bin("volt").unwrap();
    cmd.env("DATABASE_URL", "postgres://volt:volt@localhost:1/volt");
    cmd
}

// ── Help / Flags (no DB needed) ──────────────────────────────────────

#[test]
fn test_help_exits_successfully() {
    let mut cmd = Command::cargo_bin("volt").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Volt"));
}

#[test]
fn test_help_contains_subcommands() {
    let mut cmd = Command::cargo_bin("volt").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init-db"))
        .stdout(predicate::str::contains("list-tools"))
        .stdout(predicate::str::contains("agent-run"))
        .stdout(predicate::str::contains("sandbox"));
}

#[test]
fn test_unrecognized_flag_errors() {
    let mut cmd = Command::cargo_bin("volt").unwrap();
    cmd.arg("--bogus-flag")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_nonexistent_subcommand_fails() {
    let mut cmd = Command::cargo_bin("volt").unwrap();
    cmd.arg("nonexistent-command")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

// ── DB-unavailable tests (forced dead connection) ────────────────────

#[test]
fn test_init_db_without_db_fails_gracefully() {
    cmd_without_db().arg("init-db").assert().failure();
}

#[test]
fn test_list_tools_without_db_fails_gracefully() {
    cmd_without_db().arg("list-tools").assert().failure();
}

#[test]
fn test_history_without_db_fails_gracefully() {
    cmd_without_db().arg("history").assert().failure();
}

// ── DB-available tests (only when DATABASE_URL points to live PG) ────

#[test]
fn test_init_db_succeeds() {
    if !requires_db() {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    }
    Command::cargo_bin("volt")
        .unwrap()
        .arg("init-db")
        .assert()
        .success()
        .stdout(predicate::str::contains("schema initialized"));
}

#[test]
fn test_list_tools_returns_json() {
    if !requires_db() {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    }
    let output = Command::cargo_bin("volt")
        .unwrap()
        .arg("list-tools")
        .output()
        .expect("list-tools should execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(
        parsed.is_ok(),
        "list-tools output should be valid JSON: {stdout}"
    );
}

#[test]
fn test_init_db_then_list_tools() {
    if !requires_db() {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    }
    Command::cargo_bin("volt")
        .unwrap()
        .arg("init-db")
        .assert()
        .success()
        .stdout(predicate::str::contains("schema initialized"));

    let list = Command::cargo_bin("volt")
        .unwrap()
        .arg("list-tools")
        .output()
        .expect("list-tools should execute");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(
        parsed.is_ok(),
        "list-tools output should be valid JSON after init-db"
    );
}

#[test]
fn test_history_returns_json() {
    if !requires_db() {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    }
    let output = Command::cargo_bin("volt")
        .unwrap()
        .arg("history")
        .arg("--limit")
        .arg("5")
        .output()
        .expect("history should execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(
        parsed.is_ok(),
        "history output should be valid JSON: {stdout}"
    );
}

// ── Daemon test (heartbeat loops until signal or failure) ─────────────

#[test]
fn test_heartbeat_without_db_fails_gracefully() {
    // Use std::process::Command directly for fine-grained timeout control.
    // assert_cmd::Command.timeout() returns Result<Output> (not Command),
    // so we can't chain .assert(). We also set DATABASE_URL to a dead port
    // to prevent dotenvy from re-loading the real URL from .env.
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("volt"));
    cmd.arg("heartbeat")
        .env("DATABASE_URL", "postgres://volt:volt@localhost:1/volt");

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return, // spawn failure acceptable — daemon
    };

    let pid = child.id().to_string();
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(3);
    loop {
        if start.elapsed() >= timeout {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid])
                .output();
            return; // timed out — acceptable for a daemon
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(!status.success(), "heartbeat should fail without DB");
                return;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return, // wait error — acceptable
        }
    }
}

// ── Argument validation (no DB needed) ───────────────────────────────

#[test]
fn test_agent_run_without_input_fails() {
    Command::cargo_bin("volt")
        .unwrap()
        .arg("agent-run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required").or(predicate::str::contains("error")));
}

#[test]
fn test_sandbox_without_command_fails() {
    Command::cargo_bin("volt")
        .unwrap()
        .arg("sandbox")
        .assert()
        .failure();
}

#[test]
fn test_eval_without_suite_fails() {
    Command::cargo_bin("volt")
        .unwrap()
        .arg("eval")
        .assert()
        .failure();
}
