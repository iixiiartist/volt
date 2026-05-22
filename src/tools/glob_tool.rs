use crate::models::ToolResult;
use std::time::Instant;
use walkdir::WalkDir;

pub fn glob_files(pattern: &str, base: &str) -> ToolResult {
    let started = Instant::now();
    let mut matches = Vec::new();
    for entry in WalkDir::new(base).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path().to_string_lossy();
            if simple_glob_match(pattern, &path) {
                matches.push(path.to_string());
            }
        }
    }
    matches.sort();
    ToolResult {
        success: true,
        output: serde_json::to_string_pretty(&matches).unwrap_or_default(),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}

fn simple_glob_match(pattern: &str, path: &str) -> bool {
    let pat = pattern.replace("**", "__DOUBLE__").replace("*", "[^/]*").replace("__DOUBLE__", ".*");
    let re = format!("^{}$", pat);
    regex::Regex::new(&re).map(|r| r.is_match(path)).unwrap_or(false)
}