use crate::embedding::EmbeddingProvider;
use crate::models::SandboxPolicy;
use std::env;
use std::io::Write;

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
    pub use_mtp: Option<bool>,
    pub use_cot: Option<bool>,
    pub allow_write: Option<bool>,
    pub framework: Option<String>,
    pub model_variant: Option<String>,
    pub quantization: Option<String>,
}

#[derive(Clone, serde::Deserialize)]
pub struct EmbeddingConfigSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
}

impl std::fmt::Debug for EmbeddingConfigSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingConfigSection")
            .field("model", &self.model)
            .field("provider", &self.provider)
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .finish()
    }
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
    let path = project_config_path();
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&content)
        .map_err(|e| {
            eprintln!("[config] warning: invalid .volt/config.toml: {}", e);
            e
        })
        .ok()
}

pub fn project_config_path() -> std::path::PathBuf {
    std::path::Path::new(".volt").join("config.toml")
}

/// Prompt the user for configuration if none exists.
/// Returns true if a config file was written.
pub fn first_run_wizard() -> bool {
    // Skip if config already exists
    if project_config_path().exists() {
        return false;
    }

    // Skip if essential env vars are already set
    let has_llm = std::env::var("LLM_MODEL").is_ok()
        || std::env::var("LLM_BASE_URL").is_ok()
        || std::env::var("LLM_API_KEY").is_ok()
        || std::env::var("ANTHROPIC_API_KEY").is_ok()
        || std::env::var("OPENAI_API_KEY").is_ok()
        || std::env::var("GROQ_API_KEY").is_ok();
    let has_db = std::env::var("DATABASE_URL").is_ok();
    if has_llm && has_db {
        return false;
    }

    // Skip if not a TTY (non-interactive)
    #[cfg(not(test))]
    {
        use crossterm::tty::IsTty;
        if !std::io::stdin().is_tty() {
            return false;
        }
    }

    println!("╔══════════════════════════════════════════════════╗");
    println!("║     Welcome to Volt — First-Time Setup          ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
    println!("No configuration found. Let's get you set up.");
    println!();

    // ── LLM Provider ──────────────────────────────────────────
    println!("Which LLM provider would you like to use?");
    println!("  1) Ollama (local, free) — requires Ollama running on your machine");
    println!("  2) Groq — fast cloud API (free tier available, needs GROQ_API_KEY)");
    println!("  3) OpenAI — needs OPENAI_API_KEY");
    println!("  4) Anthropic (Claude) — needs ANTHROPIC_API_KEY");
    println!("  5) NVIDIA NIM — needs NVIDIA_API_KEY or LLM_API_KEY");
    print!("Choice [1]: ");
    std::io::stdout().flush().ok();
    let mut choice = String::new();
    std::io::stdin().read_line(&mut choice).ok();
    let choice = choice.trim();

    let (model, base_url, api_key_env, provider, api_key_name) = match choice {
        "2" => {
            println!();
            print!("Groq model [llama-3.1-8b-instant]: ");
            std::io::stdout().flush().ok();
            let mut m = String::new();
            std::io::stdin().read_line(&mut m).ok();
            let m = if m.trim().is_empty() {
                "llama-3.1-8b-instant"
            } else {
                m.trim()
            };
            println!();
            print!("GROQ_API_KEY: ");
            std::io::stdout().flush().ok();
            let mut k = String::new();
            std::io::stdin().read_line(&mut k).ok();
            (
                m.to_string(),
                "https://api.groq.com/openai/v1".to_string(),
                Some(k.trim().to_string()),
                "groq",
                "GROQ_API_KEY",
            )
        }
        "3" => {
            println!();
            print!("OpenAI model [gpt-4o]: ");
            std::io::stdout().flush().ok();
            let mut m = String::new();
            std::io::stdin().read_line(&mut m).ok();
            let m = if m.trim().is_empty() {
                "gpt-4o"
            } else {
                m.trim()
            };
            println!();
            print!("OPENAI_API_KEY: ");
            std::io::stdout().flush().ok();
            let mut k = String::new();
            std::io::stdin().read_line(&mut k).ok();
            (
                m.to_string(),
                "https://api.openai.com/v1".to_string(),
                Some(k.trim().to_string()),
                "openai",
                "OPENAI_API_KEY",
            )
        }
        "4" => {
            println!();
            print!("Claude model [claude-sonnet-4-5]: ");
            std::io::stdout().flush().ok();
            let mut m = String::new();
            std::io::stdin().read_line(&mut m).ok();
            let m = if m.trim().is_empty() {
                "claude-sonnet-4-5"
            } else {
                m.trim()
            };
            println!();
            print!("ANTHROPIC_API_KEY: ");
            std::io::stdout().flush().ok();
            let mut k = String::new();
            std::io::stdin().read_line(&mut k).ok();
            (
                m.to_string(),
                "https://api.anthropic.com".to_string(),
                Some(k.trim().to_string()),
                "anthropic",
                "ANTHROPIC_API_KEY",
            )
        }
        "5" => {
            println!();
            print!("NVIDIA NIM model [nvidia/llama-nemotron-embed-1b-v2]: ");
            std::io::stdout().flush().ok();
            let mut m = String::new();
            std::io::stdin().read_line(&mut m).ok();
            let m = if m.trim().is_empty() {
                "nvidia/llama-nemotron-embed-1b-v2"
            } else {
                m.trim()
            };
            println!();
            print!("NVIDIA_API_KEY (or LLM_API_KEY): ");
            std::io::stdout().flush().ok();
            let mut k = String::new();
            std::io::stdin().read_line(&mut k).ok();
            (
                m.to_string(),
                "https://integrate.api.nvidia.com/v1".to_string(),
                Some(k.trim().to_string()),
                "nvidia",
                "NVIDIA_API_KEY",
            )
        }
        _ => {
            println!();
            print!("Ollama model [phi4-mini:3.8b]: ");
            std::io::stdout().flush().ok();
            let mut m = String::new();
            std::io::stdin().read_line(&mut m).ok();
            let m = if m.trim().is_empty() {
                "phi4-mini:3.8b"
            } else {
                m.trim()
            };
            println!();
            print!("Ollama base URL [http://localhost:11434/v1]: ");
            std::io::stdout().flush().ok();
            let mut u = String::new();
            std::io::stdin().read_line(&mut u).ok();
            let u = if u.trim().is_empty() {
                "http://localhost:11434/v1"
            } else {
                u.trim()
            };
            (m.to_string(), u.to_string(), None, "ollama", "LLM_BASE_URL")
        }
    };

    // ── Embedding Provider ────────────────────────────────────
    println!();
    println!("Embedding provider (used for skill/memory search):");
    println!("  1) Ollama (local) — requires embedding model pulled");
    println!("  2) NVIDIA NIM (cloud, free tier)");
    print!("Choice [1]: ");
    std::io::stdout().flush().ok();
    let mut emb_choice = String::new();
    std::io::stdin().read_line(&mut emb_choice).ok();
    let (emb_model, emb_provider, emb_endpoint) = match emb_choice.trim() {
        "2" => (
            "nvidia/llama-nemotron-embed-1b-v2".to_string(),
            "nvidia".to_string(),
            "https://integrate.api.nvidia.com/v1/embeddings".to_string(),
        ),
        _ => (
            "mxbai-embed-large".to_string(),
            "ollama".to_string(),
            "http://localhost:11434/v1".to_string(),
        ),
    };

    // ── Database ──────────────────────────────────────────────
    println!();
    println!("Database URL (Volt needs PostgreSQL 16+ with pgvector).");
    println!("  • Local install: postgres://volt:volt@localhost:5432/volt");
    println!("  • Docker Compose: postgres://volt:volt@localhost:5432/volt");
    println!("  • Cloud: postgres://user:pass@host:5432/db");
    print!("DATABASE_URL [postgres://volt:volt@localhost:5432/volt]: ");
    std::io::stdout().flush().ok();
    let mut db_url = String::new();
    std::io::stdin().read_line(&mut db_url).ok();
    let db_url = if db_url.trim().is_empty() {
        "postgres://volt:volt@localhost:5432/volt"
    } else {
        db_url.trim()
    };

    // ── Write config ──────────────────────────────────────────
    let config_dir = std::path::Path::new(".volt");
    if !config_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(config_dir) {
            eprintln!("Warning: could not create .volt directory: {}", e);
            return false;
        }
    }

    let mut lines = Vec::new();

    // Agent section
    lines.push("[agent]".to_string());
    lines.push(format!("model = \"{}\"", model));
    lines.push(format!("provider = \"{}\"", provider));
    lines.push("max_iterations = 25".to_string());
    lines.push("temperature = 0.3".to_string());
    lines.push("use_mtp = false".to_string());
    lines.push("use_cot = false".to_string());
    lines.push("allow_write = false".to_string());
    lines.push(String::new());

    // Embedding section
    lines.push("[embedding]".to_string());
    lines.push(format!("model = \"{}\"", emb_model));
    lines.push(format!("provider = \"{}\"", emb_provider));
    lines.push(format!("endpoint = \"{}\"", emb_endpoint));
    lines.push(String::new());

    // Database section
    lines.push("[database]".to_string());
    lines.push(format!("url = \"{}\"", db_url));
    lines.push(String::new());

    let config_content = lines.join("\n");

    match std::fs::write(project_config_path(), &config_content) {
        Ok(_) => {
            println!();
            println!("✓ Configuration written to .volt/config.toml");

            // Write env vars to .env
            let env_path = std::path::Path::new(".env");

            // API key (if provided)
            if let Some(key) = api_key_env {
                let entry = format!("{}={}\n", api_key_name, key);
                if !env_path.exists() {
                    if std::fs::write(env_path, &entry).is_ok() {
                        println!("✓ {} saved to .env", api_key_name);
                    }
                } else {
                    let existing = std::fs::read_to_string(env_path).unwrap_or_default();
                    if !existing.contains(api_key_name) {
                        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(env_path) {
                            use std::io::Write;
                            let _ = writeln!(f, "{}={}", api_key_name, key);
                            println!("✓ {} appended to .env", api_key_name);
                        }
                    }
                }
            }

            // LLM_BASE_URL (for Ollama, when a custom base URL was provided)
            if provider == "ollama" && base_url != "http://localhost:11434/v1" {
                let entry = format!("LLM_BASE_URL={}\n", base_url);
                if !env_path.exists() {
                    let _ = std::fs::write(env_path, &entry);
                } else {
                    let existing = std::fs::read_to_string(env_path).unwrap_or_default();
                    if !existing.contains("LLM_BASE_URL") {
                        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(env_path) {
                            use std::io::Write;
                            let _ = writeln!(f, "LLM_BASE_URL={}", base_url);
                        }
                    }
                }
            }

            println!();
            println!("Run `volt init-db` to initialize the database schema.");
            println!("Run `volt agent-chat` to start an interactive session.");
            true
        }
        Err(e) => {
            eprintln!("Warning: could not write config: {}", e);
            false
        }
    }
}

#[derive(Clone)]
pub struct Settings {
    pub database_url: String,
    pub registry_base_url: String,
    pub registry_token: Option<String>,
    pub embedding_api_key: Option<String>,
    pub embedding_model: String,
    pub embedding_provider: EmbeddingProvider,
    pub embedding_endpoint: String,
    pub sandbox_policy: SandboxPolicy,
    pub use_mtp: bool,
    pub use_cot: bool,
    pub allow_write: bool,
    pub framework: Option<String>,
    pub model_variant: Option<String>,
    pub quantization: Option<String>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("database_url", &redact_url(&self.database_url))
            .field("registry_base_url", &self.registry_base_url)
            .field("registry_token", &redact_opt(&self.registry_token))
            .field("embedding_api_key", &redact_opt(&self.embedding_api_key))
            .field("embedding_model", &self.embedding_model)
            .field("embedding_provider", &self.embedding_provider)
            .field("embedding_endpoint", &self.embedding_endpoint)
            .field("sandbox_policy", &self.sandbox_policy)
            .finish()
    }
}

fn redact_opt(s: &Option<String>) -> &str {
    s.as_ref().map(|_| "***").unwrap_or("(none)")
}

fn redact_url(s: &str) -> String {
    if let Some(at) = s.find('@') {
        format!(
            "{}:***@{}",
            &s[..s.find("://").map(|i| i + 3).unwrap_or(0)],
            &s[at + 1..]
        )
    } else {
        s.to_string()
    }
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let project = load_project_config();

        let database_url = env::var("DATABASE_URL")
            .ok()
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.database.as_ref())
                    .and_then(|d| d.url.clone())
            })
            .ok_or_else(|| {
                anyhow::anyhow!("DATABASE_URL must be set (e.g. postgres://user:pass@host/db)")
            })?;
        let registry_base_url = env::var("VOLT_REGISTRY_BASE_URL")
            .unwrap_or_else(|_| "https://registry.voltagents.com/v1".to_string());
        let registry_token = env::var("VOLT_REGISTRY_TOKEN")
            .ok()
            .filter(|v| !v.is_empty());

        let embedding_api_key = env::var("EMBEDDING_API_KEY")
            .ok()
            .or_else(|| env::var("KIMI_API_KEY").ok())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.embedding.as_ref())
                    .and_then(|e| e.api_key.clone())
            })
            .filter(|v| !v.is_empty());
        let embedding_model = env::var("EMBEDDING_MODEL")
            .ok()
            .or_else(|| env::var("KIMI_EMBEDDING_MODEL").ok())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.embedding.as_ref())
                    .and_then(|e| e.model.clone())
            })
            .unwrap_or_else(|| "nvidia/llama-nemotron-embed-1b-v2".to_string());
        let embedding_endpoint = env::var("EMBEDDING_ENDPOINT")
            .ok()
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.embedding.as_ref())
                    .and_then(|e| e.endpoint.clone())
            })
            .unwrap_or_else(|| "https://integrate.api.nvidia.com/v1/embeddings".to_string());
        let embedding_provider_str = env::var("EMBEDDING_PROVIDER")
            .ok()
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.embedding.as_ref())
                    .and_then(|e| e.provider.clone())
            })
            .unwrap_or_else(|| "nvidia".to_string())
            .to_lowercase();
        let embedding_provider = match embedding_provider_str.as_str() {
            "moonshot" => EmbeddingProvider::Moonshot,
            "ollama" => EmbeddingProvider::Ollama,
            "openai" => EmbeddingProvider::OpenAI,
            "huggingface" => EmbeddingProvider::HuggingFace,
            "llamacpp" => EmbeddingProvider::LlamaCpp,
            "nvidia" => EmbeddingProvider::Nvidia,
            _ => EmbeddingProvider::Nvidia,
        };

        let timeout_ms = env::var("VOLT_SANDBOX_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.sandbox.as_ref())
                    .and_then(|s| s.timeout_ms)
            })
            .unwrap_or(5000);
        let max_stdout_bytes = env::var("VOLT_SANDBOX_MAX_STDOUT_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.sandbox.as_ref())
                    .and_then(|s| s.max_stdout_bytes)
            })
            .unwrap_or(262_144);

        let use_mtp = env::var("VOLT_USE_MTP")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.agent.as_ref())
                    .and_then(|a| a.use_mtp)
            })
            .unwrap_or(false);

        let use_cot = env::var("VOLT_USE_COT")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.agent.as_ref())
                    .and_then(|a| a.use_cot)
            })
            .unwrap_or(false);

        let allow_write = env::var("VOLT_ALLOW_WRITE")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|p| p.agent.as_ref())
                    .and_then(|a| a.allow_write)
            })
            .unwrap_or(false);

        let framework = env::var("VOLT_FRAMEWORK").ok().or_else(|| {
            project
                .as_ref()
                .and_then(|p| p.agent.as_ref())
                .and_then(|a| a.framework.clone())
        });

        let model_variant = env::var("VOLT_MODEL_VARIANT").ok().or_else(|| {
            project
                .as_ref()
                .and_then(|p| p.agent.as_ref())
                .and_then(|a| a.model_variant.clone())
        });

        let quantization = env::var("VOLT_QUANTIZATION").ok().or_else(|| {
            project
                .as_ref()
                .and_then(|p| p.agent.as_ref())
                .and_then(|a| a.quantization.clone())
        });

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
            use_mtp,
            use_cot,
            allow_write,
            framework,
            model_variant,
            quantization,
        })
    }
}
