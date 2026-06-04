//! `volt init` — scaffold a new project with `AGENTS.md` (and friends).
//!
//! Walks the current directory, infers the project type, and writes:
//! - `AGENTS.md` — repo-specific guidance loaded into the system prompt
//! - `SOUL.md`   — persona/voice customisation (optional)
//! - `MEMORY.md` — persistent agent memory (optional)
//! - `USER.md`   — user preferences (optional)
//!
//! Re-running with `--force` overwrites. Re-running without `--force` only
//! writes files that don't already exist.

use std::path::{Path, PathBuf};

use inquire::Confirm;

const TEMPLATE_HEADER: &str = "# AGENTS.md\n\n\
# Project-specific instructions for the Volt agent.\n\
# This file is auto-loaded into the system prompt at agent start.\n\
# Anything you write here is treated as high-priority guidance.\n\n";

const TEMPLATE_RUST: &str = "## Project type\n\nRust crate.\n\n\
## Build & test\n\n\
- `cargo build` — debug build\n\
- `cargo build --release` — release build\n\
- `cargo test --lib` — unit tests (225+ in this codebase)\n\
- `cargo test --test cli_integration_tests` — CLI integration tests\n\
- `cargo clippy -- -D warnings` — lint gate (must pass before commit)\n\
- `cargo fmt` — formatting (run before commit)\n\n\
## Layout\n\n\
- `src/main.rs` — CLI entry point (clap subcommands)\n\
- `src/agent/` — agent loop, prompt builder, tool parser\n\
- `src/tools/` — tool implementations grouped by category\n\
- `src/llm/` — LLM provider trait + per-provider impls\n\
- `src/db/` — PostgreSQL + sqlx persistence\n\
- `tests/` — integration + benchmark tests\n\
- `blueprints/` — TOML agent blueprints (Groq, NIM, Ollama, edge)\n\n\
## Conventions\n\n\
- No `unwrap()` outside tests; use `anyhow::Result` + `?`\n\
- `cargo clippy -- -D warnings` is the CI gate\n\
- One commit per logical change; no fixup commits on shared branches\n\
- All chatty output is gated through a `chat!` macro in `agent_run.rs`\n\n";

const TEMPLATE_NODE: &str = "## Project type\n\nNode.js package.\n\n\
## Build & test\n\n\
- `npm install` — install dependencies\n\
- `npm test` — run tests\n\
- `npm run build` — production build\n\
- `npm run lint` — lint\n\n\
## Layout\n\n\
- `src/` — TypeScript source\n\
- `tests/` — test files\n\
- `dist/` — build output (gitignored)\n\n";

const TEMPLATE_PYTHON: &str = "## Project type\n\nPython package.\n\n\
## Build & test\n\n\
- `pip install -e .` — editable install\n\
- `pytest` — run tests\n\
- `ruff check .` — lint\n\
- `mypy src/` — type check\n\n\
## Layout\n\n\
- `src/` — package source\n\
- `tests/` — pytest tests\n\
- `pyproject.toml` — project metadata + dependencies\n\n";

const TEMPLATE_GENERIC: &str = "## Project type\n\nUnknown / mixed.\n\n\
## Build & test\n\nDescribe the commands to build, test, and lint this project.\n\n\
## Layout\n\nDescribe the important directories and what lives in each.\n\n";

const SOUL_TEMPLATE: &str = "# SOUL.md\n\n\
# The agent's persona, voice, and behavioural defaults.\n\
# Loaded into the system prompt alongside AGENTS.md.\n\n\
## Voice\n\nTerse, technical, friendly. Use plain prose unless the user asks\nfor tables/lists. No emojis unless the user uses them first.\n\n\
## Defaults\n\n- Prefer direct answers over hedging\n- Skip tool calls for simple factual questions\n- Show diffs in unified format\n- Avoid <tool_call> / </tool_call> fences in user-facing output\n\n";

const MEMORY_TEMPLATE: &str = "# MEMORY.md\n\n\
# Persistent notes the agent can read across sessions.\n\
# Append-only. Use sparingly — keep it under 200 lines.\n\n\
## Conventions\n\n- Add entries as `## YYYY-MM-DD — short title` sections\n- Each entry should be 1–5 lines\n- Remove stale entries quarterly\n\n\
## Project-specific facts\n\n";

const USER_TEMPLATE: &str = "# USER.md\n\n\
# Persistent notes about the user (preferences, style, environment).\n\
# Loaded into the system prompt alongside AGENTS.md.\n\n\
## Environment\n\n- OS: (Windows / macOS / Linux)\n- Editor: (VS Code / Vim / etc.)\n- Shell: (PowerShell / zsh / bash / fish)\n\n\
## Preferences\n\n- Tab size: 4\n- Indent: spaces\n- Line endings: LF\n\n\
## Style\n\n- Be concise. Use lists for options, prose for explanation.\n- Skip pleasantries; jump to the answer.\n\n";

/// Public entry point for `volt init`.
pub async fn run(force: bool, only: Option<&str>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    println!("Indexing project at {}", cwd.display());
    println!();

    let project_type = detect_project_type(&cwd);
    println!("  Detected project type: {}", project_type);
    println!();

    // Build a list of (filename, content, description) tuples up front so
    // borrowed strings don't dangle past their temporary owners.
    let targets: Vec<(&str, String, &'static str)> = vec![
        (
            "AGENTS.md",
            scaffold_agents_md(project_type),
            "project guidance",
        ),
        ("SOUL.md", SOUL_TEMPLATE.to_string(), "agent persona"),
        (
            "MEMORY.md",
            MEMORY_TEMPLATE.to_string(),
            "persistent memory",
        ),
        ("USER.md", USER_TEMPLATE.to_string(), "user preferences"),
    ];

    let only_filter = |name: &str| -> bool {
        match only {
            Some(o) => o.eq_ignore_ascii_case(name),
            None => true,
        }
    };

    for (filename, content, desc) in &targets {
        if !only_filter(filename) {
            continue;
        }
        let target = cwd.join(filename);
        let action = write_or_skip(&target, content, force).await?;
        match action {
            WriteAction::Wrote => println!("  ✓ wrote {} ({})", target.display(), desc),
            WriteAction::UserDeclined => {
                println!("  • kept {} (user declined overwrite)", target.display())
            }
        }
    }

    println!();
    println!(
        "Project indexed. Next: `volt agent-run --input \"...\"` will use {}.",
        cwd.join("AGENTS.md").display()
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteAction {
    Wrote,
    UserDeclined,
}

async fn write_or_skip(target: &Path, content: &str, force: bool) -> anyhow::Result<WriteAction> {
    if target.exists() {
        if force {
            std::fs::write(target, content)?;
            return Ok(WriteAction::Wrote);
        }
        // Without --force, confirm only for files that look user-modified
        // (have non-trivial size). We always skip and ask; the user can
        // pass --force to overwrite later.
        let should_overwrite = Confirm::new(&format!(
            "  {} already exists. Overwrite?",
            target
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
        ))
        .with_default(false)
        .with_help_message("pass --force to overwrite without prompting")
        .prompt()
        .unwrap_or(false);
        if should_overwrite {
            std::fs::write(target, content)?;
            Ok(WriteAction::Wrote)
        } else {
            Ok(WriteAction::UserDeclined)
        }
    } else {
        std::fs::write(target, content)?;
        Ok(WriteAction::Wrote)
    }
}

fn detect_project_type(cwd: &Path) -> &'static str {
    if cwd.join("Cargo.toml").exists() {
        "rust"
    } else if cwd.join("package.json").exists() {
        "node"
    } else if cwd.join("pyproject.toml").exists()
        || cwd.join("setup.py").exists()
        || cwd.join("requirements.txt").exists()
    {
        "python"
    } else if cwd.join("go.mod").exists() {
        "go"
    } else if cwd.join("pom.xml").exists() || cwd.join("build.gradle").exists() {
        "java"
    } else {
        "generic"
    }
}

fn scaffold_agents_md(project_type: &str) -> String {
    let body = match project_type {
        "rust" => TEMPLATE_RUST,
        "node" => TEMPLATE_NODE,
        "python" => TEMPLATE_PYTHON,
        _ => TEMPLATE_GENERIC,
    };
    format!("{}{}", TEMPLATE_HEADER, body)
}

/// Returns the list of project types this scaffolder recognises.
/// Used by `volt --help` and the first-run wizard.
pub fn recognised_types() -> Vec<&'static str> {
    vec!["rust", "node", "python", "go", "java", "generic"]
}

/// Returns the AGENTS.md target path for a given CWD.
pub fn target_path(cwd: &Path) -> PathBuf {
    cwd.join("AGENTS.md")
}
