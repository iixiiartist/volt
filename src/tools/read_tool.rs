use crate::models::ToolResult;
use std::time::Instant;

pub fn read_file(path: &str) -> ToolResult {
    let started = Instant::now();
    match std::fs::read_to_string(path) {
        Ok(content) => ToolResult {
            success: true,
            output: content,
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("read failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
