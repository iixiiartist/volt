use std::path::Path;
use std::sync::RwLock;

static PROJECT_ROOT: RwLock<Option<String>> = RwLock::new(None);

fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

fn find_project_root() -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("Cargo.toml").exists() || dir.join(".git").exists() {
            return Some(dir.to_string_lossy().to_string());
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn project_root() -> Option<String> {
    if let Ok(cached) = PROJECT_ROOT.read() {
        if let Some(ref root) = *cached {
            if Path::new(root).join("Cargo.toml").exists() || Path::new(root).join(".git").exists() {
                return Some(root.clone());
            }
        }
    }
    let root = find_project_root();
    if let Ok(mut cached) = PROJECT_ROOT.write() {
        *cached = root.clone();
    }
    root
}

pub fn sanitize_path(path: &str) -> Result<String, String> {
    let root_str = project_root().ok_or("no project root found")?;
    let root = Path::new(&root_str).canonicalize()
        .map_err(|e| format!("project root canonicalize: {}", e))?;

    let p = Path::new(path);
    let normalized = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };

    let canonical = normalized.canonicalize().or_else(|_| {
        if is_windows() {
            let extended = Path::new(r"\\?\").join(&normalized);
            extended.canonicalize()
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "path does not exist"))
        }
    }).map_err(|_| format!("path does not exist: {}", path))?;

    if canonical.starts_with(&root) {
        Ok(canonical.to_string_lossy().to_string())
    } else {
        Err(format!("path '{}' escapes project root", path))
    }
}

pub fn resolve_path(path: &str) -> String {
    if let Ok(safe) = sanitize_path(path) {
        return safe;
    }
    let root = match project_root() {
        Some(r) => r,
        None => return path.to_string(),
    };
    let trimmed = path.trim_start_matches('/').trim_start_matches('\\');
    let trimmed = if is_windows() {
        if let Some(drive) = trimmed.get(2..) {
            if trimmed.len() > 2 && trimmed.as_bytes().get(1) == Some(&b':') {
                drive.trim_start_matches('/').trim_start_matches('\\')
            } else {
                trimmed
            }
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    let candidate = Path::new(&root).join(trimmed);
    if let Ok(canonical) = candidate.canonicalize() {
        if canonical.starts_with(&root) {
            return canonical.to_string_lossy().to_string();
        }
    }
    let components: Vec<&str> = path.split(&['/', '\\'][..]).filter(|s| !s.is_empty()).collect();
    for i in (1..=components.len()).rev() {
        let suffix = components[components.len()-i..].join("/");
        let candidate = Path::new(&root).join(&suffix);
        if let Ok(canonical) = candidate.canonicalize() {
            if canonical.starts_with(&root) {
                return canonical.to_string_lossy().to_string();
            }
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_path_traversal_rejected() {
        let p = sanitize_path("../../../etc/passwd");
        assert!(p.is_err(), "path traversal should be rejected");
    }

    #[test]
    fn test_sanitize_path_nonexistent_rejected() {
        let p = sanitize_path("/nonexistent/path/file.txt");
        assert!(p.is_err(), "nonexistent path should be rejected");
    }

    #[test]
    fn test_sanitize_path_src_exists() {
        let p = sanitize_path("src/lib.rs");
        assert!(p.is_ok(), "src/lib.rs should be accessible: {:?}", p);
        let path = p.unwrap();
        assert!(path.ends_with("src\\lib.rs") || path.ends_with("src/lib.rs"));
    }

    #[test]
    fn test_sanitize_path_absolute_project_path() {
        let root = project_root().unwrap();
        let p = sanitize_path(&format!("{}/src/lib.rs", root));
        assert!(p.is_ok(), "absolute project path should be OK");
    }

    #[test]
    fn test_resolve_path_finds_src_lib() {
        let p = resolve_path("src/lib.rs");
        assert!(p.contains("lib.rs"), "resolve should find lib.rs: {}", p);
    }
}
