use crate::embedding::vector_literal;
use crate::models::MemoryEntry;
use anyhow::Context;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn store_memory(
    pool: &PgPool,
    kind: &str,
    content: &str,
    embedding: &[f32],
    session_id: Option<Uuid>,
) -> anyhow::Result<i64> {
    let embedding_literal = vector_literal(embedding);
    let row = sqlx::query(
        r#"
        INSERT INTO memories (kind, content, embedding, session_id)
        VALUES ($1, $2, $3::vector, $4)
        RETURNING id
        "#,
    )
    .bind(kind)
    .bind(content)
    .bind(&embedding_literal)
    .bind(session_id)
    .fetch_one(pool)
    .await
    .context("failed to store memory")?;
    Ok(row.try_get("id")?)
}

pub async fn search_memories(
    pool: &PgPool,
    query_embedding: &[f32],
    limit: i64,
    kind_filter: Option<&str>,
) -> anyhow::Result<Vec<MemoryEntry>> {
    let embedding_literal = vector_literal(query_embedding);
    let rows = match kind_filter {
        Some(kind) => {
            sqlx::query(
                r#"
                SELECT id, kind, content, embedding, session_id, created_at
                FROM memories
                WHERE kind = $1
                ORDER BY embedding <=> $2::vector
                LIMIT $3
                "#,
            )
            .bind(kind)
            .bind(&embedding_literal)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query(
                r#"
                SELECT id, kind, content, embedding, session_id, created_at
                FROM memories
                ORDER BY embedding <=> $1::vector
                LIMIT $2
                "#,
            )
            .bind(&embedding_literal)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let embedding_val: Option<String> = row.try_get("embedding").ok();
        let embedding = embedding_val
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<f32>>(s).ok());
        out.push(MemoryEntry {
            id: row.try_get("id")?,
            kind: row.try_get("kind")?,
            content: row.try_get("content")?,
            embedding,
            session_id: row.try_get("session_id").ok(),
            created_at: row.try_get("created_at")?,
        });
    }
    Ok(out)
}
