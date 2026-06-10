#[cfg(feature = "webui")]
fn main() {
    // Pick up `.env` from several plausible locations so the webui
    // works no matter how the user launched it:
    //   1. Next to webui.exe (`Program Files\Volt\.env`)
    //   2. The current working directory
    //   3. The user's project root, found by walking up from CWD until
    //      we hit a directory containing `Cargo.toml` (most dev case:
    //      user double-clicked `target\debug\webui.exe` and CWD is
    //      `target\debug\`).
    // We try each in order; first hit wins. `dotenvy::from_path` only
    // sets variables that don't already exist, so later hits don't
    // clobber earlier ones.
    let env_candidates = collect_env_candidates();
    for path in &env_candidates {
        if path.exists() {
            let _ = dotenvy::from_path(path);
        }
    }

    // Loud-fail check: at least one LLM API key should be present.
    let llm_keys = [
        "GROQ_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "NVIDIA_API_KEY",
        "NVCF_API_KEY",
        "OLLAMA_API_KEY",
        "LLM_API_KEY",
    ];
    if !llm_keys.iter().any(|k| std::env::var(k).is_ok()) {
        eprintln!(
            "[webui] WARNING: no LLM API key found. Set GROQ_API_KEY, OPENAI_API_KEY, \
             ANTHROPIC_API_KEY, NVIDIA_API_KEY, or OLLAMA_API_KEY in your environment \
             or in a .env file next to webui.exe / in the project root."
        );
    }

    use volt::webui::app::App;
    dioxus::launch(App);
}

/// Build an ordered list of `.env` paths to try, in priority order.
fn collect_env_candidates() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join(".env"));
            // `target/debug/webui.exe` → also try `target/.env` and the
            // workspace root one level above `target/`.
            if let Some(target_dir) = dir.parent() {
                out.push(target_dir.join(".env"));
                if let Some(workspace) = target_dir.parent() {
                    out.push(workspace.join(".env"));
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join(".env"));
        // Walk up from CWD looking for the project root.
        let mut probe = cwd.as_path();
        for _ in 0..6 {
            if let Some(parent) = probe.parent() {
                if parent.join("Cargo.toml").exists() {
                    out.push(parent.join(".env"));
                    break;
                }
                probe = parent;
            } else {
                break;
            }
        }
    }
    out
}

#[cfg(not(feature = "webui"))]
fn main() {}
