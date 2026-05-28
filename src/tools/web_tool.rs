use crate::models::ToolResult;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Instant;
use url::Url;

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

    // Network allowlist check
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
    const MAX_BYTES: usize = 2_000_000; // 2MB cap prevents OOM on large pages

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
                    // Truncate to prevent OOM on large responses
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

pub async fn web_scrape(url: &str) -> ToolResult {
    // web_scrape delegates to web_fetch; the ToolRegistry routes both.
    web_fetch(url).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_https_ok() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn test_validate_url_scheme_rejected() {
        let r = validate_url("file:///etc/passwd");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("scheme"));
    }

    #[test]
    fn test_validate_url_private_ip_rejected() {
        let cases = vec![
            "http://127.0.0.1:8080/admin",
            "http://10.0.0.1/admin",
            "http://192.168.1.1/admin",
            "http://172.16.0.1/admin",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost:5432",
            "http://[::1]:8080",
        ];
        for url in cases {
            let r = validate_url(url);
            assert!(r.is_err(), "expected rejection for: {}", url);
        }
    }

    #[test]
    fn test_validate_url_public_ip_ok() {
        assert!(validate_url("https://93.184.216.34").is_ok());
    }

    #[test]
    fn test_validate_url_invalid_rejected() {
        let r = validate_url("not-a-url");
        assert!(r.is_err());
    }

    #[test]
    fn test_is_private_addr_v4() {
        assert!(is_private_addr("127.0.0.1".parse().unwrap()));
        assert!(is_private_addr("10.0.0.1".parse().unwrap()));
        assert!(is_private_addr("192.168.1.1".parse().unwrap()));
        assert!(!is_private_addr("8.8.8.8".parse().unwrap()));
    }
}
