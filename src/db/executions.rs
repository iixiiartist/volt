use crate::models::ExecutionRecord;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn record_execution(
    pool: &PgPool,
    tool_id: Option<i32>,
    tool_name: &str,
    input: &serde_json::Value,
    output: &serde_json::Value,
    status: &str,
    error: Option<&str>,
    duration_ms: i32,
    execution_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO tool_executions (tool_id, tool_name, input, output, status, error, duration_ms, execution_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(tool_id)
    .bind(tool_name)
    .bind(input)
    .bind(output)
    .bind(status)
    .bind(error)
    .bind(duration_ms)
    .bind(execution_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_executions(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<ExecutionRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT id, tool_id, tool_name, input, output, status, error,
               duration_ms, created_at, execution_id
        FROM tool_executions
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(ExecutionRecord {
            id: row.try_get("id")?,
            tool_id: row.try_get("tool_id")?,
            tool_name: row.try_get("tool_name")?,
            input: row.try_get("input")?,
            output: row.try_get("output")?,
            status: row.try_get("status")?,
            error: row.try_get("error")?,
            duration_ms: row.try_get("duration_ms")?,
            created_at: row.try_get("created_at")?,
            execution_id: row.try_get("execution_id")?,
        });
    }
    Ok(out)
}

pub async fn record_registry_event(
    pool: &PgPool,
    pkg_id: &str,
    tool_name: Option<&str>,
    event_type: &str,
    status: &str,
    details: serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO registry_events (pkg_id, tool_name, event_type, status, details)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(pkg_id)
    .bind(tool_name)
    .bind(event_type)
    .bind(status)
    .bind(details)
    .execute(pool)
    .await?;
    Ok(())
}
