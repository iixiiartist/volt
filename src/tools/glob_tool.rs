use crate::models::ToolResult;
use crate::tools::path::resolve_path;
use regex::Regex;
use std::time::Instant;
use walkdir::WalkDir;

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".hg", ".svn"];

fn should_skip(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|n| SKIP_DIRS.contains(&n))
}

fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

fn compile_glob(pattern: &str) -> Result<Regex, regex::Error> {
    let separator = if is_windows() { r"[^/\\]" } else { r"[^/]" };
    let pat = pattern
        .replace("**", "__DOUBLE__")
        .replace("*", separator)
        .replace("__DOUBLE__", ".*");
    Regex::new(&format!("^{}$", pat))
}

pub async fn glob_files(pattern: &str, base: &str) -> ToolResult {
    let base = resolve_path(base);
    let started = Instant::now();

    let re = match compile_glob(pattern) {
        Ok(r) => r,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("invalid glob pattern: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };

    let mut matches = Vec::new();
    for entry in WalkDir::new(&base)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let path = entry.path().to_string_lossy();
            if re.is_match(&path) {
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
