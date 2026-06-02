use crate::models::ToolResult;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

// ─── Proto message types (hand-written, proto is stable and tiny) ─────

#[derive(Clone, Debug, PartialEq, prost::Message)]
pub struct ToolDefinition {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub description: String,
    #[prost(string, tag = "3")]
    pub input_schema_json: String,
}

#[derive(Clone, Debug, PartialEq, prost::Message)]
pub struct ListToolsRequest {}

#[derive(Clone, Debug, PartialEq, prost::Message)]
pub struct ListToolsResponse {
    #[prost(message, repeated, tag = "1")]
    pub tools: Vec<ToolDefinition>,
}

#[derive(Clone, Debug, PartialEq, prost::Message)]
pub struct CallToolRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub arguments_json: String,
}

#[derive(Clone, Debug, PartialEq, prost::Message)]
pub struct CallToolResponse {
    #[prost(bool, tag = "1")]
    pub success: bool,
    #[prost(string, tag = "2")]
    pub output: String,
    #[prost(string, tag = "3")]
    pub error: String,
    #[prost(uint64, tag = "4")]
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, prost::Message)]
pub struct ToolStreamChunk {
    #[prost(oneof = "tool_stream_chunk::Payload", tags = "1, 2")]
    pub payload: Option<tool_stream_chunk::Payload>,
}

pub mod tool_stream_chunk {
    #[derive(Clone, Debug, PartialEq, prost::Oneof)]
    pub enum Payload {
        #[prost(message, tag = "1")]
        FinalResult(super::CallToolResponse),
        #[prost(string, tag = "2")]
        Partial(String),
    }
}

// ─── tonic service trait ────────────────────────────────────────

#[tonic::async_trait]
pub trait Mcp: Send + Sync + 'static {
    async fn list_tools(&self, request: ListToolsRequest) -> Result<ListToolsResponse, Status>;
    async fn call_tool(&self, request: CallToolRequest) -> Result<CallToolResponse, Status>;
    async fn call_tool_stream(
        &self,
        request: CallToolRequest,
    ) -> Result<
        Response<Pin<Box<dyn tokio_stream::Stream<Item = Result<ToolStreamChunk, Status>> + Send>>>,
        Status,
    >;
}

// ─── Server implementation ──────────────────────────────────────

pub struct McpGrpcServer {
    tools: Arc<crate::tools::ToolRegistry>,
    cap_mgr: Arc<crate::capability::CapabilityManager>,
}

impl McpGrpcServer {
    pub fn new(tools: Arc<crate::tools::ToolRegistry>) -> Self {
        let mgr = Arc::new(crate::capability::CapabilityManager::new());
        Self {
            tools,
            cap_mgr: mgr,
        }
    }
}

#[tonic::async_trait]
impl Mcp for McpGrpcServer {
    async fn list_tools(&self, _request: ListToolsRequest) -> Result<ListToolsResponse, Status> {
        let defs = self.tools.get_definitions().await;
        let tools: Vec<ToolDefinition> = defs
            .into_iter()
            .map(|d| ToolDefinition {
                name: d.name,
                description: d.description,
                input_schema_json: d.input_schema.to_string(),
            })
            .collect();
        Ok(ListToolsResponse { tools })
    }

    async fn call_tool(&self, request: CallToolRequest) -> Result<CallToolResponse, Status> {
        let args: Value = serde_json::from_str(&request.arguments_json)
            .map_err(|e| Status::invalid_argument(format!("invalid args JSON: {}", e)))?;
        match self
            .tools
            .execute_gated(&request.name, &args, &self.cap_mgr)
            .await
        {
            Ok(res) => Ok(CallToolResponse {
                success: res.success,
                output: res.output,
                error: res.error.unwrap_or_default(),
                duration_ms: res.duration_ms,
            }),
            Err(e) => Ok(CallToolResponse {
                success: false,
                output: String::new(),
                error: format!("{}", e),
                duration_ms: 0,
            }),
        }
    }

    async fn call_tool_stream(
        &self,
        request: CallToolRequest,
    ) -> Result<
        Response<Pin<Box<dyn tokio_stream::Stream<Item = Result<ToolStreamChunk, Status>> + Send>>>,
        Status,
    > {
        let (tx, rx) = mpsc::channel(64);
        let tools = self.tools.clone();
        let cap_mgr = self.cap_mgr.clone();
        let name = request.name.clone();
        let args: Value = serde_json::from_str(&request.arguments_json)
            .map_err(|e| Status::invalid_argument(format!("invalid args JSON: {}", e)))?;

        tokio::spawn(async move {
            // Send initial partial marker
            let _ = tx
                .send(Ok(ToolStreamChunk {
                    payload: Some(tool_stream_chunk::Payload::Partial(
                        "streaming started".into(),
                    )),
                }))
                .await;

            match tools.execute_gated(&name, &args, &cap_mgr).await {
                Ok(res) => {
                    let _ = tx
                        .send(Ok(ToolStreamChunk {
                            payload: Some(tool_stream_chunk::Payload::FinalResult(
                                CallToolResponse {
                                    success: res.success,
                                    output: res.output,
                                    error: res.error.unwrap_or_default(),
                                    duration_ms: res.duration_ms,
                                },
                            )),
                        }))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(Ok(ToolStreamChunk {
                            payload: Some(tool_stream_chunk::Payload::FinalResult(
                                CallToolResponse {
                                    success: false,
                                    output: String::new(),
                                    error: format!("{}", e),
                                    duration_ms: 0,
                                },
                            )),
                        }))
                        .await;
                }
            }
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(rx)) as Self::call_tool_stream::return_type
        ))
    }
}

// ─── Client ─────────────────────────────────────────────────────
// NOTE: gRPC client stub removed in v1.0. Will be re-introduced in v1.1
// with proper tonic generated stubs and tokio-tungstenite for WebSocket.
// For now, use MCPTransport::Http for all agent-to-agent communication.
