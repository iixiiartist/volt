use crate::embedding::vector_literal;
use crate::models::{AgentTool, ExecutionRecord, MemoryEntry, RegistryManifest};
use anyhow::Context;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use uuid::Uuid;

pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(database_url)
        .await
        .context("failed to connect to PostgreSQL")
}

pub async fn init_schema(pool: &PgPool) -> anyhow::Result<()> {
    let sql = include_str!("../migrations/0001_core.sql");
    for statement in split_sql_statements(sql) {
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

pub async fn store_memory(
    pool: &PgPool,
    kind: &str,
    content: &str,
    embedding: &[f32],
    session_id: Option<Uuid>,
) -> anyhow::Result<i64> {
    let embedding_literal = crate::embedding::vector_literal(embedding);
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
    let embedding_literal = crate::embedding::vector_literal(query_embedding);
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

// ─── Skills (Compiled Manifest Pattern) ─────────────────────────────

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

#[derive(Debug, Clone)]
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

// ─── Context Store persistence ────────────────────────────────

use crate::context::{ContextEntry, ContextKind};

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
        .bind(entry.frequency as i32)
        .bind(entry.success_rate)
        .bind(entry.usage_count as i32)
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
