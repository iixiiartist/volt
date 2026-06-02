use crate::models::ToolResult;
use serde_json::json;
use std::sync::Arc;

const OLLAMA_API_BASE: &str = "https://ollama.com/api";

fn get_api_key() -> Option<String> {
    std::env::var("OLLAMA_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
}

fn make_req(client: &reqwest::Client, url: &str) -> Option<reqwest::RequestBuilder> {
    let key = get_api_key()?;
    Some(
        client
            .post(url)
            .header("Authorization", format!("Bearer {}", key)),
    )
}

pub async fn register_ollama_web_tools(registry: &Arc<crate::tools::ToolRegistry>) {
    let web_search_fn: crate::tools::ToolFn = Arc::new(move |args: serde_json::Value| {
        let query = args["query"].as_str().unwrap_or("").to_string();
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);
        Box::pin(async move {
            if query.is_empty() {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("query is required".into()),
                    duration_ms: 0,
                };
            }
            let client = crate::http_client().clone();
            let req = match make_req(&client, &format!("{}/web_search", OLLAMA_API_BASE)) {
                Some(r) => r.json(&json!({"query": query, "max_results": max_results})),
                None => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("OLLAMA_API_KEY not set".into()),
                        duration_ms: 0,
                    }
                }
            };
            match req.timeout(std::time::Duration::from_secs(30)).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.json::<serde_json::Value>().await {
                        Ok(val) => ToolResult {
                            success: status.is_success(),
                            output: serde_json::to_string_pretty(&val).unwrap_or_default(),
                            error: if status.is_success() {
                                None
                            } else {
                                Some(
                                    val["error"]
                                        .as_str()
                                        .unwrap_or("web search failed")
                                        .to_string(),
                                )
                            },
                            duration_ms: 0,
                        },
                        Err(e) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("failed to parse response: {}", e)),
                            duration_ms: 0,
                        },
                    }
                }
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("request failed: {}", e)),
                    duration_ms: 0,
                },
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
    });
    registry.register_with_permission(
        "ollama_web_search",
        "Search the web using Ollama Cloud's built-in web search API. Requires OLLAMA_API_KEY.",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query" },
                "max_results": { "type": "integer", "description": "Maximum number of results (default: 5)" }
            },
            "required": ["query"]
        }),
        "builtin",
        web_search_fn,
        crate::models::PermissionLevel::Allow,
        crate::attenuation::TrustLevel::Builtin,
    ).await;

    let web_fetch_fn: crate::tools::ToolFn = Arc::new(move |args: serde_json::Value| {
        let url = args["url"].as_str().unwrap_or("").to_string();
        Box::pin(async move {
            if url.is_empty() {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("url is required".into()),
                    duration_ms: 0,
                };
            }
            let client = crate::http_client().clone();
            let req = match make_req(&client, &format!("{}/web_fetch", OLLAMA_API_BASE)) {
                Some(r) => r.json(&json!({"url": url})),
                None => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("OLLAMA_API_KEY not set".into()),
                        duration_ms: 0,
                    }
                }
            };
            match req.timeout(std::time::Duration::from_secs(30)).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.json::<serde_json::Value>().await {
                        Ok(val) => ToolResult {
                            success: status.is_success(),
                            output: serde_json::to_string_pretty(&val).unwrap_or_default(),
                            error: if status.is_success() {
                                None
                            } else {
                                Some(
                                    val["error"]
                                        .as_str()
                                        .unwrap_or("web fetch failed")
                                        .to_string(),
                                )
                            },
                            duration_ms: 0,
                        },
                        Err(e) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("failed to parse response: {}", e)),
                            duration_ms: 0,
                        },
                    }
                }
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("request failed: {}", e)),
                    duration_ms: 0,
                },
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
    });
    registry.register_with_permission(
        "ollama_web_fetch",
        "Fetch a web page's content using Ollama Cloud's built-in web fetch API. Requires OLLAMA_API_KEY.",
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch" }
            },
            "required": ["url"]
        }),
        "builtin",
        web_fetch_fn,
        crate::models::PermissionLevel::Allow,
        crate::attenuation::TrustLevel::Builtin,
    ).await;
}
