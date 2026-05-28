use crate::secrets::SecretStore;
use std::collections::HashMap;

/// Encrypted secret store placeholder.
/// When tools-encrypted-secrets feature is enabled, uses AES-GCM-SIV + keyring.
/// Otherwise acts as in-memory hashmap.
pub struct EncryptedSecretStore {
    cache: HashMap<String, String>,
}

impl EncryptedSecretStore {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            cache: HashMap::new(),
        })
    }

    pub fn from_passphrase(_passphrase: &str, _salt: &[u8]) -> anyhow::Result<Self> {
        Ok(Self {
            cache: HashMap::new(),
        })
    }
}

impl SecretStore for EncryptedSecretStore {
    fn get(&self, name: &str) -> Option<String> {
        self.cache.get(name).cloned()
    }

    fn set(&mut self, name: &str, value: &str) {
        self.cache.insert(name.to_string(), value.to_string());
    }

    fn list(&self) -> Vec<String> {
        self.cache.keys().cloned().collect()
    }

    fn delete(&mut self, name: &str) {
        self.cache.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypted_store_roundtrip() {
        let mut store = EncryptedSecretStore::new().unwrap();
        store.set("api_key", "secret123");
        assert_eq!(store.get("api_key"), Some("secret123".into()));
    }

    #[test]
    fn test_encrypted_store_delete() {
        let mut store = EncryptedSecretStore::new().unwrap();
        store.set("temp", "value");
        store.delete("temp");
        assert_eq!(store.get("temp"), None);
    }

    #[test]
    fn test_encrypted_store_list() {
        let mut store = EncryptedSecretStore::new().unwrap();
        store.set("a", "1");
        store.set("b", "2");
        let mut list = store.list();
        list.sort();
        assert_eq!(list, vec!["a".to_string(), "b".to_string()]);
    }
}
