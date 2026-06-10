use crate::embedding::EmbeddingProvider;
use crate::models::SandboxPolicy;
use std::env;

use inquire::{Confirm, Password, Select, Text};

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

/// Load `.env` from the current directory and **force** its values to win
/// over the process environment, unlike `dotenvy::dotenv()` which only sets
/// variables that are not already present.
///
/// Reads `.env`, splits on the first `=`, and calls `std::env::set_var` for
/// every non-empty, non-comment line. This is the canonical pattern across
/// bench and test binaries; centralizing it here keeps the behavior in one
/// place (e.g. add proper quote-stripping later without touching every call
/// site).
pub fn load_dotenv_overriding() {
    let _ = dotenvy::dotenv();
    if let Ok(content) = std::fs::read_to_string(".env") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                std::env::set_var(k.trim(), v.trim());
            }
        }
    }
}

/// Walk the .env file (CWD or binary dir) and warn if any KEY in it
/// is shadowed by a different value already in the process env.
/// This is the silent-failure trap that bit us twice: `dotenvy::dotenv()`
/// does not override existing env vars, so a stale shell-set env var
/// will silently win over the .env value, and the user
/// gets a confusing 401.
///
/// Prints to stderr. Intended to be called once at startup, BEFORE any
/// other env-reading code runs, so the warning reaches the user even if
/// the LLM call later fails.
pub fn warn_on_env_shadowing() {
    let path = std::path::Path::new(".env");
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return, // no .env in CWD — fall back to binary dir
    };
    warn_on_env_shadowing_from_str(&content, ".env");
}

/// Variant that reads from the binary's directory. Used when CWD has
/// no .env. (See [`warn_on_env_shadowing`].)
pub fn warn_on_env_shadowing_from_binary_dir(bin_dir: &std::path::Path) {
    let path = bin_dir.join(".env");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return,
    };
    warn_on_env_shadowing_from_str(&content, &path.display().to_string());
}

fn warn_on_env_shadowing_from_str(content: &str, source: &str) {
    let mut any_shadowed = false;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Minimal KEY=VALUE parser. Doesn't handle inline comments or
        // multi-line quoted values — those are rare in .env and the
        // false-negative cost is low (we just miss a warning).
        let Some((key, file_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let file_value = file_value.trim().trim_matches('"').trim_matches('\'');
        if file_value.is_empty() || file_value.starts_with("your_") {
            continue; // placeholder or empty in .env — not a real conflict
        }
        // Compare against the process env. If the shell already has the
        // same key, check whether the value differs. If it does, the
        // shell value is silently winning.
        if let Ok(shell_value) = std::env::var(key) {
            if shell_value != file_value && !shell_value.is_empty() {
                if !any_shadowed {
                    eprintln!(
                        "[volt] WARNING: shell environment is overriding {} (from {})",
                        source, key
                    );
                    any_shadowed = true;
                }
                let shell_masked = if shell_value.len() > 4 {
                    format!("…{}", &shell_value[shell_value.len() - 4..])
                } else {
                    format!("({} chars)", shell_value.len())
                };
                let file_masked = if file_value.len() > 4 {
                    format!("…{}", &file_value[file_value.len() - 4..])
                } else {
                    format!("({} chars)", file_value.len())
                };
                eprintln!(
                    "  {}: shell={}  {}={}  →  using shell value. To use the .env value, run `Remove-Item Env:{}` (PowerShell) or `unset {}` (bash).",
                    key, shell_masked, source, file_masked, key, key
                );
            }
        }
    }
}

/// Path to the per-user Volt home directory (`~/.volt/` on Unix,
/// `%APPDATA%\volt\` on Windows). Created lazily — callers that need it
/// to exist should `create_dir_all` themselves.
pub fn volt_home() -> std::path::PathBuf {
    if let Some(mut p) = dirs::data_dir() {
        p.push("volt");
        return p;
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::Path::new(&home).join(".volt");
    }
    std::path::Path::new(".volt").to_path_buf()
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

    // ── LLM Provider (arrow-key picker) ───────────────────────
    #[derive(Clone)]
    struct WizardProvider {
        slug: &'static str,
        label: &'static str,
        default_model: &'static str,
        base_url: &'static str,
        key_env: Option<&'static str>,
    }
    const PROVIDERS: &[WizardProvider] = &[
        WizardProvider {
            slug: "ollama",
            label: "Ollama (local, free) — requires Ollama running on your machine",
            default_model: "phi4-mini:3.8b",
            base_url: "http://localhost:11434/v1",
            key_env: None,
        },
        WizardProvider {
            slug: "groq",
            label: "Groq — fast cloud API (free tier available, needs GROQ_API_KEY)",
            default_model: "llama-3.1-8b-instant",
            base_url: "https://api.groq.com/openai/v1",
            key_env: Some("GROQ_API_KEY"),
        },
        WizardProvider {
            slug: "openai",
            label: "OpenAI — needs OPENAI_API_KEY",
            default_model: "gpt-4o",
            base_url: "https://api.openai.com/v1",
            key_env: Some("OPENAI_API_KEY"),
        },
        WizardProvider {
            slug: "anthropic",
            label: "Anthropic (Claude) — needs ANTHROPIC_API_KEY",
            default_model: "claude-sonnet-4-5",
            base_url: "https://api.anthropic.com",
            key_env: Some("ANTHROPIC_API_KEY"),
        },
        WizardProvider {
            slug: "nvidia",
            label: "NVIDIA NIM — needs NVIDIA_API_KEY or LLM_API_KEY",
            default_model: "nvidia/llama-nemotron-embed-1b-v2",
            base_url: "https://integrate.api.nvidia.com/v1",
            key_env: Some("NVIDIA_API_KEY"),
        },
    ];

    let provider: WizardProvider = match Select::new(
        "Which LLM provider?",
        PROVIDERS.iter().map(|p| p.label).collect::<Vec<_>>(),
    )
    .with_help_message("↑↓ to choose, Enter to confirm")
    .prompt()
    {
        Ok(label) => PROVIDERS
            .iter()
            .find(|p| p.label == label)
            .cloned()
            .unwrap_or_else(|| PROVIDERS[0].clone()),
        Err(_) => {
            eprintln!("(setup cancelled)");
            return false;
        }
    };

    // Model name (with default)
    let model = match Text::new("Model")
        .with_default(provider.default_model)
        .with_help_message("Press Enter to accept the default")
        .prompt()
    {
        Ok(m) if !m.trim().is_empty() => m.trim().to_string(),
        Ok(_) => provider.default_model.to_string(),
        Err(_) => {
            eprintln!("(setup cancelled)");
            return false;
        }
    };

    // API key (masked) for cloud providers; optional base URL for local
    let (base_url, api_key_env, api_key_name) = if provider.slug == "ollama" {
        let url = match Text::new("Ollama base URL")
            .with_default(provider.base_url)
            .prompt()
        {
            Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
            Ok(_) => provider.base_url.to_string(),
            Err(_) => {
                eprintln!("(setup cancelled)");
                return false;
            }
        };
        (url, None, "LLM_BASE_URL")
    } else {
        let key = match Password::new(&format!("{} API key", provider.key_env.unwrap_or("LLM")))
            .without_confirmation()
            .with_help_message("input is masked; press Enter to confirm")
            .prompt()
        {
            Ok(k) => k,
            Err(_) => {
                eprintln!("(setup cancelled)");
                return false;
            }
        };
        (
            provider.base_url.to_string(),
            Some(key),
            provider.key_env.unwrap_or("LLM_API_KEY"),
        )
    };

    // ── Embedding Provider ────────────────────────────────────
    #[derive(Clone)]
    struct WizardEmbed {
        label: &'static str,
        model: &'static str,
        provider: &'static str,
        endpoint: &'static str,
    }
    const EMBEDS: &[WizardEmbed] = &[
        WizardEmbed {
            label: "Ollama (local) — requires embedding model pulled",
            model: "mxbai-embed-large",
            provider: "ollama",
            endpoint: "http://localhost:11434/v1",
        },
        WizardEmbed {
            label: "NVIDIA NIM (cloud, free tier)",
            model: "nvidia/llama-nemotron-embed-1b-v2",
            provider: "nvidia",
            endpoint: "https://integrate.api.nvidia.com/v1/embeddings",
        },
    ];

    let emb = match Select::new(
        "Embedding provider (used for skill/memory search)?",
        EMBEDS.iter().map(|e| e.label).collect::<Vec<_>>(),
    )
    .prompt()
    {
        Ok(label) => EMBEDS
            .iter()
            .find(|e| e.label == label)
            .cloned()
            .unwrap_or_else(|| EMBEDS[0].clone()),
        Err(_) => {
            eprintln!("(setup cancelled)");
            return false;
        }
    };

    // ── Database ──────────────────────────────────────────────
    let db_url = match Text::new("DATABASE_URL (Volt needs PostgreSQL 16+ with pgvector)")
        .with_default("postgres://volt:volt@localhost:5432/volt")
        .with_help_message("Press Enter for the default local URL")
        .prompt()
    {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        Ok(_) => "postgres://volt:volt@localhost:5432/volt".to_string(),
        Err(_) => {
            eprintln!("(setup cancelled)");
            return false;
        }
    };

    // ── Confirm before writing ────────────────────────────────
    println!();
    println!("Summary:");
    println!("  LLM:     {} ({})", provider.label, model);
    if let Some(ref k) = api_key_env {
        let masked: String = "•".repeat(k.chars().count().min(8));
        println!(
            "  API key: {} (saved to .env, masked: {}…)",
            api_key_name, masked
        );
    } else {
        println!("  Base URL: {}", base_url);
    }
    println!("  Embed:   {} ({})", emb.label, emb.model);
    println!("  DB:      {}", db_url);
    println!();
    let proceed = match Confirm::new("Write configuration to .volt/config.toml?")
        .with_default(true)
        .prompt()
    {
        Ok(b) => b,
        Err(_) => {
            eprintln!("(setup cancelled)");
            return false;
        }
    };
    if !proceed {
        println!("(aborted; no files written)");
        return false;
    }

    // Pull the values back out of the (label-driven) wizard for the file writer.
    let provider_slug = provider.slug;
    let emb_model = emb.model.to_string();
    let emb_provider = emb.provider.to_string();
    let emb_endpoint = emb.endpoint.to_string();

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
    lines.push(format!("provider = \"{}\"", provider_slug));
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
            if provider_slug == "ollama" && base_url != "http://localhost:11434/v1" {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `volt_home()` should return an absolute path on any system
    /// that has a working `%APPDATA%` (Windows) or `$XDG_DATA_HOME`
    /// (Linux/macOS). The fallback to `$HOME/.volt` is a last resort.
    #[test]
    fn volt_home_is_absolute_or_home_relative() {
        let p = volt_home();
        let s = p.to_string_lossy();
        if p.is_absolute() {
            return;
        }
        // Relative fallback: must start with a directory, not be the
        // bare `.volt` literal. The current implementation can return
        // `.volt` in the worst case — that's still acceptable.
        assert!(s.ends_with(".volt"), "got {}", s);
    }

    #[test]
    fn provider_env_var_known_slugs() {
        // Cloud providers always need a key.
        assert_eq!(provider_env_var("groq").as_deref(), Some("GROQ_API_KEY"));
        assert_eq!(
            provider_env_var("openai").as_deref(),
            Some("OPENAI_API_KEY")
        );
        assert_eq!(
            provider_env_var("anthropic").as_deref(),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(
            provider_env_var("nvidia").as_deref(),
            Some("NVIDIA_API_KEY")
        );
        // Ollama: key only when the user has set OLLAMA_API_KEY (cloud tier).
        std::env::remove_var("OLLAMA_API_KEY");
        assert_eq!(provider_env_var("ollama"), None);
        std::env::set_var("OLLAMA_API_KEY", "test_cloud_key");
        assert_eq!(
            provider_env_var("ollama").as_deref(),
            Some("OLLAMA_API_KEY")
        );
        std::env::remove_var("OLLAMA_API_KEY");
        // Local servers and the OpenAI override are mapped to their
        // host / base-URL env vars (not LLM_API_KEY).
        assert_eq!(
            provider_env_var("ollama_local").as_deref(),
            Some("OLLAMA_HOST")
        );
        assert_eq!(
            provider_env_var("llamacpp").as_deref(),
            Some("LLAMA_CPP_HOST")
        );
        assert_eq!(
            provider_env_var("litertlm").as_deref(),
            Some("LITERTLM_HOST")
        );
        assert_eq!(
            provider_env_var("oai_override").as_deref(),
            Some("LLM_BASE_URL")
        );
        // Truly unknown slug returns None — caller should ask the user
        // to pick a known provider.
        assert_eq!(provider_env_var("custom"), None);
    }

    #[test]
    fn default_model_for_provider_known_slugs() {
        for slug in ["groq", "openai", "anthropic", "nvidia", "nim", "ollama"] {
            assert!(
                !default_model_for_provider(slug).is_empty(),
                "default model for {} should be non-empty",
                slug
            );
        }
    }

    #[test]
    fn save_api_key_persists_and_idempotent() {
        // Use a sentinel env var that we can verify was set.
        let env_var =
            save_api_key("groq", "gsk_test_persistence_key_12345").expect("save should succeed");
        assert_eq!(env_var, "GROQ_API_KEY");
        // Visible in process env.
        assert_eq!(
            std::env::var("GROQ_API_KEY").ok().as_deref(),
            Some("gsk_test_persistence_key_12345")
        );
        // On disk.
        let home = volt_home();
        let path = home.join(".env");
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            body.contains("GROQ_API_KEY=gsk_test_persistence_key_12345"),
            "expected GROQ_API_KEY in {}, got {}",
            path.display(),
            body
        );
        // Overwrite (re-save with new key) — old line must be replaced.
        let _ = save_api_key("groq", "gsk_test_overwrite_key_67890");
        let body2 = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !body2.contains("gsk_test_persistence_key_12345"),
            "old key should be removed"
        );
        assert!(body2.contains("GROQ_API_KEY=gsk_test_overwrite_key_67890"));
        assert_eq!(
            std::env::var("GROQ_API_KEY").ok().as_deref(),
            Some("gsk_test_overwrite_key_67890")
        );
        // Cleanup: remove the line we wrote so this test is hermetic.
        let cleaned: Vec<String> = body2
            .lines()
            .filter(|l| !l.trim_start().starts_with("GROQ_API_KEY="))
            .map(String::from)
            .collect();
        let _ = std::fs::write(&path, cleaned.join("\n") + "\n");
        std::env::remove_var("GROQ_API_KEY");
    }

    #[test]
    fn save_api_key_rejects_empty() {
        let result = save_api_key("groq", "   ");
        assert!(result.is_err(), "empty/whitespace key should be rejected");
    }

    #[test]
    fn has_any_llm_key_round_trip() {
        // Start clean.
        for k in LLM_KEY_ENV_VARS {
            std::env::remove_var(k);
        }
        assert!(!has_any_llm_key());
        std::env::set_var("LLM_API_KEY", "x");
        assert!(has_any_llm_key());
        std::env::remove_var("LLM_API_KEY");
        assert!(!has_any_llm_key());
    }
}

// =============================================================================
// Setup-wizard helpers
// =============================================================================

/// The set of LLM env vars we recognize when checking whether any provider
/// is configured. Order matters only for diagnostics; the first match wins
/// when the runtime resolves a default provider.
pub const LLM_KEY_ENV_VARS: &[&str] = &[
    "GROQ_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "NVIDIA_API_KEY",
    "NVCF_API_KEY",
    "OLLAMA_API_KEY",
    "LLM_API_KEY",
];

/// True iff at least one LLM API key is visible in the process env. Used
/// by the webui to decide whether to show the first-run setup wizard.
pub fn has_any_llm_key() -> bool {
    LLM_KEY_ENV_VARS.iter().any(|k| {
        std::env::var(k)
            .ok()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

/// Map a provider slug to the env var the runtime reads for its API key.
/// Returns `None` for providers that don't take an API key (local servers
/// configured by host instead of key).
pub fn provider_env_var(slug: &str) -> Option<String> {
    match slug {
        "groq" => Some("GROQ_API_KEY".to_string()),
        "openai" => Some("OPENAI_API_KEY".to_string()),
        "anthropic" => Some("ANTHROPIC_API_KEY".to_string()),
        "nvidia" | "nim" => Some("NVIDIA_API_KEY".to_string()),
        "moonshot" => Some("MOONSHOT_API_KEY".to_string()),
        "ollama" => std::env::var("OLLAMA_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|_| "OLLAMA_API_KEY".to_string()),
        // Local servers: configured by host, not key. We map to a known
        // env var so the WebUI / CLI can show the user what to set, but
        // it's a host, not a key.
        "ollama_local" => Some("OLLAMA_HOST".to_string()),
        "llamacpp" => Some("LLAMA_CPP_HOST".to_string()),
        "litertlm" => Some("LITERTLM_HOST".to_string()),
        // Custom OpenAI-compatible endpoint: the override URL goes in
        // LLM_BASE_URL; the key (if any) goes in LLM_API_KEY.
        "oai_override" => Some("LLM_BASE_URL".to_string()),
        _ => None,
    }
}

/// Default model id for a given provider slug.
pub fn default_model_for_provider(slug: &str) -> &'static str {
    match slug {
        "groq" => "llama-3.1-8b-instant",
        "openai" => "gpt-4o-mini",
        "anthropic" => "claude-sonnet-4-5",
        "nvidia" | "nim" => "meta/llama-3.1-8b-instruct",
        "ollama" => "llama3.2:3b",
        _ => "llama-3.1-8b-instant",
    }
}

/// Persist an API key to `volt_home()/.env`, set it in the process
/// environment, and return the resolved env-var name. Idempotent: a
/// re-write replaces the previous value.
///
/// Rejects keys with control characters (newlines, tabs) — a key
/// pasted from a browser can include an accidental newline that would
/// otherwise silently break the .env parser.
pub fn save_api_key(provider_slug: &str, api_key: &str) -> anyhow::Result<String> {
    let env_var = provider_env_var(provider_slug)
        .ok_or_else(|| anyhow::anyhow!("provider '{}' has no key", provider_slug))?;
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        anyhow::bail!("API key is empty");
    }
    if api_key.chars().any(|c| c.is_control()) {
        anyhow::bail!(
            "API key contains a control character (newline, tab, etc.). \
             Paste a single-line value from the provider's dashboard."
        );
    }
    let home = volt_home();
    std::fs::create_dir_all(&home)?;
    let env_path = home.join(".env");

    // Read existing lines, drop any matching key, then append the new one.
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| {
            let trimmed = l.trim_start();
            !trimmed.starts_with(&format!("{}=", env_var))
        })
        .map(|s| s.to_string())
        .collect();
    // Preserve a trailing newline so the appended entry starts on its own line.
    if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(true) {
        lines.push(String::new());
    }
    lines.push(format!("{}={}", env_var, api_key.trim()));
    let body = lines.join("\n") + "\n";
    std::fs::write(&env_path, body)?;

    // Make it visible to the rest of this process.
    std::env::set_var(&env_var, api_key.trim());
    // Invalidate the provider-detector cache so the next `detect()`
    // sees the new key.
    crate::llm::provider_detector::invalidate_cache();
    Ok(env_var)
}

/// Read the `.env` file the wizard would have written, if it exists.
/// Used by tests and the runtime's reload-from-disk flow.
#[cfg(test)]
pub fn read_volt_home_env() -> std::collections::HashMap<String, String> {
    let path = volt_home().join(".env");
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    content
        .lines()
        .filter_map(|l| {
            let trimmed = l.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            l.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}
