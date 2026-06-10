use super::blueprint::{load_blueprint, AgentBlueprint};
use std::path::PathBuf;

/// Filter blueprints to only those whose provider is in the active set.
pub fn filter_blueprints<'a>(
    blueprints: &'a [AgentBlueprint],
    active_providers: &[String],
) -> Vec<&'a AgentBlueprint> {
    blueprints
        .iter()
        .filter(|bp| active_providers.contains(&bp.model_card.provider))
        .collect()
}

/// Route a user prompt to the best-fit AgentBlueprint using keyword matching.
///
/// Scores each blueprint by counting how many of its keywords appear in
/// the user prompt (case-insensitive). Ties are broken by the first match.
/// Returns None if no blueprints are provided.
pub async fn route_task(
    user_prompt: &str,
    blueprints: &[AgentBlueprint],
) -> Option<AgentBlueprint> {
    let prompt_lower = user_prompt.to_lowercase();

    let scored: Vec<(&AgentBlueprint, usize)> = blueprints
        .iter()
        .map(|bp| {
            let score = bp
                .keywords
                .iter()
                .filter(|k| prompt_lower.contains(&k.to_lowercase()))
                .count();
            (bp, score)
        })
        .collect();

    let best = scored.into_iter().max_by_key(|(_, score)| *score)?;
    if best.1 > 0 {
        Some(best.0.clone())
    } else {
        // No keyword matched — fall back to first blueprint with matching name/desc
        let prompt_words: Vec<&str> = prompt_lower.split_whitespace().collect();
        blueprints
            .iter()
            .max_by_key(|bp| {
                let name_lower = bp.name.to_lowercase();
                let desc_lower = bp.description.to_lowercase();
                prompt_words
                    .iter()
                    .filter(|w| name_lower.contains(*w) || desc_lower.contains(*w))
                    .count()
            })
            .cloned()
    }
}

/// Discover blueprint TOML files from standard directories.
pub fn discover_blueprints() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let dirs = [
        std::env::current_dir().ok().map(|d| d.join("blueprints")),
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".volt").join("blueprints")),
    ];

    for dir in dirs.iter().flatten() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "toml").unwrap_or(false) {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

/// Load all blueprints from the standard directories.
pub fn load_all_blueprints() -> Vec<AgentBlueprint> {
    discover_blueprints()
        .iter()
        .filter_map(|p| load_blueprint(p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::blueprint::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn make_bp(id: &str, provider: &str, keywords: Vec<&str>) -> AgentBlueprint {
        AgentBlueprint {
            id: id.to_string(),
            name: format!("Test {}", id),
            description: format!("A test blueprint for {}", provider),
            model_card: ModelCard {
                model_name: format!("{}/test-model", provider),
                provider: provider.to_string(),
                format_dialect: FormatDialect::ChatMlTools,
                quirks: vec![],
            },
            scaffolding: ScaffoldingConfig {
                max_tools_per_turn: Some(3),
                strict_mode: false,
            },
            tools: ToolSelection {
                core_tools: vec!["bash".into()],
                builtin_tools: vec![],
            },
            prompts: PromptOverrides {
                system_prompt_override: None,
            },
            keywords: keywords.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_filter_blueprints_by_providers() {
        let groq_bp = make_bp("groq_test", "groq", vec![]);
        let nvidia_bp = make_bp("nvidia_test", "nvidia", vec![]);
        let openai_bp = make_bp("openai_test", "openai", vec![]);
        let blueprints = vec![groq_bp, nvidia_bp, openai_bp];

        let active = vec!["groq".to_string()];
        let filtered = filter_blueprints(&blueprints, &active);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "groq_test");
    }

    #[test]
    fn test_filter_blueprints_empty_active_falls_through() {
        let bp = make_bp("test", "groq", vec![]);
        let blueprints = vec![bp];
        let filtered = filter_blueprints(&blueprints, &[]);
        assert_eq!(filtered.len(), 0);
    }

    #[tokio::test]
    async fn test_route_task_keyword_match() {
        let coding_bp = make_bp(
            "coder",
            "groq",
            vec!["code", "programming", "rust", "python"],
        );
        let writing_bp = make_bp("writer", "nvidia", vec!["write", "essay", "documentation"]);
        let blueprints = vec![coding_bp, writing_bp];

        let result = route_task("write some rust code", &blueprints).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "coder");
    }

    #[tokio::test]
    async fn test_route_task_fallback_to_name_desc() {
        let bp = make_bp("default", "groq", vec![]);
        let blueprints = vec![bp];

        let result = route_task("something random", &blueprints).await;
        assert!(result.is_some());
    }
}
