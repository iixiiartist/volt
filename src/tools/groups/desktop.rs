// Desktop tools group (requires `tools-desktop` feature)
use crate::tools::registry::ToolRegistry;
use std::sync::Arc;

pub async fn register_desktop_tools(_registry: &Arc<ToolRegistry>) {
    #[cfg(feature = "tools-desktop")]
    {
        use crate::attenuation::TrustLevel;
        use crate::models::PermissionLevel;
        registry.register_with_permission("desktop_click","Click at screen coordinates.",
            serde_json::json!({"type":"object","properties":{"x":{"type":"integer"},"y":{"type":"integer"}},"required":["x","y"]}),"builtin",
            Arc::new(|args| Box::pin(async move {
                let x = args["x"].as_i64().unwrap_or(0) as i32; let y = args["y"].as_i64().unwrap_or(0) as i32;
                crate::tools::desktop_tool::desktop_click(x, y).await
            })), PermissionLevel::Prompt, TrustLevel::Builtin).await;

        registry.register_with_permission("desktop_type","Type text at cursor position.",
            serde_json::json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),"builtin",
            Arc::new(|args| Box::pin(async move {
                let t = args["text"].as_str().unwrap_or("");
                crate::tools::desktop_tool::desktop_type(t).await
            })), PermissionLevel::Prompt, TrustLevel::Builtin).await;

        registry.register_with_permission("desktop_key","Press a key (enter, tab, escape, up, down, etc.).",
            serde_json::json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}),"builtin",
            Arc::new(|args| Box::pin(async move {
                let k = args["key"].as_str().unwrap_or("");
                crate::tools::desktop_tool::desktop_key(k).await
            })), PermissionLevel::Prompt, TrustLevel::Builtin).await;

        registry.register("desktop_find_window","Find a window by title using Windows API.",
            serde_json::json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}),"builtin",
            Arc::new(|args| Box::pin(async move {
                let t = args["title"].as_str().unwrap_or("");
                crate::tools::desktop_tool::desktop_find_window(t).await
            }))).await;
    }
}
