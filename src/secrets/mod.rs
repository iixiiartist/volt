pub mod encrypted;

use std::collections::HashMap;

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Option<String>;
    fn set(&mut self, name: &str, value: &str);
    fn list(&self) -> Vec<String>;
    fn delete(&mut self, name: &str);
}

/// Transition layer: reads from environment variables (backward compat)
pub struct EnvSecretStore {
    _cache: HashMap<String, String>,
}

impl EnvSecretStore {
    pub fn new() -> Self {
        Self { _cache: HashMap::new() }
    }
}

impl SecretStore for EnvSecretStore {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn set(&mut self, _name: &str, _value: &str) {
        // Read-only fallback
    }
    fn list(&self) -> Vec<String> {
        std::env::vars()
            .filter(|(k, _)| k.ends_with("_KEY") || k.ends_with("_TOKEN") || k.ends_with("_SECRET"))
            .map(|(k, _)| k)
            .collect()
    }
    fn delete(&mut self, _name: &str) {
        // Read-only fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_store_reads_env() {
        std::env::set_var("TEST_SECRET", "12345");
        let store = EnvSecretStore::new();
        assert_eq!(store.get("TEST_SECRET"), Some("12345".into()));
    }

    #[test]
    fn test_env_store_missing() {
        let store = EnvSecretStore::new();
        assert_eq!(store.get("DEFINITELY_NOT_SET_XYZ"), None);
    }
}
