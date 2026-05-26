use crate::models::{Message, Session};
use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

pub async fn open_sessions(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .context("failed to open SQLite sessions DB")?;

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
    .execute(&pool)
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
    .execute(&pool)
    .await?;

    Ok(pool)
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
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
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
