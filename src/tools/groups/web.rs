// Web tools (web_fetch, web_scrape, web_search, you.com tools)
use crate::attenuation::TrustLevel;
use crate::models::PermissionLevel;
use crate::tools::registry::ToolRegistry;
use std::sync::Arc;

pub async fn register_web_tools(registry: &Arc<ToolRegistry>) {
    let bfcl_mode = std::env::var("VOLT_BFCL_MODE").is_ok();

    registry
        .register_with_permission(
            "web_fetch",
            "Fetch a URL and return its content",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    crate::tools::web_tool::web_fetch(url).await
                })
            }),
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;

    registry
        .register_with_permission(
            "web_scrape",
            "Extract structured content from a URL using a CSS selector. Returns text content of all matching elements.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to scrape" },
                    "selector": { "type": "string", "description": "CSS selector to match elements" }
                },
                "required": ["url", "selector"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    let selector = args["selector"].as_str().unwrap_or("");
                    crate::tools::scrape_tool::web_scrape(url, selector).await
                })
            }),
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;

    registry
        .register_with_permission(
            "web_scrape_all",
            "Fetch a URL and extract all human-readable content (headings, paragraphs, links). General-purpose page reading without needing a CSS selector.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch and extract" }
                },
                "required": ["url"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    crate::tools::scrape_tool::web_scrape_all(url).await
                })
            }),
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;

    if !bfcl_mode {
        registry
            .register(
                "web_search",
                "Search the web for real-time information using you.com Search API. Returns structured results with URLs, titles, snippets, and optional full-page content via livecrawl. Use this when you need current information from the internet.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "The search query" },
                        "count": { "type": "integer", "description": "Number of results (1-100, default 10)", "default": 10 },
                        "freshness": { "type": "string", "description": "Recency filter: day, week, month, year, or YYYY-MM-DDtoYYYY-MM-DD", "enum": ["day", "week", "month", "year"], "default": null },
                        "livecrawl": { "type": "string", "description": "Fetch full page content: web, news, or all", "enum": ["web", "news", "all"], "default": null }
                    },
                    "required": ["query"]
                }),
                "builtin",
                Arc::new(|args| {
                    Box::pin(async move {
                        let query = args["query"].as_str().unwrap_or("");
                        let count = args["count"].as_u64().map(|c| c as u32);
                        let freshness = args["freshness"].as_str();
                        let livecrawl = args["livecrawl"].as_str();
                        crate::tools::you_tools::web_search(query, count, freshness, livecrawl).await
                    })
                }),
            )
            .await;

        registry
            .register(
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
                Arc::new(|args| {
                    Box::pin(async move {
                        let input = args["input"].as_str().unwrap_or("");
                        let effort = args["research_effort"].as_str();
                        crate::tools::you_tools::you_research(input, effort).await
                    })
                }),
            )
            .await;

        registry
            .register(
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
                Arc::new(|args| {
                    Box::pin(async move {
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
                    })
                }),
            )
            .await;
    }
}
