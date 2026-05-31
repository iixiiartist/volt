use crate::attenuation::TrustLevel;
use crate::models::{PermissionLevel, ToolResult};
use crate::tools::ToolRegistry;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

static ALLOWED_BINARIES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "task",
        "crm",
        "hledger",
        "khal",
        "vdirsyncer",
        "qsv",
        "himalaya",
    ])
});

pub async fn register_cli_tools(registry: &ToolRegistry) {
    registry
        .register(
            "cli_exec",
            "Execute a whitelisted enterprise CLI binary (task, crm, hledger, khal, vdirsyncer, qsv, himalaya) with precise structured arguments. Returns stdout, stderr, and exit code. No shell piping or chaining. Use when you need to mutate state (add a task, send email) or get raw text output.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "binary": {
                        "type": "string",
                        "enum": ["task", "crm", "hledger", "khal", "vdirsyncer", "qsv", "himalaya"],
                        "description": "Whitelisted CLI binary to execute"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments passed directly to the binary. No shell operators allowed."
                    }
                },
                "required": ["binary", "args"]
            }),
            "business",
            make_cli_exec_fn(),
        )
        .await;

    registry
        .register_with_permission(
            "cli_query",
            "Execute a whitelisted CLI binary and return structured JSON output. Automatically parses stdout as JSON if possible, otherwise wraps as text payload. Use for read-only queries like listing tasks, checking balances, or exporting data.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "binary": {
                        "type": "string",
                        "enum": ["task", "crm", "hledger", "khal", "vdirsyncer", "qsv", "himalaya"],
                        "description": "Whitelisted CLI binary to query"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments passed directly to the binary. No shell operators allowed."
                    }
                },
                "required": ["binary", "args"]
            }),
            "business",
            make_cli_query_fn(),
            PermissionLevel::ReadOnly,
            TrustLevel::Builtin,
        )
        .await;
}

fn make_cli_exec_fn() -> crate::tools::ToolFn {
    Arc::new(|args| {
        Box::pin(async move {
            let started = Instant::now();
            let binary = match args["binary"].as_str() {
                Some(b) => b.to_string(),
                None => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("missing required field: binary".into()),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            };
            let raw_args: Vec<String> = args["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if !ALLOWED_BINARIES.contains(binary.as_str()) {
                let allowed: Vec<&str> = ALLOWED_BINARIES.iter().copied().collect();
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Security Violation: binary '{}' not in whitelist {:?}",
                        binary, allowed
                    )),
                    duration_ms: started.elapsed().as_millis(),
                };
            }

            let mut cmd = tokio::process::Command::new(&binary);
            cmd.args(&raw_args);
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            let output = match cmd.output().await {
                Ok(o) => o,
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("failed to spawn '{}': {}", binary, e)),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            };

            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

            if !output.status.success() {
                return ToolResult {
                    success: false,
                    output: stdout,
                    error: Some(format!(
                        "exit code {}: {}",
                        exit_code,
                        if stderr.is_empty() {
                            "no stderr"
                        } else {
                            &stderr
                        }
                    )),
                    duration_ms: started.elapsed().as_millis(),
                };
            }

            ToolResult {
                success: true,
                output: serde_json::json!({
                    "exit_code": exit_code,
                    "stdout": stdout,
                    "stderr": stderr,
                })
                .to_string(),
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        })
    })
}

fn make_cli_query_fn() -> crate::tools::ToolFn {
    Arc::new(|args| {
        Box::pin(async move {
            let started = Instant::now();
            let binary = match args["binary"].as_str() {
                Some(b) => b.to_string(),
                None => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("missing required field: binary".into()),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            };
            let raw_args: Vec<String> = args["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if !ALLOWED_BINARIES.contains(binary.as_str()) {
                let allowed: Vec<&str> = ALLOWED_BINARIES.iter().copied().collect();
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Security Violation: binary '{}' not in whitelist {:?}",
                        binary, allowed
                    )),
                    duration_ms: started.elapsed().as_millis(),
                };
            }

            let mut cmd = tokio::process::Command::new(&binary);
            cmd.args(&raw_args);
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            let output = match cmd.output().await {
                Ok(o) => o,
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("failed to spawn '{}': {}", binary, e)),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            };

            if !output.status.success() {
                let exit_code = output.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "exit code {}: {}",
                        exit_code,
                        if stderr.is_empty() {
                            "no stderr"
                        } else {
                            &stderr
                        }
                    )),
                    duration_ms: started.elapsed().as_millis(),
                };
            }

            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

            let payload = if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&stdout) {
                json_val
            } else {
                serde_json::json!({ "text_output": stdout })
            };

            ToolResult {
                success: true,
                output: payload.to_string(),
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        })
    })
}
