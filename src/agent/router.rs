use super::blueprint::{AgentBlueprint, load_blueprint};
use crate::llm::LLMProvider;
use crate::models::{LLMMessage, LLMRequest};
use std::path::PathBuf;
use std::sync::Arc;

/// Detect which model providers have active API keys or local availability.
/// Filters out providers whose credentials are not present, to avoid
/// selecting a blueprint that would fail at execution time.
pub fn get_active_providers() -> Vec<String> {
    let mut providers = Vec::new();

    if std::env::var("GROQ_API_KEY").is_ok() {
        providers.push("groq".to_string());
    }
    if std::env::var("NVIDIA_API_KEY").is_ok() {
        providers.push("nvidia".to_string());
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        providers.push("openai".to_string());
    }
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        providers.push("anthropic".to_string());
    }

    // Ollama: local (OLLAMA_HOST) or cloud (OLLAMA_API_KEY)
    if std::env::var("OLLAMA_HOST").is_ok() || std::env::var("OLLAMA_API_KEY").is_ok() {
        providers.push("ollama".to_string());
    }
    if std::env::var("LLAMA_CPP_HOST").is_ok() {
        providers.push("llamacpp".to_string());
    }
    if std::env::var("LITERTLM_HOST").is_ok() {
        providers.push("litertlm".to_string());
    }

    // If no remote provider keys are present, add local fallbacks automatically
    let has_remote = providers.iter().any(|p| matches!(p.as_str(), "groq" | "nvidia" | "openai" | "anthropic"));
    if !has_remote {
        providers.push("ollama".to_string());
        providers.push("llamacpp".to_string());
        providers.push("litertlm".to_string());
    }

    providers
}

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

/// Route a user prompt to the best-fit AgentBlueprint using the LLM.
///
/// Filters the blueprint list to match the user's active provider credentials,
/// then asks the LLM to select the best match.
pub async fn route_task(
    user_prompt: &str,
    blueprints: &[AgentBlueprint],
    llm_client: &dyn LLMProvider,
) -> Option<AgentBlueprint> {
    let active = get_active_providers();
    let filtered = filter_blueprints(blueprints, &active);

    if filtered.is_empty() {
        tracing::warn!(
            "[router] no blueprints match active providers ({:?}); falling back to full list",
            active
        );
        // Fall through with full list rather than returning None
    }

    let candidates: Vec<&AgentBlueprint> = if filtered.is_empty() {
        blueprints.iter().collect()
    } else {
        filtered
    };

    let bp_list: String = candidates
        .iter()
        .map(|bp| format!("  - id: \"{}\"\n    name: \"{}\"\n    description: \"{}\"", bp.id, bp.name, bp.description))
        .collect::<Vec<_>>()
        .join("\n");

    let sys_prompt = format!(
        r#"You are the Volt Routing Orchestrator. The following blueprints have been filtered to match the user's currently active API keys and local hardware. Select the single best-fitting blueprint for the task.

Available blueprints:
{}

Respond with ONLY a JSON object: {{"blueprint_id": "<id>"}}

Do NOT include any other text, explanation, or markdown."#,
        bp_list
    );

    let messages = vec![
        LLMMessage {
            role: "system".into(),
            content: Arc::new(sys_prompt),
            tool_calls: None,
            tool_call_id: None,
        },
        LLMMessage {
            role: "user".into(),
            content: Arc::new(user_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let request = LLMRequest {
        model: "llama-3.1-8b-instant".into(),
        messages,
        temperature: Some(0.1),
        max_tokens: Some(128),
        stop: None,
        tools: None,
        stream: false,
        ..Default::default()
    };

    let response = match llm_client.complete(&request).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("[router] LLM call failed: {}", e);
            return None;
        }
    };

    let text = response.content.trim();

    // Extract JSON from the response (handle possible markdown fences or preamble)
    let json_str = if let Some(start) = text.find('{') {
        if let Some(end) = text[start..].rfind('}') {
            &text[start..=start + end]
        } else {
            text
        }
    } else {
        text
    };

    let parsed: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[router] failed to parse LLM response as JSON: {} — raw: {}", e, text);
            return None;
        }
    };

    let bp_id = match parsed.get("blueprint_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            tracing::warn!("[router] LLM response missing 'blueprint_id' field: {}", text);
            return None;
        }
    };

    // Match against the candidate blueprints (filtered list)
    candidates.into_iter().find(|bp| bp.id == bp_id).cloned()
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

    /// Serialize env-var-dependent tests to avoid race conditions from parallel execution.
    static ENV_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn reset_env() {
        unsafe {
            std::env::remove_var("GROQ_API_KEY");
            std::env::remove_var("NVIDIA_API_KEY");
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("OLLAMA_HOST");
            std::env::remove_var("LLAMA_CPP_HOST");
            std::env::remove_var("LITERTLM_HOST");
        }
    }

    fn make_bp(id: &str, provider: &str) -> AgentBlueprint {
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
        }
    }

    #[test]
    fn test_get_active_providers_groq_only() {
        let _lock = ENV_MUTEX.lock().unwrap();
        reset_env();
        unsafe { std::env::set_var("GROQ_API_KEY", "test-key"); }

        let active = get_active_providers();
        assert!(active.contains(&"groq".to_string()));
        assert!(!active.contains(&"nvidia".to_string()));
        assert!(!active.contains(&"openai".to_string()));
        assert!(!active.contains(&"anthropic".to_string()));
        assert!(!active.contains(&"ollama".to_string()));
    }

    #[test]
    fn test_get_active_providers_no_remote_adds_local() {
        let _lock = ENV_MUTEX.lock().unwrap();
        reset_env();

        let active = get_active_providers();
        assert!(active.contains(&"ollama".to_string()));
        assert!(active.contains(&"llamacpp".to_string()));
        assert!(!active.contains(&"groq".to_string()));
    }

    #[test]
    fn test_filter_blueprints_by_providers() {
        let groq_bp = make_bp("groq_test", "groq");
        let nvidia_bp = make_bp("nvidia_test", "nvidia");
        let openai_bp = make_bp("openai_test", "openai");
        let blueprints = vec![groq_bp, nvidia_bp, openai_bp];

        let active = vec!["groq".to_string()];
        let filtered = filter_blueprints(&blueprints, &active);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "groq_test");

        let active = vec!["nvidia".to_string(), "openai".to_string()];
        let filtered = filter_blueprints(&blueprints, &active);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|bp| bp.id == "nvidia_test"));
        assert!(filtered.iter().any(|bp| bp.id == "openai_test"));

        let active = vec!["anthropic".to_string()];
        let filtered = filter_blueprints(&blueprints, &active);
        assert_eq!(filtered.len(), 0);
    }
}
