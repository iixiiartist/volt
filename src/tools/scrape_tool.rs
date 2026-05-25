use crate::models::ToolResult;
use crate::tools::web_tool::validate_url;
use scraper::{Html, Selector};
use std::time::Instant;

pub async fn web_scrape(url: &str, selector: &str) -> ToolResult {
    let started = Instant::now();

    let parsed = match validate_url(url) {
        Ok(u) => u,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("client build failed: {}", e)),
                duration_ms: 0,
            };
        }
    };

    match client.get(parsed.as_str()).send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("HTTP {}", status)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }
            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("body read failed: {}", e)),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            };

            let document = Html::parse_document(&body);
            let sel = match Selector::parse(selector) {
                Ok(s) => s,
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("invalid CSS selector '{}': {}", selector, e)),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            };

            let results: Vec<String> = document
                .select(&sel)
                .map(|el| el.text().collect::<Vec<_>>().concat())
                .collect();

            if results.is_empty() {
                return ToolResult {
                    success: true,
                    output: "".to_string(),
                    error: Some(format!("no elements matched selector '{}'", selector)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }

            let output = results.join("\n---\n");
            ToolResult {
                success: true,
                output,
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("fetch failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn web_scrape_all(url: &str) -> ToolResult {
    let started = Instant::now();

    let parsed = match validate_url(url) {
        Ok(u) => u,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("client build failed: {}", e)),
                duration_ms: 0,
            };
        }
    };

    match client.get(parsed.as_str()).send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("HTTP {}", status)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }
            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("body read failed: {}", e)),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            };

            let document = Html::parse_document(&body);

            let mut output = String::new();

            // Extract title
            if let Ok(sel) = Selector::parse("title") {
                if let Some(el) = document.select(&sel).next() {
                    let text = el.text().collect::<Vec<_>>().concat();
                    if !text.is_empty() {
                        output.push_str(&format!("Title: {}\n\n", text));
                    }
                }
            }

            // Extract all headings
            for level in 1..=6 {
                if let Ok(sel) = Selector::parse(&format!("h{}", level)) {
                    for el in document.select(&sel) {
                        let text = el.text().collect::<Vec<_>>().concat();
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            output.push_str(&format!(
                                "{} {}\n",
                                "#".repeat(level as usize),
                                trimmed
                            ));
                        }
                    }
                }
            }
            if !output.is_empty() {
                output.push('\n');
            }

            // Extract all paragraph text
            if let Ok(sel) = Selector::parse("p") {
                for el in document.select(&sel) {
                    let text = el.text().collect::<Vec<_>>().concat();
                    let trimmed = text.trim();
                    if !trimmed.is_empty() && trimmed.len() > 20 {
                        output.push_str(&format!("{}\n\n", trimmed));
                    }
                }
            }

            // Extract links
            if let Ok(sel) = Selector::parse("a[href]") {
                let mut links = Vec::new();
                for el in document.select(&sel) {
                    if let Some(href) = el.value().attr("href") {
                        let text = el.text().collect::<Vec<_>>().concat();
                        let trimmed = text.trim();
                        if !trimmed.is_empty()
                            && !href.starts_with('#')
                            && !href.starts_with("javascript:")
                        {
                            links.push(format!("- [{}]({})", trimmed, href));
                        }
                    }
                }
                if !links.is_empty() && links.len() <= 50 {
                    output.push_str("## Links\n");
                    for link in links {
                        output.push_str(&link);
                        output.push('\n');
                    }
                }
            }

            if output.is_empty() {
                // Fallback: just return all text
                output = document.root_element().text().collect::<Vec<_>>().concat();
            }

            ToolResult {
                success: true,
                output,
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("fetch failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_reused() {
        assert!(validate_url("https://example.com").is_ok());
    }

    #[test]
    fn test_css_selector_validation() {
        assert!(Selector::parse("h1").is_ok());
        assert!(Selector::parse("div.content").is_ok());
        assert!(Selector::parse("a[href]").is_ok());
        assert!(Selector::parse("div > p:first-child").is_ok());
    }
}
