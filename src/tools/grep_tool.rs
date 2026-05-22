use crate::models::ToolResult;
use regex::Regex;
use std::time::Instant;

pub fn grep_files(pattern: &str, path: &str) -> ToolResult {
    let started = Instant::now();
    let re = match Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("invalid regex: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };

    let mut results = Vec::new();
    for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path_str = entry.path().to_string_lossy();
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                for (i, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        results.push(format!("{}:{}: {}", path_str, i + 1, line.trim()));
                    }
                }
            }
        }
    }

    ToolResult {
        success: true,
        output: serde_json::to_string_pretty(&results).unwrap_or_default(),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}