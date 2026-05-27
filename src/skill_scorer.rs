use regex::Regex;

#[derive(Debug, Clone, Default)]
pub struct SkillActivation {
    pub keywords: Vec<String>,
    pub patterns: Vec<String>, // stored as strings, compiled to Regex on demand
    pub tags: Vec<String>,
    pub exclude_keywords: Vec<String>,
    pub max_context_tokens: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub content: String,
    pub mcp_servers: Vec<String>,
    pub source_path: Option<String>,
    pub activation: SkillActivation,
}

pub struct SkillScorer;

impl SkillScorer {
    /// Deterministic fast score: keyword + regex + tag match. No LLM required.
    pub fn score(message: &str, skill: &SkillManifest) -> u32 {
        let msg = message.to_lowercase();

        // Gating: required binaries / env vars would be checked here (future)
        // Exclude check
        for ex in &skill.activation.exclude_keywords {
            if msg.contains(&ex.to_lowercase()) {
                return 0;
            }
        }

        let mut score = 0u32;

        // Keyword match
        for kw in &skill.activation.keywords {
            let kw_lc = kw.to_lowercase();
            if msg.contains(&format!(" {} ", kw_lc)) || msg.starts_with(&format!("{}", kw_lc)) {
                score += 10;
            } else if msg.contains(&kw_lc) {
                score += 5;
            }
        }

        // Regex match
        for pat in &skill.activation.patterns {
            if let Ok(re) = Regex::new(pat) {
                if re.is_match(&msg) {
                    score += 50;
                }
            }
        }

        // Tag match (overlap)
        for tag in &skill.activation.tags {
            if msg.contains(&tag.to_lowercase()) {
                score += 2;
            }
        }

        // Description overlap (lightweight)
        let desc_lc = skill.description.to_lowercase();
        for word in desc_lc.split_whitespace() {
            if word.len() > 4 && msg.contains(word) {
                score += 1;
            }
        }

        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str, activation: SkillActivation) -> SkillManifest {
        SkillManifest {
            name: name.into(),
            version: "1.0.0".into(),
            description: "test".into(),
            content: "".into(),
            mcp_servers: vec![],
            source_path: None,
            activation,
        }
    }

    #[test]
    fn test_keyword_score() {
        let skill = make_skill(
            "test",
            SkillActivation {
                keywords: vec!["deploy".into(), "k8s".into()],
                ..Default::default()
            },
        );
        assert!(SkillScorer::score("deploy to k8s", &skill) >= 15);
        assert!(SkillScorer::score("hello world", &skill) == 0);
    }

    #[test]
    fn test_pattern_score() {
        let skill = make_skill(
            "test",
            SkillActivation {
                patterns: vec![r"\bdeploy\b.*\bprod\b".into()],
                ..Default::default()
            },
        );
        assert!(SkillScorer::score("deploy to prod", &skill) >= 50);
    }

    #[test]
    fn test_exclusion_zero() {
        let skill = make_skill(
            "test",
            SkillActivation {
                keywords: vec!["deploy".into()],
                exclude_keywords: vec!["rollback".into()],
                ..Default::default()
            },
        );
        assert_eq!(SkillScorer::score("deploy to prod", &skill), 10);
        assert_eq!(SkillScorer::score("deploy rollback plan", &skill), 0);
    }
}
