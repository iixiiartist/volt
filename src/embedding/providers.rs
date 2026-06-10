use super::EmbeddingProvider;
use reqwest::Client;

#[derive(Debug, Clone)]
pub(crate) struct ProviderConfig {
    pub(crate) provider: EmbeddingProvider,
    pub(crate) model: String,
    pub(crate) endpoint: String,
    pub(crate) api_key: Option<String>,
}

impl ProviderConfig {
    pub(crate) fn from_env(provider_str: &str) -> Option<Self> {
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
                    "BAAI/bge-large-en-v1.5".into()
                } else {
                    model
                },
                endpoint: if endpoint.is_empty() {
                    "https://router.huggingface.co/hf-inference/models/BAAI/bge-large-en-v1.5"
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

pub(crate) async fn auto_detect_providers(http: &Client) -> Vec<ProviderConfig> {
    let mut providers = Vec::new();

    if is_ollama_running(http).await {
        let model = std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "mxbai-embed-large".into());
        let endpoint = std::env::var("EMBEDDING_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434/api/embed".into());
        providers.push(ProviderConfig {
            provider: EmbeddingProvider::Ollama,
            model: model.clone(),
            endpoint,
            api_key: None,
        });
    }

    // NOTE: Ollama Cloud does NOT support embeddings (confirmed by maintainer Mar 2026).
    // Use local ONNX BGE, NVIDIA NIM, OpenAI, or HF instead.

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

    let hf_key = std::env::var("HF_TOKEN")
        .or_else(|_| std::env::var("HUGGINGFACE_TOKEN"))
        .ok()
        .filter(|k| !k.is_empty());
    if let Some(key) = hf_key {
        providers.push(ProviderConfig {
            provider: EmbeddingProvider::HuggingFace,
            model: "BAAI/bge-large-en-v1.5".into(),
            endpoint: "https://router.huggingface.co/hf-inference/models/BAAI/bge-large-en-v1.5"
                .into(),
            api_key: Some(key),
        });
    }

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

async fn is_ollama_running(http: &Client) -> bool {
    match http
        .get("http://localhost:11434/api/tags")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => {
            if !resp.status().is_success() {
                return false;
            }
            // Verify the response body looks like Ollama (has "models" key)
            resp.text()
                .await
                .ok()
                .is_some_and(|body| body.contains("models"))
        }
        Err(_) => false,
    }
}
