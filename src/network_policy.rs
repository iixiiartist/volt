use regex::Regex;

pub struct NetworkPolicy;

pub enum PolicyError {
    BlockedHost(String),
    InvalidUrl,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::BlockedHost(h) => write!(f, "host '{}' is not in allowlist", h),
            PolicyError::InvalidUrl => write!(f, "invalid URL"),
        }
    }
}

impl NetworkPolicy {
    /// Check if a URL is allowed against an optional host allowlist.
    /// If allowed_hosts is None or empty, all public hosts are allowed.
    pub fn check(url: &str, allowed_hosts: Option<&[String]>) -> Result<(), PolicyError> {
        let allowed = match allowed_hosts {
            Some(a) if !a.is_empty() => a,
            _ => return Ok(()),
        };

        let host = extract_host(url).ok_or(PolicyError::InvalidUrl)?;
        let host_lc = host.to_lowercase();

        for pattern in allowed {
            let p = pattern.to_lowercase();
            if p == host_lc
                || (p.starts_with("*.") && host_lc.ends_with(&p[2..]))
                || (p.starts_with(" *. ") && host_lc.ends_with(&p[3..]))
            {
                return Ok(());
            }
        }
        Err(PolicyError::BlockedHost(host))
    }
}

fn extract_host(url: &str) -> Option<String> {
    let rest = url
        .find("://")
        .map(|i| &url[i + 3..])
        .unwrap_or(url);
    let end = rest
        .find('/')
        .or_else(|| rest.find('?'))
        .or_else(|| rest.find('#'))
        .unwrap_or(rest.len());
    let host_port = &rest[..end];
    Some(
        host_port
            .rfind(':')
            .map(|i| &host_port[..i])
            .unwrap_or(host_port)
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_allowlist() {
        assert!(NetworkPolicy::check("https://example.com/test", None).is_ok());
    }

    #[test]
    fn test_exact_match() {
        let hosts = ["example.com".to_string(), "docs.rs".to_string()];
        assert!(NetworkPolicy::check("https://example.com/a", Some(&hosts)).is_ok());
        assert!(NetworkPolicy::check("https://docs.rs/a", Some(&hosts)).is_ok());
    }

    #[test]
    fn test_wildcard() {
        let hosts = ["*.example.com".to_string()];
        assert!(NetworkPolicy::check("https://api.example.com/a", Some(&hosts)).is_ok());
        assert!(NetworkPolicy::check("https://evil.com/a", Some(&hosts)).is_err());
    }

    #[test]
    fn test_blocked() {
        let hosts = ["good.com".to_string()];
        assert!(NetworkPolicy::check("https://evil.com/a", Some(&hosts)).is_err());
    }
}
