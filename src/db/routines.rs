use sqlx::{PgPool, Row};

pub async fn list_routines(pool: &PgPool) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        r#"SELECT id, name, cron, action_prompt, enabled, last_run, next_run, trigger_type, trigger_config, guardrails_max_tokens, guardrails_max_tool_calls, guardrails_allowed_tools, guardrails_timeout_secs FROM routines ORDER BY name"#
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for row in rows {
        out.push(serde_json::json!({
            "id": row.try_get::<uuid::Uuid, _>("id")?,
            "name": row.try_get::<String, _>("name")?,
            "cron": row.try_get::<Option<String>, _>("cron")?,
            "action_prompt": row.try_get::<String, _>("action_prompt")?,
            "enabled": row.try_get::<bool, _>("enabled")?,
            "last_run": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_run")?,
            "next_run": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("next_run")?,
            "trigger_type": row.try_get::<String, _>("trigger_type")?,
            "trigger_config": row.try_get::<Option<serde_json::Value>, _>("trigger_config")?,
            "guardrails_max_tokens": row.try_get::<Option<i64>, _>("guardrails_max_tokens")?,
            "guardrails_max_tool_calls": row.try_get::<Option<i32>, _>("guardrails_max_tool_calls")?,
            "guardrails_allowed_tools": row.try_get::<Option<Vec<String>>, _>("guardrails_allowed_tools")?,
            "guardrails_timeout_secs": row.try_get::<Option<i64>, _>("guardrails_timeout_secs")?,
        }));
    }
    Ok(out)
}
