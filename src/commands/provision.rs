use crate::config::Settings;
use crate::db;
use crate::embedding::EmbeddingClient;
use crate::models::RegistryFetchOptions;
use crate::registry::{provision_manifest, RegistryClient};
use std::path::PathBuf;

pub async fn provision_file(
    manifest: PathBuf,
    marketplace_verified: bool,
    settings: &Settings,
) -> anyhow::Result<()> {
    let manifest = crate::registry::load_manifest(&manifest).await?;
    let pool = db::connect(&settings.database_url).await?;
    let embedder = EmbeddingClient::new_smart().await;
    let result = provision_manifest(&pool, &embedder, manifest, marketplace_verified).await?;
    let output = serde_json::to_string_pretty(&result)?;
    println!("{}", output);
    Ok(())
}

pub async fn provision_remote(
    pkg_id: String,
    registry_base_url: Option<String>,
    auth_token: Option<String>,
    settings: &Settings,
) -> anyhow::Result<()> {
    let pool = db::connect(&settings.database_url).await?;
    let registry = RegistryClient::new();
    let options = RegistryFetchOptions {
        pkg_id,
        registry_base_url: registry_base_url.unwrap_or(settings.registry_base_url.clone()),
        auth_token: auth_token.or(settings.registry_token.clone()),
    };
    let manifest = registry.fetch_manifest(&options).await?;
    let embedder = EmbeddingClient::new_smart().await;
    let result = provision_manifest(&pool, &embedder, manifest, true).await?;
    let output = serde_json::to_string_pretty(&result)?;
    println!("{}", output);
    Ok(())
}
