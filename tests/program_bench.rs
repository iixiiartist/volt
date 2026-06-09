use std::sync::Arc;
use std::time::Instant;
use volt::agent::Agent;
use volt::models::*;
use volt::tools::ToolRegistry;

const PROBLEMS: &[(&str, &str, &str)] = &[
    ("two_sum", "Write a Python function two_sum(nums, target) that returns indices of two numbers that add up to target. Then call it and print the result of two_sum([2,7,11,15], 9).", "[0, 1]"),
    ("reverse_string", "Write a Python function reverse_string(s) that reverses a list of chars in-place. Test it with s=['h','e','l','l','o'] and print ''.join(s).", "olleh"),
    ("valid_parentheses", "Write a Python function is_valid(s) that checks if brackets are properly nested. Test with print(is_valid('()[]{}'), is_valid('(]'), is_valid('([)]')).", "True False False"),
    ("fizzbuzz", "Write a Python function fizzbuzz(n) returns list of strings. Print fizzbuzz(15).", "['1', '2', 'Fizz', '4', 'Buzz', 'Fizz', '7', '8', 'Fizz', 'Buzz', '11', 'Fizz', '13', '14', 'FizzBuzz']"),
    ("palindrome", "Write a Python function is_palindrome(x) checks if integer is palindrome. Test with print(is_palindrome(121), is_palindrome(-121), is_palindrome(10)).", "True False False"),
    ("single_number", "Write a Python function single_number(nums) that finds the element appearing only once. Test with print(single_number([2,2,1]), single_number([4,1,2,1,2])).", "1 4"),
    ("missing_number", "Write a Python function missing_number(nums) that returns the missing number in [0,n]. Test with print(missing_number([3,0,1]), missing_number([0,1])).", "2 2"),
    ("valid_anagram", "Write a Python function is_anagram(s,t). Test with print(is_anagram('anagram','nagaram'), is_anagram('rat','car')).", "True False"),
];

fn build_tools() -> Arc<ToolRegistry> {
    ToolRegistry::new()
}

fn build_provider() -> Box<dyn volt::llm::LLMProvider> {
    let route = volt::orchestrator::resolve_provider("llama-3.1-8b-instant");
    Box::new(volt::llm::openai::OpenAIProvider::new(
        route.api_key,
        route.base_url,
        "program-bench".into(),
    ))
}

#[tokio::test]
async fn test_program_bench() {
    let _ = dotenvy::dotenv();
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

    let provider = build_provider();
    let tools = build_tools();
    let config = AgentConfig {
        name: "program-bench".into(),
        model: "llama-3.1-8b-instant".into(),
        provider: "openai".into(),
        system_prompt: None,
        max_iterations: 5,
        temperature: 0.0,
        toolsets: vec![],
        hidden: false,
        allow_all: true,
        enabled_context_kinds: volt::models::default_context_kinds(),
        essential_tools: volt::models::default_essential_tools(),
        context_kind_quotas: Default::default(),
        use_mtp: false,
        use_cot: false,
        allow_write: false,
        framework: None,
        model_variant: None,
        quantization: None,
        format_dialect: Default::default(),
        quirks: vec![],
        strict_mode: false,
        max_tools_per_turn: None,
        blueprint_path: None,
    };
    let agent = Agent::new(config, provider, tools).await;

    let mut correct = 0u32;
    let total = PROBLEMS.len();
    let mut total_duration = 0u128;

    println!("\n{}", "=".repeat(70));
    println!("{:^70}", "Volt ProgramBench — Agent Integration Test");
    println!("{}", "=".repeat(70));
    println!("Model: llama-3.1-8b-instant  |  Problems: {}", total);
    println!("{}", "=".repeat(70));

    for (i, (pid, task, expected)) in PROBLEMS.iter().enumerate() {
        let started = Instant::now();
        let result = agent.run(task).await;
        let duration = started.elapsed().as_millis();
        total_duration += duration;

        let passed = match &result {
            Ok(output) => {
                let norm_out = output.trim().to_lowercase();
                let norm_exp = expected.trim().to_lowercase();
                norm_out.contains(&norm_exp)
            }
            Err(_) => false,
        };

        if passed {
            correct += 1;
        }
        let status = if passed { "PASS" } else { "FAIL" };
        println!(
            "  [{}/{}] {} | {} | {}ms",
            i + 1,
            total,
            status,
            pid,
            duration
        );
        if !passed {
            match &result {
                Ok(out) => println!(
                    "         expected: {} | got: {}",
                    expected,
                    out.trim().chars().take(100).collect::<String>()
                ),
                Err(e) => println!("         error: {}", e),
            }
        }
    }

    let pct = correct as f64 / total as f64 * 100.0;
    println!("{}", "=".repeat(70));
    println!("RESULTS — ProgramBench | {}ms total", total_duration);
    println!("  Accuracy: {}/{} = {:.1}%", correct, total, pct);
    println!("{}", "=".repeat(70));

    assert!(
        total > 0,
        "program_bench: PROBLEMS list is empty, cannot run"
    );
    if std::env::var("VOLT_PROGRAM_BENCH_REQUIRE_PASS").as_deref() == Ok("1") {
        assert!(
            correct > 0,
            "program_bench: agent got 0/{} correct (require at least 1)",
            total
        );
    }
}
