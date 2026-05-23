use crate::models::{RegistryManifest, ValidationReport};
use sha2::{Digest, Sha256};

const DENIED_PATTERNS: &[&str] = &[
    "std::env::vars",
    "std::env::var(",
    "env::vars",
    "env::var(",
    "std::fs::read_dir(\"/",
    "std::fs::read_to_string(\"/etc",
    "std::process::Command::new(\"sh\")",
    "std::process::Command::new(\"bash\")",
    "Command::new(\"sh\")",
    "Command::new(\"bash\")",
    "Command::new(\"cmd\")",
    "Command::new(\"powershell\")",
    "TcpStream::connect",
    "UdpSocket::bind",
    "reqwest::",
    "hyper::",
    "/dev/",
    "/proc/",
    "/sys/",
    "std::os::unix",
    "std::os::windows",
    "std::net::",
    "unsafe {",
];
pub fn validate_manifest(manifest: &RegistryManifest) -> ValidationReport {
    let source_lower = manifest.source_code.to_lowercase();
    let mut denied = Vec::new();

    for pattern in DENIED_PATTERNS {
        if source_lower.contains(pattern) {
            denied.push(pattern.to_string());
        }
    }

    let mut warnings = Vec::new();
    if manifest.signature.as_deref().unwrap_or_default().is_empty() {
        warnings.push("manifest has no cryptographic signature".to_string());
    }
    if manifest.parameter_schema.get("type").is_none() {
        warnings.push("parameter_schema does not declare a JSON schema type".to_string());
    }
    if !matches!(manifest.language.as_str(), "rust" | "python" | "wasm" | "bash" | "javascript") {
        warnings.push(format!("language '{}' is not in the default allowlist", manifest.language));
    }

    ValidationReport {
        accepted: denied.is_empty(),
        language: manifest.language.clone(),
        denied_patterns: denied,
        warnings,
    }
}

pub fn compute_source_sha256(source_code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_code.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn verify_declared_sha(manifest: &RegistryManifest) -> anyhow::Result<String> {
    let computed = compute_source_sha256(&manifest.source_code);
    if let Some(declared) = &manifest.source_sha256 {
        if declared != &computed {
            anyhow::bail!(
                "source_sha256 mismatch for {}: declared {}, computed {}",
                manifest.tool_name,
                declared,
                computed
            );
        }
    }
    Ok(computed)
}