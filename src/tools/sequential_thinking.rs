use crate::models::ToolResult;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

static THOUGHTS: LazyLock<Mutex<HashMap<String, Vec<Thought>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Thought {
    number: u32,
    thought: String,
}

pub async fn sequentialthinking(
    thought: &str,
    next_thought_needed: bool,
    branch_id: Option<&str>,
    branch_from_thought: Option<u32>,
) -> ToolResult {
    let started = Instant::now();
    let branch_key = branch_id.unwrap_or("default").to_string();

    let mut store = THOUGHTS.lock().unwrap();
    let thoughts = store.entry(branch_key.clone()).or_default();

    let thought_number = thoughts.len() as u32 + 1;
    thoughts.push(Thought {
        number: thought_number,
        thought: thought.to_string(),
    });

    let history: Vec<String> = thoughts
        .iter()
        .map(|t| format!("Thought {}: {}", t.number, t.thought))
        .collect();

    let mut output = format!(
        "=== Sequential Thought Chain (branch: {}) ===\n",
        branch_key
    );
    output.push_str(&history.join("\n"));
    output.push_str(&format!(
        "\n---\nCurrent thought: {} of {}",
        thought_number, thought_number
    ));

    if let Some(from) = branch_from_thought {
        output.push_str(&format!("\nBranched from thought {}", from));
    }

    if next_thought_needed {
        output.push_str("\nNext thought needed — continue the reasoning chain.");
    } else {
        output.push_str("\nReasoning complete.");
    }

    ToolResult {
        success: true,
        output,
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}
