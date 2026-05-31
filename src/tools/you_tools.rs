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

    // Default livecrawl to "all" so the LLM gets full page content (markdown)
    // instead of just meta-descriptions. This is the key fix for the "generic
    // weather description" problem — you.com will scrape the actual page and
    // return real data in contents.markdown.
    let livecrawl = livecrawl.unwrap_or("all");

    let mut url =
        reqwest::Url::parse_with_params("https://ydc-index.io/v1/search", &[("query", query)])
            .unwrap();
    if let Some(c) = count {
        url.query_pairs_mut().append_pair("count", &c.to_string());
    }
    if let Some(f) = freshness {
        url.query_pairs_mut().append_pair("freshness", f);
    }
    url.query_pairs_mut().append_pair("livecrawl", livecrawl);
    url.query_pairs_mut()
        .append_pair("livecrawl_formats", "markdown");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
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
                Ok(body) => {
                    // Post-process: extract snippets + full content into
                    // clean formatted text the LLM can actually read.
                    let formatted = if status.is_success() {
                        format_search_results(&body)
                    } else {
                        body
                    };
                    ToolResult {
                        success: status.is_success(),
                        output: formatted,
                        error: if status.is_success() {
                            None
                        } else {
                            Some(format!("HTTP {}", status))
                        },
                        duration_ms: started.elapsed().as_millis(),
                    }
                }
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

/// Parse the you.com Search API JSON response and format it into clean
/// LLM-readable text. Extracts snippets and full-page markdown content
/// for each result, discarding raw JSON structure.
fn format_search_results(raw_json: &str) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw_json);
    let value = match parsed {
        Ok(v) => v,
        Err(_) => return raw_json.to_string(),
    };

    let mut out = String::new();

    // Extract web results
    if let Some(web_results) = value["results"]["web"].as_array() {
        if !web_results.is_empty() {
            out.push_str("=== Web Results ===\n\n");
            for (i, result) in web_results.iter().enumerate() {
                let title = result["title"].as_str().unwrap_or("(no title)");
                let url = result["url"].as_str().unwrap_or("");
                let desc = result["description"].as_str().unwrap_or("");
                out.push_str(&format!("{}. {}\n   URL: {}\n", i + 1, title, url));

                if !desc.is_empty() {
                    out.push_str(&format!("   Description: {}\n", desc));
                }

                // Snippets — the key LLM-ready excerpts
                if let Some(snippets) = result["snippets"].as_array() {
                    for snippet in snippets {
                        if let Some(text) = snippet.as_str() {
                            if !text.is_empty() {
                                out.push_str(&format!("   > {}\n", text));
                            }
                        }
                    }
                }

                // Full page markdown content (from livecrawl)
                if let Some(contents) = result["contents"].as_object() {
                    if let Some(md) = contents.get("markdown").and_then(|m| m.as_str()) {
                        if !md.is_empty() {
                            // Truncate long content to avoid token overflow
                            let max_len = 4000;
                            let truncated = if md.len() > max_len {
                                format!(
                                    "{}...\n[truncated {} chars]",
                                    &md[..max_len],
                                    md.len() - max_len
                                )
                            } else {
                                md.to_string()
                            };
                            out.push_str(&format!("   Content:\n{}\n", truncated));
                        }
                    }
                }
                out.push('\n');
            }
        }
    }

    // Extract news results
    if let Some(news_results) = value["results"]["news"].as_array() {
        if !news_results.is_empty() {
            out.push_str("=== News Results ===\n\n");
            for (i, result) in news_results.iter().enumerate() {
                let title = result["title"].as_str().unwrap_or("(no title)");
                let url = result["url"].as_str().unwrap_or("");
                let desc = result["description"].as_str().unwrap_or("");
                out.push_str(&format!(
                    "{}. {}\n   URL: {}\n   {}\n\n",
                    i + 1,
                    title,
                    url,
                    desc
                ));
            }
        }
    }

    if out.is_empty() {
        // Fallback: return raw JSON if we couldn't extract anything
        raw_json.to_string()
    } else {
        out
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
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
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
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
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
