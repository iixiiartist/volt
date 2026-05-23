use crate::models::ToolResult;
use crate::tools::path::resolve_path;
use regex::Regex;
use std::time::Instant;

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".hg", ".svn"];

fn should_skip(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|n| SKIP_DIRS.contains(&n))
}

const MAX_RESULTS: usize = 1000;

pub async fn grep_files(pattern: &str, path: &str) -> ToolResult {
    let path = resolve_path(path);
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
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path_str = entry.path().to_string_lossy();
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            for (i, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    results.push(format!("{}:{}: {}", path_str, i + 1, line.trim()));
                    if results.len() >= MAX_RESULTS {
                        break;
                    }
                }
            }
        }
        if results.len() >= MAX_RESULTS {
            break;
        }
    }

    if results.len() >= MAX_RESULTS {
        results.push("-- results truncated at 1000 matches --".into());
    }

    ToolResult {
        success: true,
        output: serde_json::to_string_pretty(&results).unwrap_or_default(),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}