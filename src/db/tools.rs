use crate::embedding::vector_literal;
use crate::models::{AgentTool, RegistryManifest};
use anyhow::Context;
use sqlx::{PgPool, Row};

pub async fn upsert_tool(
    pool: &PgPool,
    manifest: &RegistryManifest,
    embedding: &[f32],
    source_sha256: &str,
    marketplace_verified: bool,
) -> anyhow::Result<()> {
    let embedding_literal = vector_literal(embedding);
    let manifest_json = serde_json::to_value(manifest)?;
    let signature = manifest.signature.clone().unwrap_or_default();

    sqlx::query(
        r#"
        INSERT INTO agent_tools (
            tool_name, description, language, source_code,
            parameter_schema, embedding, is_marketplace_verified,
            cryptographic_signature, source_sha256, manifest
        )
        VALUES ($1, $2, $3, $4, $5, $6::vector, $7, $8, $9, $10)
        ON CONFLICT (tool_name) DO UPDATE SET
            description = EXCLUDED.description,
            language = EXCLUDED.language,
            source_code = EXCLUDED.source_code,
            parameter_schema = EXCLUDED.parameter_schema,
            embedding = EXCLUDED.embedding,
            is_marketplace_verified = EXCLUDED.is_marketplace_verified,
            cryptographic_signature = EXCLUDED.cryptographic_signature,
            source_sha256 = EXCLUDED.source_sha256,
            manifest = EXCLUDED.manifest
        "#,
    )
    .bind(&manifest.tool_name)
    .bind(&manifest.description)
    .bind(&manifest.language)
    .bind(&manifest.source_code)
    .bind(&manifest.parameter_schema)
    .bind(&embedding_literal)
    .bind(marketplace_verified)
    .bind(&signature)
    .bind(source_sha256)
    .bind(&manifest_json)
    .execute(pool)
    .await
    .context("failed to upsert agent tool")?;

    Ok(())
}

pub async fn list_tools(pool: &PgPool) -> anyhow::Result<Vec<AgentTool>> {
    let rows = sqlx::query(
        r#"
        SELECT id, tool_name, description, language, is_marketplace_verified,
               source_sha256, created_at, updated_at
        FROM agent_tools
        ORDER BY updated_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(AgentTool {
            id: row.try_get("id")?,
            tool_name: row.try_get("tool_name")?,
            description: row.try_get("description")?,
            language: row.try_get("language")?,
            is_marketplace_verified: row.try_get("is_marketplace_verified")?,
            source_sha256: row.try_get("source_sha256")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        });
    }
    Ok(out)
}

pub async fn get_tool_by_name(pool: &PgPool, tool_name: &str) -> anyhow::Result<Option<AgentTool>> {
    let row = sqlx::query(
        r#"
        SELECT id, tool_name, description, language, is_marketplace_verified,
               source_sha256, created_at, updated_at
        FROM agent_tools
        WHERE tool_name = $1
        "#,
    )
    .bind(tool_name)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => {
            let tool = AgentTool {
                id: r
                    .try_get("id")
                    .map_err(|e| anyhow::anyhow!("db: id: {}", e))?,
                tool_name: r
                    .try_get("tool_name")
                    .map_err(|e| anyhow::anyhow!("db: tool_name: {}", e))?,
                description: r
                    .try_get("description")
                    .map_err(|e| anyhow::anyhow!("db: description: {}", e))?,
                language: r
                    .try_get("language")
                    .map_err(|e| anyhow::anyhow!("db: language: {}", e))?,
                is_marketplace_verified: r
                    .try_get("is_marketplace_verified")
                    .map_err(|e| anyhow::anyhow!("db: is_marketplace_verified: {}", e))?,
                source_sha256: r
                    .try_get("source_sha256")
                    .map_err(|e| anyhow::anyhow!("db: source_sha256: {}", e))?,
                created_at: r
                    .try_get("created_at")
                    .map_err(|e| anyhow::anyhow!("db: created_at: {}", e))?,
                updated_at: r
                    .try_get("updated_at")
                    .map_err(|e| anyhow::anyhow!("db: updated_at: {}", e))?,
            };
            Ok(Some(tool))
        }
        None => Ok(None),
    }
}

pub async fn get_tool_source(pool: &PgPool, tool_name: &str) -> anyhow::Result<Option<String>> {
    let row = sqlx::query("SELECT source_code FROM agent_tools WHERE tool_name = $1")
        .bind(tool_name)
        .fetch_optional(pool)
        .await?;

    Ok(row.and_then(|r| r.try_get("source_code").ok()))
}

/// A tool row loaded from the `agent_tools` table.
///
/// Marked `#[non_exhaustive]` so downstream consumers cannot destructure
/// every field by name; new columns can be added without breaking semver.
/// `sqlx::FromRow` still works since it doesn't go through pattern matching.
#[derive(sqlx::FromRow)]
#[non_exhaustive]
pub struct DbTool {
    pub tool_name: String,
    pub description: String,
    pub parameter_schema: serde_json::Value,
    pub source_code: String,
}

impl DbTool {
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn parameter_schema(&self) -> &serde_json::Value {
        &self.parameter_schema
    }
    pub fn source_code(&self) -> &str {
        &self.source_code
    }
}

pub async fn list_tools_with_schema(pool: &PgPool) -> anyhow::Result<Vec<DbTool>> {
    let rows = sqlx::query_as::<_, DbTool>(
        "SELECT tool_name, description, parameter_schema, source_code FROM agent_tools",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
