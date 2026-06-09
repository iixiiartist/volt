use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use volt::embedding::EmbeddingClient;
use volt::llm::openai::OpenAIProvider;
use volt::llm::LLMProvider;
use volt::models::*;
use volt::tools::ToolRegistry;

const DISTRACTOR_COUNT: usize = 50;
const TOP_K: usize = 8;

fn build_provider() -> Box<dyn LLMProvider> {
    let route = volt::orchestrator::resolve_provider("llama-3.1-8b-instant");
    Box::new(OpenAIProvider::new(
        route.api_key,
        route.base_url,
        "bfcl-bench".into(),
    ))
}

fn fix_params(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            if let Some(t) = m.get("type").and_then(|v| v.as_str()) {
                let fixed = match t {
                    "dict" | "Dict" | "Dictionary" => "object",
                    "String" => "string",
                    "Boolean" => "boolean",
                    "Integer" => "integer",
                    "Number" | "float" | "double" => "number",
                    "Array" | "List" => "array",
                    "any" | "Any" | "Function" | "Element" | "HTMLElement" | "Promise" => "string",
                    "void" | "undefined" | "null" => "null",
                    _ => t,
                };
                out.insert("type".into(), Value::String(fixed.into()));
            }
            if let Some(props) = m.get("properties").and_then(|v| v.as_object()) {
                let fp: serde_json::Map<_, _> = props
                    .iter()
                    .map(|(k, v)| (k.clone(), fix_params(v)))
                    .collect();
                out.insert("properties".into(), Value::Object(fp));
            }
            if let Some(items) = m.get("items") {
                out.insert("items".into(), fix_params(items));
            }
            if let Some(req) = m.get("required") {
                out.insert("required".into(), req.clone());
            }
            Value::Object(out)
        }
        _ => v.clone(),
    }
}

fn extract_query(question: &Value) -> String {
    if let Some(turns) = question.as_array() {
        if let Some(first) = turns.first().and_then(|v| v.as_array()) {
            for msg in first {
                if msg.get("role").and_then(|v| v.as_str()) == Some("user") {
                    return msg
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
        }
    }
    String::new()
}

#[tokio::test]
async fn test_bfcl_voltr_pipeline() {
    let _ = dotenvy::dotenv();
    // Force env vars from .env
    if let Ok(content) = std::fs::read_to_string(".env") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                std::env::set_var(k.trim(), v.trim());
            }
        }
    }
    std::env::set_var(
        "OPENAI_API_KEY",
        std::env::var("GROQ_API_KEY").unwrap_or_default(),
    );
    std::env::set_var("OPENAI_BASE_URL", "https://api.groq.com/openai/v1");

    // Load data
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/bfcl_data.json").unwrap()).unwrap();
    let cases = raw.as_array().unwrap();
    let dist_text =
        std::fs::read_to_string("tests/distractors.json").expect("Missing tests/distractors.json");
    let dist_raw: Value = serde_json::from_str(&dist_text).expect("Invalid distractors.json");
    let distractors = dist_raw
        .as_array()
        .expect("distractors.json must be an array");

    let provider = build_provider();
    let embedder = EmbeddingClient::new_smart().await;

    println!("\n{}", "=".repeat(70));
    println!(
        "{:^70}",
        "VOLT RAG vs STATIC (Rust + Ollama + pgvector pipeline)"
    );
    println!("{}", "=".repeat(70));
    println!(
        "Model: llama-3.1-8b-instant  |  Cases: {}",
        cases.len().min(15)
    );
    println!("Embedding: Ollama (mxbai-embed-large) via EmbeddingClient");
    println!(
        "RAG: cosine similarity via ToolRegistry::search_tools (top-{})",
        TOP_K
    );
    println!("{}", "=".repeat(70));

    for mode in &["static", "rag"] {
        println!("\n--- {} MODE ---", mode.to_uppercase());
        let mut correct = 0u32;
        let mut total_tok = 0u64;
        let mut total = 0u32;

        for (i, case) in cases.iter().enumerate().take(15) {
            let id = case.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let empty_vec = vec![];
            let functions = case
                .get("function")
                .and_then(|v| v.as_array())
                .unwrap_or(&empty_vec);
            let query = extract_query(case.get("question").unwrap_or(&Value::Null));
            let started = Instant::now();

            // Build Volt ToolRegistry with BFCL functions + distractors
            let registry = ToolRegistry::new();
            let seed: u64 = id.bytes().fold(0u64, |a, b| a.wrapping_add(b as u64));

            for f in functions {
                let name = f
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let desc = f
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let params = fix_params(f.get("parameters").unwrap_or(&Value::Null));
                registry
                    .register(
                        &name,
                        &desc,
                        params,
                        "bfcl",
                        Arc::new(|_| {
                            Box::pin(async move {
                                ToolResult {
                                    success: true,
                                    output: "ok".into(),
                                    error: None,
                                    duration_ms: 0,
                                }
                            })
                        }),
                    )
                    .await;
            }

            // Add distractors
            let divisor = distractors.len().saturating_sub(DISTRACTOR_COUNT).max(1);
            let start_idx = (seed as usize) % divisor;
            for j in 0..DISTRACTOR_COUNT {
                let d = &distractors[(start_idx + j) % distractors.len()];
                let name = d
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let desc = d
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let params = fix_params(d.get("parameters").unwrap_or(&Value::Null));
                registry
                    .register(
                        &name,
                        &desc,
                        params,
                        "dist",
                        Arc::new(|_| {
                            Box::pin(async move {
                                ToolResult {
                                    success: true,
                                    output: "ok".into(),
                                    error: None,
                                    duration_ms: 0,
                                }
                            })
                        }),
                    )
                    .await;
            }

            // Compute embeddings via Ollama (Volt's actual pipeline)
            registry.compute_embeddings(&embedder).await;

            // Select tools
            let tool_defs = if *mode == "rag" {
                if let Ok(qe) = embedder.embed_description(&query).await {
                    registry.search_tools(&qe, TOP_K, &[], None).await
                } else {
                    registry.get_definitions().await
                }
            } else {
                registry.get_definitions().await
            };

            // Call LLM
            let request = LLMRequest {
                model: "llama-3.1-8b-instant".into(),
                messages: vec![LLMMessage {
                    role: "user".into(),
                    content: Arc::new(query.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                temperature: Some(0.0),
                max_tokens: Some(1024),
                stop: None,
                tools: Some(tool_defs.clone()),
                stream: false,
                ..Default::default()
            };

            let result = provider.complete(&request).await;
            let duration_ms = started.elapsed().as_millis();

            let (pass, pt, ct) = match result {
                Ok(r) => {
                    let pt = r.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0);
                    let ct = r.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0);
                    let ok = if let Some(ref tcs) = r.tool_calls {
                        let pred: Vec<&str> = tcs.iter().map(|tc| tc.name.as_str()).collect();
                        let exp: Vec<&str> = functions
                            .iter()
                            .filter_map(|f| f.get("name").and_then(|v| v.as_str()))
                            .collect();
                        pred == exp
                    } else {
                        false
                    };
                    (ok, pt, ct)
                }
                Err(e) => {
                    eprintln!("  [{}/15] {} LLM error: {}", i + 1, id, e);
                    (false, 0, 0)
                }
            };

            let status = if pass { "PASS" } else { "FAIL" };
            println!(
                "  [{}/15] {} | {} | tools: {}->{} | tokens: {}P+{}C | {}ms",
                i + 1,
                status,
                id,
                functions.len() + DISTRACTOR_COUNT,
                tool_defs.len(),
                pt,
                ct,
                duration_ms
            );
            if pass {
                correct += 1;
            }
            total_tok += pt;
            total += 1;
        }

        let pct = correct as f64 / total as f64 * 100.0;
        let avg_tok = total_tok as f64 / total as f64;
        println!("  --- {} RESULTS ---", mode.to_uppercase());
        println!("  Accuracy: {}/{} = {:.1}%", correct, total, pct);
        println!("  Avg prompt tokens: {:.0}", avg_tok);

        assert!(total > 0, "bfcl_pipeline: no cases were evaluated");
        if std::env::var("VOLT_BFCL_REQUIRE_PASS").as_deref() == Ok("1") {
            assert!(correct > 0, "bfcl_pipeline {}: 0/{} correct", mode, total);
        }
    }

    println!("\n{}", "=".repeat(70));
    println!("Pipeline test complete. See above for comparison.");
    println!("{}", "=".repeat(70));
}
