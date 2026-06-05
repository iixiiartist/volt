#![deny(deprecated)]

pub mod agent;
pub mod attenuation;
pub mod capability;

pub mod channels;
pub mod checkpoint_journal;
pub mod code_parser;
pub mod command_guard;
pub mod commands;
pub mod config;
pub mod context;
pub mod db;
pub mod embedding;
pub mod eval;
pub mod events;
pub mod graph_rag;
pub mod heartbeat;
pub mod jobs;
pub mod leak_detector;
pub mod llm;
pub mod local_embed;
pub mod mcp;
pub mod models;
pub mod network_policy;
pub mod orchestrator;
pub mod registry;
pub mod routines;
pub mod safety_layer;
pub mod sandbox;
pub mod secrets;
pub mod session;
pub mod skill_scorer;
pub mod skills;
pub mod telemetry;
pub mod tool_failure_tracker;
pub mod tools;
pub mod tui;
#[cfg(feature = "tools-turbovec")]
pub mod turbovec_index;
pub mod validation;
pub mod vector_index;
#[cfg(feature = "webui")]
pub mod webui;
pub mod worker;

#[cfg(any(test, feature = "testutils"))]
pub mod test_utils;

use std::sync::OnceLock;

/// Returns a shared reqwest::Client backed by a single global connection pool.
/// All callers share the same pool (`pool_max_idle_per_host=100`,
/// `tcp_keepalive=60s`), eliminating the 800+ idle-ephemeral-connection
/// problem from ad-hoc per-tool client creation.
///
/// HTTP/2 is enabled with adaptive window sizing and keep-alive pings to
/// eliminate TLS handshake latency on rapid-fire sequential tool-use loops.
pub fn http_client() -> reqwest::Client {
    static GLOBAL_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    GLOBAL_HTTP_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .pool_max_idle_per_host(100)
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .tcp_keepalive(std::time::Duration::from_secs(60))
                .connect_timeout(std::time::Duration::from_secs(10))
                // HTTP/2 optimizations for cloud API latency
                .http2_keep_alive_interval(std::time::Duration::from_secs(15))
                .http2_keep_alive_timeout(std::time::Duration::from_secs(5))
                .http2_adaptive_window(true)
                .build()
                .expect("global reqwest client build must succeed")
        })
        .clone()
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
