use crate::models::{Message, Session};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Open (or create) the SQLite sessions database and run pending schema migrations.
pub async fn open_sessions(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(16)
        .connect_with(options)
        .await
        .context("failed to open SQLite sessions DB")?;

    // Schema version tracking for future migrations
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Run migrations based on current version
    let current: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
        .fetch_one(&pool)
        .await?;

    if current < 1 {
        run_migration_v1(&pool).await?;
    }

    if current < 2 {
        run_migration_v2(&pool).await?;
    }

    if current < 3 {
        run_migration_v3(&pool).await?;
    }

    Ok(pool)
}

async fn run_migration_v1(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            agent_name TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT 'untitled',
            message_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL REFERENCES sessions(id),
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            tool_calls TEXT,
            tool_result TEXT,
            tool_name TEXT,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("INSERT INTO schema_version (version, applied_at) VALUES (1, ?)")
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await?;

    // Performance index for session message lookups
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn run_migration_v2(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS checkpoints (
            session_id TEXT NOT NULL REFERENCES sessions(id),
            iteration INTEGER NOT NULL,
            messages_json TEXT NOT NULL,
            token_counts_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            PRIMARY KEY (session_id, iteration)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("INSERT INTO schema_version (version, applied_at) VALUES (2, ?)")
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await?;

    Ok(())
}

async fn run_migration_v3(pool: &SqlitePool) -> anyhow::Result<()> {
    // Add retry tracking columns to checkpoints table for circuit breaker
    sqlx::query(
        r#"
        ALTER TABLE checkpoints ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0
        "#,
    )
    .execute(pool)
    .await
    // May fail if column already exists (idempotent)
    .ok();

    sqlx::query(
        r#"
        ALTER TABLE checkpoints ADD COLUMN state_hash TEXT NOT NULL DEFAULT ''
        "#,
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query("INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (3, ?)")
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await?;

    Ok(())
}

/// Snapshot of agent state at one iteration for checkpoint rehydration.
#[derive(Serialize, Deserialize)]
pub struct CheckpointData {
    pub session_id: Uuid,
    pub iteration: u32,
    pub messages: Vec<Message>,
    pub token_prompt: u64,
    pub token_completion: u64,
}

/// Maximum retries from the same state hash before circuit breaker trips.
const MAX_CONSECUTIVE_RETRIES: u32 = 3;

/// Check whether the session has been poisoned by repeated rehydration
/// to the same state (detect infinite crash-recovery loops).
pub async fn check_circuit_breaker(
    pool: &SqlitePool,
    session_id: Uuid,
    iteration: u32,
    state_hash: &str,
) -> Result<(), String> {
    let row: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT retry_count FROM checkpoints
        WHERE session_id = ? AND iteration = ? AND state_hash = ?
        "#,
    )
    .bind(session_id.to_string())
    .bind(iteration as i64)
    .bind(state_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    if let Some(count) = row {
        if count as u32 >= MAX_CONSECUTIVE_RETRIES {
            return Err(format!(
                "circuit breaker tripped: session {} iteration {} hash {} retried {} times",
                session_id, iteration, state_hash, count
            ));
        }
    }
    Ok(())
}

/// Compute a hash of the message history for poison-pill detection.
/// Uses the last N messages' content + roles to detect state stagnation.
pub fn compute_state_hash(messages: &[Message]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for msg in messages.iter().rev().take(4) {
        msg.role.hash(&mut hasher);
        msg.content.as_str().hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Save a checkpoint for a given session+iteration with retry tracking.
pub async fn save_checkpoint(
    pool: &SqlitePool,
    data: &CheckpointData,
) -> anyhow::Result<()> {
    let state_hash = compute_state_hash(&data.messages);

    // Check if this checkpoint already exists — if so, increment retry count
    let existing: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT retry_count FROM checkpoints
        WHERE session_id = ? AND iteration = ?
        "#,
    )
    .bind(data.session_id.to_string())
    .bind(data.iteration as i64)
    .fetch_optional(pool)
    .await?;

    let retry_count = existing.map(|c| c + 1).unwrap_or(0);

    sqlx::query(
        r#"
        INSERT OR REPLACE INTO checkpoints
            (session_id, iteration, messages_json, token_counts_json, created_at, retry_count, state_hash)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(data.session_id.to_string())
    .bind(data.iteration as i64)
    .bind(serde_json::to_string(&data.messages)?)
    .bind(serde_json::to_string(&serde_json::json!({
        "prompt": data.token_prompt,
        "completion": data.token_completion,
    }))?)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(retry_count)
    .bind(&state_hash)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load the most recent checkpoint for a session (highest iteration).
pub async fn load_latest_checkpoint(
    pool: &SqlitePool,
    session_id: Uuid,
) -> anyhow::Result<Option<CheckpointData>> {
    let row = sqlx::query(
        r#"
        SELECT iteration, messages_json, token_counts_json
        FROM checkpoints
        WHERE session_id = ?
        ORDER BY iteration DESC
        LIMIT 1
        "#,
    )
    .bind(session_id.to_string())
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else { return Ok(None); };

    let iteration: i64 = row.try_get("iteration")?;
    let messages_json: String = row.try_get("messages_json")?;
    let token_json: String = row.try_get("token_counts_json")?;

    let messages: Vec<Message> = serde_json::from_str(&messages_json)?;
    let tokens: serde_json::Value = serde_json::from_str(&token_json)?;
    let token_prompt = tokens.get("prompt").and_then(|v| v.as_u64()).unwrap_or(0);
    let token_completion = tokens.get("completion").and_then(|v| v.as_u64()).unwrap_or(0);

    Ok(Some(CheckpointData {
        session_id,
        iteration: iteration as u32,
        messages,
        token_prompt,
        token_completion,
    }))
}

pub async fn list_sessions(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<Session>> {
    let rows = sqlx::query(
        r#"
        SELECT id, agent_name, title, message_count, created_at, updated_at
        FROM sessions
        ORDER BY updated_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(Session {
            id: Uuid::parse_str(row.try_get::<&str, _>("id")?)
                .context("invalid session UUID in store")?,
            agent_name: row.try_get("agent_name")?,
            title: row.try_get("title")?,
            message_count: row.try_get("message_count")?,
            created_at: row
                .try_get::<&str, _>("created_at")?
                .parse()
                .context("invalid created_at")?,
            updated_at: row
                .try_get::<&str, _>("updated_at")?
                .parse()
                .context("invalid updated_at")?,
        });
    }
    Ok(out)
}

pub async fn create_session(pool: &SqlitePool, session: &Session) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sessions (id, agent_name, title, message_count, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            message_count = excluded.message_count,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(session.id.to_string())
    .bind(&session.agent_name)
    .bind(&session.title)
    .bind(session.message_count)
    .bind(session.created_at.to_rfc3339())
    .bind(session.updated_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_message(
    pool: &SqlitePool,
    session_id: Uuid,
    msg: &Message,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO messages (session_id, role, content, tool_calls, tool_result, tool_name, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(session_id.to_string())
    .bind(&msg.role)
    .bind(msg.content.as_str())
    .bind(msg.tool_calls.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
    .bind(msg.tool_result.as_ref())
    .bind(msg.tool_name.as_ref())
    .bind(msg.created_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Atomically replace all messages for a session (delete-then-insert in a transaction).
pub async fn save_session_messages_atomic(
    pool: &SqlitePool,
    session_id: Uuid,
    messages: &[Message],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM messages WHERE session_id = ?")
        .bind(session_id.to_string())
        .execute(&mut *tx)
        .await?;
    for msg in messages {
        sqlx::query(
            "INSERT INTO messages (session_id, role, content, tool_calls, tool_result, tool_name, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(session_id.to_string())
        .bind(&msg.role)
        .bind(msg.content.as_str())
        .bind(msg.tool_calls.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
        .bind(msg.tool_result.as_ref())
        .bind(msg.tool_name.as_ref())
        .bind(msg.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn load_messages(pool: &SqlitePool, session_id: Uuid) -> anyhow::Result<Vec<Message>> {
    let rows = sqlx::query(
        r#"
        SELECT role, content, tool_calls, tool_result, tool_name, created_at
        FROM messages
        WHERE session_id = ?
        ORDER BY id ASC
        "#,
    )
    .bind(session_id.to_string())
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(Message {
            id: uuid::Uuid::nil(),
            parent_message_id: None,
            role: row.try_get("role")?,
            content: Arc::new(row.try_get("content")?),
            tool_calls: row
                .try_get::<Option<&str>, _>("tool_calls")?
                .and_then(|s| serde_json::from_str(s).ok()),
            tool_result: row.try_get("tool_result")?,
            tool_name: row.try_get("tool_name")?,
            created_at: row
                .try_get::<&str, _>("created_at")?
                .parse()
                .unwrap_or_else(|_| chrono::Utc::now()),
        });
    }
    Ok(out)
}

pub async fn delete_session_messages(pool: &SqlitePool, session_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM messages WHERE session_id = ?")
        .bind(session_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    async fn test_pool() -> SqlitePool {
        let dir = std::env::temp_dir().join(format!("volt_test_{}", uuid::Uuid::new_v4()));
        let options = SqliteConnectOptions::new()
            .filename(&dir)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT 'untitled',
                message_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                tool_calls TEXT,
                tool_result TEXT,
                tool_name TEXT,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_create_and_list_sessions() {
        let pool = test_pool().await;
        let session = Session {
            id: Uuid::new_v4(),
            agent_name: "test-agent".into(),
            title: "test session".into(),
            message_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        create_session(&pool, &session).await.unwrap();
        let sessions = list_sessions(&pool, 10).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].agent_name, "test-agent");
    }

    #[tokio::test]
    async fn test_save_and_load_messages() {
        let pool = test_pool().await;
        let session_id = Uuid::new_v4();
        let session = Session {
            id: session_id,
            agent_name: "test-agent".into(),
            title: "test".into(),
            message_count: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        create_session(&pool, &session).await.unwrap();

        let msg = Message {
            id: uuid::Uuid::nil(),
            parent_message_id: None,
            role: "user".into(),
            content: Arc::new("hello".to_string()),
            tool_calls: None,
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        };
        save_message(&pool, session_id, &msg).await.unwrap();

        let msgs = load_messages(&pool, session_id).await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content.as_str(), "hello");
    }

    #[tokio::test]
    async fn test_delete_session_messages() {
        let pool = test_pool().await;
        let session_id = Uuid::new_v4();
        let session = Session {
            id: session_id,
            agent_name: "test-agent".into(),
            title: "test".into(),
            message_count: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        create_session(&pool, &session).await.unwrap();

        let msg = Message {
            id: uuid::Uuid::nil(),
            parent_message_id: None,
            role: "user".into(),
            content: Arc::new("data".to_string()),
            tool_calls: None,
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        };
        save_message(&pool, session_id, &msg).await.unwrap();
        delete_session_messages(&pool, session_id).await.unwrap();

        let msgs = load_messages(&pool, session_id).await.unwrap();
        assert_eq!(msgs.len(), 0);
    }
}
