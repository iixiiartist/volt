use regex::Regex;

pub struct LeakDetector {
    patterns: Vec<(String, Regex)>,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub redacted_text: String,
    pub found: Vec<Match>,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub category: String,
    pub start: usize,
    pub end: usize,
    pub snippet: String,
}

impl LeakDetector {
    pub fn new() -> Self {
        let patterns = vec![
            (
                "openai_key".to_string(),
                Regex::new(r"\bsk-[a-zA-Z0-9]{20,}\b").unwrap(),
            ),
            (
                "aws_key".to_string(),
                Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
            ),
            (
                "github_token".to_string(),
                Regex::new(r"\bghp_[a-zA-Z0-9]{36}\b").unwrap(),
            ),
            (
                "generic_key".to_string(),
                Regex::new(r"\bak-[a-zA-Z0-9]{16,}\b").unwrap(),
            ),
            (
                "session_token".to_string(),
                Regex::new(r"\bsess-[a-zA-Z0-9]{24}\b").unwrap(),
            ),
            (
                "private_key".to_string(),
                Regex::new(r"-----BEGIN (RSA|EC|DSA|OPENSSH) PRIVATE KEY-----").unwrap(),
            ),
            (
                "postgres_url".to_string(),
                Regex::new(r"postgres://[^:]+:[^@]+@").unwrap(),
            ),
            (
                "jwt".to_string(),
                Regex::new(r"\beyJ[a-zA-Z0-9_-]*\.eyJ[a-zA-Z0-9_-]*\.[a-zA-Z0-9_-]*\b").unwrap(),
            ),
            (
                "generic_auth_url".to_string(),
                Regex::new(r"\b[A-Za-z0-9_]{20,}:[A-Za-z0-9_]{20,}@[a-z0-9.-]+\b").unwrap(),
            ),
        ];
        Self { patterns }
    }

    pub fn scan(&self, text: &str) -> ScanResult {
        let mut redacted = text.to_string();
        let mut found = Vec::new();
        let mut replacements: Vec<(usize, usize, String)> = Vec::new();

        for (category, re) in &self.patterns {
            for m in re.find_iter(text) {
                let start = m.start();
                let end = m.end();
                let snippet = m.as_str().chars().take(8).collect::<String>() + "...";
                found.push(Match {
                    category: category.clone(),
                    start,
                    end,
                    snippet,
                });
                replacements.push((start, end, format!("[REDACTED:{}]", category)));
            }
        }

        // Sort by start descending so we can replace without invalidating offsets
        replacements.sort_by(|a, b| b.0.cmp(&a.0));
        for (start, end, replacement) in replacements {
            redacted.replace_range(start..end, &replacement);
        }

        ScanResult {
            redacted_text: redacted,
            found,
        }
    }
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_key_redaction() {
        let ld = LeakDetector::new();
        let text = "My key is sk-abc123def456ghi789jkl012mno345pqr678stu";
        let result = ld.scan(text);
        assert!(!result.found.is_empty());
        assert!(result.redacted_text.contains("[REDACTED:openai_key]"));
        assert!(!result.redacted_text.contains("sk-abc123"));
    }

    #[test]
    fn test_no_false_positives() {
        let ld = LeakDetector::new();
        let text = "The quick brown fox jumps over the lazy dog. No secrets here.";
        let result = ld.scan(text);
        assert!(result.found.is_empty());
        assert_eq!(result.redacted_text, text);
    }

    #[test]
    fn test_jwt_redaction() {
        let ld = LeakDetector::new();
        let text = "token: eyJhbGciOiJIUzI1NiIs.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMe";
        let result = ld.scan(text);
        assert!(!result.found.is_empty());
        assert!(result.redacted_text.contains("[REDACTED:jwt]"));
    }
}
