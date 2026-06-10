use crate::embedding::vector_literal;
use anyhow::Context;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// A skill row loaded from the skills table.
///
/// `#[non_exhaustive]` prevents downstream consumers from destructuring by
/// name; new columns can be added without breaking semver. `Clone` is
/// retained because skill entries are cloned into the context store.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SkillEntry {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub version: String,
    pub content: String,
    pub mcp_servers: Vec<String>,
    pub source_path: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl SkillEntry {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn content(&self) -> &str {
        &self.content
    }
    pub fn mcp_servers(&self) -> &[String] {
        &self.mcp_servers
    }
    pub fn source_path(&self) -> Option<&str> {
        self.source_path.as_deref()
    }
    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.created_at
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_skill(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    description: &str,
    version: &str,
    content: &str,
    embedding: &[f32],
    mcp_servers: &[String],
    source_path: Option<&str>,
) -> anyhow::Result<()> {
    let embedding_literal = vector_literal(embedding);
    let mcp_servers_json = serde_json::to_value(mcp_servers)?;
    let source = source_path.unwrap_or("");

    sqlx::query(
        r#"
        INSERT INTO skills (id, name, description, version, content, embedding, mcp_servers, source_path)
        VALUES ($1, $2, $3, $4, $5, $6::vector, $7, $8)
        ON CONFLICT (name) DO UPDATE SET
            description = EXCLUDED.description,
            version = EXCLUDED.version,
            content = EXCLUDED.content,
            embedding = EXCLUDED.embedding,
            mcp_servers = EXCLUDED.mcp_servers,
            source_path = EXCLUDED.source_path
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(version)
    .bind(content)
    .bind(&embedding_literal)
    .bind(&mcp_servers_json)
    .bind(source)
    .execute(pool)
    .await
    .context("failed to upsert skill")?;

    Ok(())
}

pub async fn search_skills(
    pool: &PgPool,
    query_embedding: &[f32],
    limit: i64,
) -> anyhow::Result<Vec<SkillEntry>> {
    let embedding_literal = vector_literal(query_embedding);

    let rows = sqlx::query(
        r#"
        SELECT id, name, description, version, content, mcp_servers, source_path, created_at
        FROM skills
        ORDER BY embedding <=> $1::vector
        LIMIT $2
        "#,
    )
    .bind(&embedding_literal)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(SkillEntry {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            version: row.try_get("version")?,
            content: row.try_get("content")?,
            mcp_servers: serde_json::from_str(row.try_get::<&str, _>("mcp_servers")?)
                .unwrap_or_default(),
            source_path: row.try_get("source_path").ok(),
            created_at: row.try_get("created_at")?,
        });
    }
    Ok(out)
}

pub async fn list_skills(pool: &PgPool) -> anyhow::Result<Vec<SkillEntry>> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, description, version, content, mcp_servers, source_path, created_at
        FROM skills
        ORDER BY name
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(SkillEntry {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            version: row.try_get("version")?,
            content: row.try_get("content")?,
            mcp_servers: serde_json::from_str(row.try_get::<&str, _>("mcp_servers")?)
                .unwrap_or_default(),
            source_path: row.try_get("source_path").ok(),
            created_at: row.try_get("created_at")?,
        });
    }
    Ok(out)
}
