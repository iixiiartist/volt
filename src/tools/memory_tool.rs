use crate::models::ToolResult;
use std::time::Instant;

pub async fn memory_append(kind: &str, content: &str) -> ToolResult {
    let started = Instant::now();
    let path = "MEMORY.md";
    let entry = format!("## {}\n{}\n\n", kind, content);
    match std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(e) = writeln!(file, "{}", entry) {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("memory write failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }
            ToolResult {
                success: true,
                output: format!("appended to {}: {}", path, kind),
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("memory open failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
