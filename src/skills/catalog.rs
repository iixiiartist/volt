use crate::db;
use crate::embedding::EmbeddingClient;
use crate::http_client;
use serde::Deserialize;
use sqlx::PgPool;

const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/iixiiartist/volt/main/skills/catalog.json";

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillCatalog {
    pub version: u32,
    pub base_url: String,
    pub skills: Vec<CatalogEntry>,
}

/// Fetch the skill catalog from the default registry URL.
pub async fn fetch_catalog(url: Option<&str>) -> anyhow::Result<SkillCatalog> {
    let url = url.unwrap_or(DEFAULT_CATALOG_URL);
    let client = http_client(30);
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "catalog fetch returned HTTP {}",
            resp.status().as_u16()
        );
    }
    let catalog: SkillCatalog = resp.json().await?;
    Ok(catalog)
}

/// List all skills available in the catalog.
pub fn list_catalog(catalog: &SkillCatalog) -> Vec<&CatalogEntry> {
    catalog.skills.iter().collect()
}

/// Search catalog by keyword (name or description).
pub fn search_catalog<'a>(catalog: &'a SkillCatalog, query: &str) -> Vec<&'a CatalogEntry> {
    let q = query.to_lowercase();
    catalog
        .skills
        .iter()
        .filter(|s| {
            s.name.to_lowercase().contains(&q)
                || s.description.to_lowercase().contains(&q)
                || s.author.to_lowercase().contains(&q)
        })
        .collect()
}

/// Download a SKILL.md from the catalog and compile it into the database.
/// Tries the remote URL first, then falls back to local file path.
pub async fn install_skill(
    catalog: &SkillCatalog,
    name: &str,
    pool: &PgPool,
    embedder: &EmbeddingClient,
) -> anyhow::Result<()> {
    let entry = catalog
        .skills
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow::anyhow!("skill '{}' not found in catalog", name))?;

    let content = fetch_skill_content(catalog, entry).await?;

    // Write to a temp file and parse
    let tmp_dir = std::env::temp_dir().join(format!("volt-skill-{}", name));
    std::fs::create_dir_all(&tmp_dir).ok();
    let tmp_path = tmp_dir.join("SKILL.md");
    std::fs::write(&tmp_path, &content)?;

    let manifest = crate::skills::parse_skill_manifest(&tmp_path)?;
    let embedding = embedder.embed_description(&manifest.description).await?;

    let source = format!("{}/{}", catalog.base_url.trim_end_matches('/'), entry.path);

    db::upsert_skill(
        pool,
        uuid::Uuid::new_v4(),
        &manifest.name,
        &manifest.description,
        &manifest.version,
        &manifest.content,
        &embedding,
        &manifest.mcp_servers,
        Some(&source),
    )
    .await?;

    std::fs::remove_dir_all(&tmp_dir).ok();
    Ok(())
}

/// Fetch skill content from remote URL, falling back to local file.
async fn fetch_skill_content(catalog: &SkillCatalog, entry: &CatalogEntry) -> anyhow::Result<String> {
    // Try remote first
    let skill_url = format!("{}/{}", catalog.base_url.trim_end_matches('/'), entry.path);
    let client = http_client(10);
    match client.get(&skill_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            return resp.text().await.map_err(|e| anyhow::anyhow!("failed to read response: {}", e));
        }
        _ => {}
    }

    // Fallback: try local file
    let local_path = std::path::Path::new(&entry.path);
    if local_path.exists() {
        return std::fs::read_to_string(local_path)
            .map_err(|e| anyhow::anyhow!("failed to read local file {}: {}", local_path.display(), e));
    }

    anyhow::bail!(
        "could not fetch skill '{}' from {} (offline and file not found at {})",
        entry.name, skill_url, local_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_catalog() {
        let catalog = SkillCatalog {
            version: 1,
            base_url: "https://example.com".into(),
            skills: vec![
                CatalogEntry {
                    name: "github-pr-reviewer".into(),
                    description: "Automated code review with security scanning".into(),
                    version: "1.0.0".into(),
                    author: "Volt".into(),
                    path: "examples/github-pr-reviewer/SKILL.md".into(),
                },
                CatalogEntry {
                    name: "system-diagnostics".into(),
                    description: "Local system health checks and diagnostics".into(),
                    version: "1.0.0".into(),
                    author: "Volt".into(),
                    path: "examples/system-diagnostics/SKILL.md".into(),
                },
            ],
        };

        let results = search_catalog(&catalog, "review");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "github-pr-reviewer");

        let results = search_catalog(&catalog, "system");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "system-diagnostics");

        let results = search_catalog(&catalog, "volt");
        assert_eq!(results.len(), 2);

        let results = search_catalog(&catalog, "nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn test_list_catalog() {
        let catalog = SkillCatalog {
            version: 1,
            base_url: "https://example.com".into(),
            skills: vec![
                CatalogEntry {
                    name: "skill-a".into(),
                    description: "desc a".into(),
                    version: "1.0.0".into(),
                    author: "author-a".into(),
                    path: "path/a".into(),
                },
                CatalogEntry {
                    name: "skill-b".into(),
                    description: "desc b".into(),
                    version: "2.0.0".into(),
                    author: "author-b".into(),
                    path: "path/b".into(),
                },
            ],
        };

        let list = list_catalog(&catalog);
        assert_eq!(list.len(), 2);
    }
}
