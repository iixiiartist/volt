use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutineTrigger {
    Cron(String),
    Event {
        event: String,
        filter: Option<serde_json::Value>,
    },
    Webhook {
        path: String,
        secret: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guardrails {
    pub max_tokens: Option<u64>,
    pub max_tool_calls: Option<u32>,
    pub allowed_tools: Option<Vec<String>>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {
    pub id: Uuid,
    pub name: String,
    pub trigger: RoutineTrigger,
    pub action_prompt: String,
    pub enabled: bool,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub guardrails: Guardrails,
    pub created_at: DateTime<Utc>,
}

/// Validate an action prompt at definition time.
/// Returns Ok(()) if the prompt is valid, or an Err with a description.
pub fn validate_action_prompt(prompt: &str) -> Result<(), String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Err("action_prompt must not be empty".into());
    }
    if trimmed.len() < 10 {
        return Err("action_prompt is too short (minimum 10 characters)".into());
    }
    Ok(())
}

pub mod engine;
