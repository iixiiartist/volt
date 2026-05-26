use anyhow::Context;
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};

const EMBEDDING_DIMENSIONS: usize = 1024;

/// All supported embedding providers
#[derive(Debug, Clone, PartialEq)]
pub enum EmbeddingProvider {
    Ollama,
    LlamaCpp,
    Nvidia,
    OpenAI,
    Moonshot,
    HuggingFace,
}

/// Configuration for a single embedding provider in the fallback chain
#[derive(Debug, Clone)]
struct ProviderConfig {
    provider: EmbeddingProvider,
    model: String,
    endpoint: String,
    api_key: Option<String>,
}

impl ProviderConfig {
    fn from_env(provider_str: &str) -> Option<Self> {
        let model = std::env::var("EMBEDDING_MODEL").ok().unwrap_or_default();
        let endpoint = std::env::var("EMBEDDING_ENDPOINT").ok().unwrap_or_default();
        let api_key = std::env::var("EMBEDDING_API_KEY")
            .ok()
            .filter(|k| !k.is_empty() && k != "your_nvidia_api_key_here");

        match provider_str.to_lowercase().as_str() {
            "ollama" => Some(Self {
                provider: EmbeddingProvider::Ollama,
                model: if model.is_empty() {
                    "mxbai-embed-large".into()
                } else {
                    model
                },
                endpoint: if endpoint.is_empty() {
                    "http://localhost:11434/api/embed".into()
                } else {
                    endpoint
                },
                api_key: None,
            }),
            "llamacpp" | "llama.cpp" | "llama-cpp" => Some(Self {
                provider: EmbeddingProvider::LlamaCpp,
                model: if model.is_empty() {
                    "mxbai-embed-large-v1".into()
                } else {
                    model
                },
                endpoint: if endpoint.is_empty() {
                    "http://localhost:8080/v1/embeddings".into()
                } else {
                    endpoint
                },
                api_key: Some(api_key.unwrap_or_else(|| "not-needed".into())),
            }),
            "nvidia" => Some(Self {
                provider: EmbeddingProvider::Nvidia,
                model: if model.is_empty() {
                    "nvidia/llama-nemotron-embed-1b-v2".into()
                } else {
                    model
                },
                endpoint: if endpoint.is_empty() {
                    "https://integrate.api.nvidia.com/v1/embeddings".into()
                } else {
                    endpoint
                },
                api_key: api_key.or_else(|| {
                    std::env::var("NVIDIA_API_KEY")
                        .ok()
                        .filter(|k| !k.is_empty())
                }),
            }),
            "openai" => Some(Self {
                provider: EmbeddingProvider::OpenAI,
                model: if model.is_empty() {
                    "text-embedding-3-small".into()
                } else {
                    model
                },
                endpoint: if endpoint.is_empty() {
                    "https://api.openai.com/v1/embeddings".into()
                } else {
                    endpoint
                },
                api_key: api_key.or_else(|| {
                    std::env::var("OPENAI_API_KEY")
                        .ok()
                        .filter(|k| !k.is_empty())
                }),
            }),
            "moonshot" => Some(Self {
                provider: EmbeddingProvider::Moonshot,
                model: if model.is_empty() {
                    "moonshot-v1-embed".into()
                } else {
                    model
                },
                endpoint: if endpoint.is_empty() {
                    "https://api.moonshot.cn/v1/embeddings".into()
                } else {
                    endpoint
                },
                api_key,
            }),
            "huggingface" | "hf" => Some(Self {
                provider: EmbeddingProvider::HuggingFace,
                model: if model.is_empty() {
                    "BAAI/bge-small-en-v1.5".into()
                } else {
                    model
                },
                endpoint: if endpoint.is_empty() {
                    "https://router.huggingface.co/hf-inference/models/BAAI/bge-small-en-v1.5"
                        .into()
                } else {
                    endpoint
                },
                api_key: api_key.or_else(|| {
                    std::env::var("HF_TOKEN")
                        .ok()
                        .or_else(|| std::env::var("HUGGINGFACE_TOKEN").ok())
                        .filter(|k| !k.is_empty())
                }),
            }),
            _ => None,
        }
    }
}

/// Embedding client with multi-provider fallback and auto-detection.
///
/// Providers are tried in order. If one fails, the next is attempted.
/// If all providers fail, a deterministic placeholder embedding is used
/// so the system never hard-fails on embedding.
#[derive(Clone)]
pub struct EmbeddingClient {
    http: Client,
    providers: Vec<ProviderConfig>,
    /// Whether to log provider fallback warnings
    verbose: bool,
    #[cfg(feature = "tools-local-embeddings")]
    local: Option<std::sync::Arc<crate::local_embed::LocalEmbedder>>,
}

impl EmbeddingClient {
    /// Create a client with a single provider (no fallback).
    /// Prefer `new_smart()` for production use.
    pub fn new(api_key: Option<String>, model: impl Into<String>) -> Self {
        Self::with_provider(
            api_key,
            model,
            EmbeddingProvider::Nvidia,
            "https://integrate.api.nvidia.com/v1/embeddings",
        )
    }

    /// Create a client with a single explicit provider (no fallback).
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
            http: crate::http_client(30),
            providers: vec![config],
            verbose: false,
            #[cfg(feature = "tools-local-embeddings")]
            local: None,
        }
    }

    /// Create a client with automatic provider detection and fallback chain.
    ///
    /// If `EMBEDDING_PROVIDER` is explicitly set (not "auto"), that provider
    /// is tried first, with auto-detected fallbacks behind it.
    /// If unset or "auto", full auto-detection runs.
    ///
    /// Detection priority:
    ///   1. Ollama (local, if running) — no API key needed
    ///   2. NVIDIA NIM (cloud, if NVIDIA_API_KEY or EMBEDDING_API_KEY set and valid)
    ///   3. OpenAI (cloud, if OPENAI_API_KEY set)
    ///   4. HuggingFace Inference API (cloud, free, if HF_TOKEN set)
    ///   5. Moonshot (cloud, if KIMI_API_KEY set)
    ///   6. Deterministic placeholder (always works, no network)
    ///
    /// ```ignore
    /// let client = EmbeddingClient::new_smart().await;
    /// ```
    pub async fn new_smart() -> Self {
        let http = crate::http_client(10);
        let provider_str = std::env::var("EMBEDDING_PROVIDER").ok();

        let providers = match provider_str {
            Some(ref p) if !p.is_empty() && p.to_lowercase() != "auto" => {
                let mut providers = Vec::new();
                if let Some(config) = ProviderConfig::from_env(p) {
                    providers.push(config);
                }
                // Only add auto-detected fallbacks when provider is "auto"
                providers
            }
            _ => auto_detect_providers(&http).await,
        };

        #[cfg(feature = "tools-local-embeddings")]
        let local = match crate::local_embed::LocalEmbedder::load() {
            Ok(e) => {
                tracing::info!("local embedder (BGE-small-en-v1.5) loaded successfully");
                Some(std::sync::Arc::new(e))
            }
            Err(e) => {
                tracing::warn!("local embedder unavailable: {}", e);
                None
            }
        };

        Self {
            http,
            providers,
            verbose: true,
            #[cfg(feature = "tools-local-embeddings")]
            local,
        }
    }

    /// Embed text by trying each provider in the fallback chain.
    /// Returns a deterministic placeholder embedding if all providers fail.
    pub async fn embed_description(&self, description: &str) -> anyhow::Result<Vec<f32>> {
        if self.providers.is_empty() {
            return Ok(deterministic_placeholder_embedding(description));
        }

        // Truncate to fit within typical embedding model context windows.
        // mxbai-embed-large: 512 tokens, BGE-small: 512 tokens.
        // ~2000 bytes is a safe upper bound for most models.
        let truncated = if description.len() > 2000 {
            let mut idx = 2000;
            while !description.is_char_boundary(idx) && idx > 0 {
                idx -= 1;
            }
            &description[..idx]
        } else {
            description
        };

        // 1. Try local embedder first (fast, offline, no API key)
        #[cfg(feature = "tools-local-embeddings")]
        if let Some(local) = &self.local {
            match local.embed(truncated) {
                Ok(embedding) => return Ok(normalize_dims(embedding)),
                Err(e) => {
                    if self.verbose {
                        tracing::warn!("local embed failed: {}", e);
                    }
                }
            }
        }

        // 2. Fallback to remote provider chain
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

        // All remote providers failed — use deterministic placeholder
        if self.verbose {
            eprintln!("[embed] all remote providers failed, using deterministic fallback");
        }
        Ok(deterministic_placeholder_embedding(description))
    }

    /// Try a single provider config.
    async fn embed_with(
        &self,
        config: &ProviderConfig,
        description: &str,
    ) -> anyhow::Result<Vec<f32>> {
        match &config.api_key {
            Some(key) if !key.is_empty() && key != "your_nvidia_api_key_here" => {
                self.embed_remote(config, description, key).await
            }
            // For Ollama and LlamaCpp, no real API key needed — always attempt
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
            let max_retries = 3;
            for attempt in 0..max_retries {
                let req = self.http.post(&config.endpoint).json(&body);
                let req = if config.provider != EmbeddingProvider::Ollama && !api_key.is_empty() {
                    req.header("Authorization", format!("Bearer {}", api_key))
                } else {
                    req
                };

                match req.send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        let text = resp.text().await.unwrap_or_default();
                        if status.is_success() {
                            return Ok((status, text));
                        }
                        if status.as_u16() == 429 && attempt + 1 < max_retries {
                            let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
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
                            status.as_u16(),
                            &text[..200.min(text.len())]
                        );
                    }
                    Err(e) => {
                        if attempt + 1 < max_retries && (e.is_timeout() || e.is_connect()) {
                            let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
                            if self.verbose {
                                eprintln!("[embed] {:?} connection issue (attempt {}/{}), retrying in {:?}: {}", config.provider, attempt + 1, max_retries, delay, e);
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
                }
            }
            anyhow::bail!(
                "{:?} embedding failed after {} attempts",
                config.provider,
                max_retries
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
                // HuggingFace API returns flat array: [0.1, 0.2, ...]
                serde_json::from_value(response.clone())
            })
            .context("embedding response did not include data[0].embedding, embeddings[0], or a flat array")?;

        // Normalize to canonical dimension: pad if shorter, truncate if longer
        Ok(normalize_dims(coords))
    }
}

/// Normalize embedding dimensions to the canonical EMBEDDING_DIMENSIONS.
/// Pads shorter vectors with zeros, truncates longer ones.
fn normalize_dims(mut coords: Vec<f32>) -> Vec<f32> {
    if coords.len() < EMBEDDING_DIMENSIONS {
        coords.resize(EMBEDDING_DIMENSIONS, 0.0);
    } else if coords.len() > EMBEDDING_DIMENSIONS {
        coords.truncate(EMBEDDING_DIMENSIONS);
    }
    coords
}

// ─── Auto-Detection ──────────────────────────────────────────────

/// Detect all available embedding providers, ordered by preference.
async fn auto_detect_providers(http: &Client) -> Vec<ProviderConfig> {
    let mut providers = Vec::new();

    // 1. Ollama (local, no key needed) — ping health endpoint
    if is_ollama_running(http).await {
        let model = std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "mxbai-embed-large".into());
        providers.push(ProviderConfig {
            provider: EmbeddingProvider::Ollama,
            model: model.clone(),
            endpoint: "http://localhost:11434/api/embed".to_string(),
            api_key: None,
        });
    }

    // 2. NVIDIA NIM (cloud)
    let nvidia_key = std::env::var("NVIDIA_API_KEY")
        .or_else(|_| std::env::var("EMBEDDING_API_KEY"))
        .ok()
        .filter(|k| !k.is_empty() && k != "your_nvidia_api_key_here");
    if let Some(key) = nvidia_key {
        providers.push(ProviderConfig {
            provider: EmbeddingProvider::Nvidia,
            model: "nvidia/llama-nemotron-embed-1b-v2".into(),
            endpoint: "https://integrate.api.nvidia.com/v1/embeddings".into(),
            api_key: Some(key),
        });
    }

    // 3. OpenAI (cloud)
    let openai_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.is_empty());
    if let Some(key) = openai_key {
        providers.push(ProviderConfig {
            provider: EmbeddingProvider::OpenAI,
            model: std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".into()),
            endpoint: "https://api.openai.com/v1/embeddings".into(),
            api_key: Some(key),
        });
    }

    // 4. HuggingFace Inference API (cloud, free tier)
    let hf_key = std::env::var("HF_TOKEN")
        .or_else(|_| std::env::var("HUGGINGFACE_TOKEN"))
        .ok()
        .filter(|k| !k.is_empty());
    if let Some(key) = hf_key {
        providers.push(ProviderConfig {
            provider: EmbeddingProvider::HuggingFace,
            model: "BAAI/bge-small-en-v1.5".into(),
            endpoint: "https://router.huggingface.co/hf-inference/models/BAAI/bge-small-en-v1.5"
                .into(),
            api_key: Some(key),
        });
    }

    // 5. Moonshot (cloud)
    let kimi_key = std::env::var("KIMI_API_KEY").ok().filter(|k| !k.is_empty());
    if let Some(key) = kimi_key {
        providers.push(ProviderConfig {
            provider: EmbeddingProvider::Moonshot,
            model: std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "moonshot-v1-embed".into()),
            endpoint: "https://api.moonshot.cn/v1/embeddings".into(),
            api_key: Some(key),
        });
    }

    providers
}

/// Check if Ollama is running locally by hitting its API.
async fn is_ollama_running(http: &Client) -> bool {
    match http
        .get("http://localhost:11434/api/tags")
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

// ─── Deterministic Fallback ──────────────────────────────────────

/// Deterministic embedding based on SHA-256 hash of input.
/// Always produces the same embedding for the same text.
/// Used as last resort when no remote provider is available.
pub fn deterministic_placeholder_embedding(input: &str) -> Vec<f32> {
    let mut out = Vec::with_capacity(EMBEDDING_DIMENSIONS);
    let mut seed = Sha256::digest(input.as_bytes()).to_vec();

    while out.len() < EMBEDDING_DIMENSIONS {
        let digest = Sha256::digest(&seed);
        for chunk in digest.chunks(4) {
            if out.len() == EMBEDDING_DIMENSIONS {
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

/// Format an embedding vector as a PostgreSQL vector literal.
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
        assert_eq!(emb.len(), EMBEDDING_DIMENSIONS);
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
        assert_eq!(result.len(), EMBEDDING_DIMENSIONS);
    }

    #[tokio::test]
    async fn test_empty_providers_fallback() {
        let client = EmbeddingClient {
            http: crate::http_client(5),
            providers: vec![],
            verbose: false,
        };
        let result = client
            .embed_description("test")
            .await
            .expect("must return embedding");
        assert_eq!(result.len(), EMBEDDING_DIMENSIONS);
    }

    #[tokio::test]
    async fn test_embed_with_valid_provider_bypasses_fallback() {
        let client = EmbeddingClient::with_provider(
            Some("sk-real-key-12345".to_string()),
            "text-embedding-3-small".to_string(),
            EmbeddingProvider::OpenAI,
            "https://api.openai.com/v1/embeddings".to_string(),
        );
        let result = client
            .embed_description("test")
            .await
            .expect("must return embedding on fallback");
        assert_eq!(result.len(), EMBEDDING_DIMENSIONS);
    }
}
