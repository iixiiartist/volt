use crate::models::ToolResult;
use crate::tools::path::resolve_path;
use std::time::Instant;

pub async fn write_file(path: &str, content: &str) -> ToolResult {
    let path = resolve_path(path);
    let started = Instant::now();
    // Auto-create parent directories to avoid "path not found" errors
    let path_obj = std::path::Path::new(&path);
    if let Some(parent) = path_obj.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("failed to create parent directories: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    }
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
