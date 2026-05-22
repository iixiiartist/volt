use crate::models::{MCPServerConfig, ToolResult};
use serde_json::Value;

pub async fn call_mcp_tool(server: &MCPServerConfig, tool: &str, args: &Value) -> ToolResult {
    match &server.transport {
        crate::models::MCPTransport::Stdio { command, args: cmd_args } => {
            let mut cmd = tokio::process::Command::new(command);
            cmd.args(cmd_args);
            if let Some(env) = &server.env {
                cmd.envs(env);
            }

            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {
                    "name": tool,
                    "arguments": args
                },
                "id": 1
            });

            let mut child = cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| anyhow::anyhow!("MCP spawn failed: {}", e))
                .unwrap();

            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(request.to_string().as_bytes()).await.unwrap();
                drop(stdin);
            }

            let output = child.wait_with_output().await.unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            ToolResult {
                success: output.status.success(),
                output: stdout,
                error: if output.status.success() { None } else { Some(String::from_utf8_lossy(&output.stderr).to_string()) },
                duration_ms: 0,
            }
        }
        crate::models::MCPTransport::Http { url, headers: _ } => {
            let client = reqwest::Client::new();
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {
                    "name": tool,
                    "arguments": args
                },
                "id": 1
            });

            match client.post(url).json(&request).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.text().await {
                        Ok(body) => ToolResult {
                            success: status.is_success(),
                            output: body,
                            error: None,
                            duration_ms: 0,
                        },
                        Err(e) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("MCP HTTP body: {}", e)),
                            duration_ms: 0,
                        },
                    }
                }
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("MCP HTTP failed: {}", e)),
                    duration_ms: 0,
                },
            }
        }
    }
}