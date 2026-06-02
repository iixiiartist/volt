// Browser tools group (requires `tools-browser` feature)
use crate::tools::registry::ToolRegistry;
use std::sync::Arc;

#[allow(unused_variables)]
pub async fn register_browser_tools(registry: &Arc<ToolRegistry>) {
    #[cfg(feature = "tools-browser")]
    {
        use crate::attenuation::TrustLevel;
        use crate::models::PermissionLevel;
        registry.register_with_permission("browser_navigate","Open a URL in headless Chrome and return the URL.",
            serde_json::json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),"builtin",
            Arc::new(|args| Box::pin(async move {
                let u = args["url"].as_str().unwrap_or("");
                crate::tools::browser_tool::browser_navigate(u).await
            })), PermissionLevel::Prompt, TrustLevel::Builtin).await;

        registry.register_with_permission("browser_extract","Open a URL and extract text via CSS selector.",
            serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"selector":{"type":"string"}},"required":["url","selector"]}),"builtin",
            Arc::new(|args| Box::pin(async move {
                let u = args["url"].as_str().unwrap_or(""); let s = args["selector"].as_str().unwrap_or("");
                crate::tools::browser_tool::browser_extract(u, s).await
            })), PermissionLevel::Prompt, TrustLevel::Builtin).await;

        registry.register_with_permission("browser_screenshot","Open a URL and save a page screenshot.",
            serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"output_path":{"type":"string"}},"required":["url","output_path"]}),"builtin",
            Arc::new(|args| Box::pin(async move {
                let u = args["url"].as_str().unwrap_or(""); let o = args["output_path"].as_str().unwrap_or("screenshot.png");
                crate::tools::browser_tool::browser_screenshot(u, o).await
            })), PermissionLevel::Prompt, TrustLevel::Builtin).await;
    }
}
