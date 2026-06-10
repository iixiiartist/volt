//! Role-based model routing.
//!
//! A workflow declares a *role* on a node (e.g. `supervisor`,
//! `classifier`, `coder`). The `RoleRegistry` resolves the role to a
//! concrete model ID at execution time using a TOML config file
//! (`~/.volt/volt.models.toml` by default). This lets an operator
//! change which model serves which role without touching workflow
//! files — the same workflow runs on Llama 3.3 70B today, Qwen 3
//! 32B tomorrow.
//!
//! ## File format
//!
//! ```toml
//! [roles.supervisor]
//! model = "meta-llama/Llama-3.3-70B-Instruct"
//! temperature = 0.3
//! max_tokens = 4096
//!
//! [roles.classifier]
//! model = "meta-llama/Llama-3.1-8B-Instant"
//! temperature = 0.0
//! max_tokens = 512
//! ```
//!
//! ## Resolution semantics
//!
//! When a `WorkflowNode` has both a `role` and a `model` field, the
//! `role` is used and the `model` field is ignored. When a
//! `WorkflowNode` has only a `model` field, that field is used as-is
//! (no role lookup). When a `WorkflowNode` has neither, resolution
//! fails with a clear error.
//!
//! ## Multi-modal roles (Phase 2b)
//!
//! Future node types (vision, audio, embedding) will use role names
//! that map to a specific vLLM endpoint task (chat, embed, classify).
//! The `RoleMapping::modality` field is reserved for that work and
//! is not yet consumed by the orchestrator.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the default config location.
/// Default: `~/.volt/volt.models.toml`.
pub const VOLT_MODELS_CONFIG_ENV: &str = "VOLT_MODELS_CONFIG";

/// One role in the registry. Maps a logical role name to a concrete
/// model served by the active provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleMapping {
    /// Concrete model ID as served by the active vLLM endpoint
    /// (e.g. `meta-llama/Llama-3.3-70B-Instruct`).
    pub model: String,
    /// Sampling temperature. `None` means use the default.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Max tokens to generate. `None` means use the default.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Modality hint. Reserved for Phase 2b multi-modality. Values:
    /// `chat` (default), `embedding`, `vision`, `audio`.
    #[serde(default)]
    pub modality: Option<String>,
    /// Optional system-prompt append. Concatenated to the agent's
    /// primary system prompt when this role is used.
    #[serde(default)]
    pub system_prompt_append: Option<String>,
}

/// Top-level shape of `volt.models.toml`. Wraps a `[roles]` table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoltModelsConfig {
    #[serde(default)]
    pub roles: HashMap<String, RoleMapping>,
}

/// The role registry — in-memory cache of the parsed config file.
#[derive(Debug, Clone, Default)]
pub struct RoleRegistry {
    config: VoltModelsConfig,
    /// Path the config was loaded from, for diagnostics.
    source: Option<PathBuf>,
}

/// A single role resolution, suitable for the audit log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleResolution {
    /// Node ID the role was resolved for.
    pub node_id: String,
    /// The role name on the node, if any.
    pub role: Option<String>,
    /// The model ID the node resolves to.
    pub model_id: String,
    /// Source of the model_id: "role" (from TOML) or "literal"
    /// (from the node's `model` field).
    pub source: ResolutionSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResolutionSource {
    /// Resolved from a role in `volt.models.toml`.
    Role,
    /// Used the literal `model` field on the node (no role lookup).
    Literal,
    /// Resolved from the default role for a node kind (e.g. agents
    /// default to "supervisor" if no role is set).
    Default,
}

impl RoleRegistry {
    /// Load the registry from the default location. Creates a default
    /// config file at `~/.volt/volt.models.toml` if one does not
    /// already exist. This is the call to use at startup.
    pub fn load_default() -> Result<Self> {
        let path = default_config_path();
        if !path.exists() {
            write_default_config(&path)
                .with_context(|| format!("failed to write default config at {:?}", path))?;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {:?}", path))?;
        let config: VoltModelsConfig =
            toml::from_str(&text).with_context(|| format!("failed to parse TOML at {:?}", path))?;
        Ok(Self {
            config,
            source: Some(path),
        })
    }

    /// Load the registry from an explicit path. Does not create the
    /// file if missing.
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config at {:?}", path))?;
        let config: VoltModelsConfig =
            toml::from_str(&text).with_context(|| format!("failed to parse TOML at {:?}", path))?;
        Ok(Self {
            config,
            source: Some(path.to_path_buf()),
        })
    }

    /// Construct a registry from an in-memory config. Used by tests.
    pub fn from_config(config: VoltModelsConfig) -> Self {
        Self {
            config,
            source: None,
        }
    }

    /// Path the config was loaded from (None if from in-memory).
    pub fn source_path(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// Look up a role by name. Returns the role's mapping if found.
    pub fn resolve(&self, role: &str) -> Option<&RoleMapping> {
        self.config.roles.get(role)
    }

    /// All configured role names. Sorted for determinism.
    pub fn role_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.config.roles.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }

    /// Resolve a single (role, model_override) pair. `role` takes
    /// precedence if it matches a configured role; otherwise the
    /// `model_override` is used literally. Returns an error if both
    /// are absent.
    pub fn resolve_node(
        &self,
        role: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<(String, ResolutionSource)> {
        if let Some(role_name) = role {
            if let Some(mapping) = self.config.roles.get(role_name) {
                return Ok((mapping.model.clone(), ResolutionSource::Role));
            }
            // Role name not found in config — fall through to the
            // model_override if any, so a workflow that names a role
            // the operator hasn't configured can still work. We log
            // this in the resolution record so the operator sees it.
            if let Some(m) = model_override {
                return Ok((m.to_string(), ResolutionSource::Literal));
            }
            bail!(
                "role '{}' is not in volt.models.toml and no model override was provided",
                role_name
            );
        }
        if let Some(m) = model_override {
            return Ok((m.to_string(), ResolutionSource::Literal));
        }
        bail!("node has neither a role nor a model override — cannot resolve")
    }
}

/// Default config path. `~/.volt/volt.models.toml` on all platforms.
pub fn default_config_path() -> PathBuf {
    if let Ok(p) = std::env::var(VOLT_MODELS_CONFIG_ENV) {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".volt").join("volt.models.toml")
}

/// Write the shipped-default config to `path`. Idempotent — caller
/// should check `path.exists()` first.
fn write_default_config(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {:?}", parent))?;
    }
    std::fs::write(path, DEFAULT_CONFIG_TOML)
        .with_context(|| format!("failed to write default config to {:?}", path))?;
    Ok(())
}

/// The default config shipped with volt. Operators edit this file to
/// fit their deployment. Roles map to models served by the active
/// vLLM endpoint (or any provider, when `VOLT_ENABLE_CLOUD_PROVIDERS=1`).
pub const DEFAULT_CONFIG_TOML: &str = r#"# volt.models.toml
# Maps role names to model IDs served by the active vLLM endpoint.
# Override these to fit your deployment. Restart volt to apply changes.
#
# Roles referenced in workflow nodes (`role: "supervisor"`, etc.)
# resolve to the `model` field below at execution time. The same
# workflow file runs against any model by editing this file.
#
# This file lives at ~/.volt/volt.models.toml by default. Override
# the path with VOLT_MODELS_CONFIG=/path/to/file.toml.

[roles.supervisor]
# The "smart" model. Used for the DAG supervisor and any node that
# needs to reason about ambiguous or multi-step tasks.
model = "meta-llama/Llama-3.3-70B-Instruct"
temperature = 0.3
max_tokens = 4096

[roles.classifier]
# The "fast" model. Used for classification, routing, simple
# structured extraction, and tool-call argument generation.
model = "meta-llama/Llama-3.1-8B-Instant"
temperature = 0.0
max_tokens = 512

[roles.coder]
# The specialist model. Used for code generation, code review, and
# refactor nodes.
model = "Qwen/Qwen2.5-Coder-32B-Instruct"
temperature = 0.1
max_tokens = 4096

[roles.summarizer]
# Mid-sized generalist. Used for summarization, extraction, and
# intermediate condensation nodes.
model = "meta-llama/Llama-3.1-8B-Instant"
temperature = 0.2
max_tokens = 1024
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_resolves_known_role() {
        let mut cfg = VoltModelsConfig::default();
        cfg.roles.insert(
            "supervisor".into(),
            RoleMapping {
                model: "meta-llama/Llama-3.3-70B-Instruct".into(),
                temperature: Some(0.3),
                max_tokens: Some(4096),
                modality: None,
                system_prompt_append: None,
            },
        );
        let reg = RoleRegistry::from_config(cfg);
        let resolved = reg.resolve("supervisor").unwrap();
        assert_eq!(resolved.model, "meta-llama/Llama-3.3-70B-Instruct");
        assert_eq!(resolved.temperature, Some(0.3));
    }

    #[test]
    fn from_config_resolves_unknown_role_to_none() {
        let reg = RoleRegistry::from_config(VoltModelsConfig::default());
        assert!(reg.resolve("nonexistent").is_none());
    }

    #[test]
    fn resolve_node_prefers_role_over_override() {
        let mut cfg = VoltModelsConfig::default();
        cfg.roles.insert(
            "coder".into(),
            RoleMapping {
                model: "Qwen/Qwen2.5-Coder-32B-Instruct".into(),
                temperature: None,
                max_tokens: None,
                modality: None,
                system_prompt_append: None,
            },
        );
        let reg = RoleRegistry::from_config(cfg);
        let (model, source) = reg.resolve_node(Some("coder"), Some("gpt-4o")).unwrap();
        assert_eq!(model, "Qwen/Qwen2.5-Coder-32B-Instruct");
        assert_eq!(source, ResolutionSource::Role);
    }

    #[test]
    fn resolve_node_falls_back_to_override_when_role_unknown() {
        let reg = RoleRegistry::from_config(VoltModelsConfig::default());
        let (model, source) = reg
            .resolve_node(Some("nonexistent"), Some("gpt-4o"))
            .unwrap();
        assert_eq!(model, "gpt-4o");
        assert_eq!(source, ResolutionSource::Literal);
    }

    #[test]
    fn resolve_node_uses_override_when_no_role() {
        let reg = RoleRegistry::from_config(VoltModelsConfig::default());
        let (model, source) = reg.resolve_node(None, Some("groq/llama-3.1-70b")).unwrap();
        assert_eq!(model, "groq/llama-3.1-70b");
        assert_eq!(source, ResolutionSource::Literal);
    }

    #[test]
    fn resolve_node_errors_when_no_role_and_no_override() {
        let reg = RoleRegistry::from_config(VoltModelsConfig::default());
        let err = reg.resolve_node(None, None).unwrap_err();
        assert!(err.to_string().contains("neither a role nor a model"));
    }

    #[test]
    fn resolve_node_errors_when_unknown_role_and_no_override() {
        let reg = RoleRegistry::from_config(VoltModelsConfig::default());
        let err = reg.resolve_node(Some("ghost"), None).unwrap_err();
        assert!(err.to_string().contains("not in volt.models.toml"));
    }

    #[test]
    fn role_names_is_sorted() {
        let mut cfg = VoltModelsConfig::default();
        cfg.roles.insert(
            "zulu".into(),
            RoleMapping {
                model: "x".into(),
                temperature: None,
                max_tokens: None,
                modality: None,
                system_prompt_append: None,
            },
        );
        cfg.roles.insert(
            "alpha".into(),
            RoleMapping {
                model: "y".into(),
                temperature: None,
                max_tokens: None,
                modality: None,
                system_prompt_append: None,
            },
        );
        let reg = RoleRegistry::from_config(cfg);
        assert_eq!(reg.role_names(), vec!["alpha", "zulu"]);
    }

    #[test]
    fn default_config_toml_parses() {
        let cfg: VoltModelsConfig = toml::from_str(DEFAULT_CONFIG_TOML).unwrap();
        let names: Vec<&str> = cfg.roles.keys().map(|s| s.as_str()).collect();
        assert!(names.contains(&"supervisor"));
        assert!(names.contains(&"classifier"));
        assert!(names.contains(&"coder"));
        assert!(names.contains(&"summarizer"));
        let sup = cfg.roles.get("supervisor").unwrap();
        assert!(sup.model.contains("Llama-3.3-70B"));
    }

    #[test]
    fn load_from_path_reads_arbitrary_toml() {
        let dir = std::env::temp_dir().join("volt_role_registry_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("custom_models.toml");
        std::fs::write(
            &path,
            r#"
[roles.embedder]
model = "BAAI/bge-large-en-v1.5"
modality = "embedding"
"#,
        )
        .unwrap();

        let reg = RoleRegistry::load_from_path(&path).unwrap();
        let emb = reg.resolve("embedder").unwrap();
        assert_eq!(emb.model, "BAAI/bge-large-en-v1.5");
        assert_eq!(emb.modality.as_deref(), Some("embedding"));
        std::fs::remove_file(&path).ok();
    }
}
