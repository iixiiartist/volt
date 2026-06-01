use super::blueprint::{AgentBlueprint, load_blueprint};
use crate::llm::LLMProvider;
use crate::models::{LLMMessage, LLMRequest};
use std::path::PathBuf;
use std::sync::Arc;

/// Route a user prompt to the best-fit AgentBlueprint using the LLM.
///
/// Sends a structured prompt listing all available blueprints (id + description)
/// and asks the LLM to return a JSON `{"blueprint_id": "..."}`. If the LLM
/// response cannot be parsed or no blueprint matches, returns `None`.
pub async fn route_task(
    user_prompt: &str,
    blueprints: &[AgentBlueprint],
    llm_client: &dyn LLMProvider,
) -> Option<AgentBlueprint> {
    if blueprints.is_empty() {
        return None;
    }

    let bp_list: String = blueprints
        .iter()
        .map(|bp| format!("  - id: \"{}\"\n    name: \"{}\"\n    description: \"{}\"", bp.id, bp.name, bp.description))
        .collect::<Vec<_>>()
        .join("\n");

    let sys_prompt = format!(
        r#"You are a routing agent. Given a user request and a list of available agent blueprints, select the single best-fitting blueprint.

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

    // Match against the provided blueprints
    blueprints.iter().find(|bp| bp.id == bp_id).cloned()
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
