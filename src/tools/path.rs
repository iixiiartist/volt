use std::path::Path;

fn find_project_root() -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("Cargo.toml").exists() {
            return Some(dir.to_string_lossy().to_string());
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn resolve_path(path: &str) -> String {
    let p = Path::new(path);
    if p.exists() {
        return path.to_string();
    }
    let root = match find_project_root() {
        Some(r) => r,
        None => return path.to_string(),
    };
    // Try root/path (handles relative paths and absolute paths with leading / stripped)
    let candidate = Path::new(&root).join(path.trim_start_matches('/').trim_start_matches('\\'));
    if candidate.exists() {
        return candidate.to_string_lossy().to_string();
    }
    // Model often fabricates Linux paths like /home/user/project/src.
    // Try the last 1-3 components joined against the project root.
    let components: Vec<&str> = path.split(&['/', '\\'][..]).filter(|s| !s.is_empty()).collect();
    for i in (1..=components.len().min(3)).rev() {
        let suffix = components[components.len()-i..].join("/");
        let candidate = Path::new(&root).join(&suffix);
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }
    path.to_string()
}
