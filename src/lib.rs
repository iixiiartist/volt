pub mod agent;
pub mod attenuation;
pub mod code_parser;
pub mod command_guard;
pub mod commands;
pub mod config;
pub mod context;
pub mod db;
pub mod embedding;
pub mod eval;
pub mod graph_rag;
pub mod leak_detector;
pub mod llm;
pub mod local_embed;
pub mod mcp;
pub mod models;
pub mod network_policy;
pub mod orchestrator;
pub mod registry;
pub mod safety_layer;
pub mod sandbox;
pub mod session;
pub mod skill_scorer;
pub mod skills;
pub mod telemetry;
pub mod tool_failure_tracker;
pub mod tools;
pub mod tui;
pub mod validation;
pub mod vector_index;
pub mod worker;

#[cfg(any(test, feature = "testutils"))]
pub mod test_utils;

pub fn http_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .expect("reqwest client build must succeed")
}

/// Cosine similarity between two vectors of equal length.
/// Shared utility used by context store, tool registry, skills, and vector index.
#[inline]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b).max(f32::EPSILON)
}
