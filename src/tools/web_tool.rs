use crate::models::ToolResult;
use scraper::{Html, Selector};
use std::net::{IpAddr, Ipv4Addr};
use std::time::Instant;
use url::Url;

const MIN_PARAGRAPH_CHARS: usize = 20;

fn is_private_addr(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || matches!(v4.octets(), [10, _, _, _])
                || matches!(v4.octets(), [172, 16..=31, _, _])
                || matches!(v4.octets(), [192, 168, _, _])
                || matches!(v4.octets(), [169, 254, _, _])
                || matches!(v4.octets(), [100, 64..=127, _, _])
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || matches!(v6.segments(), [0xfd, _, _, _, _, _, _, _])
        }
    }
}

fn is_ssrf_risk(host: &str) -> bool {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_addr(ip);
    }
    if let Ok(addr) = host.parse::<Ipv4Addr>() {
        return is_private_addr(IpAddr::V4(addr));
    }
    if host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("127.0.0.1")
        || host.eq_ignore_ascii_case("0.0.0.0")
        || host.eq_ignore_ascii_case("[::1]")
        || host.eq_ignore_ascii_case("::1")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }
    false
}

pub fn validate_url(url_str: &str) -> Result<Url, String> {
    let parsed = Url::parse(url_str).map_err(|e| format!("invalid URL: {}", e))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(format!(
                "disallowed URL scheme '{}'; only http/https allowed",
                scheme
            ))
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL missing host".to_string())?;

    let allowed_hosts: Option<Vec<String>> = std::env::var("VOLT_ALLOWED_HOSTS").ok().map(|s| {
        s.split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect()
    });
    if let Some(ref hosts) = allowed_hosts {
        if let Err(e) = crate::network_policy::NetworkPolicy::check(url_str, Some(hosts.as_slice()))
        {
            return Err(format!("network policy: {}", e));
        }
    }

    if let Some(port) = parsed.port() {
        if port == 25 || port == 137 || port == 138 || port == 139 || port == 445 {
            return Err(format!("disallowed port {}", port));
        }
    }

    if is_ssrf_risk(host) {
        return Err(format!("URL targets internal/private address: {}", host));
    }

    Ok(parsed)
}

pub async fn web_fetch(url: &str) -> ToolResult {
    let started = Instant::now();
    const MAX_BYTES: usize = 2_000_000;

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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
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
                duration_ms: 0,
            };
        }
    };
    match client.get(parsed.as_str()).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(body) => {
                    let truncated: String = body.chars().take(MAX_BYTES).collect();
                    let truncated_notice = if body.len() > MAX_BYTES {
                        format!(" [truncated from {} bytes]", body.len())
                    } else {
                        String::new()
                    };
                    ToolResult {
                        success: status.is_success(),
                        output: format!("{}{}", truncated, truncated_notice),
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
            error: Some(format!("fetch failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn web_fetch_selector(url: &str, selector: &str) -> ToolResult {
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

    let body = match fetch_body(parsed.as_str()).await {
        Ok(b) => b,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };

    let fragment = Html::parse_fragment(&body);
    let css_sel = match Selector::parse(selector) {
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

    let mut results = Vec::new();
    for element in fragment.select(&css_sel) {
        let text: String = element.text().collect::<Vec<_>>().join(" ");
        results.push(text);
    }

    if results.is_empty() {
        return ToolResult {
            success: true,
            output: format!("No elements matched selector '{}'", selector),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        };
    }

    ToolResult {
        success: true,
        output: results.join("\n---\n"),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}

pub async fn web_fetch_all(url: &str) -> ToolResult {
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

    let body = match fetch_body(parsed.as_str()).await {
        Ok(b) => b,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };

    let document = Html::parse_document(&body);
    let mut parts = Vec::new();

    for heading in ["h1", "h2", "h3", "h4", "h5", "h6"].iter() {
        if let Ok(sel) = Selector::parse(heading) {
            for element in document.select(&sel) {
                let text: String = element.text().collect::<Vec<_>>().join(" ");
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("## {}", trimmed));
                }
            }
        }
    }

    if let Ok(p_sel) = Selector::parse("p") {
        for element in document.select(&p_sel) {
            let text: String = element.text().collect::<Vec<_>>().join(" ");
            let trimmed = text.trim();
            if trimmed.len() >= MIN_PARAGRAPH_CHARS {
                parts.push(trimmed.to_string());
            }
        }
    }

    if let Ok(a_sel) = Selector::parse("a[href]") {
        for element in document.select(&a_sel) {
            if let Some(href) = element.value().attr("href") {
                let text: String = element.text().collect::<Vec<_>>().join(" ");
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("[{}]({})", trimmed, href));
                }
            }
        }
    }

    if parts.is_empty() {
        parts.push("No structured content found on the page.".to_string());
    }

    ToolResult {
        success: true,
        output: parts.join("\n\n"),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}

async fn fetch_body(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("client build failed: {}", e))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("fetch failed: {}", e))?;

    resp.text()
        .await
        .map_err(|e| format!("body read failed: {}", e))
}
