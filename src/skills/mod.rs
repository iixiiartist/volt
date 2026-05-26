pub mod catalog;
pub mod importer;

use crate::db::{self, SkillEntry};
use crate::embedding::EmbeddingClient;
use sqlx::PgPool;
use std::path::Path;
use uuid::Uuid;

/// Parsed SKILL.md manifest
#[derive(Debug, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub content: String,
    pub mcp_servers: Vec<String>,
    pub source_path: Option<String>,
}

/// Runtime skill registry: DB-backed with in-memory fallback
pub struct SkillRegistry {
    pool: Option<PgPool>,
    #[allow(dead_code)]
    embedder: Option<EmbeddingClient>,
    cache: Vec<SkillEntry>,
}

impl SkillRegistry {
    pub fn new(pool: Option<PgPool>, embedder: Option<EmbeddingClient>) -> Self {
        Self {
            pool,
            embedder,
            cache: Vec::new(),
        }
    }

    /// Load skills from database (with in-memory cache)
    pub async fn load_from_db(&mut self) -> anyhow::Result<()> {
        if let Some(ref pool) = self.pool {
            self.cache = db::list_skills(pool).await?;
        }
        Ok(())
    }

    /// Search skills by embedding similarity (DB-backed)
    pub async fn search(&self, query_embedding: &[f32], limit: usize) -> Vec<SkillEntry> {
        if let Some(ref pool) = self.pool {
            db::search_skills(pool, query_embedding, limit as i64)
                .await
                .unwrap_or_default()
        } else if let Some(ref embedder) = self.embedder {
            let mut scored: Vec<(f32, SkillEntry)> = Vec::new();
            for skill in &self.cache {
                if let Ok(emb) = embedder.embed_description(&skill.description).await {
                    let sim = cosine_similarity(query_embedding, &emb);
                    scored.push((sim, skill.clone()));
                }
            }
            scored.sort_unstable_by(|a, b| {
                b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            scored.truncate(limit);
            scored.into_iter().map(|(_, s)| s).collect()
        } else {
            Vec::new()
        }
    }

    /// Compile a SKILL.md file and store in database
    pub async fn compile_skill(
        &self,
        path: &Path,
        embedder: &EmbeddingClient,
    ) -> anyhow::Result<()> {
        let manifest = parse_skill_manifest(path)?;
        let embedding = embedder.embed_description(&manifest.description).await?;

        if let Some(ref pool) = self.pool {
            db::upsert_skill(
                pool,
                Uuid::new_v4(),
                &manifest.name,
                &manifest.description,
                &manifest.version,
                &manifest.content,
                &embedding,
                &manifest.mcp_servers,
                manifest.source_path.as_deref(),
            )
            .await?;
        }

        Ok(())
    }

    /// List all compiled skills
    pub fn list(&self) -> &[SkillEntry] {
        &self.cache
    }
}

/// Parse a SKILL.md file (frontmatter + content)
pub fn parse_skill_manifest(path: &Path) -> anyhow::Result<SkillManifest> {
    let content = std::fs::read_to_string(path)?;
    let content = content.trim();

    // Parse frontmatter (YAML between --- markers)
    let (frontmatter, body) = if content.starts_with("---") {
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() >= 3 {
            (parts[1].trim(), parts[2].trim())
        } else {
            ("", content)
        }
    } else {
        ("", content)
    };

    // Parse frontmatter fields (simple key: value)
    let mut name = String::new();
    let mut version = String::from("1.0.0");
    let mut description = String::new();
    let mut mcp_servers: Vec<String> = Vec::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "name" => name = value.to_string(),
                "version" => version = value.to_string(),
                "description" => description = value.to_string(),
                "mcp_servers"
                    // Parse array: ["server1", "server2"]
                    if value.starts_with('[') && value.ends_with(']') => {
                        mcp_servers = value[1..value.len() - 1]
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                _ => {}
            }
        }
    }

    // Extract description from first heading if not in frontmatter
    if description.is_empty() {
        if let Some(first_line) = body.lines().find(|l| l.starts_with('#')) {
            description = first_line.trim_start_matches('#').trim().to_string();
        }
    }

    // Use body as content
    let content = body.to_string();

    if name.is_empty() {
        anyhow::bail!("SKILL.md missing required 'name' field");
    }

    Ok(SkillManifest {
        name,
        version,
        description,
        content,
        mcp_servers,
        source_path: path.to_str().map(String::from),
    })
}

use crate::cosine_similarity;
