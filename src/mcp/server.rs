use crate::capability::{CapabilityManager, CapabilityScope};
use crate::tools::ToolRegistry;
use axum::{extract::State, routing::post, Json, Router};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Shared application state for the MCP HTTP server.
pub struct McpAppState {
    pub tools: Arc<ToolRegistry>,
    pub agent_name: String,
    pub capability_manager: Arc<CapabilityManager>,
}

pub struct MCPServer {
    tools: Arc<ToolRegistry>,
    capability_manager: Arc<CapabilityManager>,
}

impl MCPServer {
    pub async fn new(tools: Arc<ToolRegistry>) -> Self {
        let mgr = Arc::new(CapabilityManager::new());
        mgr.issue(CapabilityScope::FsRead, 50, chrono::Duration::hours(1)).await;
        mgr.issue(CapabilityScope::FsWrite, 20, chrono::Duration::hours(1)).await;
        mgr.issue(CapabilityScope::System, 20, chrono::Duration::hours(1)).await;
        mgr.issue(CapabilityScope::Network, 100, chrono::Duration::hours(1)).await;
        mgr.issue(CapabilityScope::Database, 10, chrono::Duration::hours(1)).await;
        mgr.issue(CapabilityScope::Memory, 20, chrono::Duration::hours(1)).await;
        Self { tools, capability_manager: mgr }
    }

    /// Serve MCP over stdio (stdin/stdout JSON-RPC).
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

    /// Serve MCP over HTTP on the given address.
    /// Enables agent-to-agent tool sharing — other agents can connect
    /// to this server and use the registered tools remotely.
    pub async fn serve_http(
        tools: Arc<ToolRegistry>,
        addr: &str,
        agent_name: &str,
    ) -> anyhow::Result<()> {
        let mgr = {
            let m = Arc::new(CapabilityManager::new());
            m.issue(CapabilityScope::FsRead, 50, chrono::Duration::hours(1)).await;
            m.issue(CapabilityScope::FsWrite, 20, chrono::Duration::hours(1)).await;
            m.issue(CapabilityScope::System, 20, chrono::Duration::hours(1)).await;
            m.issue(CapabilityScope::Network, 100, chrono::Duration::hours(1)).await;
            m.issue(CapabilityScope::Database, 10, chrono::Duration::hours(1)).await;
            m.issue(CapabilityScope::Memory, 20, chrono::Duration::hours(1)).await;
            m
        };
        let state = Arc::new(McpAppState {
            tools,
            agent_name: agent_name.to_string(),
            capability_manager: mgr,
        });

        let app = Router::new()
            .route("/mcp", post(handle_mcp_request))
            .route("/mcp/tools/list", post(handle_tools_list))
            .route("/mcp/tools/call", post(handle_tools_call))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("MCP server '{}' listening on http://{}", agent_name, addr);
        axum::serve(listener, app).await?;
        Ok(())
    }

    /// Handle a JSON-RPC MCP request.
    pub async fn handle_request(&self, request: &serde_json::Value) -> serde_json::Value {
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
                let result = self.tools.execute_gated(name, args, &self.capability_manager).await;
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

/// Axum handler for generic JSON-RPC MCP requests.
async fn handle_mcp_request(
    State(state): State<Arc<McpAppState>>,
    Json(request): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let server = MCPServer {
        tools: state.tools.clone(),
        capability_manager: state.capability_manager.clone(),
    };
    let response = server.handle_request(&request).await;
    Json(response)
}

/// Axum handler for tools/list.
async fn handle_tools_list(State(state): State<Arc<McpAppState>>) -> Json<serde_json::Value> {
    let defs = state.tools.get_definitions().await;
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
    Json(serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "tools": tools },
        "id": 1
    }))
}

/// Axum handler for tools/call.
async fn handle_tools_call(
    State(state): State<Arc<McpAppState>>,
    Json(request): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let name = request["params"]["name"].as_str().unwrap_or("");
    let args = &request["params"]["arguments"];
    let server = MCPServer {
        tools: state.tools.clone(),
        capability_manager: state.capability_manager.clone(),
    };
    let fake_req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": { "name": name, "arguments": args },
        "id": request.get("id").cloned().unwrap_or(serde_json::json!(1))
    });
    let response = server.handle_request(&fake_req).await;
    Json(response)
}
