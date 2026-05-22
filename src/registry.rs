use crate::embedding::EmbeddingClient;
use crate::models::{ProvisionResult, RegistryFetchOptions, RegistryManifest};
use crate::validation::{validate_manifest, verify_declared_sha};
use anyhow::Context;
use reqwest::Client;
use sqlx::PgPool;
use uuid::Uuid;

pub struct RegistryClient {
    http: Client,
}

impl RegistryClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn fetch_manifest(&self, options: &RegistryFetchOptions) -> anyhow::Result<RegistryManifest> {
        let url = format!(
            "{}/packages/{}",
            options.registry_base_url.trim_end_matches('/'),
            options.pkg_id
        );
        let mut req = self.http.get(&url);
        if let Some(token) = &options.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        req.send()
            .await
            .context("failed to fetch registry manifest")?
            .error_for_status()
            .context("registry returned an error")?
            .json::<RegistryManifest>()
            .await
            .context("failed to parse registry manifest")
    }
}

pub async fn provision_manifest(
    pool: &PgPool,
    embedder: &EmbeddingClient,
    manifest: RegistryManifest,
    marketplace_verified: bool,
) -> anyhow::Result<ProvisionResult> {
    let validation = validate_manifest(&manifest);
    if !validation.accepted {
        anyhow::bail!(
            "manifest rejected by static validation: {:?}",
            validation.denied_patterns
        );
    }

    let source_sha256 = verify_declared_sha(&manifest)?;
    let embedding = embedder.embed_description(&manifest.description).await?;
    crate::db::upsert_tool(pool, &manifest, &embedding, &source_sha256, marketplace_verified).await?;
    crate::db::record_registry_event(
        pool,
        &manifest.tool_name,
        Some(&manifest.tool_name),
        "provision",
        "ok",
        serde_json::json!({
            "source_sha256": source_sha256,
            "marketplace_verified": marketplace_verified
        }),
    )
    .await?;

    Ok(ProvisionResult {
        tool_name: manifest.tool_name,
        source_sha256,
        verified: marketplace_verified,
        execution_id: Uuid::new_v4(),
    })
}