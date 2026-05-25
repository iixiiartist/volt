use crate::agent::loop_rs::Agent;
use std::time::Instant;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EvalTask {
    pub task: String,
    pub expected_substrings: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EvalSuite {
    pub name: String,
    pub tasks: Vec<EvalTask>,
}

#[derive(Debug, Clone)]
pub struct EvalResult {
    pub task: String,
    pub passed: bool,
    pub output: String,
    pub duration_ms: u128,
    pub missing_substrings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EvalSummary {
    pub suite_name: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub total_duration_ms: u128,
    pub results: Vec<EvalResult>,
}

pub async fn run_suite(suite: &EvalSuite, agent: &Agent) -> EvalSummary {
    let started = Instant::now();
    let mut results = Vec::with_capacity(suite.tasks.len());

    for eval_task in &suite.tasks {
        let task_started = Instant::now();
        let output = match agent.run(&eval_task.task).await {
            Ok(o) => o,
            Err(e) => format!("error: {}", e),
        };

        let mut passed = true;
        let mut missing = Vec::new();

        if let Some(ref expected) = eval_task.expected_substrings {
            for sub in expected {
                if !output.contains(sub.as_str()) {
                    passed = false;
                    missing.push(sub.clone());
                }
            }
        }

        results.push(EvalResult {
            task: eval_task.task.clone(),
            passed,
            output,
            duration_ms: task_started.elapsed().as_millis(),
            missing_substrings: missing,
        });
    }

    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;

    EvalSummary {
        suite_name: suite.name.clone(),
        total,
        passed,
        failed,
        total_duration_ms: started.elapsed().as_millis(),
        results,
    }
}

pub fn print_summary(summary: &EvalSummary) {
    println!("\n=== Eval Suite: {} ===", summary.suite_name);
    println!(
        "Total: {}  Passed: {}  Failed: {}  Duration: {}ms",
        summary.total, summary.passed, summary.failed, summary.total_duration_ms
    );
    println!();

    for result in &summary.results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        println!(
            "  [{}] {:?} ({}ms)",
            status,
            result.task.chars().take(60).collect::<String>(),
            result.duration_ms
        );
        if !result.passed && !result.missing_substrings.is_empty() {
            for s in &result.missing_substrings {
                println!("       missing: {:?}", s);
            }
        }
    }
    println!();
}
