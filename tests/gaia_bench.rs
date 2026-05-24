use std::sync::Arc;
use std::time::Instant;
use volt::agent::loop_rs::Agent;
use volt::models::*;
use volt::tools::ToolRegistry;

const GAIA_QUESTIONS: &[(&str, &str, &[&str])] = &[
    ("gaia_0", "What is the capital of France?", &["Paris"]),
    ("gaia_1", "What is the chemical formula for water?", &["H2O", "H₂O"]),
    ("gaia_2", "What year did World War II end?", &["1945"]),
];

fn build_tools() -> Arc<ToolRegistry> {
    let registry = ToolRegistry::new();
    registry
}

fn build_provider() -> Box<dyn volt::llm::LLMProvider> {
    let route = volt::orchestrator::resolve_provider("llama-3.1-8b-instant");
    Box::new(volt::llm::openai::OpenAIProvider::new(route.api_key, route.base_url, "gaia-bench".into()))
}

#[tokio::test]
async fn test_gaia_smoke() {
    let _ = dotenvy::dotenv();
    if let Ok(content) = std::fs::read_to_string(".env") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some((k, v)) = line.split_once('=') {
                std::env::set_var(k.trim(), v.trim());
            }
        }
    }

    let provider = build_provider();
    let tools = build_tools();
    let config = AgentConfig {
        name: "gaia-bench".into(),
        model: "llama-3.1-8b-instant".into(),
        provider: "openai".into(),
        system_prompt: None,
        max_iterations: 10,
        temperature: 0.0,
        toolsets: vec![],
        hidden: false,
        allow_all: true,
    };
    let agent = Agent::new(config, provider, tools);

    let mut correct = 0u32;
    let total = GAIA_QUESTIONS.len();
    let mut total_duration = 0u128;

    println!("\n{}", "=".repeat(70));
    println!("{:^70}", "Volt GAIA — Agent Integration Test (smoke)");
    println!("{}", "=".repeat(70));
    println!("Model: llama-3.1-8b-instant  |  Questions: {}", total);
    println!("{}", "=".repeat(70));

    for (i, (qid, question, expected_keywords)) in GAIA_QUESTIONS.iter().enumerate() {
        let started = Instant::now();
        let result = agent.run(question).await;
        let duration = started.elapsed().as_millis();
        total_duration += duration;

        let passed = match &result {
            Ok(output) => {
                let out_lower = output.to_lowercase();
                expected_keywords.iter().all(|k| out_lower.contains(&k.to_lowercase()))
            }
            Err(_) => false,
        };

        if passed { correct += 1; }
        let status = if passed { "PASS" } else { "FAIL" };
        println!("  [{}/{}] {} | {} | {}ms", i + 1, total, status, qid, duration);
        if !passed {
            match &result {
                Ok(out) => println!("         expected keywords: {:?} | got: {}", expected_keywords, out.trim().chars().take(120).collect::<String>()),
                Err(e) => println!("         error: {}", e),
            }
        }
    }

    let pct = correct as f64 / total as f64 * 100.0;
    println!("{}", "=".repeat(70));
    println!("RESULTS — GAIA smoke | {}ms total", total_duration);
    println!("  Accuracy: {}/{} = {:.1}%", correct, total, pct);
    println!("{}", "=".repeat(70));
}
