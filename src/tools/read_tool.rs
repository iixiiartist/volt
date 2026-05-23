use crate::models::ToolResult;
use crate::tools::path::resolve_path;
use std::time::Instant;

pub fn read_file(path: &str) -> ToolResult {
    let path = resolve_path(path);
    let started = Instant::now();
    match std::fs::read_to_string(&path) {
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
