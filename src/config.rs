use crate::embedding::EmbeddingProvider;
use crate::models::SandboxPolicy;
use std::env;
use std::path::Path;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ProjectConfig {
    pub agent: Option<AgentConfigSection>,
    pub embedding: Option<EmbeddingConfigSection>,
    pub database: Option<DatabaseConfigSection>,
    pub sandbox: Option<SandboxConfigSection>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentConfigSection {
    pub name: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub system_prompt: Option<String>,
    pub max_iterations: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EmbeddingConfigSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DatabaseConfigSection {
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SandboxConfigSection {
    pub timeout_ms: Option<u64>,
    pub max_stdout_bytes: Option<usize>,
}

pub fn load_project_config() -> Option<ProjectConfig> {
    let path = Path::new(".volt").join("config.toml");
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&content).ok()
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub database_url: String,
    pub registry_base_url: String,
    pub registry_token: Option<String>,
    pub embedding_api_key: Option<String>,
    pub embedding_model: String,
    pub embedding_provider: EmbeddingProvider,
    pub embedding_endpoint: String,
    pub sandbox_policy: SandboxPolicy,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let project = load_project_config();

        let database_url = env::var("DATABASE_URL").ok()
            .or_else(|| project.as_ref().and_then(|p| p.database.as_ref()).and_then(|d| d.url.clone()))
            .unwrap_or_else(|| "postgres://volt:volt@localhost:5432/volt".to_string());
        let registry_base_url = env::var("VOLT_REGISTRY_BASE_URL")
            .unwrap_or_else(|_| "https://registry.voltagents.com/v1".to_string());
        let registry_token = env::var("VOLT_REGISTRY_TOKEN").ok().filter(|v| !v.is_empty());

        let embedding_api_key = env::var("EMBEDDING_API_KEY").ok()
            .or_else(|| env::var("KIMI_API_KEY").ok())
            .or_else(|| project.as_ref().and_then(|p| p.embedding.as_ref()).and_then(|e| e.api_key.clone()))
            .filter(|v| !v.is_empty());
        let embedding_model = env::var("EMBEDDING_MODEL").ok()
            .or_else(|| env::var("KIMI_EMBEDDING_MODEL").ok())
            .or_else(|| project.as_ref().and_then(|p| p.embedding.as_ref()).and_then(|e| e.model.clone()))
            .unwrap_or_else(|| "nvidia/llama-nemotron-embed-1b-v2".to_string());
        let embedding_endpoint = env::var("EMBEDDING_ENDPOINT").ok()
            .or_else(|| project.as_ref().and_then(|p| p.embedding.as_ref()).and_then(|e| e.endpoint.clone()))
            .unwrap_or_else(|| "https://integrate.api.nvidia.com/v1/embeddings".to_string());
        let embedding_provider_str = env::var("EMBEDDING_PROVIDER").ok()
            .or_else(|| project.as_ref().and_then(|p| p.embedding.as_ref()).and_then(|e| e.provider.clone()))
            .unwrap_or_else(|| "nvidia".to_string())
            .to_lowercase();
        let embedding_provider = match embedding_provider_str.as_str() {
            "moonshot" => EmbeddingProvider::Moonshot,
            "ollama" => EmbeddingProvider::Ollama,
            _ => EmbeddingProvider::Nvidia,
        };

        let timeout_ms = env::var("VOLT_SANDBOX_TIMEOUT_MS").ok()
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| project.as_ref().and_then(|p| p.sandbox.as_ref()).and_then(|s| s.timeout_ms))
            .unwrap_or(5000);
        let max_stdout_bytes = env::var("VOLT_SANDBOX_MAX_STDOUT_BYTES").ok()
            .and_then(|v| v.parse::<usize>().ok())
            .or_else(|| project.as_ref().and_then(|p| p.sandbox.as_ref()).and_then(|s| s.max_stdout_bytes))
            .unwrap_or(262_144);

        Ok(Self {
            database_url,
            registry_base_url,
            registry_token,
            embedding_api_key,
            embedding_model,
            embedding_provider,
            embedding_endpoint,
            sandbox_policy: SandboxPolicy {
                timeout_ms,
                max_stdout_bytes,
                working_dir: None,
            },
        })
    }
}