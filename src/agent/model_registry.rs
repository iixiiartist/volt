use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

/// Static model registry mapping (variant, quant, framework) → ModelSpec.
pub static MODEL_REGISTRY: LazyLock<HashMap<(&'static str, &'static str, &'static str), ModelSpec>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        let tool_dir = std::env::var("VOLT_TOOL_BIN_DIR").unwrap_or_default();
        // Gemma-4 E4B via LiteRT-LM
        map.insert(
            ("gemma-4-e4b", "SFP8", "litertlm"),
            ModelSpec {
                binary_path: PathBuf::from(&tool_dir).join("litert_lm.exe"),
                memory_mb: 1500,
                framework: "litertlm",
            },
        );
        // Gemma-4 31B via llama.cpp
        map.insert(
            ("gemma-4-31b", "SFP8", "llamacpp"),
            ModelSpec {
                binary_path: PathBuf::from(&tool_dir).join("llama.exe"),
                memory_mb: 15000,
                framework: "llamacpp",
            },
        );
        map
    });

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub binary_path: PathBuf,
    pub memory_mb: u64,
    pub framework: &'static str,
}

/// Resolve a model spec from the registry keys.
pub fn resolve_model(variant: &str, quant: &str, framework: &str) -> Option<ModelSpec> {
    MODEL_REGISTRY
        .get(&(variant, quant, framework))
        .cloned()
}

/// Check if system RAM is sufficient for the given model (memory requirement in MB).
pub fn has_enough_ram(required_mb: u64) -> bool {
    let total_mb = get_total_ram_mb();
    total_mb >= required_mb
}

fn get_total_ram_mb() -> u64 {
    // TODO: Use sysinfo or another crate to get actual RAM. For now, stub huge value.
    32_768
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_model() {
        let spec = resolve_model("gemma-4-e4b", "SFP8", "litertlm");
        assert!(spec.is_some());
        let s = spec.unwrap();
        assert_eq!(s.framework, "litertlm");
        assert!(s.memory_mb > 0);
    }

    #[test]
    fn test_has_enough_ram() {
        assert!(has_enough_ram(1));
        assert!(has_enough_ram(1000));
    }
}
