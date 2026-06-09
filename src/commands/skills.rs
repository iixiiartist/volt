use crate::config::Settings;
use crate::db;
use crate::embedding::EmbeddingClient;
use crate::skills::catalog;
use crate::skills::SkillRegistry;
use std::path::PathBuf;

pub async fn provision_skill(path: PathBuf, settings: &Settings) -> anyhow::Result<()> {
    let embedder = EmbeddingClient::new_smart().await;
    let pool = db::connect(&settings.database_url).await?;
    let registry = SkillRegistry::new(Some(pool), Some(embedder.clone()));
    registry.compile_skill(&path, &embedder).await?;
    println!("Skill compiled from {:?} and stored in database.", path);
    Ok(())
}

pub async fn list_catalog(catalog_url: Option<String>) -> anyhow::Result<()> {
    let catalog = catalog::fetch_catalog(catalog_url.as_deref()).await?;
    println!("Available skills ({}):", catalog.skills.len());
    for skill in catalog::list_catalog(&catalog) {
        println!(
            "  {} v{} — {}",
            skill.name, skill.version, skill.description
        );
    }
    Ok(())
}

pub async fn search_catalog(query: String, catalog_url: Option<String>) -> anyhow::Result<()> {
    let catalog = catalog::fetch_catalog(catalog_url.as_deref()).await?;
    let results = catalog::search_catalog(&catalog, &query);
    if results.is_empty() {
        println!("No skills found matching '{}'", query);
    } else {
        println!("Skills matching '{}' ({}):", query, results.len());
        for skill in results {
            println!(
                "  {} v{} — {}",
                skill.name, skill.version, skill.description
            );
        }
    }
    Ok(())
}

pub async fn install_skill(
    name: String,
    catalog_url: Option<String>,
    settings: &Settings,
) -> anyhow::Result<()> {
    let embedder = EmbeddingClient::new_smart().await;
    let pool = db::connect(&settings.database_url).await?;
    let catalog = catalog::fetch_catalog(catalog_url.as_deref()).await?;
    catalog::install_skill(&catalog, &name, &pool, &embedder).await?;
    println!("✓ Skill '{}' installed successfully.", name);
    Ok(())
}

pub async fn import_skill(
    path: PathBuf,
    format: String,
    name: Option<String>,
    settings: &Settings,
) -> anyhow::Result<()> {
    use crate::skills::importer;

    if !path.exists() {
        tracing::error!("File not found: {:?}", path);
        std::process::exit(1);
    }

    let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
        tracing::error!("Failed to read {:?}: {}", path, e);
        anyhow::anyhow!("read error")
    })?;

    let source_fmt = match format.as_str() {
        "claude" => importer::SourceFormat::Claude,
        "cursor" => importer::SourceFormat::Cursor,
        "copilot" => importer::SourceFormat::Copilot,
        "opencode" => importer::SourceFormat::OpenCode,
        "markdown" => importer::SourceFormat::Markdown,
        _ => importer::detect_format(&path, &content),
    };

    if source_fmt == importer::SourceFormat::Volt {
        println!("✓ File is already a native Volt SKILL.md. Use `volt provision-skill --path {:?}` instead.", path);
        return Ok(());
    }

    println!("Detected format: {}", importer::format_label(&source_fmt));
    let converted = importer::convert_to_volt_skill(&path, &content, &source_fmt, name.as_deref());
    let tmp_dir = std::env::temp_dir().join(format!("volt-import-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).ok();
    let tmp_path = tmp_dir.join("SKILL.md");
    std::fs::write(&tmp_path, &converted)?;
    let embedder = EmbeddingClient::new_smart().await;
    let pool = db::connect(&settings.database_url).await?;
    let registry = SkillRegistry::new(Some(pool), Some(embedder.clone()));
    registry.compile_skill(&tmp_path, &embedder).await?;
    let manifest = crate::skills::parse_skill_manifest(&tmp_path).ok();
    let skill_name = manifest.map(|m| m.name).unwrap_or_else(|| "unknown".into());
    println!(
        "✓ Imported from {} as skill '{}' with RAG embedding.",
        importer::format_label(&source_fmt),
        skill_name
    );
    std::fs::remove_dir_all(&tmp_dir).ok();
    Ok(())
}
