use crate::mcp::MCPServer;

pub async fn serve_stdio() -> anyhow::Result<()> {
    let tools = crate::tools::register_all_tools().await;
    let server = MCPServer::new(tools).await;
    server.serve_stdio().await?;
    Ok(())
}
