use crate::attenuation::TrustLevel;
use crate::models::PermissionLevel;
use crate::register_tool;
use crate::register_tool_with_permission;
use crate::tools::registry::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;

pub async fn register_web_tools(registry: &Arc<ToolRegistry>) {
    let bfcl_mode = std::env::var("VOLT_BFCL_MODE").is_ok();

    register_tool_with_permission!(
        registry,
        "web_fetch",
        "Fetch a URL and return its content. Optionally pass a CSS selector to extract specific elements. Use ONLY when the user asks for information from a specific URL or when web_search results point to a page you need to read.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" },
                "selector": { "type": "string", "description": "Optional CSS selector to extract specific elements (e.g. 'h1', '.content', '#main'). If omitted, returns full page content." }
            },
            "required": ["url"]
        }),
        "builtin",
        |args: Value| async move {
            let url = args["url"].as_str().unwrap_or("");
            let selector = args["selector"].as_str();
            match selector {
                Some(s) if !s.is_empty() => crate::tools::web_tool::web_fetch_selector(url, s).await,
                _ => crate::tools::web_tool::web_fetch(url).await,
            }
        },
        PermissionLevel::Prompt,
        TrustLevel::Builtin
    );

    if !bfcl_mode {
        register_tool!(
            registry,
            "web_search",
            "Search the web for real-time information using you.com Search API. Use ONLY when the user asks for current events, real-time data, or facts that may have changed after your training cutoff. Do NOT use for timeless facts, math, code explanations, or general knowledge you already know.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" },
                    "count": { "type": "integer", "description": "Number of results (1-100, default 10)", "default": 10 },
                    "freshness": { "type": "string", "description": "Recency filter: day, week, month, year, or YYYY-MM-DDtoYYYY-MM-DD", "enum": ["day", "week", "month", "year"], "default": null },
                    "livecrawl": { "type": "string", "description": "Fetch full page content: web, news, or all (default: all)", "enum": ["web", "news", "all"], "default": "all" }
                },
                "required": ["query"]
            }),
            "builtin",
            |args: Value| async move {
                let query = args["query"].as_str().unwrap_or("");
                let count = args["count"].as_u64().map(|c| c as u32);
                let freshness = args["freshness"].as_str();
                let livecrawl = args["livecrawl"].as_str();
                crate::tools::you_tools::web_search(query, count, freshness, livecrawl).await
            }
        );

        register_tool!(
            registry,
            "you_research",
            "Deep research via you.com Research API. Runs multiple searches, reads sources, and synthesizes a thorough, well-cited answer. Use for complex questions requiring multi-step research.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "The research question or topic" },
                    "research_effort": { "type": "string", "description": "Research depth: lite, standard, deep, or exhaustive", "enum": ["lite", "standard", "deep", "exhaustive"], "default": "standard" }
                },
                "required": ["input"]
            }),
            "builtin",
            |args: Value| async move {
                let input = args["input"].as_str().unwrap_or("");
                let effort = args["research_effort"].as_str();
                crate::tools::you_tools::you_research(input, effort).await
            }
        );

        register_tool!(
            registry,
            "you_contents",
            "Fetch clean Markdown or HTML content from specific URLs using you.com Contents API. Takes a list of URLs and returns structured page content. Use when you already have URLs and need their full text content.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "urls": { "type": "array", "items": { "type": "string" }, "description": "List of URLs to fetch content from" },
                    "format": { "type": "string", "description": "Content format: markdown or html", "enum": ["markdown", "html"], "default": "markdown" }
                },
                "required": ["urls"]
            }),
            "builtin",
            |args: Value| async move {
                let urls: Vec<String> = args["urls"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let fmt = args["format"].as_str();
                crate::tools::you_tools::you_contents(&urls, fmt).await
            }
        );
    }
}
