use volt::network_policy::NetworkPolicy;

#[test]
fn test_allow_all_without_allowlist() {
    assert!(NetworkPolicy::check("https://example.com", None).is_ok());
    assert!(NetworkPolicy::check("https://evil.com", None).is_ok());
}

#[test]
fn test_exact_match() {
    let hosts = ["example.com".to_string(), "docs.rs".to_string()];
    assert!(NetworkPolicy::check("https://example.com/", Some(&hosts)).is_ok());
    assert!(NetworkPolicy::check("https://docs.rs/rust", Some(&hosts)).is_ok());
    assert!(NetworkPolicy::check("https://evil.com/", Some(&hosts)).is_err());
}

#[test]
fn test_wildcard_subdomain() {
    let hosts = ["*.example.com".to_string()];
    assert!(NetworkPolicy::check("https://api.example.com/v1", Some(&hosts)).is_ok());
    assert!(NetworkPolicy::check("https://sub.example.com/", Some(&hosts)).is_ok());
    assert!(NetworkPolicy::check("https://example.com/", Some(&hosts)).is_err());
    assert!(NetworkPolicy::check("https://other.com/", Some(&hosts)).is_err());
}

#[test]
fn test_empty_allowlist_allows_all() {
    let hosts: [String; 0] = [];
    assert!(NetworkPolicy::check("https://anything.com", Some(&hosts)).is_ok());
}

#[test]
fn test_port_in_url() {
    let hosts = ["example.com".to_string()];
    assert!(NetworkPolicy::check("https://example.com:8443/path", Some(&hosts)).is_ok());
}

#[test]
fn test_case_insensitive() {
    let hosts = ["EXAMPLE.COM".to_string()];
    assert!(NetworkPolicy::check("https://example.com/", Some(&hosts)).is_ok());
}
