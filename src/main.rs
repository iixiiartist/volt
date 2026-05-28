#![deny(deprecated)]

use clap::{Parser, Subcommand};
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
    },
    McpServe,
    Serve {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
        #[arg(long)]
        max_iterations: Option<u32>,
        #[arg(long, default_value = "balanced")]
        mode: String,
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    Workflow {
        #[arg(long)]
        pattern: String,
        #[arg(long)]
        agents: String,
        #[arg(long)]
        tasks: String,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
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
        } => {
            commands::agent_run::run(commands::agent_run::AgentRunOptions {
                input,
                model: commands::agent_run::AgentRunOptions::model_or_default(model),
                allow,
                load_tools,
                context_kinds,
                mode,
                session_id,
                max_iterations,
                settings,
            })
            .await?
        }
        Commands::AgentTui {
            model,
            allow,
            max_iterations,
            mode,
        } => {
            commands::agent_tui::run(commands::agent_tui::AgentTuiOptions {
                model: commands::agent_tui::AgentTuiOptions::model_or_default(model),
                allow,
                max_iterations,
                mode,
                settings,
            })
            .await?
        }
        Commands::Workflow {
            pattern,
            agents,
            tasks,
            allow,
        } => commands::workflow::run(pattern, agents, tasks, allow).await?,
        Commands::Eval { suite, model } => commands::eval::run(suite, model).await?,
        Commands::McpServe => commands::mcp::serve_stdio().await?,
        Commands::Serve {
            model,
            allow,
            max_iterations,
            mode,
            port,
        } => {
            commands::serve::serve(commands::serve::ServeOptions {
                model: commands::serve::ServeOptions::model_or_default(model),
                allow,
                max_iterations: max_iterations.unwrap_or(25),
                mode,
                port,
                settings,
            })
            .await?
        }
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
        },
        Commands::Jobs { subcommand: JobsSubcommand::List } => {
            let pool = volt::db::connect(&settings.database_url).await?;
            let manager = volt::jobs::JobManager::new(Some(pool));
            let jobs = manager.list_jobs(None).await?;
            println!("{}", serde_json::to_string_pretty(&jobs)?);
        },
        Commands::Routines { subcommand: RoutinesSubcommand::List } => {
            let pool = volt::db::connect(&settings.database_url).await?;
            let routines = volt::db::list_routines(&pool).await?;
            println!("{}", serde_json::to_string_pretty(&routines)?);
        },
        Commands::AgentChat { .. } => {
            eprintln!("AgentChat is deprecated — use AgentRun or AgentTui");
        }
    }
    Ok(())
}
