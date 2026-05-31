use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentPreset {
    pub agent: Option<AgentPresetSection>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentPresetSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub temperature: Option<f32>,
    pub max_iterations: Option<u32>,
    pub system_prompt: Option<String>,
    pub allow: Option<bool>,
    pub framework: Option<String>,
    pub model_variant: Option<String>,
    pub quantization: Option<String>,
    pub use_mtp: Option<bool>,
    pub use_cot: Option<bool>,
    pub allow_write: Option<bool>,
}

fn user_config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE") {
        PathBuf::from(home).join(".volt").join("presets")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".volt").join("presets")
    } else {
        PathBuf::from(".volt").join("presets")
    }
}

pub fn preset_dir() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let local = cwd.join("presets");
    if local.exists() {
        return local;
    }
    let home = user_config_dir();
    if home.exists() {
        return home;
    }
    local
}

pub fn list_presets() -> Vec<(String, PathBuf)> {
    let dir = preset_dir();
    if !dir.exists() {
        return Vec::new();
    }
    let mut presets = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    presets.push((name.to_string(), path));
                }
            }
        }
    }
    presets.sort_by(|a, b| a.0.cmp(&b.0));
    presets
}

pub fn load_preset(name: &str) -> Option<(String, AgentPreset)> {
    let dir = preset_dir();
    let path = dir.join(format!("{}.toml", name));
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    let preset: AgentPreset = toml::from_str(&content).ok()?;
    Some((path.to_string_lossy().to_string(), preset))
}

pub fn load_agent_file(path: &Path) -> Option<AgentPreset> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}
