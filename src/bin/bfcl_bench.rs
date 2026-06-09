//! Pure Rust BFCL v4 Benchmark — tests all Groq models on function calling accuracy.
//!
//! Usage:
//!   cargo run --release --bin bfcl_bench
//!   cargo run --release --bin bfcl_bench -- --limit 10 --models llama-3.1-8b-instant,qwen/qwen3-32b
//!   cargo run --release --bin bfcl_bench -- --categories simple_python,parallel

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use serde::Deserialize;
use serde_json::Value;
use volt::config::load_dotenv_overriding;
use volt::llm::openai::OpenAIProvider;
use volt::llm::LLMProvider;
use volt::models::{LLMMessage, LLMRequest, ToolDefinition};

// ── BFCL v4 categories ──────────────────────────────────────────
const CATEGORIES: &[(&str, &str)] = &[
    ("simple_python", "BFCL_v4_simple_python.json"),
    ("simple_java", "BFCL_v4_simple_java.json"),
    ("simple_javascript", "BFCL_v4_simple_javascript.json"),
    ("parallel", "BFCL_v4_parallel.json"),
    ("multiple", "BFCL_v4_multiple.json"),
    ("irrelevance", "BFCL_v4_irrelevance.json"),
    ("live_simple", "BFCL_v4_live_simple.json"),
    ("live_parallel", "BFCL_v4_live_parallel.json"),
    ("live_multiple", "BFCL_v4_live_multiple.json"),
    ("live_irrelevance", "BFCL_v4_live_irrelevance.json"),
    ("live_relevance", "BFCL_v4_live_relevance.json"),
    (
        "live_parallel_multiple",
        "BFCL_v4_live_parallel_multiple.json",
    ),
    ("multi_turn_base", "BFCL_v4_multi_turn_base.json"),
    (
        "multi_turn_long_context",
        "BFCL_v4_multi_turn_long_context.json",
    ),
    ("multi_turn_miss_func", "BFCL_v4_multi_turn_miss_func.json"),
    (
        "multi_turn_miss_param",
        "BFCL_v4_multi_turn_miss_param.json",
    ),
];

const GROQ_MODELS: &[&str] = &[
    "llama-3.1-8b-instant",
    "llama-3.3-70b-versatile",
    "llama-4-scout-17b-16e-instruct",
    "qwen/qwen3-32b",
    "openai/gpt-oss-20b",
    "openai/gpt-oss-120b",
    "groq/compound-mini",
    "groq/compound",
];

#[derive(Deserialize)]
struct BfclCase {
    id: Option<String>,
    question: Option<Value>,
    function: Option<Vec<BfclFunction>>,
}

#[derive(Deserialize, Clone)]
struct BfclFunction {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<Value>,
}

#[derive(Parser)]
#[command(name = "bfcl_bench", about = "BFCL v4 benchmark for Volt (Rust)")]
struct Args {
    #[arg(long, default_value = "50")]
    limit: usize,
    #[arg(long)]
    models: Option<String>,
    #[arg(long)]
    categories: Option<String>,
    #[arg(long, default_value = "false")]
    quiet: bool,
}

// ── Parameter normalization (BFCL has non-standard JSON Schema types) ──

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
                    "Number" | "float" | "double" | "int" => "number",
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

fn load_cases(data_dir: &std::path::Path) -> anyhow::Result<Vec<(String, Vec<BfclCase>)>> {
    let mut all = Vec::new();
    for &(name, filename) in CATEGORIES {
        let path = data_dir.join(filename);
        if !path.exists() {
            eprintln!("  [warn] missing data file: {}", path.display());
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut cases = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(case) = serde_json::from_str::<BfclCase>(line) {
                cases.push(case);
            }
        }
        all.push((name.to_string(), cases));
    }
    Ok(all)
}

fn evaluate_tool_call(predicted: &[ToolDefinition], expected: &[BfclFunction]) -> (bool, String) {
    let pred_names: Vec<&str> = predicted.iter().map(|t| t.name.as_str()).collect();
    let exp_names: Vec<&str> = expected.iter().filter_map(|f| f.name.as_deref()).collect();

    if pred_names.is_empty() {
        return (false, "no tool calls".to_string());
    }

    let exp_set: std::collections::BTreeSet<&str> = exp_names.iter().copied().collect();
    let pred_set: std::collections::BTreeSet<&str> = pred_names.iter().copied().collect();

    let is_subset = pred_set.is_subset(&exp_set);

    if pred_set == exp_set {
        (true, format!("called {:?} (exact match)", pred_set))
    } else if is_subset && !pred_set.is_empty() {
        (
            true,
            format!("called {:?} (subset of {:?}, accepted)", pred_set, exp_set),
        )
    } else if is_subset {
        (
            false,
            format!("subset: called {:?} of {:?}", pred_set, exp_set),
        )
    } else if exp_set.is_subset(&pred_set) {
        (
            false,
            format!("superset: called {:?} vs expected {:?}", pred_set, exp_set),
        )
    } else {
        (
            false,
            format!("mismatch: called {:?} vs expected {:?}", pred_set, exp_set),
        )
    }
}

async fn run_category(
    provider: &dyn LLMProvider,
    model: &str,
    _cat_name: &str,
    cases: &[BfclCase],
    limit: usize,
    quiet: bool,
) -> (u32, u32, u64, u64, u128) {
    let total = cases.len().min(limit);
    let mut passed = 0u32;
    let mut total_prompt = 0u64;
    let mut total_completion = 0u64;
    let mut total_latency = 0u128;

    for (i, case) in cases.iter().enumerate().take(total) {
        let query = case
            .question
            .as_ref()
            .map(extract_query)
            .unwrap_or_default();
        let functions = case.function.as_deref().unwrap_or_default();

        if query.is_empty() {
            continue;
        }

        // Build tool definitions
        let tool_defs: Vec<ToolDefinition> = functions
            .iter()
            .map(|f| {
                let params = f
                    .parameters
                    .as_ref()
                    .map(fix_params)
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
                ToolDefinition {
                    name: f.name.clone().unwrap_or_default(),
                    description: f.description.clone().unwrap_or_default(),
                    input_schema: params,
                    category: "bfcl".into(),
                }
            })
            .collect();

        // For irrelevance tests, the model should not call any function
        let expects_no_call = functions.is_empty();

        // Thinking models need extra token budget for chain-of-thought
        let max_tokens =
            if model.contains("qwen3") || model.contains("qwq") || model.contains("deepseek-r1") {
                4096u32
            } else {
                1024u32
            };

        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![LLMMessage {
                role: "user".into(),
                content: Arc::new(format!(
                    "Use the available tools to answer this question. You MUST call the appropriate function.\n\nQuestion: {}",
                    query
                )),
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: Some(0.0),
            max_tokens: Some(max_tokens),
            stop: None,
            tools: Some(tool_defs.clone()),
            stream: false,
            ..Default::default()
        };

        let started = Instant::now();
        let result = provider.complete(&request).await;
        let latency = started.elapsed().as_millis();
        total_latency += latency;

        let (ok, reason) = match &result {
            Ok(r) => {
                let pt = r.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0);
                let ct = r.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0);
                total_prompt += pt;
                total_completion += ct;

                if expects_no_call {
                    let has_calls = r.tool_calls.as_ref().is_some_and(|c| !c.is_empty());
                    if has_calls {
                        (false, "called function when none expected".to_string())
                    } else {
                        (true, "correctly avoided calling".to_string())
                    }
                } else if let Some(ref tcs) = r.tool_calls {
                    let pred_defs: Vec<ToolDefinition> = tcs
                        .iter()
                        .map(|tc| ToolDefinition {
                            name: tc.name.clone(),
                            description: String::new(),
                            input_schema: tc.arguments.clone(),
                            category: "predicted".into(),
                        })
                        .collect();
                    evaluate_tool_call(&pred_defs, functions)
                } else {
                    (false, "no tool calls returned".to_string())
                }
            }
            Err(e) => (false, format!("LLM error: {}", e)),
        };

        if ok {
            passed += 1;
        }

        if !quiet {
            let status = if ok { "PASS" } else { "FAIL" };
            let id = case.id.as_deref().unwrap_or("?");
            let pt = result
                .as_ref()
                .ok()
                .and_then(|r| r.usage.as_ref().map(|u| u.prompt_tokens))
                .unwrap_or(0);
            let ct = result
                .as_ref()
                .ok()
                .and_then(|r| r.usage.as_ref().map(|u| u.completion_tokens))
                .unwrap_or(0);
            println!(
                "    [{}/{}] {} | {} | tokens: {}P+{}C | {}ms | {}",
                i + 1,
                total,
                status,
                id,
                pt,
                ct,
                latency,
                reason,
            );
        }
    }

    (
        passed,
        total as u32,
        total_prompt,
        total_completion,
        total_latency,
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    load_dotenv_overriding();
    // Map Groq API key to OPENAI_API_KEY for the OpenAI provider
    std::env::set_var(
        "OPENAI_API_KEY",
        std::env::var("GROQ_API_KEY").unwrap_or_default(),
    );
    std::env::set_var("OPENAI_BASE_URL", "https://api.groq.com/openai/v1");

    // Determine models to test
    let models: Vec<&str> = if let Some(m) = &args.models {
        m.split(',').collect()
    } else {
        GROQ_MODELS.to_vec()
    };

    // Load BFCL data
    let data_dir = PathBuf::from("volt-bfcl/data");
    let all_categories = load_cases(&data_dir)?;

    // Filter categories if specified
    let active_categories: Vec<(String, Vec<BfclCase>)> = if let Some(cat_str) = &args.categories {
        let wanted: std::collections::HashSet<&str> =
            cat_str.split(',').map(|s| s.trim()).collect();
        all_categories
            .into_iter()
            .filter(|(name, _)| wanted.contains(name.as_str()))
            .collect()
    } else {
        all_categories
    };

    // ── Run benchmark ──
    println!("\n{}", "=".repeat(90));
    println!("{:^90}", "BFCL v4 BENCHMARK — Pure Rust");
    println!("{}", "=".repeat(90));
    println!("Models:   {}", models.join(", "));
    println!(
        "Categories: {}",
        active_categories
            .iter()
            .map(|(n, c)| format!("{} ({} cases)", n, c.len()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("Limit:    {} cases per category", args.limit);
    println!("Provider: Groq (api.groq.com/openai/v1)");
    println!("{}", "=".repeat(90));

    #[allow(clippy::type_complexity)]
    let mut all_results: Vec<(String, String, u32, u32, f64, f64, u64, u64, f64)> = Vec::new();

    for model in &models {
        // Build provider
        let route = volt::orchestrator::resolve_provider(model);
        if route.api_key.is_empty() {
            eprintln!("\n  [skip] {}: no API key available", model);
            continue;
        }
        let provider =
            OpenAIProvider::new(route.api_key, route.base_url, format!("bfcl-{}", model));

        println!("\n{}", "-".repeat(90));
        println!("  MODEL: {}", model);
        println!("{}", "-".repeat(90));

        for (cat_name, cases) in &active_categories {
            print!("  [{:20}] ", cat_name);
            let (passed, total, pt, ct, latency) =
                run_category(&provider, model, cat_name, cases, args.limit, args.quiet).await;
            let acc = if total > 0 {
                passed as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let avg_lat = if total > 0 {
                latency as f64 / total as f64
            } else {
                0.0
            };
            println!(
                "{}/{} = {:5.1}% | tokens: {}P+{}C | avg {}ms",
                passed, total, acc, pt, ct, avg_lat as u64
            );

            all_results.push((
                model.to_string(),
                cat_name.clone(),
                passed,
                total,
                acc,
                avg_lat,
                pt,
                ct,
                avg_lat,
            ));
        }
    }

    // ── Final summary table ──
    println!("\n{}", "=".repeat(90));
    println!("{:^90}", "FINAL RESULTS");
    println!("{}", "=".repeat(90));
    print!("{:<30}", "Model");
    for (cat_name, _) in &active_categories {
        print!(" {:>10}", &cat_name[..cat_name.len().min(10)]);
    }
    println!(" {:>8}", "AVG");
    println!("{}", "-".repeat(90));

    for model in &models {
        print!("{:<30}", model);
        let mut sum_acc = 0.0;
        let mut cat_count = 0;
        for (cat_name, _) in &active_categories {
            let acc = all_results
                .iter()
                .find(|(m, c, _, _, _, _, _, _, _)| m == model && c == cat_name)
                .map(|r| r.4)
                .unwrap_or(-1.0);
            if acc >= 0.0 {
                print!(" {:>9.1}%", acc);
                sum_acc += acc;
                cat_count += 1;
            } else {
                print!(" {:>10}", "N/A");
            }
        }
        let avg = if cat_count > 0 {
            sum_acc / cat_count as f64
        } else {
            0.0
        };
        println!(" {:>7.1}%", avg);
    }
    println!("{}", "=".repeat(90));

    // Save results
    let results_path = PathBuf::from("volt-bfcl/results/bfcl_v4_rust_results.json");
    if let Some(parent) = results_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let output = serde_json::json!({
        "models": models,
        "categories": active_categories.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        "results": all_results.iter().map(|(m, c, p, t, a, l, pt, ct, _)| {
            serde_json::json!({
                "model": m,
                "category": c,
                "passed": p,
                "total": t,
                "accuracy_pct": format!("{:.1}", a),
                "avg_latency_ms": format!("{:.0}", l),
                "prompt_tokens": pt,
                "completion_tokens": ct,
            })
        }).collect::<Vec<_>>(),
    });
    std::fs::write(&results_path, serde_json::to_string_pretty(&output)?)?;
    println!("\nResults saved to: {}", results_path.display());
    println!();

    Ok(())
}
