mod context;
mod executions;
mod memory;
mod routines;
mod skills;
mod tools;

pub use context::*;
pub use executions::*;
pub use memory::*;
pub use routines::*;
pub use skills::*;
pub use tools::*;

use anyhow::Context;

const SERIALIZATION_RETRY_MAX_ATTEMPTS: u32 = 3;
const SERIALIZATION_RETRY_BASE_DELAY_MS: u64 = 50;
const SERIALIZATION_RETRY_JITTER_MS: u64 = 20;
const SQLSTATE_SERIALIZATION_FAILURE: &str = "40001";
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;
use std::time::Duration;

pub async fn build_shared_pg_pool(database_url: &str) -> anyhow::Result<Arc<PgPool>> {
    let pool = PgPoolOptions::new()
        .max_connections(50)
        .min_connections(5)
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await
        .context("failed to connect to PostgreSQL")?;
    Ok(Arc::new(pool))
}

pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    let arc = build_shared_pg_pool(database_url).await?;
    let pool = Arc::unwrap_or_clone(arc);
    // Auto-migrate on first connect. Migrations are idempotent
    // (CREATE INDEX IF NOT EXISTS, etc.) so re-running them is safe
    // and fast (~5ms when no work is needed). This removes the
    // "did you run `volt init-db`?" failure mode for first-time users.
    if let Err(e) = init_schema(&pool).await {
        tracing::warn!(
            "[db] auto-migrate failed (non-fatal): {}. Run `volt migrate` to retry.",
            e
        );
    }
    Ok(pool)
}

pub async fn execute_with_serialization_retry<F, Fut, T>(mut f: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, sqlx::Error>>,
{
    let max_attempts = SERIALIZATION_RETRY_MAX_ATTEMPTS;
    for attempt in 0..max_attempts {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                let should_retry = e
                    .as_database_error()
                    .and_then(|pg_err| pg_err.code())
                    .map(|code| code.as_ref() == SQLSTATE_SERIALIZATION_FAILURE)
                    .unwrap_or(false);
                if should_retry && attempt + 1 < max_attempts {
                    let delay_ms = SERIALIZATION_RETRY_BASE_DELAY_MS * (attempt as u64 + 1)
                        + (rand::random::<u64>() % SERIALIZATION_RETRY_JITTER_MS);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    continue;
                }
                return Err(anyhow::Error::from(e));
            }
        }
    }
    unreachable!("retry loop should have returned or errored")
}

pub async fn init_schema(pool: &PgPool) -> anyhow::Result<()> {
    let dim = crate::embedding::embedding_dimension();
    let sql = include_str!("../../migrations/0001_core.sql")
        .replace("vector(1024)", &format!("vector({})", dim));
    for statement in split_sql_statements(&sql) {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement).execute(pool).await?;
        }
    }
    let sql2 = include_str!("../../migrations/0002_jobs_and_routines.sql");
    for statement in split_sql_statements(sql2) {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement).execute(pool).await?;
        }
    }
    let sql3 = include_str!("../../migrations/0003_storage_optimizations.sql");
    for statement in split_sql_statements(sql3) {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement).execute(pool).await?;
        }
    }
    let sql4 = include_str!("../../migrations/0004_audit_log.sql");
    for statement in split_sql_statements(sql4) {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement).execute(pool).await?;
        }
    }
    Ok(())
}

fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut in_dollar_quote = false;

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'$') {
            current.push(ch);
            current.push(chars.next().unwrap());
            in_dollar_quote = !in_dollar_quote;
            continue;
        }
        if ch == ';' && !in_dollar_quote {
            statements.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
    }

    if !current.trim().is_empty() {
        statements.push(current);
    }

    statements
}
