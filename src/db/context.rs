use crate::context::{ContextEntry, ContextKind};
use crate::embedding::vector_literal;
use sqlx::{PgPool, Row};

pub async fn insert_context_entry(pool: &PgPool, entry: &ContextEntry) -> anyhow::Result<()> {
    if let Some(ref emb) = entry.embedding {
        let emb_literal = vector_literal(emb);
        sqlx::query(
            r#"
            INSERT INTO context_entries (id, kind, content, embedding, metadata, frequency, success_rate, usage_count, last_used_at, created_at)
            VALUES ($1, $2, $3, $4::vector, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (id) DO UPDATE SET
                frequency = context_entries.frequency + EXCLUDED.frequency,
                success_rate = (context_entries.success_rate * context_entries.usage_count::real + EXCLUDED.success_rate * EXCLUDED.usage_count::real) / NULLIF(context_entries.usage_count + EXCLUDED.usage_count, 0)::real,
                usage_count = context_entries.usage_count + EXCLUDED.usage_count,
                last_used_at = EXCLUDED.last_used_at,
                content = EXCLUDED.content,
                embedding = EXCLUDED.embedding,
                metadata = EXCLUDED.metadata
            "#,
        )
        .bind(entry.id)
        .bind(entry.kind.as_str())
        .bind(&entry.content)
        .bind(&emb_literal)
        .bind(&entry.metadata)
        .bind(i32::try_from(entry.frequency).unwrap_or(i32::MAX))
        .bind(entry.success_rate)
        .bind(i32::try_from(entry.usage_count).unwrap_or(i32::MAX))
        .bind(entry.last_used_at)
        .bind(entry.created_at)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            r#"
            INSERT INTO context_entries (id, kind, content, embedding, metadata, frequency, success_rate, usage_count, last_used_at, created_at)
            VALUES ($1, $2, $3, NULL::vector, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE SET
                frequency = context_entries.frequency + EXCLUDED.frequency,
                success_rate = (context_entries.success_rate * context_entries.usage_count::real + EXCLUDED.success_rate * EXCLUDED.usage_count::real) / NULLIF(context_entries.usage_count + EXCLUDED.usage_count, 0)::real,
                usage_count = context_entries.usage_count + EXCLUDED.usage_count,
                last_used_at = EXCLUDED.last_used_at,
                content = EXCLUDED.content,
                metadata = EXCLUDED.metadata
            "#,
        )
        .bind(entry.id)
        .bind(entry.kind.as_str())
        .bind(&entry.content)
        .bind(&entry.metadata)
        .bind(i32::try_from(entry.frequency).unwrap_or(i32::MAX))
        .bind(entry.success_rate)
        .bind(i32::try_from(entry.usage_count).unwrap_or(i32::MAX))
        .bind(entry.last_used_at)
        .bind(entry.created_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn search_context_entries(
    pool: &PgPool,
    query_embedding: &[f32],
    limit: i64,
    kind_filter: Option<&str>,
    min_score: f32,
) -> anyhow::Result<Vec<ContextEntry>> {
    let emb_literal = vector_literal(query_embedding);
    let rows = match kind_filter {
        Some(kind) => {
            sqlx::query(
                r#"
                SELECT id, kind, content, embedding, metadata, frequency, success_rate, usage_count, last_used_at, created_at,
                       1.0 - (embedding <=> $1::vector) AS similarity
                FROM context_entries
                WHERE kind = $2 AND 1.0 - (embedding <=> $1::vector) >= $4
                ORDER BY embedding <=> $1::vector
                LIMIT $3
                "#,
            )
            .bind(&emb_literal)
            .bind(kind)
            .bind(limit)
            .bind(min_score)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query(
                r#"
                SELECT id, kind, content, embedding, metadata, frequency, success_rate, usage_count, last_used_at, created_at,
                       1.0 - (embedding <=> $1::vector) AS similarity
                FROM context_entries
                WHERE 1.0 - (embedding <=> $1::vector) >= $3
                ORDER BY embedding <=> $1::vector
                LIMIT $2
                "#,
            )
            .bind(&emb_literal)
            .bind(limit)
            .bind(min_score)
            .fetch_all(pool)
            .await?
        }
    };

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let embedding_raw: Option<String> = row.try_get("embedding").ok();
        let embedding = embedding_raw
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<f32>>(s).ok());
        out.push(ContextEntry {
            id: row.try_get("id")?,
            kind: {
                let s: String = row.try_get("kind")?;
                match s.as_str() {
                    "tool" => ContextKind::Tool,
                    "skill" => ContextKind::Skill,
                    "memory" => ContextKind::Memory,
                    "conversation" => ContextKind::Conversation,
                    "agent_run" => ContextKind::AgentRun,
                    "artifact" => ContextKind::Artifact,
                    "system_prompt" => ContextKind::SystemPrompt,
                    "few_shot" => ContextKind::FewShot,
                    "policy" => ContextKind::Policy,
                    "permission" => ContextKind::Permission,
                    "security" => ContextKind::Security,
                    "mcp_config" => ContextKind::MCPConfig,
                    _ => ContextKind::Memory,
                }
            },
            content: row.try_get("content")?,
            embedding,
            metadata: row.try_get("metadata").unwrap_or_default(),
            frequency: row.try_get::<i32, _>("frequency").unwrap_or(0) as u32,
            success_rate: row.try_get("success_rate").unwrap_or(0.0),
            usage_count: row.try_get::<i32, _>("usage_count").unwrap_or(0) as u32,
            last_used_at: row.try_get("last_used_at")?,
            created_at: row.try_get("created_at")?,
        });
    }
    Ok(out)
}

pub async fn load_context_entries(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<ContextEntry>> {
    let rows = sqlx::query(
        r#"
        SELECT id, kind, content, embedding, metadata, frequency, success_rate, usage_count, last_used_at, created_at
        FROM context_entries
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let embedding_raw: Option<String> = row.try_get("embedding").ok();
        let embedding = embedding_raw
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<f32>>(s).ok());
        out.push(ContextEntry {
            id: row.try_get("id")?,
            kind: {
                let s: String = row.try_get("kind")?;
                match s.as_str() {
                    "tool" => ContextKind::Tool,
                    "skill" => ContextKind::Skill,
                    "memory" => ContextKind::Memory,
                    "conversation" => ContextKind::Conversation,
                    "agent_run" => ContextKind::AgentRun,
                    "artifact" => ContextKind::Artifact,
                    "system_prompt" => ContextKind::SystemPrompt,
                    "few_shot" => ContextKind::FewShot,
                    "policy" => ContextKind::Policy,
                    "permission" => ContextKind::Permission,
                    "security" => ContextKind::Security,
                    "mcp_config" => ContextKind::MCPConfig,
                    _ => ContextKind::Memory,
                }
            },
            content: row.try_get("content")?,
            embedding,
            metadata: row.try_get("metadata").unwrap_or_default(),
            frequency: row.try_get::<i32, _>("frequency").unwrap_or(0) as u32,
            success_rate: row.try_get("success_rate").unwrap_or(0.0),
            usage_count: row.try_get::<i32, _>("usage_count").unwrap_or(0) as u32,
            last_used_at: row.try_get("last_used_at")?,
            created_at: row.try_get("created_at")?,
        });
    }
    Ok(out)
}
