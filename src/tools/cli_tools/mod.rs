use crate::attenuation::TrustLevel;
use crate::models::{PermissionLevel, ToolResult};
use crate::tools::ToolRegistry;
use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

static ALLOWED_BINARIES: &[&str] = &[
    "task",
    "crm",
    "hledger",
    "khal",
    "vdirsyncer",
    "qsv",
    "himalaya",
];

static ALLOWED_BINARIES_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| ALLOWED_BINARIES.iter().copied().collect());

/// Subcommands/verbs for each binary that mutate state. Used to reject
/// mutating calls when `cli_query` is invoked under a ReadOnly permission.
static MUTATING_VERBS: LazyLock<HashMap<&'static str, &'static [&'static str]>> =
    LazyLock::new(|| {
        HashMap::from([
            (
                "task",
                &[
                    "add", "delete", "modify", "done", "start", "stop", "edit", "annotate", "denotate",
                    "log", "append", "prepend", "replace", "import", "export", "config",
                ][..],
            ),
            (
                "crm",
                &["add", "create", "update", "delete", "remove", "set", "import"][..],
            ),
            (
                "hledger",
                &["add", "import", "rewrite", "print", "register", "balance"][..],
            ),
            (
                "khal",
                &["new", "add", "edit", "delete", "remove", "import", "export"][..],
            ),
            (
                "vdirsyncer",
                &["sync", "discover", "upload", "download"][..],
            ),
            (
                "qsv",
                &[
                    "slice", "sort", "rename", "select", "dedup", "frequency", "luf", "cat",
                    "apply", "enum", "fill", "replace", "update", "join", "flatten", "pivot",
                    "transpose", "search", "edit",
                ][..],
            ),
            (
                "himalaya",
                &[
                    "template", "flag", "copy", "move", "delete", "send", "save", "compose",
                    "reply", "forward",
                ][..],
            ),
        ])
    });

const ERR_MISSING_BINARY: &str = "missing required field: binary";

fn allowed_binaries_schema() -> serde_json::Value {
    serde_json::json!(ALLOWED_BINARIES)
}

fn whitelist_violation(binary: &str) -> String {
    format!(
        "Security Violation: binary '{}' not in whitelist {:?}",
        binary, ALLOWED_BINARIES
    )
}

pub async fn register_cli_tools(registry: &ToolRegistry) {
    registry
        .register(
            "cli_exec",
            "[ENTERPRISE ONLY] Execute a whitelisted enterprise CLI binary (task, crm, hledger, khal, vdirsyncer, qsv, himalaya). Requires VOLT_ENABLE_CLI_TOOLS=1. Use ONLY when the user explicitly asks you to run one of these specific business tools. Do NOT use for general queries or data the user did not request.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "binary": {
                        "type": "string",
                        "enum": allowed_binaries_schema(),
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
            "[ENTERPRISE ONLY] Execute a whitelisted CLI binary and return structured JSON output. Requires VOLT_ENABLE_CLI_TOOLS=1. Use ONLY for read-only queries on the specific business tools listed above, when the user explicitly requests it. Mutating subcommands (add/delete/modify/send/sync, etc.) are rejected.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "binary": {
                        "type": "string",
                        "enum": allowed_binaries_schema(),
                        "description": "Whitelisted CLI binary to query"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments passed directly to the binary. No shell operators allowed. Mutating verbs are rejected."
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

fn parse_args(args: &serde_json::Value) -> Vec<String> {
    args["args"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn fail(started: Instant, err: String) -> ToolResult {
    ToolResult {
        success: false,
        output: String::new(),
        error: Some(err),
        duration_ms: started.elapsed().as_millis(),
    }
}

async fn run_cli(binary: &str, raw_args: &[String]) -> Result<(i32, String, String), String> {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(raw_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to spawn '{}': {}", binary, e))?;
    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((exit_code, stdout, stderr))
}

fn make_cli_exec_fn() -> crate::tools::ToolFn {
    Arc::new(|args| {
        Box::pin(async move {
            let started = Instant::now();
            let binary = match args["binary"].as_str() {
                Some(b) => b.to_string(),
                None => return fail(started, ERR_MISSING_BINARY.into()),
            };
            let raw_args = parse_args(&args);

            if !ALLOWED_BINARIES_SET.contains(binary.as_str()) {
                return fail(started, whitelist_violation(&binary));
            }

            match run_cli(&binary, &raw_args).await {
                Ok((exit_code, stdout, stderr)) => {
                    if exit_code != 0 {
                        return ToolResult {
                            success: false,
                            output: stdout,
                            error: Some(format!(
                                "exit code {}: {}",
                                exit_code,
                                if stderr.is_empty() { "no stderr" } else { &stderr }
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
                }
                Err(e) => fail(started, e),
            }
        })
    })
}

fn first_non_flag_arg(args: &[String]) -> Option<&str> {
    args.iter()
        .map(|s| s.as_str())
        .find(|s| !s.starts_with('-'))
}

fn make_cli_query_fn() -> crate::tools::ToolFn {
    Arc::new(|args| {
        Box::pin(async move {
            let started = Instant::now();
            let binary = match args["binary"].as_str() {
                Some(b) => b.to_string(),
                None => return fail(started, ERR_MISSING_BINARY.into()),
            };
            let raw_args = parse_args(&args);

            if !ALLOWED_BINARIES_SET.contains(binary.as_str()) {
                return fail(started, whitelist_violation(&binary));
            }

            if let Some(verb) = first_non_flag_arg(&raw_args) {
                let lower = verb.to_ascii_lowercase();
                if let Some(denied) = MUTATING_VERBS
                    .get(binary.as_str())
                    .and_then(|verbs| {
                        verbs
                            .iter()
                            .find(|v| v.eq_ignore_ascii_case(&lower))
                            .map(|s| s.to_string())
                    })
                {
                    return fail(
                        started,
                        format!(
                            "cli_query is read-only: verb '{}' mutates state for binary '{}'. Use cli_exec instead.",
                            denied, binary
                        ),
                    );
                }
            }

            match run_cli(&binary, &raw_args).await {
                Ok((exit_code, stdout, stderr)) => {
                    if exit_code != 0 {
                        return ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "exit code {}: {}",
                                exit_code,
                                if stderr.is_empty() { "no stderr" } else { &stderr }
                            )),
                            duration_ms: started.elapsed().as_millis(),
                        };
                    }
                    let payload = match serde_json::from_str::<serde_json::Value>(&stdout) {
                        Ok(v) => v,
                        Err(_) => serde_json::json!({ "text_output": stdout }),
                    };
                    ToolResult {
                        success: true,
                        output: payload.to_string(),
                        error: None,
                        duration_ms: started.elapsed().as_millis(),
                    }
                }
                Err(e) => fail(started, e),
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_non_flag_arg_skips_flags() {
        let args = vec!["-v".to_string(), "--json".to_string(), "list".to_string()];
        assert_eq!(first_non_flag_arg(&args), Some("list"));
        let empty: Vec<String> = vec![];
        assert_eq!(first_non_flag_arg(&empty), None);
    }

    #[test]
    fn mutating_verbs_cover_all_binaries() {
        for b in ALLOWED_BINARIES {
            assert!(
                MUTATING_VERBS.contains_key(*b),
                "binary '{}' missing mutating-verb denylist",
                b
            );
        }
    }
}
