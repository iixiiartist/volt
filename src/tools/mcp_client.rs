use crate::models::{MCPServerConfig, MCPTransport, ToolResult};
use serde_json::Value;

pub async fn call_mcp_tool(server: &MCPServerConfig, tool: &str, args: &Value) -> ToolResult {
    match &server.transport {
        MCPTransport::Stdio {
            command,
            args: cmd_args,
        } => {
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

            let mut child = match cmd
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("MCP spawn failed: {}", e)),
                        duration_ms: 0,
                    }
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                if let Err(e) = stdin.write_all(request.to_string().as_bytes()).await {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("MCP write failed: {}", e)),
                        duration_ms: 0,
                    };
                }
            }

            let output = match child.wait_with_output().await {
                Ok(o) => o,
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("MCP wait failed: {}", e)),
                        duration_ms: 0,
                    }
                }
            };
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            ToolResult {
                success: output.status.success(),
                output: stdout,
                error: if output.status.success() {
                    None
                } else {
                    Some(String::from_utf8_lossy(&output.stderr).to_string())
                },
                duration_ms: 0,
            }
        }
        crate::models::MCPTransport::Http { url, headers } => {
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {
                    "name": tool,
                    "arguments": args
                },
                "id": 1
            });

            http_send(url, headers, request).await
        }
        MCPTransport::WebSocket { url, headers } => {
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {
                    "name": tool,
                    "arguments": args
                },
                "id": 1
            });
            http_send(url, headers, request).await
        }
    }
}

async fn http_send(
    url: &str,
    headers: &Option<std::collections::HashMap<String, String>>,
    request: serde_json::Value,
) -> ToolResult {
    let client = reqwest::Client::new();
    let mut req_builder = client.post(url).json(&request);
    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            req_builder = req_builder.header(k, v);
        }
    }
    match req_builder.send().await {
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
