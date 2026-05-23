use crate::models::ToolResult;
use crate::tools::path::resolve_path;
use std::time::Instant;

pub fn write_file(path: &str, content: &str) -> ToolResult {
    let path = resolve_path(path);
    let started = Instant::now();
    match std::fs::write(&path, content) {
        Ok(()) => ToolResult {
            success: true,
            output: format!("wrote {} bytes to {}", content.len(), path),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("write failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
