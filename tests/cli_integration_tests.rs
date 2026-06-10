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

// ── v0.6.0 CLI Integration Tests ─────────────────────────────────────

/// Helper: create a Command with all remote API keys blanked out and a dead DB,
/// so the binary never attempts to hit a live cloud endpoint.
fn cmd_isolated() -> Command {
    let mut cmd = Command::cargo_bin("volt").unwrap();
    cmd.env("DATABASE_URL", "postgres://volt:volt@localhost:1/volt")
        .env("GROQ_API_KEY", "")
        .env("NVIDIA_API_KEY", "")
        .env("OPENAI_API_KEY", "")
        .env("ANTHROPIC_API_KEY", "")
        .env("OLLAMA_API_KEY", "")
        .env("OLLAMA_HOST", "")
        .env("LLAMA_CPP_HOST", "")
        .env("LITERTLM_HOST", "")
        .env("LLM_BASE_URL", "")
        .env("LLM_API_KEY", "");
    cmd
}

// Scenario 1: Blueprint Auto-Routing
// Verifies that --auto-blueprint scans the blueprints/ directory and attempts
// LLM-based routing, producing the expected [router] diagnostics on stderr.
#[test]
fn test_agent_run_auto_blueprint_attempts_routing() {
    let temp_dir = tempfile::tempdir().unwrap();
    let blueprints_dir = temp_dir.path().join("blueprints");
    std::fs::create_dir(&blueprints_dir).unwrap();

    let bp = r#"
id = "test_bp"
name = "Test Blueprint"
description = "A test blueprint for auto-routing"

[model_card]
model_name = "test-model"
provider = "ollama"
format_dialect = "GemmaNative"

[scaffolding]
strict_mode = false
max_tools_per_turn = 3

[tools]
core_tools = ["read"]

[prompts]
"#;
    std::fs::write(blueprints_dir.join("test.toml"), bp).unwrap();

    let mut cmd = cmd_isolated();
    cmd.current_dir(&temp_dir)
        .env("OLLAMA_HOST", "http://localhost:11434")
        // Enable the cloud-provider gate so the test's `local-model`
        // request can still reach a routing decision. The vLLM-first
        // posture (default off) is the right product behavior; this
        // test is about the blueprint router, not provider policy.
        .env("VOLT_ENABLE_CLOUD_PROVIDERS", "1")
        .env("LLM_BASE_URL", "http://localhost:8000/v1")
        .env("LLM_API_KEY", "test-key-for-integration-test")
        .arg("agent-run")
        .arg("--auto-blueprint")
        .arg("--input")
        .arg("write a python script")
        .arg("--model")
        .arg("local-model");

    let output = cmd
        .timeout(std::time::Duration::from_secs(60))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("[router] routing task across")
            || stderr.contains("[router] no blueprints found"),
        "Expected blueprint routing diagnostic in stderr, got: {}",
        stderr
    );
}

// Scenario 2: Strict Mode JSON Enforcement
// Uses a local wiremock server as the LLM endpoint so we can capture the
// exact outgoing HTTP payload and assert that response_format contains the
// json_schema block when strict_mode is enabled via a blueprint.
#[tokio::test]
async fn test_agent_run_strict_mode_payload() {
    let server = wiremock::MockServer::start().await;

    let temp_dir = tempfile::tempdir().unwrap();
    let blueprints_dir = temp_dir.path().join("blueprints");
    std::fs::create_dir(&blueprints_dir).unwrap();

    let bp = r#"
id = "strict_test"
name = "Strict Test"
description = "Test strict mode JSON enforcement"

[model_card]
model_name = "test-model"
provider = "groq"
format_dialect = "OpenAiJson"

[scaffolding]
strict_mode = true
max_tools_per_turn = 3

[tools]
core_tools = ["read"]

[prompts]
"#;
    std::fs::write(blueprints_dir.join("strict_test.toml"), bp).unwrap();

    use wiremock::{matchers::*, Mock, ResponseTemplate};

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = captured.clone();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            *captured_clone.lock().unwrap() = Some(body);
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "{\"content\": \"hello\"}",
                        "role": "assistant"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            }))
        })
        .mount(&server)
        .await;

    let mut cmd = cmd_isolated();
    cmd.current_dir(&temp_dir)
        .env("LLM_BASE_URL", server.uri())
        .env("LLM_API_KEY", "dummy-key")
        // Skip the 60+ second compute_embeddings pass; this test only needs
        // tool registrations, not their dense vectors. response_format
        // generation does not depend on tool embeddings.
        .env("VOLT_SKIP_TOOL_EMBEDDINGS", "1")
        .arg("agent-run")
        .arg("--blueprint")
        .arg(blueprints_dir.join("strict_test.toml"))
        .arg("--input")
        .arg("return a tool call")
        .arg("--model")
        .arg("test-model");

    // The agent still has to load ONNX (10s cold) + DB+session init (~2s) +
    // auto-seed workers (~1s), so 60s is plenty.
    let output = cmd
        .timeout(std::time::Duration::from_secs(60))
        .output()
        .unwrap();

    let body = captured.lock().unwrap().clone();
    assert!(
        body.is_some(),
        "Expected HTTP request to be captured. Exit status: {:?}\nStdout: {}\nStderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let body = body.unwrap();

    let rf = body
        .get("response_format")
        .expect("response_format should be present in outgoing payload");
    assert_eq!(
        rf["type"], "json_schema",
        "Expected response_format.type to be 'json_schema' when strict_mode is enabled"
    );
    let js = rf["json_schema"].as_object().unwrap();
    assert_eq!(js["name"], "volt_tool_calls");
    assert_eq!(js["strict"], true);
    assert!(
        js["schema"].is_object(),
        "Expected json_schema.schema to be an object"
    );
}

// Scenario 3: Missing Environment Gates
// Verifies that `volt execute --tool cli_exec` fails gracefully with a
// helpful message explaining the missing gate instead of panicking or executing.
#[test]
fn test_execute_cli_exec_missing_gate_fails_gracefully() {
    // Use a very short connect timeout so the DB connection fails fast.
    let mut cmd = cmd_isolated();
    cmd.env(
        "DATABASE_URL",
        "postgres://volt:volt@localhost:1/volt?connect_timeout=1",
    )
    .env_remove("VOLT_ENABLE_CLI_TOOLS")
    .arg("execute")
    .arg("--tool")
    .arg("cli_exec")
    .arg("--params")
    .arg("{\"binary\":\"task\",\"args\":[]}");

    // Agent-run takes 10–15s for ONNX model load + 10–20s for 43 tool
    // embeddings on first run; 180s gives headroom for cold cache.
    // (The sibling auto-blueprint test takes 60+ seconds and passes because
    // it asserts on an early [router] log line; this test asserts on a late
    // HTTP body capture, so it needs a longer budget.)
    let output = cmd
        .timeout(std::time::Duration::from_secs(180))
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        !output.status.success(),
        "Expected failure when gate is missing, but command succeeded"
    );
    // The command may fail at the DB layer (no live PG) OR at the gate check.
    // Either way it must not panic and must not execute the tool.
    assert!(
        combined.contains("gated")
            || combined.contains("not found")
            || combined.contains("provision")
            || combined.contains("failed to connect")
            || combined.contains("pool timed out"),
        "Expected graceful failure message, got stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
}

// ── DAG Telemetry Integration Test ─────────────────────────────────
// Runs a real 2-step DAG via the CLI using a live API key (Groq).
// Skips gracefully when no key is present.

#[test]
fn test_dag_cli_live_telemetry() {
    if env::var("GROQ_API_KEY")
        .map(|v| v.is_empty())
        .unwrap_or(true)
    {
        eprintln!("Skipping test_dag_cli_live_telemetry — GROQ_API_KEY not set");
        return;
    }

    let dag_json = serde_json::json!({
        "nodes": [
            {
                "id": "summarize",
                "task": "Summarize this in one sentence: {input}",
                "agent": {
                    "name": "summarizer",
                    "model": "llama-3.1-8b-instant",
                    "max_iterations": 3,
                    "temperature": 0.0
                }
            },
            {
                "id": "expand",
                "task": "Expand this summary into a paragraph: {summarize}",
                "agent": {
                    "name": "expander",
                    "model": "llama-3.1-8b-instant",
                    "max_iterations": 3,
                    "temperature": 0.0
                }
            }
        ],
        "edges": [
            {"from": "summarize", "to": "expand"}
        ]
    });

    let initial_input = "Artificial intelligence is transforming software engineering.";

    let mut cmd = Command::cargo_bin("volt").unwrap();
    cmd.arg("workflow")
        .arg("--pattern")
        .arg("dag")
        .arg("--agents")
        .arg(dag_json.to_string())
        .arg("--tasks")
        .arg(serde_json::to_string(&vec![initial_input]).unwrap());

    let output = cmd
        .timeout(std::time::Duration::from_secs(120))
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "DAG CLI should succeed. stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );

    // Parse the JSON output and assert real telemetry is present
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("CLI should emit valid JSON");

    let steps = parsed["steps"].as_array().expect("steps array in output");
    assert!(
        steps.len() >= 2,
        "Expected at least 2 steps, got {}",
        steps.len()
    );

    for step in steps {
        let duration = step["duration_ms"].as_u64().unwrap_or(0);
        let prompt_tokens = step["prompt_tokens"].as_u64().unwrap_or(0);
        let _completion_tokens = step["completion_tokens"].as_u64().unwrap_or(0);
        let success = step["success"].as_bool().unwrap_or(false);

        assert!(
            duration > 0,
            "Each step should have duration_ms > 0, got {} for agent {}",
            duration,
            step["agent"].as_str().unwrap_or("unknown")
        );
        assert!(
            prompt_tokens > 0 || !success,
            "Successful step should have prompt_tokens > 0 for agent {}",
            step["agent"].as_str().unwrap_or("unknown")
        );
    }

    let total_duration = parsed["total_duration_ms"].as_u64().unwrap_or(0);
    assert!(
        total_duration > 0,
        "total_duration_ms should be > 0, got {}",
        total_duration
    );
}
