use crate::models::ToolResult;
use crate::tools::path::resolve_path;
use std::time::Instant;

pub async fn edit_file(path: &str, old_string: &str, new_string: &str) -> ToolResult {
    let path = resolve_path(path);
    let started = Instant::now();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            if !content.contains(old_string) {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("old_string not found in {}", path)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }
            let new_content = content.replacen(old_string, new_string, 1);
            match std::fs::write(&path, &new_content) {
                Ok(()) => ToolResult {
                    success: true,
                    output: format!(
                        "edited {} ({} replacements)",
                        path,
                        content.matches(old_string).count()
                    ),
                    error: None,
                    duration_ms: started.elapsed().as_millis(),
                },
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("edit write failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("edit read failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
