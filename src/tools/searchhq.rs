use crate::mcp::client::MCPClient;
use crate::models::*;
use crate::tools::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;

const SEARCHHQ_URL: &str = "https://searchhq.setique.com/.netlify/functions/mcp-server";

/// Register all SearchHQ MCP tools into Volt's ToolRegistry with embeddings.
/// Returns the count of tools registered.
pub async fn register_searchhq_tools(
    registry: &ToolRegistry,
    api_token: &str,
) -> anyhow::Result<usize> {
    let transport = MCPTransport::Http {
        url: SEARCHHQ_URL.into(),
        headers: None,
    };

    let client = Arc::new(MCPClient::new(transport));
    client.set_token(api_token);

    // Discover full tool definitions from the MCP server
    let tool_defs = client.list_tools_full().await?;

    for tool in &tool_defs {
        let name = tool["name"].as_str().unwrap_or("").to_string();
        let description = tool["description"].as_str().unwrap_or("").to_string();
        let input_schema = tool["inputSchema"].clone();

        if name.is_empty() {
            continue;
        }

        let exec_client = client.clone();
        let exec_name = name.clone();

        registry
            .register(
                &format!("searchhq_{}", name),
                &format!("[SearchHQ] {} — {}", name, description),
                if input_schema.is_object() {
                    input_schema
                } else {
                    serde_json::json!({"type": "object", "properties": {}})
                },
                "searchhq-mcp",
                Arc::new(move |args: Value| {
                    let client = exec_client.clone();
                    let tool_name = exec_name.clone();
                    Box::pin(async move {
                        let started = std::time::Instant::now();
                        match client.call_tool(&tool_name, &args).await {
                            Ok(result) => ToolResult {
                                success: true,
                                output: serde_json::to_string_pretty(&result["result"])
                                    .unwrap_or_default(),
                                error: None,
                                duration_ms: started.elapsed().as_millis(),
                            },
                            Err(e) => ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!("SearchHQ {} failed: {}", tool_name, e)),
                                duration_ms: started.elapsed().as_millis(),
                            },
                        }
                    })
                }),
            )
            .await;
    }

    Ok(tool_defs.len())
}
