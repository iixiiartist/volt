#![deny(deprecated)]

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;
use volt::commands;

#[derive(Parser, Debug)]
#[command(name = "volt")]
#[command(about = "Volt — agent tool runtime and registry CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    InitDb,
    Validate {
        #[arg(long)]
        manifest: PathBuf,
    },
    ProvisionFile {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value_t = false)]
        marketplace_verified: bool,
    },
    Provision {
        #[arg(long)]
        pkg_id: String,
        #[arg(long)]
        registry_base_url: Option<String>,
        #[arg(long)]
        auth_token: Option<String>,
    },
    ListTools,
    History {
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    Execute {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        params: Option<String>,
    },
    Sandbox {
        #[arg(long)]
        command: String,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    AgentRun {
        #[arg(long)]
        input: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
        #[arg(long)]
        load_tools: Option<String>,
        #[arg(long, value_delimiter = ',')]
        context_kinds: Vec<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        max_iterations: Option<u32>,
        #[arg(long, default_value = "balanced")]
        mode: String,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long)]
        agent_file: Option<PathBuf>,
        #[arg(long)]
        use_mtp: bool,
        #[arg(long)]
        use_cot: bool,
        #[arg(long)]
        allow_write: bool,
        #[arg(long)]
        framework: Option<String>,
        #[arg(long)]
        model_variant: Option<String>,
        #[arg(long)]
        quantization: Option<String>,
        #[arg(long)]
        blueprint: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        auto_blueprint: bool,
        /// Emit the response to stdout and suppress progress chatter (eprintln).
        /// Ideal for piping into shell scripts.
        #[arg(long, short = 'p', default_value_t = false)]
        print: bool,
        /// Emit a single JSON envelope on stdout (one line) with the response
        /// and metadata. All other output is suppressed.
        #[arg(long, default_value_t = false)]
        json: bool,
        /// Read-only plan mode: ask the agent to output a plan as text
        /// before invoking any tool. Useful for reviewing the agent's
        /// intent before approving tool calls.
        #[arg(long, default_value_t = false)]
        plan: bool,
        /// Run the agent inside a fresh `git worktree` on a dedicated
        /// branch (`volt-session/<short-id>`). All file changes the
        /// agent makes are isolated to that worktree; review with
        /// `volt worktree list` / `volt worktree merge <id>` /
        /// `volt worktree clean <id>`.
        #[arg(long, default_value_t = false)]
        worktree: bool,
    },
    Agent {
        #[command(subcommand)]
        subcommand: AgentSubcommand,
    },
    #[command(hide = true)]
    AgentChat {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
    },
    AgentTui {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
        #[arg(long)]
        max_iterations: Option<u32>,
        #[arg(long, default_value = "balanced")]
        mode: String,
        #[arg(long)]
        use_mtp: bool,
        #[arg(long)]
        use_cot: bool,
        #[arg(long)]
        allow_write: bool,
        #[arg(long)]
        framework: Option<String>,
        #[arg(long)]
        model_variant: Option<String>,
        #[arg(long)]
        quantization: Option<String>,
        /// Run inside a fresh `git worktree` so file changes are isolated.
        #[arg(long, default_value_t = false)]
        worktree: bool,
    },
    McpServe,
    Workflow {
        #[arg(long)]
        pattern: String,
        #[arg(long)]
        agents: Option<String>,
        #[arg(long)]
        tasks: Option<String>,
        #[arg(long)]
        agents_file: Option<PathBuf>,
        #[arg(long)]
        tasks_file: Option<PathBuf>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
    },
    Eval {
        #[arg(long)]
        suite: PathBuf,
        #[arg(long)]
        model: Option<String>,
    },
    ProvisionSkill {
        #[arg(long)]
        path: PathBuf,
    },
    ListCatalogSkills {
        #[arg(long)]
        catalog_url: Option<String>,
    },
    SearchCatalogSkills {
        #[arg(long)]
        query: String,
        #[arg(long)]
        catalog_url: Option<String>,
    },
    InstallSkill {
        #[arg(long)]
        name: String,
        #[arg(long)]
        catalog_url: Option<String>,
    },
    ImportSkill {
        #[arg(long)]
        path: PathBuf,
        #[arg(long, default_value = "auto")]
        format: String,
        #[arg(long)]
        name: Option<String>,
    },
    Heartbeat,
    Migrate,
    /// Generate shell completions and write to stdout or a file.
    /// Usage: `volt completion bash > ~/.local/share/bash-completion/completions/volt`
    Completion {
        /// Target shell: bash, zsh, fish, powershell, elvish
        shell: Shell,
        /// Write to a file instead of stdout
        #[arg(long, short = 'o')]
        out: Option<PathBuf>,
    },
    /// Run a health check and report environment / config / DB status.
    Doctor,
    /// Print the latest volt release info (does not auto-install).
    Update {
        /// Only print the latest version and exit.
        #[arg(long)]
        check: bool,
        /// Specific version to check for (default: latest).
        #[arg(long)]
        version: Option<String>,
    },
    /// Scaffold AGENTS.md / SOUL.md / MEMORY.md / USER.md in the current dir.
    Init {
        /// Overwrite existing files without prompting.
        #[arg(long, short = 'f')]
        force: bool,
        /// Only write this file (e.g. --only AGENTS.md).
        #[arg(long)]
        only: Option<String>,
    },
    /// Manage worktrees created by `volt agent-run --worktree`. Each
    /// session that ran with `--worktree` lives on its own branch in
    /// `.volt-worktrees/<short-id>`. Use this command to list, review,
    /// merge, or discard those worktrees.
    Worktree {
        #[command(subcommand)]
        subcommand: WorktreeSubcommand,
    },
    Jobs {
        #[command(subcommand)]
        subcommand: JobsSubcommand,
    },
    Routines {
        #[command(subcommand)]
        subcommand: RoutinesSubcommand,
    },
    JobsMonitor,
    RoutinesEngine,
    /// Manage LLM provider API keys, base URLs, and on-disk config.
    /// Replaces the previous requirement to hand-edit `.env`.
    ///
    ///   volt config list                    # show all providers and their status
    ///   volt config set <slug> <key>        # write a key to .env + process env
    ///   volt config get <slug>              # show one provider's masked key + base URL
    ///   volt config unset <slug>            # remove a key from .env
    ///   volt config doctor                  # provider-focused diagnostics
    ///   volt config wizard                  # interactive setup
    Config {
        #[command(subcommand)]
        subcommand: ConfigCmd,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// List all known providers and their active/inactive status.
    List,
    /// Show one provider's masked key and base URL.
    Get {
        /// Provider slug, e.g. `groq`, `nvidia`, `openai`, `anthropic`,
        /// `ollama`, `moonshot`, `ollama_local`, `llamacpp`, `litertlm`.
        provider: String,
    },
    /// Set an API key. Writes to `volt_home()/.env` and the process env.
    Set {
        provider: String,
        key: String,
    },
    /// Remove an API key from `volt_home()/.env`.
    Unset {
        provider: String,
    },
    /// Provider-focused diagnostics. Surfaces the active set, lists
    /// missing keys with the env var name, and explains how to enable
    /// each provider.
    Doctor,
    /// Interactive first-time setup. Walks the user through choosing a
    /// provider and pasting their key.
    Wizard,
}

#[derive(Subcommand, Debug)]
enum JobsSubcommand {
    List,
}

#[derive(Subcommand, Debug)]
enum RoutinesSubcommand {
    List,
    Create {
        name: String,
        action_prompt: String,
        #[arg(long)]
        cron: Option<String>,
    },
    Edit {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        action_prompt: Option<String>,
        #[arg(long)]
        cron: Option<String>,
    },
    Delete {
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum AgentSubcommand {
    List,
    Run {
        #[arg(long)]
        preset: Option<String>,
        #[arg(long)]
        input: Option<String>,
        /// Run the agent inside a fresh `git worktree` so file changes
        /// are isolated to a branch (`volt-session/<short-id>`).
        #[arg(long, default_value_t = false)]
        worktree: bool,
    },
    /// Launch the interactive TUI (alias for the top-level `agent-tui`
    /// command — both spellings do the same thing).
    Tui {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
        #[arg(long)]
        max_iterations: Option<u32>,
        /// Run the agent inside a fresh `git worktree` so file changes
        /// are isolated to a branch (`volt-session/<short-id>`).
        #[arg(long, default_value_t = false)]
        worktree: bool,
    },
}

#[derive(Subcommand, Debug)]
enum WorktreeSubcommand {
    /// List all volt-managed worktrees (`volt-session/*` branches).
    List,
    /// Show a `git diff --stat` summary of the changes in a worktree's
    /// branch relative to the current HEAD.
    Status {
        /// Short id of the worktree (first 8 hex chars of the session uuid).
        id: String,
    },
    /// Merge a session branch back into the current branch (`--no-ff`).
    /// The worktree is left in place; run `volt worktree clean` after.
    Merge {
        /// Short id of the worktree to merge.
        id: String,
    },
    /// Remove a worktree and delete its branch. Discards the worktree's
    /// changes; export them first if you want to keep them.
    Clean {
        /// Short id of the worktree to discard.
        id: String,
        /// Skip the confirmation prompt.
        #[arg(long, short = 'f')]
        force: bool,
    },
}

async fn handle_worktree_subcommand(sub: WorktreeSubcommand) -> anyhow::Result<()> {
    use crate::commands::worktree::WorktreeManager;
    let cwd = std::env::current_dir()?;
    let repo_root = match WorktreeManager::detect_repo_root(&cwd).await? {
        Some(r) => r,
        None => {
            anyhow::bail!("not inside a git repository; `volt worktree` requires git");
        }
    };
    let mgr = WorktreeManager::new(repo_root);
    match sub {
        WorktreeSubcommand::List => {
            let infos = mgr.list().await?;
            if infos.is_empty() {
                println!("(no volt worktrees)");
                return Ok(());
            }
            println!("{:<10}  {:<35}  {:<10}  PATH", "SHORT", "BRANCH", "HEAD");
            for info in infos {
                let head = mgr.head_short(&info.branch).await.unwrap_or_default();
                println!(
                    "{:<10}  {:<35}  {:<10}  {}",
                    info.short_id,
                    info.branch,
                    head,
                    info.path.display()
                );
            }
        }
        WorktreeSubcommand::Status { id } => {
            let branch = format!("volt-session/{}", id);
            let summary = mgr.diff_summary(&branch).await?;
            if summary.trim().is_empty() {
                println!("(no changes in {} relative to HEAD)", branch);
            } else {
                println!("Changes in {} (vs HEAD):\n{}", branch, summary);
            }
        }
        WorktreeSubcommand::Merge { id } => {
            let branch = format!("volt-session/{}", id);
            println!("Merging {} into current branch (--no-ff)...", branch);
            mgr.merge_back(&branch).await?;
            println!(
                "Done. The worktree is still on disk; run `volt worktree clean {}` to remove it.",
                id
            );
        }
        WorktreeSubcommand::Clean { id, force } => {
            let branch = format!("volt-session/{}", id);
            let path = mgr.parent_dir().join(&id);
            if !force {
                eprint!(
                    "This will remove {} and delete branch {}. Continue? [y/N] ",
                    path.display(),
                    branch
                );
                use std::io::Write;
                std::io::stderr().flush().ok();
                let mut buf = String::new();
                std::io::stdin().read_line(&mut buf).ok();
                if buf.trim().to_lowercase() != "y" {
                    println!("aborted");
                    return Ok(());
                }
            }
            if path.exists() {
                mgr.remove(&path, true).await?;
            }
            mgr.delete_branch(&branch, true).await?;
            println!("Removed {} and branch {}.", path.display(), branch);
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Detect .env shadowing by shell env BEFORE dotenvy loads anything,
    // so the warning is based on what the user actually has in their
    // process environment (not what dotenvy is about to skip).
    volt::config::warn_on_env_shadowing();
    // Load .env with fallback to binary directory (handles CWD edge cases on Windows)
    if dotenvy::dotenv().is_err() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let _ = dotenvy::from_path(dir.join(".env"));
                // Also warn on shadowing from the binary-dir .env.
                volt::config::warn_on_env_shadowing_from_binary_dir(dir);
            }
        }
    }
    // Verify critical keys are present; warn if missing before telemetry is up
    for key in &[
        "GROQ_API_KEY",
        "NVIDIA_API_KEY",
        "OLLAMA_API_KEY",
        "HF_TOKEN",
    ] {
        if std::env::var(key).map_or(true, |v| v.is_empty() || v.starts_with("your_")) {
            eprintln!("[warn] {} not set or still has placeholder value", key);
        }
    }
    let cli = Cli::parse();
    // TUI routes tracing to a log file (so DB warnings / OTel exporter
    // setup / worker progress don't pollute the chat). All other
    // subcommands keep the default stderr writer so logs are visible
    // inline (`--print` / `--json` modes already gate their own
    // `eprintln!` chatter).
    match &cli.command {
        Commands::AgentTui { .. } => {
            let log_dir = volt::config::volt_home().join("logs");
            if let Err(e) = volt::telemetry::init_otel_for_tui("volt", &log_dir) {
                eprintln!(
                    "[warn] failed to open TUI log file, falling back to stderr: {}",
                    e
                );
                volt::telemetry::init_otel("volt");
            }
        }
        _ => volt::telemetry::init_otel("volt"),
    }
    volt::config::first_run_wizard();
    let settings = volt::config::Settings::from_env()?;

    match cli.command {
        Commands::InitDb => commands::tools::init_db(&settings.database_url).await?,
        Commands::Validate { manifest } => commands::tools::validate_manifest(manifest).await?,
        Commands::ProvisionFile {
            manifest,
            marketplace_verified,
        } => commands::provision::provision_file(manifest, marketplace_verified, &settings).await?,
        Commands::Provision {
            pkg_id,
            registry_base_url,
            auth_token,
        } => {
            commands::provision::provision_remote(pkg_id, registry_base_url, auth_token, &settings)
                .await?
        }
        Commands::ListTools => commands::tools::list_tools(&settings.database_url).await?,
        Commands::History { limit } => {
            commands::tools::history(limit, &settings.database_url).await?
        }
        Commands::Execute { tool, params } => {
            commands::tools::execute(tool, params, &settings).await?
        }
        Commands::Sandbox {
            command,
            timeout_ms,
        } => commands::tools::sandbox_command(command, timeout_ms, &settings).await?,
        Commands::AgentRun {
            input,
            model,
            allow,
            load_tools,
            context_kinds,
            session_id,
            max_iterations,
            mode,
            preset,
            agent_file,
            use_mtp,
            use_cot,
            allow_write,
            framework,
            model_variant,
            quantization,
            blueprint,
            auto_blueprint,
            print: print_mode,
            json: json_mode,
            plan,
            worktree,
        } => {
            let model = commands::agent_run::AgentRunOptions::model_or_default(model);

            // Load from preset or agent file
            let model = if let Some(ref name) = preset {
                volt::agent::preset::load_preset(name)
                    .and_then(|(_, p)| p.agent?.model)
                    .unwrap_or(model)
            } else if let Some(ref path) = agent_file {
                volt::agent::preset::load_agent_file(path)
                    .and_then(|p| p.agent?.model)
                    .unwrap_or(model)
            } else {
                model
            };

            // Apply env vars from preset
            if let Some(ref name) = preset {
                if let Some((_, p)) = volt::agent::preset::load_preset(name) {
                    if let Some(ref env) = p.env {
                        for (k, v) in env {
                            std::env::set_var(k, v);
                        }
                    }
                }
            } else if let Some(ref path) = agent_file {
                if let Some(p) = volt::agent::preset::load_agent_file(path) {
                    if let Some(ref env) = p.env {
                        for (k, v) in env {
                            std::env::set_var(k, v);
                        }
                    }
                }
            }

            commands::agent_run::run(commands::agent_run::AgentRunOptions {
                input,
                model,
                allow,
                load_tools,
                context_kinds,
                mode,
                session_id,
                max_iterations,
                settings,
                use_mtp,
                use_cot,
                allow_write,
                framework,
                model_variant,
                quantization,
                blueprint,
                auto_blueprint,
                print: print_mode,
                json: json_mode,
                plan,
                worktree,
            })
            .await?
        }
        Commands::Agent {
            subcommand: AgentSubcommand::List,
        } => commands::agent::cmd_list().await,
        Commands::Agent {
            subcommand: AgentSubcommand::Tui { .. },
        } => {
            // `volt agent tui` is a friendly alias for the top-level
            // `volt agent-tui`. Re-dispatch by destructuring the
            // already-parsed subcommand and calling the same handler
            // the top-level command uses. Fields not on the subcommand
            // (mode, use_mtp, etc.) fall back to env defaults.
            let (model, allow, max_iterations, worktree) = match &cli.command {
                Commands::Agent {
                    subcommand:
                        AgentSubcommand::Tui {
                            model,
                            allow,
                            max_iterations,
                            worktree,
                        },
                } => (model.clone(), *allow, *max_iterations, *worktree),
                _ => unreachable!("matched above"),
            };
            let mode =
                std::env::var("VOLT_CONTEXT_MODE").unwrap_or_else(|_| "balanced".to_string());
            commands::agent_tui::run(commands::agent_tui::AgentTuiOptions {
                model: commands::agent_tui::AgentTuiOptions::model_or_default(model),
                allow,
                max_iterations,
                mode,
                settings: settings.clone(),
                use_mtp: settings.use_mtp,
                use_cot: settings.use_cot,
                allow_write: settings.allow_write,
                framework: settings.framework.clone(),
                model_variant: settings.model_variant.clone(),
                quantization: settings.quantization.clone(),
                worktree,
            })
            .await?
        }
        Commands::Agent {
            subcommand:
                AgentSubcommand::Run {
                    preset,
                    input,
                    worktree,
                },
        } => {
            if let Some(input) = input {
                // Non-interactive: load preset and run
                let (model_name, allow, max_iter, env) = if let Some(ref name) = preset {
                    volt::agent::preset::load_preset(name)
                        .map(|(_, p)| {
                            let m = p.agent.as_ref().and_then(|a| a.model.clone());
                            let a = p.agent.as_ref().and_then(|a| a.allow).unwrap_or(true);
                            let i = p.agent.as_ref().and_then(|a| a.max_iterations);
                            let e = p.env.clone();
                            (m, a, i, e)
                        })
                        .unwrap_or((None, true, None, None))
                } else {
                    (None, true, None, None)
                };
                if let Some(ref env) = env {
                    for (k, v) in env {
                        std::env::set_var(k, v);
                    }
                }
                let settings = volt::config::Settings::from_env()?;
                let resolved_model = commands::agent_run::AgentRunOptions::model_or_default(
                    model_name.clone(),
                );
                if resolved_model.is_empty() {
                    anyhow::bail!(
                        "no model configured. Pass --model, set LLM_MODEL in .env, \
                         or run `volt config` to choose a provider and model."
                    );
                }
                commands::agent_run::run(commands::agent_run::AgentRunOptions {
                    input,
                    model: resolved_model,
                    allow,
                    load_tools: None,
                    context_kinds: Vec::new(),
                    mode: "balanced".into(),
                    session_id: None,
                    max_iterations: max_iter,
                    settings,
                    use_mtp: false,
                    use_cot: false,
                    allow_write: false,
                    framework: None,
                    model_variant: None,
                    quantization: None,
                    blueprint: None,
                    auto_blueprint: false,
                    print: false,
                    json: false,
                    plan: false,
                    worktree: false,
                })
                .await?
            } else {
                commands::agent::cmd_run_interactive(worktree).await?
            }
        }
        Commands::AgentTui {
            model,
            allow,
            max_iterations,
            mode,
            use_mtp,
            use_cot,
            allow_write,
            framework,
            model_variant,
            quantization,
            worktree,
        } => {
            commands::agent_tui::run(commands::agent_tui::AgentTuiOptions {
                model: commands::agent_tui::AgentTuiOptions::model_or_default(model),
                allow,
                max_iterations,
                mode,
                settings,
                use_mtp,
                use_cot,
                allow_write,
                framework,
                model_variant,
                quantization,
                worktree,
            })
            .await?
        }
        Commands::Workflow {
            pattern,
            agents,
            tasks,
            agents_file,
            tasks_file,
            allow,
        } => {
            commands::workflow::run(pattern, agents, tasks, agents_file, tasks_file, allow).await?
        }
        Commands::Eval { suite, model } => commands::eval::run(suite, model).await?,
        Commands::McpServe => commands::mcp::serve_stdio().await?,
        Commands::ProvisionSkill { path } => {
            commands::skills::provision_skill(path, &settings).await?
        }
        Commands::ListCatalogSkills { catalog_url } => {
            commands::skills::list_catalog(catalog_url).await?
        }
        Commands::SearchCatalogSkills { query, catalog_url } => {
            commands::skills::search_catalog(query, catalog_url).await?
        }
        Commands::InstallSkill { name, catalog_url } => {
            commands::skills::install_skill(name, catalog_url, &settings).await?
        }
        Commands::ImportSkill { path, format, name } => {
            commands::skills::import_skill(path, format, name, &settings).await?
        }
        Commands::Heartbeat => commands::daemon::run_heartbeat(&settings).await?,
        Commands::JobsMonitor => commands::daemon::run_jobs_monitor(&settings).await?,
        Commands::RoutinesEngine => commands::daemon::run_routines_engine(&settings).await?,
        Commands::Migrate => {
            let pool = volt::db::connect(&settings.database_url).await?;
            volt::db::init_schema(&pool).await?;
            println!("schema migrated");
        }
        Commands::Completion { shell, out } => {
            use clap::CommandFactory;
            use std::io::Write;
            let mut cmd = Cli::command();
            let bin = cmd.get_name().to_string();
            let mut buf: Vec<u8> = Vec::new();
            clap_complete::generate(shell, &mut cmd, bin, &mut buf);
            match out {
                Some(p) => {
                    if let Some(parent) = p.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&p, &buf)?;
                    eprintln!("wrote {} ({} bytes)", p.display(), buf.len());
                }
                None => {
                    std::io::stdout().lock().write_all(&buf)?;
                }
            }
        }
        Commands::Doctor => {
            commands::doctor::run(&settings).await?;
        }
        Commands::Update { check, version } => {
            commands::doctor::check_update(check, version.as_deref()).await?;
        }
        Commands::Init { force, only } => {
            commands::init::run(force, only.as_deref()).await?;
        }
        Commands::Config { subcommand } => {
            use commands::config::ConfigSubcommand;
            let sub = match subcommand {
                ConfigCmd::List => ConfigSubcommand::List,
                ConfigCmd::Get { provider } => ConfigSubcommand::Get { provider },
                ConfigCmd::Set { provider, key } => ConfigSubcommand::Set { provider, key },
                ConfigCmd::Unset { provider } => ConfigSubcommand::Unset { provider },
                ConfigCmd::Doctor => ConfigSubcommand::Doctor,
                ConfigCmd::Wizard => ConfigSubcommand::Wizard,
            };
            commands::config::run(sub).await?;
        }
        Commands::Worktree { subcommand } => {
            handle_worktree_subcommand(subcommand).await?;
        }
        Commands::Jobs {
            subcommand: JobsSubcommand::List,
        } => {
            let pool = volt::db::connect(&settings.database_url).await?;
            let manager = volt::jobs::JobManager::new(Some(pool));
            let jobs = manager.list_jobs(None).await?;
            println!("{}", serde_json::to_string_pretty(&jobs)?);
        }
        Commands::Routines { subcommand } => match subcommand {
            RoutinesSubcommand::List => {
                let pool = volt::db::connect(&settings.database_url).await?;
                let routines = volt::db::list_routines(&pool).await?;
                println!("{}", serde_json::to_string_pretty(&routines)?);
            }
            RoutinesSubcommand::Create { name, action_prompt, cron } => {
                volt::routines::validate_action_prompt(&action_prompt)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                let pool = volt::db::connect(&settings.database_url).await?;
                let id = uuid::Uuid::new_v4();
                let trigger_type = if cron.is_some() { "cron" } else { "manual" };
                sqlx::query(
                    "INSERT INTO routines (id, name, action_prompt, enabled, trigger_type, cron) VALUES ($1, $2, $3, true, $4, $5)"
                )
                .bind(id)
                .bind(&name)
                .bind(&action_prompt)
                .bind(trigger_type)
                .bind(cron.as_deref())
                .execute(&pool)
                .await?;
                println!("Created routine {} ({})", name, id);
            }
            RoutinesSubcommand::Edit { id, name, action_prompt, cron } => {
                let pool = volt::db::connect(&settings.database_url).await?;
                let uuid: uuid::Uuid = id.parse().map_err(|e| anyhow::anyhow!("invalid id: {}", e))?;
                if let Some(ref p) = action_prompt {
                    volt::routines::validate_action_prompt(p)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                }
                if let Some(n) = &name {
                    sqlx::query("UPDATE routines SET name = $1 WHERE id = $2")
                        .bind(n).bind(uuid).execute(&pool).await?;
                }
                if let Some(p) = &action_prompt {
                    sqlx::query("UPDATE routines SET action_prompt = $1 WHERE id = $2")
                        .bind(p).bind(uuid).execute(&pool).await?;
                }
                if let Some(c) = &cron {
                    sqlx::query("UPDATE routines SET cron = $1 WHERE id = $2")
                        .bind(c).bind(uuid).execute(&pool).await?;
                }
                println!("Updated routine {}", uuid);
            }
            RoutinesSubcommand::Delete { id } => {
                let pool = volt::db::connect(&settings.database_url).await?;
                let uuid: uuid::Uuid = id.parse().map_err(|e| anyhow::anyhow!("invalid id: {}", e))?;
                sqlx::query("DELETE FROM routines WHERE id = $1")
                    .bind(uuid).execute(&pool).await?;
                println!("Deleted routine {}", uuid);
            }
        }
        Commands::AgentChat { .. } => {
            eprintln!("AgentChat is deprecated — use AgentRun or AgentTui");
        }
    }
    Ok(())
}
