use crate::models::ToolResult;
use std::time::Instant;

pub fn todo_add(task: &str) -> ToolResult {
    let started = Instant::now();
    let path = ".volt_tasks.md";
    let entry = format!("- [ ] {}\n", task);
    match std::fs::OpenOptions::new().append(true).create(true).open(path) {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(e) = file.write_all(entry.as_bytes()) {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("todo write failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }
            ToolResult {
                success: true,
                output: format!("added task: {}", task),
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("todo open failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}