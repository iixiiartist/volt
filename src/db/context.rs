use crate::context::{ContextEntry, ContextKind};
use crate::embedding::vector_literal;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use uuid::Uuid;

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

/// Bulk insert context entries using a single PostgreSQL statement.
/// Eliminates N round-trips for N entries. Falls back gracefully
/// via ON CONFLICT (id) DO UPDATE for idempotent re-seeding.
pub async fn bulk_insert_context_entries(
    pool: &PgPool,
    entries: &[ContextEntry],
) -> anyhow::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO context_entries (id, kind, content, embedding, metadata, frequency, success_rate, usage_count, last_used_at, created_at) "
    );

    builder.push_values(entries, |mut b, entry| {
        b.push_bind(entry.id)
            .push_bind(entry.kind.as_str())
            .push_bind(&entry.content);
        if let Some(emb) = &entry.embedding {
            b.push(format!("{}::vector", vector_literal(emb)));
        } else {
            b.push("NULL::vector");
        }
        b.push_bind(&entry.metadata)
            .push_bind(i32::try_from(entry.frequency).unwrap_or(i32::MAX))
            .push_bind(entry.success_rate)
            .push_bind(i32::try_from(entry.usage_count).unwrap_or(i32::MAX))
            .push_bind(entry.last_used_at)
            .push_bind(entry.created_at);
    });

    builder.push(
        " ON CONFLICT (id) DO UPDATE SET
            frequency = context_entries.frequency + EXCLUDED.frequency,
            success_rate = (context_entries.success_rate * context_entries.usage_count::real + EXCLUDED.success_rate * EXCLUDED.usage_count::real) / NULLIF(context_entries.usage_count + EXCLUDED.usage_count, 0)::real,
            usage_count = context_entries.usage_count + EXCLUDED.usage_count,
            last_used_at = EXCLUDED.last_used_at,
            content = EXCLUDED.content,
            embedding = EXCLUDED.embedding,
            metadata = EXCLUDED.metadata"
    );

    builder.build().execute(pool).await?;
    Ok(())
}

/// Delete context entries by their UUIDs.
/// Returns the number of rows affected.
pub async fn delete_context_entries_by_ids(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut builder = QueryBuilder::<Postgres>::new("DELETE FROM context_entries WHERE id IN (");
    let mut separated = builder.separated(",");
    for id in ids {
        separated.push_bind(*id);
    }
    separated.push_unseparated(");");
    let result = builder.build().execute(pool).await?;
    Ok(result.rows_affected())
}

/// Bulk update embeddings for existing context entries.
/// Uses UNNEST for a single round-trip.
pub async fn bulk_update_embeddings(
    pool: &PgPool,
    updates: &[(Uuid, Vec<f32>)],
) -> anyhow::Result<()> {
    if updates.is_empty() {
        return Ok(());
    }

    let ids: Vec<Uuid> = updates.iter().map(|(id, _)| *id).collect();
    let embeddings: Vec<String> = updates.iter().map(|(_, emb)| vector_literal(emb)).collect();

    sqlx::query(
        "UPDATE context_entries SET embedding = v.emb::vector
         FROM (SELECT unnest($1::uuid[]) as id, unnest($2::text[]) as emb) as v
         WHERE context_entries.id = v.id",
    )
    .bind(&ids[..])
    .bind(&embeddings[..])
    .execute(pool)
    .await?;

    Ok(())
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
