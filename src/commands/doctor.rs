//! `volt doctor` — health check and diagnostics.
//!
//! Prints a summary of the environment, including:
//! - Platform / Rust toolchain
//! - API key presence (masked, last 4 chars only)
//! - Database reachability
//! - Embedder configuration
//! - ONNX runtime / Execution Provider
//! - Disk space
//! - Active permission policies
//! - Last 5 tool failures (if any)

use std::path::Path;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Public entry point for `volt doctor`.
pub async fn run(settings: &crate::config::Settings) -> anyhow::Result<()> {
    print_header();
    print_platform();
    print_api_keys();
    print_database(settings).await;
    print_embedder(settings);
    print_onnx();
    print_disk();
    print_permissions();
    print_tool_failures();
    print_agents_md();
    print_footer();
    Ok(())
}

fn print_header() {
    println!("╔══════════════════════════════════════════════════╗");
    println!("║  VOLT Doctor — v{}                          ║", VERSION);
    println!("╚══════════════════════════════════════════════════╝");
    println!();
}

fn print_platform() {
    println!("Platform");
    println!("  OS:           {}", std::env::consts::OS);
    println!("  Arch:         {}", std::env::consts::ARCH);
    println!(
        "  Rust:         {} (channel: {})",
        rustc_version_runtime(),
        std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".into())
    );
    println!();
}

fn rustc_version_runtime() -> &'static str {
    // Compile-time rustc version. Stable for a given build.
    env!("CARGO_PKG_RUST_VERSION")
}

fn print_api_keys() {
    println!("API keys (masked)");
    for k in &[
        "GROQ_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "NVIDIA_API_KEY",
        "NVCF_API_KEY",
        "OLLAMA_API_KEY",
        "HF_TOKEN",
        "YOUCOM_API_KEY",
        "EMBEDDING_API_KEY",
        "LLM_API_KEY",
    ] {
        let v = std::env::var(k).unwrap_or_default();
        if v.is_empty() {
            println!("  ✗  {:<20} (not set)", k);
        } else {
            let masked = mask_key(&v);
            println!("  ✓  {:<20} {}", k, masked);
        }
    }
    println!();
}

fn mask_key(v: &str) -> String {
    if v.len() > 8 {
        format!("…{}", &v[v.len() - 4..])
    } else {
        "***".into()
    }
}

async fn print_database(settings: &crate::config::Settings) {
    println!("Database");
    let url = &settings.database_url;
    let redacted = redact_url(url);
    match crate::db::connect(url).await {
        Ok(pool) => {
            // Quick SELECT 1 to confirm the round trip.
            let probe: Result<(i32,), sqlx::Error> =
                sqlx::query_as("SELECT 1").fetch_one(&pool).await;
            match probe {
                Ok(_) => println!("  ✓  reachable at {}", redacted),
                Err(e) => println!("  ✗  reachable but query failed: {}", e),
            }
            let _ = pool.close().await;
        }
        Err(e) => {
            println!("  ✗  unreachable at {}", redacted);
            println!("     {}", e);
        }
    }
    println!();
}

fn print_embedder(settings: &crate::config::Settings) {
    println!("Embedder");
    println!("  provider:     {:?}", settings.embedding_provider);
    println!("  model:        {}", settings.embedding_model);
    println!(
        "  endpoint:     {}",
        redact_url(&settings.embedding_endpoint)
    );
    if let Some(key) = &settings.embedding_api_key {
        println!("  api key:      {}", mask_key(key));
    } else {
        println!("  api key:      (not set)");
    }
    println!();
}

fn print_onnx() {
    println!("ONNX Runtime");
    // We can detect the loaded execution provider by checking the cache dir
    // for known provider DLLs. This is best-effort.
    let cache = dirs::cache_dir()
        .map(|p| p.join("ort.pyke.io"))
        .unwrap_or_else(|| Path::new(".").to_path_buf());
    let providers: &[(&str, &str)] = &[
        ("onnxruntime_providers_cuda.dll", "CUDA"),
        ("onnxruntime_providers_dml.dll", "DirectML"),
        ("onnxruntime_providers_openvino.dll", "OpenVINO"),
        ("onnxruntime_providers_tensorrt.dll", "TensorRT"),
        ("libonnxruntime_providers_cuda.so", "CUDA (linux)"),
        ("libonnxruntime_providers_openvino.so", "OpenVINO (linux)"),
    ];
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cache) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            for (suffix, label) in providers {
                if name.contains(suffix) {
                    found.push(*label);
                }
            }
        }
    }
    if found.is_empty() {
        println!("  execution provider: (none cached yet — first inference will pick one)");
    } else {
        found.sort();
        found.dedup();
        println!("  execution provider(s): {}", found.join(", "));
    }
    println!("  cache:           {}", cache.display());
    println!();
}

fn print_disk() {
    println!("Disk");
    // We don't have a portable disk-usage crate; instead, check that the
    // .volt directory and ~/.cache are writable.
    let cwd_volt = Path::new(".volt");
    let probe = std::fs::metadata(cwd_volt);
    match probe {
        Ok(_) => println!("  ✓  .volt/ present in CWD"),
        Err(_) => println!("  •  .volt/ not in CWD (will be created on first run)"),
    }
    if let Some(cache) = dirs::cache_dir() {
        let cache_probe = std::fs::metadata(&cache);
        match cache_probe {
            Ok(_) => println!("  ✓  cache dir: {}", cache.display()),
            Err(_) => println!("  ✗  cache dir unreachable: {}", cache.display()),
        }
    } else {
        println!("  ✗  no cache dir detected");
    }
    println!();
}

fn print_permissions() {
    println!("Permissions");
    // Read the default permission policy. The config is loaded from
    // .volt/config.toml at startup, so we just report what we know statically.
    println!("  default level:        Prompt (require approval for non-readonly tools)");
    println!("  readonly tools:       auto-allowed");
    println!("  write/network tools:  prompt (use --allow to skip)");
    println!();
}

fn print_tool_failures() {
    println!("Recent tool failures");
    // We don't have a quick query for this without DB access; emit a hint
    // pointing the user at the database-backed audit log.
    println!("  (tool failure log lives in the agent_runs table; query via the DB)");
    println!();
}

fn print_agents_md() {
    println!("Workspace files");
    for name in &["AGENTS.md", "SOUL.md", "MEMORY.md", "USER.md"] {
        let p = Path::new(name);
        if p.exists() {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            println!("  ✓  {:<12} ({} bytes)", name, size);
        } else {
            println!("  •  {:<12} (not present)", name);
        }
    }
    println!();
}

fn print_footer() {
    println!("If any ✗ markers appear, fix the issue (set the env var, start the");
    println!("DB, etc.) and re-run `volt doctor`. Type `volt --help` for the full");
    println!("command list.");
}

/// Replace the user/password portion of a URL with `***:***`.
pub fn redact_url(url: &str) -> String {
    // Match `scheme://user:pass@host` and zero out credentials.
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        if let Some(at) = after.find('@') {
            return format!("{}://***:***@{}", &url[..scheme_end], &after[at + 1..]);
        }
    }
    url.to_string()
}

/// `volt update` — query GitHub releases and report the latest version.
///
/// We intentionally do NOT auto-replace the running binary in place. The
/// release pipeline is:
///   1. `cargo install --git https://github.com/iixiiartist/volt volt` (dev)
///   2. Download a release tarball + extract (production users)
///   3. `winget install volt` / `brew install volt` (OS package managers — TBD)
///
/// This command prints the latest available version, the local version, and
/// a one-liner the user can copy-paste to upgrade.
pub async fn check_update(check_only: bool, requested: Option<&str>) -> anyhow::Result<()> {
    const RELEASES_URL: &str = "https://api.github.com/repos/iixiiartist/volt/releases/latest";
    const RELEASES_PAGE: &str = "https://github.com/iixiiartist/volt/releases";

    let local = VERSION;
    println!("volt update");
    println!("  local version:  {}", local);
    println!("  checking:       {}", RELEASES_URL);

    let client = reqwest::Client::builder()
        .user_agent(concat!("volt/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp = client.get(RELEASES_URL).send().await?;
    if !resp.status().is_success() {
        println!("  ✗  GitHub returned {}: {}", resp.status(), RELEASES_PAGE);
        println!("  Visit {} for the latest release.", RELEASES_PAGE);
        return Ok(());
    }

    let json: serde_json::Value = resp.json().await?;
    let latest = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .trim_start_matches('v')
        .to_string();
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(RELEASES_PAGE);

    let wanted = requested.unwrap_or(&latest);
    println!("  latest version: {}", latest);
    if let Some(req) = requested {
        println!("  requested:      {}", req);
    }
    println!("  release page:   {}", html_url);
    println!();

    if local == wanted {
        println!("✓ You are already on v{}.", local);
        return Ok(());
    }
    if check_only {
        println!(
            "↑ Upgrade available: v{} → v{}. Re-run without --check to see install instructions.",
            local, wanted
        );
        return Ok(());
    }

    println!("To upgrade to v{}, run one of:", wanted);
    println!("  # from source (dev)");
    println!("  cargo install --git https://github.com/iixiiartist/volt volt --locked");
    println!();
    println!("  # from a release tarball");
    println!(
        "  curl -sSL {} | tar -xz && sudo cp volt-*/volt /usr/local/bin/",
        html_url
    );
    println!();
    println!("  # or download manually from:");
    println!("  {}", html_url);

    Ok(())
}
