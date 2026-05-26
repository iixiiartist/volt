use crate::models::ToolResult;
use std::time::Instant;

pub async fn final_answer(answer: &str) -> ToolResult {
    let started = Instant::now();
    ToolResult {
        success: true,
        output: answer.to_string(),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}
