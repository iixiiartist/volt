use serde_json::json;
use volt::models::RegistryManifest;
use volt::validation::{compute_source_sha256, validate_manifest};

fn manifest(source_code: &str) -> RegistryManifest {
    RegistryManifest {
        tool_name: "test-tool".to_string(),
        description: "A test tool".to_string(),
        language: "rust".to_string(),
        source_code: source_code.to_string(),
        parameter_schema: json!({"type":"object"}),
        signature: None,
        source_sha256: None,
        relationships: vec![],
        metadata: json!({}),
    }
}

#[test]
fn accepts_simple_source() {
    let report = validate_manifest(&manifest("fn main() { println!(\"ok\"); }"));
    assert!(report.accepted);
}

#[test]
fn rejects_env_access() {
    let report = validate_manifest(&manifest("fn main() { std::env::vars(); }"));
    assert!(!report.accepted);
}

#[test]
fn hashes_source_code() {
    let hash = compute_source_sha256("abc");
    assert_eq!(hash.len(), 64);
}

#[test]
fn rejects_unsafe_block() {
    let report = validate_manifest(&manifest("fn main() { unsafe { let x = 1; } }"));
    assert!(!report.accepted);
}

#[test]
fn warns_on_missing_signature() {
    let report = validate_manifest(&manifest("fn main() {}"));
    assert!(report.warnings.iter().any(|w| w.contains("signature")));
}

#[test]
fn warns_on_non_allowlisted_language() {
    let mut m = manifest("print('hello')");
    m.language = "cobol".to_string();
    let report = validate_manifest(&m);
    assert!(report.warnings.iter().any(|w| w.contains("cobol")));
}
