use crate::models::ToolResult;
use std::time::Instant;

fn api_key() -> String {
    std::env::var("YOUCOM_API_KEY").unwrap_or_default()
}

pub async fn web_search(
    query: &str,
    count: Option<u32>,
    freshness: Option<&str>,
    livecrawl: Option<&str>,
) -> ToolResult {
    let started = Instant::now();
    let key = api_key();
    if key.is_empty() {
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some("YOUCOM_API_KEY not set".into()),
            duration_ms: started.elapsed().as_millis(),
        };
    }

    let mut url =
        reqwest::Url::parse_with_params("https://ydc-index.io/v1/search", &[("query", query)])
            .unwrap();
    if let Some(c) = count {
        url.query_pairs_mut().append_pair("count", &c.to_string());
    }
    if let Some(f) = freshness {
        url.query_pairs_mut().append_pair("freshness", f);
    }
    if let Some(l) = livecrawl {
        url.query_pairs_mut().append_pair("livecrawl", l);
        url.query_pairs_mut()
            .append_pair("livecrawl_formats", "markdown");
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("client build failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };

    match client.get(url).header("X-API-Key", &key).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(body) => ToolResult {
                    success: status.is_success(),
                    output: body,
                    error: if status.is_success() {
                        None
                    } else {
                        Some(format!("HTTP {}", status))
                    },
                    duration_ms: started.elapsed().as_millis(),
                },
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("body read failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("search request failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn you_research(input: &str, research_effort: Option<&str>) -> ToolResult {
    let started = Instant::now();
    let key = api_key();
    if key.is_empty() {
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some("YOUCOM_API_KEY not set".into()),
            duration_ms: started.elapsed().as_millis(),
        };
    }

    let mut body = serde_json::json!({
        "input": input,
    });
    if let Some(effort) = research_effort {
        body["research_effort"] = serde_json::json!(effort);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("client build failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };

    match client
        .post("https://api.you.com/v1/research")
        .header("X-API-Key", &key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(text) => ToolResult {
                    success: status.is_success(),
                    output: text,
                    error: if status.is_success() {
                        None
                    } else {
                        Some(format!("HTTP {}", status))
                    },
                    duration_ms: started.elapsed().as_millis(),
                },
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("body read failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("research request failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn you_contents(urls: &[String], content_format: Option<&str>) -> ToolResult {
    let started = Instant::now();
    for url in urls {
        if let Err(e) = crate::tools::web_tool::validate_url(url) {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("URL validation failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    }
    let key = api_key();
    if key.is_empty() {
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some("YOUCOM_API_KEY not set".into()),
            duration_ms: started.elapsed().as_millis(),
        };
    }

    let formats: Vec<&str> = if let Some(f) = content_format {
        vec![f]
    } else {
        vec!["markdown"]
    };

    let body = serde_json::json!({
        "urls": urls,
        "formats": formats,
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("client build failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };

    match client
        .post("https://ydc-index.io/v1/contents")
        .header("X-API-Key", &key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(text) => ToolResult {
                    success: status.is_success(),
                    output: text,
                    error: if status.is_success() {
                        None
                    } else {
                        Some(format!("HTTP {}", status))
                    },
                    duration_ms: started.elapsed().as_millis(),
                },
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("body read failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("contents request failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
