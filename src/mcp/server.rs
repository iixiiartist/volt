use crate::tools::ToolRegistry;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct MCPServer {
    tools: Arc<ToolRegistry>,
}

impl MCPServer {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }

    pub async fn serve_stdio(&self) -> anyhow::Result<()> {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let request: serde_json::Value = serde_json::from_str(&line)?;
            let response = self.handle_request(&request).await;
            let output = serde_json::to_string(&response)?;
            let mut stdout = tokio::io::stdout();
            stdout.write_all(output.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
        Ok(())
    }

    async fn handle_request(&self, request: &serde_json::Value) -> serde_json::Value {
        let method = request["method"].as_str().unwrap_or("");
        let id = request["id"].clone();

        match method {
            "tools/list" => {
                let defs = self.tools.get_definitions().await;
                let tools: Vec<serde_json::Value> = defs
                    .into_iter()
                    .map(|d| {
                        serde_json::json!({
                            "name": d.name,
                            "description": d.description,
                            "inputSchema": d.input_schema
                        })
                    })
                    .collect();
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "result": { "tools": tools },
                    "id": id
                })
            }
            "tools/call" => {
                let name = request["params"]["name"].as_str().unwrap_or("");
                let args = &request["params"]["arguments"];
                let result = self.tools.execute(name, args).await;
                match result {
                    Ok(res) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": res.output
                            }],
                            "isError": !res.success
                        },
                        "id": id
                    }),
                    Err(e) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": format!("error: {}", e)
                            }],
                            "isError": true
                        },
                        "id": id
                    }),
                }
            }
            _ => serde_json::json!({
                "jsonrpc": "2.0",
                "error": { "code": -32601, "message": "method not found" },
                "id": id
            }),
        }
    }
}
