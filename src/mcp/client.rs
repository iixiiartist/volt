use crate::models::MCPTransport;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct MCPClient {
    transport: MCPTransport,
    request_id: AtomicU64,
    access_token: Mutex<Option<String>>,
}

impl MCPClient {
    pub fn new(transport: MCPTransport) -> Self {
        Self {
            transport,
            request_id: AtomicU64::new(0),
            access_token: Mutex::new(None),
        }
    }

    pub fn set_token(&self, token: &str) {
        *self.access_token.lock().unwrap() = Some(token.to_string());
    }

    pub fn load_token_from(&self, path: &str) {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(token) = val.get("access_token").and_then(|v| v.as_str()) {
                    *self.access_token.lock().unwrap() = Some(token.to_string());
                }
            }
        }
    }

    pub async fn list_tools(&self) -> anyhow::Result<Vec<String>> {
        let tools = self.list_tools_full().await?;
        Ok(tools
            .iter()
            .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
            .collect())
    }

    pub async fn list_tools_full(&self) -> anyhow::Result<Vec<Value>> {
        let body = jsonrpc_request(
            "tools/list",
            None,
            self.request_id.fetch_add(1, Ordering::Relaxed) + 1,
        );
        let resp = self.send(&body).await?;
        Ok(resp["result"]["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub async fn call_tool(&self, name: &str, args: &Value) -> anyhow::Result<Value> {
        let params = serde_json::json!({ "name": name, "arguments": args });
        let body = jsonrpc_request(
            "tools/call",
            Some(&params),
            self.request_id.fetch_add(1, Ordering::Relaxed) + 1,
        );
        self.send(&body).await
    }

    async fn send(&self, body: &Value) -> anyhow::Result<Value> {
        match &self.transport {
            MCPTransport::Stdio { command, args } => {
                let mut cmd = Command::new(command);
                cmd.args(args);
                cmd.stdin(std::process::Stdio::piped());
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());
                let mut child = cmd.spawn()?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(body.to_string().as_bytes()).await?;
                    stdin.flush().await?;
                }
                let output = child.wait_with_output().await?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(serde_json::from_str(&stdout)?)
            }
            MCPTransport::Http { url, headers } => {
                let client = reqwest::Client::new();
                let mut req = client.post(url).json(body);
                if let Some(hdrs) = headers {
                    for (k, v) in hdrs {
                        req = req.header(k, v);
                    }
                }
                if let Some(token) = self.access_token.lock().unwrap().as_ref() {
                    req = req.header("Authorization", format!("Bearer {}", token));
                }
                let resp = req.send().await?;
                if !resp.status().is_success() {
                    anyhow::bail!("HTTP {} - {}", resp.status(), resp.text().await?);
                }
                Ok(resp.json().await?)
            }
        }
    }
}

fn jsonrpc_request(method: &str, params: Option<&Value>, id: u64) -> Value {
    let mut req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "id": id,
    });
    if let Some(p) = params {
        req["params"] = p.clone();
    }
    req
}
