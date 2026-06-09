use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{LazyLock, OnceLock};

/// Static model registry mapping (variant, quant, framework) → ModelSpec.
pub static MODEL_REGISTRY: LazyLock<
    HashMap<(&'static str, &'static str, &'static str), ModelSpec>,
> = LazyLock::new(|| {
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
    MODEL_REGISTRY.get(&(variant, quant, framework)).cloned()
}

/// Check if system RAM is sufficient for the given model (memory requirement in MB).
pub fn has_enough_ram(required_mb: u64) -> bool {
    let total_mb = get_total_ram_mb();
    total_mb >= required_mb
}

static TOTAL_RAM_MB: OnceLock<u64> = OnceLock::new();

fn get_total_ram_mb() -> u64 {
    *TOTAL_RAM_MB.get_or_init(detect_total_ram_mb)
}

fn detect_total_ram_mb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(kb_str) = rest.split_whitespace().next() {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return kb / 1024;
                        }
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("sysctl").args(["-n", "hw.memsize"]).output() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                if let Ok(bytes) = s.trim().parse::<u64>() {
                    return bytes / (1024 * 1024);
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(out) = Command::new("systeminfo").output() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                for line in s.lines() {
                    if let Some(rest) = line.trim_start().strip_prefix("Total Physical Memory:") {
                        let cleaned: String = rest
                            .chars()
                            .filter(|c| c.is_ascii_digit())
                            .collect();
                        if let Ok(mb) = cleaned.parse::<u64>() {
                            return mb;
                        }
                    }
                }
            }
        }
    }
    0
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
        let total = detect_total_ram_mb();
        assert!(has_enough_ram(1));
        if total > 0 {
            assert!(has_enough_ram(total));
            assert!(!has_enough_ram(total + 10_000_000));
        } else {
            assert!(has_enough_ram(1000));
        }
    }

    #[test]
    fn test_detect_total_ram_mb_returns_nonzero() {
        assert!(detect_total_ram_mb() > 0);
    }
}
