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
}

#[derive(Subcommand, Debug)]
enum JobsSubcommand {
    List,
}

#[derive(Subcommand, Debug)]
enum RoutinesSubcommand {
    List,
}

#[derive(Subcommand, Debug)]
enum AgentSubcommand {
    List,
    Run {
        #[arg(long)]
        preset: Option<String>,
        #[arg(long)]
        input: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env with fallback to binary directory (handles CWD edge cases on Windows)
    if dotenvy::dotenv().is_err() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let _ = dotenvy::from_path(dir.join(".env"));
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
    volt::telemetry::init_otel("volt");
    let cli = Cli::parse();
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
            })
            .await?
        }
        Commands::Agent {
            subcommand: AgentSubcommand::List,
        } => commands::agent::cmd_list().await,
        Commands::Agent {
            subcommand: AgentSubcommand::Run { preset, input },
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
                commands::agent_run::run(commands::agent_run::AgentRunOptions {
                    input,
                    model: model_name.unwrap_or_else(|| "gemma4:e4b".into()),
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
                })
                .await?
            } else {
                commands::agent::cmd_run_interactive().await?
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
        Commands::Jobs {
            subcommand: JobsSubcommand::List,
        } => {
            let pool = volt::db::connect(&settings.database_url).await?;
            let manager = volt::jobs::JobManager::new(Some(pool));
            let jobs = manager.list_jobs(None).await?;
            println!("{}", serde_json::to_string_pretty(&jobs)?);
        }
        Commands::Routines {
            subcommand: RoutinesSubcommand::List,
        } => {
            let pool = volt::db::connect(&settings.database_url).await?;
            let routines = volt::db::list_routines(&pool).await?;
            println!("{}", serde_json::to_string_pretty(&routines)?);
        }
        Commands::AgentChat { .. } => {
            eprintln!("AgentChat is deprecated — use AgentRun or AgentTui");
        }
    }
    Ok(())
}
