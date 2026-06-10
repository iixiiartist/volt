use crate::{db, llm};
use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

/// Shared HTTP client factory with sensible defaults for tool calls.
/// Uses connection pooling and a standard timeout.
pub fn http_client_with_timeout(timeout: Duration) -> reqwest::Result<Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
}

/// Shared HTTP client factory for LLM calls.
pub fn build_http_client() -> reqwest::Result<Client> {
    http_client_with_timeout(Duration::from_secs(300))
}
