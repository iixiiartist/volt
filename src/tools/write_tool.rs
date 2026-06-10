use crate::models::ToolResult;
use crate::tools::path::resolve_path;
use std::path::Path;
use std::time::Instant;

pub async fn write_file(path: &str, content: &str) -> ToolResult {
    let path = resolve_path(path);
    let started = Instant::now();
    if let Some(parent) = Path::new(&path).parent() {
        if !parent.exists() {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "parent directory '{}' does not exist; create it first (auto-create disabled to prevent planting code in sensitive locations like .git/hooks)",
                    parent.display()
                )),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_file_rejects_missing_parent() {
        // .git/hooks/evil does not exist in a fresh tempdir — should fail
        // with a clear error rather than auto-creating it.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested").join("file.txt");
        let result = futures::executor::block_on(write_file(target.to_str().unwrap(), "x"));
        assert!(!result.success, "must not auto-create missing parents");
        assert!(result.error.unwrap().contains("does not exist"));
        assert!(!target.exists(), "file must not have been created");
    }
}
