use crate::models::MCPTransport;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct MCPClient {
    transport: MCPTransport,
    request_id: u64,
}

impl MCPClient {
    pub fn new(transport: MCPTransport) -> Self {
        Self {
            transport,
            request_id: 0,
        }
    }

    pub async fn list_tools(&mut self) -> anyhow::Result<Vec<String>> {
        self.request_id += 1;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": self.request_id
        });
        let response = self.send_request(&request).await?;
        let tools = response["result"]["tools"]
            .as_array()
            .map(|arr| arr.iter().map(|t| t["name"].as_str().unwrap_or("").to_string()).collect())
            .unwrap_or_default();
        Ok(tools)
    }

    pub async fn call_tool(&mut self, name: &str, args: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
        self.request_id += 1;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": args
            },
            "id": self.request_id
        });
        self.send_request(&request).await
    }

    async fn send_request(&self, request: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
        match &self.transport {
            MCPTransport::Stdio { command, args } => {
                let mut cmd = Command::new(command);
                cmd.args(args);
                cmd.stdin(std::process::Stdio::piped());
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                let mut child = cmd.spawn()?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(request.to_string().as_bytes()).await?;
                    stdin.flush().await?;
                }

                let output = child.wait_with_output().await?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(serde_json::from_str(&stdout)?)
            }
            MCPTransport::Http { url, headers } => {
                let client = reqwest::Client::new();
                let mut req = client.post(url).json(request);
                if let Some(hdrs) = headers {
                    for (k, v) in hdrs {
                        req = req.header(k, v);
                    }
                }
                let resp = req.send().await?.error_for_status()?;
                Ok(resp.json().await?)
            }
        }
    }
}