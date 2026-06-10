mod providers;

use anyhow::Context;
use providers::ProviderConfig;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub fn embedding_dimension() -> usize {
    static DIM: OnceLock<usize> = OnceLock::new();
    *DIM.get_or_init(|| {
        std::env::var("EMBEDDING_DIMENSION")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024)
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum EmbeddingProvider {
    Ollama,
    LlamaCpp,
    Nvidia,
    OpenAI,
    Moonshot,
    HuggingFace,
}

#[derive(Clone)]
pub struct EmbeddingClient {
    http: reqwest::Client,
    providers: Vec<ProviderConfig>,
    verbose: bool,
    #[cfg(feature = "tools-local-embeddings")]
    local: Option<std::sync::Arc<crate::local_embed::LocalEmbedder>>,
}

impl EmbeddingClient {
    /// Construct a client with one explicit provider. The previous version
    /// of this constructor silently defaulted to NVIDIA NIM, which masked
    /// configuration errors and could even bill the user for a key they
    /// hadn't configured. New code should use `new_smart` (which respects
    /// `EMBEDDING_PROVIDER` and falls back to auto-detect) or `with_provider`
    /// (which requires an explicit slug). This constructor is kept for
    /// backward compatibility but emits a startup warning when the
    /// default NIM path is taken without a key in scope.
    pub fn new(api_key: Option<String>, model: impl Into<String>) -> Self {
        let key = api_key.filter(|k| !k.is_empty());
        if key.is_none() {
            tracing::warn!(
                "[embedding] `EmbeddingClient::new` called with no API key; falling back to NIM \
                 without a key will produce 401s at first call. Use `new_smart` or set \
                 EMBEDDING_PROVIDER + EMBEDDING_API_KEY to avoid this."
            );
        }
        Self::with_provider(
            key,
            model,
            EmbeddingProvider::Nvidia,
            "https://integrate.api.nvidia.com/v1/embeddings",
        )
    }

    pub fn with_provider(
        api_key: Option<String>,
        model: impl Into<String>,
        provider: EmbeddingProvider,
        endpoint: impl Into<String>,
    ) -> Self {
        let config = ProviderConfig {
            provider,
            model: model.into(),
            endpoint: endpoint.into(),
            api_key,
        };
        Self {
            http: crate::http_client(),
            providers: vec![config],
            verbose: false,
            #[cfg(feature = "tools-local-embeddings")]
            local: None,
        }
    }

    pub async fn new_smart() -> Self {
        let http = crate::http_client();
        let provider_str = std::env::var("EMBEDDING_PROVIDER").ok();

        let providers = match provider_str {
            Some(ref p) if !p.is_empty() && p.to_lowercase() != "auto" => {
                let mut pv = Vec::new();
                if let Some(config) = ProviderConfig::from_env(p) {
                    pv.push(config);
                }
                pv
            }
            _ => providers::auto_detect_providers(&http).await,
        };

        // Log the detected inventory at startup so the user can see
        // which providers are active (audit issue 5: previously silent).
        if !providers.is_empty() {
            let names: Vec<String> = providers
                .iter()
                .map(|p| format!("{:?}", p.provider))
                .collect();
            tracing::info!("[embedding] auto-detected providers: {}", names.join(", "));
        } else {
            tracing::warn!(
                "[embedding] no embedding providers detected. Local ONNX model + remote keys \
                 were both unavailable. Tool/memory/skill retrieval will use SHA-256 fallback \
                 embeddings (low-quality). Set EMBEDDING_PROVIDER, EMBEDDING_API_KEY, or \
                 install the local ONNX model to enable semantic retrieval."
            );
        }

        // Load local ONNX embedder (ort with EP fallback chain:
        // OpenVINO → DirectML → CUDA → CPU). Falls through to remote or
        // deterministic placeholder if model files are not cached yet.
        #[cfg(feature = "tools-local-embeddings")]
        let local = match crate::local_embed::LocalEmbedder::load() {
            Ok(embedder) => {
                tracing::info!(
                    "local ONNX embedder loaded (ort EP chain: OpenVINO → DirectML → CUDA → CPU)"
                );
                Some(std::sync::Arc::new(embedder))
            }
            Err(e) => {
                tracing::warn!(
                    "local ONNX embedder unavailable: {}. Will use deterministic fallback + BM25.",
                    e
                );
                None
            }
        };

        // Clear providers when no local or remote path is configured —
        // avoids slow Ollama retry delays when Ollama is not actually running.
        #[cfg(not(feature = "tools-local-embeddings"))]
        let local: Option<std::sync::Arc<crate::local_embed::LocalEmbedder>> = None;
        if local.is_none()
            && providers.is_empty()
            && provider_str
                .as_deref()
                .is_none_or(|s| s.is_empty() || s.eq_ignore_ascii_case("auto"))
        {
            // keep providers as-is (may have auto-detected Ollama); embed will try them first
        }

        Self {
            http,
            providers,
            verbose: true,
            #[cfg(feature = "tools-local-embeddings")]
            local,
        }
    }

    pub async fn embed_description(&self, description: &str) -> anyhow::Result<Vec<f32>> {
        if self.providers.is_empty() {
            return Ok(deterministic_placeholder_embedding(description));
        }

        let truncated = truncate_description(description);

        #[cfg(feature = "tools-local-embeddings")]
        if let Some(local) = &self.local {
            let cloned = local.clone();
            match cloned.embed_async(truncated.to_string()).await {
                Ok(embedding) => return Ok(normalize_dims(embedding)),
                Err(e) => {
                    if self.verbose {
                        tracing::warn!("local embed failed: {}", e);
                    }
                }
            }
        }

        for (i, config) in self.providers.iter().enumerate() {
            match self.embed_with(config, truncated).await {
                Ok(embedding) => {
                    if i > 0 && self.verbose {
                        eprintln!(
                            "[embed] primary provider failed, used fallback #{} ({:?})",
                            i, config.provider
                        );
                    }
                    return Ok(embedding);
                }
                Err(e) => {
                    if self.verbose {
                        eprintln!(
                            "[embed] {:?} failed: {}. Trying next provider...",
                            config.provider, e
                        );
                    }
                }
            }
        }

        if self.verbose {
            eprintln!("[embed] all remote providers failed, using deterministic fallback");
        }
        Ok(deterministic_placeholder_embedding(description))
    }

    async fn embed_with(
        &self,
        config: &ProviderConfig,
        description: &str,
    ) -> anyhow::Result<Vec<f32>> {
        match &config.api_key {
            Some(key) if !key.is_empty() && key != "your_nvidia_api_key_here" => {
                self.embed_remote(config, description, key).await
            }
            _ if config.provider == EmbeddingProvider::Ollama
                || config.provider == EmbeddingProvider::LlamaCpp =>
            {
                self.embed_remote(config, description, "").await
            }
            _ => {
                anyhow::bail!("no valid API key for {:?}", config.provider);
            }
        }
    }

    async fn embed_remote(
        &self,
        config: &ProviderConfig,
        description: &str,
        api_key: &str,
    ) -> anyhow::Result<Vec<f32>> {
        let body = match config.provider {
            EmbeddingProvider::Nvidia => json!({
                "model": config.model,
                "input": description,
                "input_type": "query",
                "encoding_format": "float"
            }),
            EmbeddingProvider::OpenAI | EmbeddingProvider::LlamaCpp => json!({
                "model": config.model,
                "input": description,
                "encoding_format": "float"
            }),
            EmbeddingProvider::Moonshot => json!({
                "model": config.model,
                "input": description
            }),
            EmbeddingProvider::Ollama => json!({
                "model": config.model,
                "input": description
            }),
            EmbeddingProvider::HuggingFace => json!({
                "inputs": description
            }),
        };

        let (_status, resp_text) = async {
            const MAX_RETRIES: u32 = 3;
            const RETRY_BACKOFF_BASE_MS: u64 = 1000;
            // Per-attempt HTTP timeout. Without this a hung TCP connection
            // (firewall drops, dead proxy, server-side deadlock) can block
            // the entire embedding pipeline for minutes. 30s is generous
            // for any real remote embedder (HF API usually responds in 2-5s).
            const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
            for attempt in 0..MAX_RETRIES {
                let req = self.http.post(&config.endpoint).json(&body);
                let req = if config.provider != EmbeddingProvider::Ollama && !api_key.is_empty() {
                    req.header("Authorization", format!("Bearer {}", api_key))
                } else {
                    req
                };

                match tokio::time::timeout(REQUEST_TIMEOUT, req.send()).await {
                    Ok(Ok(resp)) => {
                        let status = resp.status();
                        let text = resp.text().await.unwrap_or_default();
                        if status.is_success() {
                            return Ok((status, text));
                        }
                        // Fast-fail on 401/403 — retrying bad credentials will never succeed
                        let sc = status.as_u16();
                        if sc == 401 || sc == 403 {
                            anyhow::bail!(
                                "embedding endpoint returned {} (auth error): {}",
                                sc,
                                &text[..200.min(text.len())]
                            );
                        }
                        if sc == 429 && attempt + 1 < MAX_RETRIES {
                            let delay = std::time::Duration::from_millis(RETRY_BACKOFF_BASE_MS * 2u64.pow(attempt));
                            if self.verbose {
                                eprintln!(
                                    "[embed] {:?} rate limited (429), retrying in {:?}",
                                    config.provider, delay
                                );
                            }
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        anyhow::bail!(
                            "embedding endpoint returned {}: {}",
                            sc,
                            &text[..200.min(text.len())]
                        );
                    }
                    Ok(Err(e)) => {
                        // Network-level failure (connect refused, DNS, TLS, etc.)
                        if attempt + 1 < MAX_RETRIES && (e.is_timeout() || e.is_connect()) {
                            let delay = std::time::Duration::from_millis(RETRY_BACKOFF_BASE_MS * 2u64.pow(attempt));
                            if self.verbose {
                                eprintln!("[embed] {:?} connection issue (attempt {}/{}), retrying in {:?}: {}", config.provider, attempt + 1, MAX_RETRIES, delay, e);
                            }
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        let msg = format!(
                            "failed to call embedding endpoint ({}): {}",
                            config.endpoint, e
                        );
                        anyhow::bail!(msg);
                    }
                    Err(_elapsed) => {
                        // Per-attempt timeout (REQUEST_TIMEOUT). Treat the same
                        // as a connection timeout — retryable up to MAX_RETRIES.
                        if attempt + 1 < MAX_RETRIES {
                            let delay = std::time::Duration::from_millis(RETRY_BACKOFF_BASE_MS * 2u64.pow(attempt));
                            if self.verbose {
                                eprintln!(
                                    "[embed] {:?} request timeout after {:?} (attempt {}/{}), retrying in {:?}",
                                    config.provider,
                                    REQUEST_TIMEOUT,
                                    attempt + 1,
                                    MAX_RETRIES,
                                    delay
                                );
                            }
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        anyhow::bail!(
                            "{:?} embedding timed out after {} attempts ({}s each)",
                            config.provider,
                            MAX_RETRIES,
                            REQUEST_TIMEOUT.as_secs()
                        );
                    }
                }
            }
            anyhow::bail!(
                "{:?} embedding failed after {} attempts",
                config.provider,
                MAX_RETRIES
            )
        }
        .await?;

        let response: serde_json::Value =
            serde_json::from_str(&resp_text).context("failed to decode embedding response")?;

        let coords: Vec<f32> = serde_json::from_value(response["data"][0]["embedding"].clone())
            .or_else(|_| {
                serde_json::from_value(response["embeddings"][0].clone())
            })
            .or_else(|_| {
                serde_json::from_value(response.clone())
            })
            .context("embedding response did not include data[0].embedding, embeddings[0], or a flat array")?;

        Ok(normalize_dims(coords))
    }
}

fn normalize_dims(mut coords: Vec<f32>) -> Vec<f32> {
    let dim = embedding_dimension();
    if coords.len() < dim {
        coords.resize(dim, 0.0);
    } else if coords.len() > dim {
        coords.truncate(dim);
    }
    coords
}

fn truncate_description(description: &str) -> &str {
    if description.len() > 512 {
        let mut idx = 512;
        while !description.is_char_boundary(idx) && idx > 0 {
            idx -= 1;
        }
        &description[..idx]
    } else {
        description
    }
}

pub fn deterministic_placeholder_embedding(input: &str) -> Vec<f32> {
    let dim = embedding_dimension();
    let mut out = Vec::with_capacity(dim);
    let mut seed = Sha256::digest(input.as_bytes()).to_vec();

    while out.len() < dim {
        let digest = Sha256::digest(&seed);
        for chunk in digest.chunks(4) {
            if out.len() == dim {
                break;
            }
            let mut bytes = [0u8; 4];
            bytes[..chunk.len()].copy_from_slice(chunk);
            let raw = u32::from_be_bytes(bytes);
            let normalized = (raw as f32 / u32::MAX as f32) * 2.0 - 1.0;
            out.push(normalized);
        }
        seed = digest.to_vec();
    }

    out
}

pub fn vector_literal(coords: &[f32]) -> String {
    let body = coords
        .iter()
        .map(|v| format!("{:.8}", v))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_is_deterministic() {
        let a = deterministic_placeholder_embedding("hello world");
        let b = deterministic_placeholder_embedding("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn test_deterministic_different_inputs() {
        let a = deterministic_placeholder_embedding("hello");
        let b = deterministic_placeholder_embedding("world");
        assert_ne!(a, b);
    }

    #[test]
    fn test_deterministic_dimensions() {
        let emb = deterministic_placeholder_embedding("test");
        assert_eq!(emb.len(), embedding_dimension());
    }

    #[test]
    fn test_deterministic_normalized_range() {
        let emb = deterministic_placeholder_embedding("test");
        for v in &emb {
            assert!(*v >= -1.0 && *v <= 1.0, "value {} out of range", v);
        }
    }

    #[test]
    fn test_vector_literal_format() {
        let coords = vec![0.5_f32, -0.25_f32, 0.125_f32];
        let literal = vector_literal(&coords);
        assert!(literal.starts_with('['));
        assert!(literal.ends_with(']'));
        assert!(literal.contains("0.50000000"), "literal was: {}", literal);
        assert!(literal.contains("-0.25000000"), "literal was: {}", literal);
        assert!(literal.contains("0.12500000"), "literal was: {}", literal);
    }

    #[tokio::test]
    async fn test_embed_with_single_provider_fallback_on_bad_key() {
        let client = EmbeddingClient::with_provider(
            Some("your_nvidia_api_key_here".to_string()),
            "nvidia/llama-nemotron-embed-1b-v2".to_string(),
            EmbeddingProvider::Nvidia,
            "https://integrate.api.nvidia.com/v1/embeddings".to_string(),
        );
        let result = client
            .embed_description("test")
            .await
            .expect("must return embedding");
        assert_eq!(result.len(), embedding_dimension());
    }

    #[tokio::test]
    async fn test_empty_providers_fallback() {
        #[cfg(feature = "tools-local-embeddings")]
        let client = EmbeddingClient {
            http: crate::http_client(),
            providers: vec![],
            verbose: false,
            local: None,
        };
        #[cfg(not(feature = "tools-local-embeddings"))]
        let client = EmbeddingClient {
            http: crate::http_client(),
            providers: vec![],
            verbose: false,
        };
        let result = client
            .embed_description("test")
            .await
            .expect("must return embedding");
        assert_eq!(result.len(), embedding_dimension());
    }

    #[tokio::test]
    async fn test_embed_falls_back_when_remote_provider_fails() {
        let client = EmbeddingClient::with_provider(
            Some("sk-real-key-12345".to_string()),
            "text-embedding-3-small".to_string(),
            EmbeddingProvider::OpenAI,
            "https://api.openai.com/v1/embeddings".to_string(),
        );
        let result = client
            .embed_description("test")
            .await
            .expect("must return embedding via fallback");
        assert_eq!(result.len(), embedding_dimension());
    }
}
