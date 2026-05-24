pub mod agent;
pub mod code_parser;
pub mod config;
pub mod context;
pub mod db;
pub mod embedding;
pub mod eval;
pub mod graph_rag;
pub mod hnsw_index;
pub mod llm;
pub mod local_embed;
pub mod mcp;
pub mod models;
pub mod orchestrator;
pub mod registry;
pub mod sandbox;
pub mod session;
pub mod skills;
pub mod telemetry;
pub mod tools;
pub mod tui;
pub mod validation;
pub mod worker;

#[cfg(any(test, feature = "testutils"))]
pub mod test_utils;

pub fn http_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .expect("reqwest client build must succeed")
}