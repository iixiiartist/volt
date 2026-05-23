use anyhow::Context;
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};

const EMBEDDING_DIMENSIONS: usize = 1024;

#[derive(Clone)]
pub struct EmbeddingClient {
    http: Client,
    api_key: Option<String>,
    model: String,
    provider: EmbeddingProvider,
    endpoint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EmbeddingProvider {
    Moonshot,
    Nvidia,
    Ollama,
}

impl EmbeddingClient {
    pub fn new(api_key: Option<String>, model: impl Into<String>) -> Self {
        Self::with_provider(api_key, model, EmbeddingProvider::Nvidia, "https://integrate.api.nvidia.com/v1/embeddings")
    }

    pub fn with_provider(
        api_key: Option<String>,
        model: impl Into<String>,
        provider: EmbeddingProvider,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            http: crate::http_client(30),
            api_key,
            model: model.into(),
            provider,
            endpoint: endpoint.into(),
        }
    }

    pub async fn embed_description(&self, description: &str) -> anyhow::Result<Vec<f32>> {
        match &self.api_key {
            Some(key) => self.embed_remote(description, key).await,
            None => Ok(deterministic_placeholder_embedding(description)),
        }
    }

    async fn embed_remote(&self, description: &str, api_key: &str) -> anyhow::Result<Vec<f32>> {
        let body = match self.provider {
            EmbeddingProvider::Nvidia => json!({
                "model": self.model,
                "input": description,
                "input_type": "query",
                "encoding_format": "float",
                "dimensions": 1024
            }),
            EmbeddingProvider::Moonshot => json!({
                "model": self.model,
                "input": description
            }),
            EmbeddingProvider::Ollama => json!({
                "model": self.model,
                "input": description
            }),
        };

        let mut req = self
            .http
            .post(&self.endpoint)
            .json(&body);

        if self.provider != EmbeddingProvider::Ollama {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }

        let response: serde_json::Value = req
            .send()
            .await
            .context("failed to call embedding endpoint")?
            .error_for_status()
            .context("embedding endpoint returned an error")?
            .json()
            .await
            .context("failed to decode embedding response")?;

        let coords: Vec<f32> = serde_json::from_value(response["data"][0]["embedding"].clone())
            .context("embedding response did not include data[0].embedding")?;

        Ok(coords)
    }
}

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

pub fn vector_literal(coords: &[f32]) -> String {
    let body = coords
        .iter()
        .map(|v| format!("{:.8}", v))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", body)
}