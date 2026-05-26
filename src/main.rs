use clap::{Parser, Subcommand};
use std::path::PathBuf;
use uuid::Uuid;
use volt::agent::loop_rs::Agent;
use volt::config::Settings;
use volt::context::ContextStore;
use volt::db;
use volt::embedding::EmbeddingClient;
use volt::mcp::MCPServer;
use volt::models::*;
use volt::registry::{provision_manifest, RegistryClient};
use volt::{orchestrator, sandbox, validation, worker};

#[derive(Parser, Debug)]
#[command(name = "volt")]
#[command(about = "Volt â€” agent tool runtime and registry CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize PostgreSQL schema.
    InitDb,

    /// Validate a package manifest without installing it.
    Validate {
        #[arg(long)]
        manifest: PathBuf,
    },

    /// Provision a manifest from a local JSON file.
    ProvisionFile {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value_t = false)]
        marketplace_verified: bool,
    },

    /// Provision a manifest from the remote registry.
    Provision {
        #[arg(long)]
        pkg_id: String,
        #[arg(long)]
        registry_base_url: Option<String>,
        #[arg(long)]
        auth_token: Option<String>,
    },

    /// List locally installed tools.
    ListTools,

    /// List execution history.
    History {
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },

    /// Execute a provisioned tool by name with parameters.
    Execute {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        params: Option<String>,
    },

    /// Run a command in the sandbox runner.
    Sandbox {
        #[arg(long)]
        command: String,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },

    /// Run the agent with a single input and exit.
    AgentRun {
        #[arg(long)]
        input: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
        /// Load tool stubs from a JSONL file (one tool per line, BFCL format)
        #[arg(long)]
        load_tools: Option<String>,
        /// Comma-separated context kinds to retrieve (default: all)
        #[arg(long, value_delimiter = ',')]
        context_kinds: Vec<String>,
        /// Session ID for multi-turn episodic memory (creates new session if omitted)
        #[arg(long)]
        session_id: Option<String>,
        /// Maximum agent loop iterations (default: 8)
        #[arg(long)]
        max_iterations: Option<u32>,
    },

    /// Start an interactive agent chat session.
    AgentChat {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
    },

    /// Start the MCP stdio server for tool access.
    McpServe,

    /// Start interactive TUI chat session.
    AgentTui {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, short = 'a', default_value_t = false)]
        allow: bool,
        /// Maximum agent loop iterations (default: 25)
        #[arg(long)]
        max_iterations: Option<u32>,
    },

    /// Multi-agent workflow: parallel, pipeline, or supervisor.
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

    /// Run an eval benchmark against a task suite.
    Eval {
        #[arg(long)]
        suite: PathBuf,
        #[arg(long)]
        model: Option<String>,
    },

    /// Compile a SKILL.md file into the database.
    ProvisionSkill {
        #[arg(long)]
        path: PathBuf,
    },

    /// List skills available in the remote catalog.
    ListCatalogSkills {
        #[arg(long)]
        catalog_url: Option<String>,
    },

    /// Search for skills in the remote catalog.
    SearchCatalogSkills {
        #[arg(long)]
        query: String,
        #[arg(long)]
        catalog_url: Option<String>,
    },

    /// Install a skill from the catalog into the database.
    InstallSkill {
        #[arg(long)]
        name: String,
        #[arg(long)]
        catalog_url: Option<String>,
    },

    /// Import a skill from an external file (CLAUDE.md, .cursorrules, copilot-instructions.md, or plain markdown) into Volt's RAG.
    ImportSkill {
        /// Path to the external skill file
        #[arg(long)]
        path: PathBuf,
        /// Override source format: auto | claude | cursor | copilot | markdown
        #[arg(long, default_value = "auto")]
        format: String,
        /// Override the skill name (default: derived from filename)
        #[arg(long)]
        name: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    volt::telemetry::init_otel("volt");

    let cli = Cli::parse();
    volt::config::first_run_wizard();
    let settings = Settings::from_env()?;

    match cli.command {
        Commands::InitDb => {
            let pool = db::connect(&settings.database_url).await?;
            db::init_schema(&pool).await?;
            println!("schema initialized");
        }
        Commands::Validate { manifest } => {
            let manifest = volt::registry::load_manifest(&manifest).await?;
            let report = validation::validate_manifest(&manifest);
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.accepted {
                std::process::exit(2);
            }
        }
        Commands::ProvisionFile {
            manifest,
            marketplace_verified,
        } => {
            let manifest = volt::registry::load_manifest(&manifest).await?;
            let pool = db::connect(&settings.database_url).await?;
            let embedder = EmbeddingClient::new_smart().await;
            let result =
                provision_manifest(&pool, &embedder, manifest, marketplace_verified).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Provision {
            pkg_id,
            registry_base_url,
            auth_token,
        } => {
            let pool = db::connect(&settings.database_url).await?;
            let registry = RegistryClient::new();
            let options = RegistryFetchOptions {
                pkg_id,
                registry_base_url: registry_base_url.unwrap_or(settings.registry_base_url),
                auth_token: auth_token.or(settings.registry_token),
            };
            let manifest = registry.fetch_manifest(&options).await?;
            let embedder = EmbeddingClient::new_smart().await;
            let result = provision_manifest(&pool, &embedder, manifest, true).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::ListTools => {
            let pool = db::connect(&settings.database_url).await?;
            let tools = db::list_tools(&pool).await?;
            println!("{}", serde_json::to_string_pretty(&tools)?);
        }
        Commands::History { limit } => {
            let pool = db::connect(&settings.database_url).await?;
            let records = db::list_executions(&pool, limit).await?;
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        Commands::Execute { tool, params } => {
            let pool = db::connect(&settings.database_url).await?;
            let tool_params: serde_json::Value = params
                .as_deref()
                .map(|p| {
                    serde_json::from_str(p).unwrap_or_else(|e| {
                        eprintln!(
                            "[cli] warning: invalid JSON params '{}': {}. Using empty object.",
                            p, e
                        );
                        serde_json::json!({})
                    })
                })
                .unwrap_or(serde_json::json!({}));
            let tool_info = db::get_tool_by_name(&pool, &tool).await?;
            let tool_id = tool_info.as_ref().map(|t| t.id);
            let source = db::get_tool_source(&pool, &tool).await?;
            let execution_id = Uuid::new_v4();

            match source {
                Some(code) => {
                    let stdin_input = tool_params.to_string();
                    let result = sandbox::run_command_direct(
                        "python3",
                        &["-c", &code],
                        Some(&stdin_input),
                        &settings.sandbox_policy,
                    )
                    .await;
                    let output_val = serde_json::from_str::<serde_json::Value>(&result.stdout)
                        .unwrap_or(serde_json::json!({ "raw": result.stdout }));
                    let status = if result.status == "ok" {
                        "success"
                    } else {
                        "failed"
                    };

                    db::record_execution(
                        &pool,
                        tool_id,
                        &tool,
                        &tool_params,
                        &output_val,
                        status,
                        if result.status != "ok" {
                            Some(&result.stderr)
                        } else {
                            None
                        },
                        result.duration_ms as i32,
                        execution_id,
                    )
                    .await?;

                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "execution_id": execution_id.to_string(),
                            "status": status,
                            "output": output_val,
                            "duration_ms": result.duration_ms,
                        }))?
                    );
                }
                None => {
                    anyhow::bail!("tool '{}' not found; provision it first", tool);
                }
            }
        }
        Commands::Sandbox {
            command,
            timeout_ms,
        } => {
            let policy = SandboxPolicy {
                timeout_ms: timeout_ms.unwrap_or(settings.sandbox_policy.timeout_ms),
                max_stdout_bytes: settings.sandbox_policy.max_stdout_bytes,
                working_dir: settings.sandbox_policy.working_dir,
            };
            let result = sandbox::run_command(&command, &policy).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::AgentRun {
            input,
            model,
            allow,
            load_tools,
            context_kinds,
            session_id,
            max_iterations,
        } => {
            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
            });
            let (provider, provider_kind) = orchestrator::build_provider(&model, "volt-agent");

            let cancel = volt::models::CancelToken::new();
            let c = cancel.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                eprintln!("\n[interrupt] shutting down...");
                c.cancel();
            });

            let embedder = EmbeddingClient::new_smart().await;
            let tools = volt::tools::setup_tools(Some(&embedder)).await;

            // Load tool stubs from a JSONL file (BFCL format)
            if let Some(ref path) = load_tools {
                match std::fs::read_to_string(path) {
                    Ok(contents) => {
                        let mut count = 0;
                        for line in contents.lines() {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            if let Ok(fn_def) = serde_json::from_str::<serde_json::Value>(line) {
                                let name = fn_def["name"].as_str().unwrap_or("unknown");
                                let desc =
                                    fn_def["description"].as_str().unwrap_or("No description");
                                let schema = fn_def["parameters"].clone();
                                if name != "unknown" {
                                    let name_owned = name.to_string();
                                    tools
                                        .register(
                                            name,
                                            desc,
                                            schema,
                                            "bfcl",
                                            std::sync::Arc::new(move |_args| {
                                                let msg = format!(
                                                    "[stub] {} called — no real implementation",
                                                    name_owned
                                                );
                                                Box::pin(async move {
                                                    volt::models::ToolResult {
                                                        success: true,
                                                        output: msg,
                                                        error: None,
                                                        duration_ms: 0,
                                                    }
                                                })
                                            }),
                                        )
                                        .await;
                                    count += 1;
                                }
                            }
                        }
                        eprintln!("[tools] loaded {} BFCL stubs from {}", count, path);
                        tools.compute_embeddings(&embedder).await;
                    }
                    Err(e) => eprintln!("[tools] failed to load {}: {}", path, e),
                }
            }

            // Clone before moves for worker wiring
            let tools_for_agent = tools.clone();
            let tools_for_seed = tools.clone();
            let embedder_for_skills = embedder.clone();
            let embedder_for_worker = embedder.clone();
            let cancel_for_agent = cancel.clone();
            let cancel_for_worker = cancel.clone();

            let config = AgentConfig {
                name: "volt-agent".into(),
                model,
                provider: provider_kind,
                system_prompt: None,
                max_iterations: max_iterations.unwrap_or(8),
                temperature: 0.3,
                toolsets: vec!["builtin".into()],
                hidden: false,
                allow_all: allow,
                enabled_context_kinds: volt::context::parse_context_kinds(&context_kinds),
                essential_tools: volt::models::default_essential_tools(),
                context_kind_quotas: Default::default(),
            };
            let config_quotas = config.context_kind_quotas.clone();
            let mut agent = Agent::new(config, provider, tools_for_agent)
                .with_workspace(std::env::current_dir().unwrap_or_default())
                .with_cancel(cancel_for_agent)
                .with_stream(std::sync::Arc::new(|token| {
                    print!("{}", token);
                }));

            // Wire up SQLite session for episodic memory
            let session_db_path = std::path::Path::new("volt_sessions.db");
            if let Ok(sqlite_pool) = volt::session::open_sessions(session_db_path).await {
                let sid = if let Some(ref sid_str) = session_id {
                    uuid::Uuid::parse_str(sid_str).unwrap_or_else(|_| uuid::Uuid::new_v4())
                } else {
                    uuid::Uuid::new_v4()
                };
                let session = volt::models::Session {
                    id: sid,
                    agent_name: "volt-agent".into(),
                    title: input.clone(),
                    message_count: 0,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };
                let _ = volt::session::create_session(&sqlite_pool, &session).await;
                agent = agent.with_session(sid, sqlite_pool);
                if session_id.is_none() {
                    eprintln!("[session] created new session {}", sid);
                } else {
                    eprintln!("[session] resumed session {}", sid);
                }
            }
            let pool = match db::connect(&settings.database_url).await {
                Ok(pool) => Some(pool),
                Err(e) => {
                    eprintln!(
                        "[db] warning: connection failed: {}. Running without memory.",
                        e
                    );
                    None
                }
            };
            if let Some(ref p) = pool {
                agent = agent.with_memory(p.clone(), embedder.clone());
            } else {
                agent = agent.with_memory_embedder_only(embedder.clone());
            }
            let skills = volt::skills::setup_skills(pool.clone(), Some(embedder_for_skills)).await;
            agent = agent.with_skills(skills);

            let context_store = if let Some(ref p) = pool {
                let store = ContextStore::new_with_db(p.clone());
                match store.hydrate_from_db(2000).await {
                    Ok(n) if n > 0 => eprintln!("[context] hydrated {} entries from DB", n),
                    Err(_) => {}
                    _ => {}
                }
                store
            } else {
                ContextStore::new()
            };
            if !config_quotas.is_empty() {
                context_store.set_quotas(&config_quotas).await;
            }

            if let Some(ref pool) = pool {
                let skill_store = context_store.clone();
                let skill_pool = pool.clone();
                let skill_emb = embedder.clone();
                tokio::spawn(async move {
                    worker::seed_skills_from_db(&skill_store, &skill_pool, &skill_emb).await;
                });
            }
            let (seed_channel, seed_rx) = worker::create_seed_channel();
            agent = agent
                .with_context(context_store.clone())
                .with_seed_channel(seed_channel);

            worker::AutoSeedWorker::new(
                context_store.clone(),
                embedder_for_worker,
                cancel_for_worker,
            )
            .spawn(seed_rx);

            tokio::spawn(worker::seed_background(
                context_store.clone(),
                embedder.clone(),
                tools_for_seed.clone(),
                settings.sandbox_policy.clone(),
            ));

            if let Ok(pool) = db::connect(&settings.database_url).await {
                context_store.set_db(pool.clone());
                agent = agent.with_memory(pool.clone(), embedder.clone());
                let skill_store = context_store.clone();
                let skill_emb = embedder.clone();
                tokio::spawn(async move {
                    worker::seed_skills_from_db(&skill_store, &pool, &skill_emb).await;
                });
            } else {
                agent = agent.with_memory_embedder_only(embedder.clone());
            }

            let sessions_pool =
                match volt::session::open_sessions(&std::path::PathBuf::from("volt_sessions.db"))
                    .await
                {
                    Ok(sp) => Some(sp),
                    Err(e) => {
                        eprintln!("[session] warning: failed to open sessions DB: {}", e);
                        None
                    }
                };

            // Run the input once and print the result (non-interactive)
            match agent.run(&input).await {
                Ok(result) => println!("{}", result),
                Err(e) => {
                    let state = agent.state().lock().await;
                    let last_text = state.messages.iter().rev().find_map(|m| {
                        let c = m.content.trim();
                        if !c.is_empty() { Some(c.to_string()) } else { 
                            m.tool_result.as_ref().and_then(|r| {
                                let t = r.trim();
                                if !t.is_empty() { Some(t.to_string()) } else { None }
                            })
                        }
                    });
                    match last_text {
                        Some(text) => println!("{}", text),
                        None => eprintln!("error: {}", e),
                    }
                }
            }

            // Save session messages if available
            if let Some(ref sp) = sessions_pool {
                let state = agent.state().lock().await;
                let _ = volt::session::create_session(
                    sp,
                    &Session {
                        id: state.session_id,
                        agent_name: state.name.clone(),
                        title: input.chars().take(60).collect::<String>(),
                        message_count: state.messages.len() as u32,
                        created_at: state.created_at,
                        updated_at: state.updated_at,
                    },
                )
                .await;
                let _ = volt::session::save_session_messages_atomic(
                    sp,
                    state.session_id,
                    &state.messages,
                )
                .await;
            }
        }
        Commands::AgentTui {
            model,
            allow,
            max_iterations,
        } => {
            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
            });
            let (provider, provider_kind) = orchestrator::build_provider(&model, "volt-agent");
            let tools = volt::tools::register_all_tools().await;
            let embedder = EmbeddingClient::new_smart().await;
            tools.compute_embeddings(&embedder).await;

            let config = AgentConfig {
                name: "volt-agent".into(),
                model,
                provider: provider_kind,
                system_prompt: None,
                max_iterations: max_iterations.unwrap_or(25),
                temperature: 0.3,
                toolsets: vec!["builtin".into()],
                hidden: false,
                allow_all: allow,
                enabled_context_kinds: volt::models::default_context_kinds(),
                essential_tools: volt::models::default_essential_tools(),
                context_kind_quotas: Default::default(),
            };
            let mut agent = Agent::new(config, provider, tools.clone())
                .with_workspace(std::env::current_dir().unwrap_or_default());

            let context_store = ContextStore::new();
            let (seed_channel, seed_rx) = worker::create_seed_channel();
            agent = agent
                .with_context(context_store.clone())
                .with_seed_channel(seed_channel);

            let cancel_tui = CancelToken::new();
            worker::AutoSeedWorker::new(context_store.clone(), embedder.clone(), cancel_tui)
                .spawn(seed_rx);

            tokio::spawn(worker::seed_background(
                context_store.clone(),
                embedder.clone(),
                tools.clone(),
                settings.sandbox_policy.clone(),
            ));

            if let Ok(pool) = db::connect(&settings.database_url).await {
                context_store.set_db(pool.clone());
                agent = agent.with_memory(pool.clone(), embedder.clone());
                let skill_store = context_store.clone();
                let skill_emb = embedder.clone();
                tokio::spawn(async move {
                    worker::seed_skills_from_db(&skill_store, &pool, &skill_emb).await;
                });
            } else {
                agent = agent.with_memory_embedder_only(embedder);
            }

            if let Ok(sp) =
                volt::session::open_sessions(&std::path::PathBuf::from("volt_sessions.db")).await
            {
                if let Ok(sessions) = volt::session::list_sessions(&sp, 10).await {
                    if !sessions.is_empty() {
                        println!("Past sessions:");
                        for (i, s) in sessions.iter().enumerate() {
                            println!(
                                "  {}. {} ({} msgs, {})",
                                i + 1,
                                s.title,
                                s.message_count,
                                s.created_at.format("%b %d %H:%M")
                            );
                        }
                        print!("Resume a session? [1-{}/N] ", sessions.len());
                        use std::io::Write;
                        std::io::stdout().flush()?;
                        let answer = tokio::task::spawn_blocking(|| {
                            let mut buf = String::new();
                            std::io::stdin().read_line(&mut buf).ok();
                            buf
                        })
                        .await
                        .unwrap_or_default();
                        if let Ok(idx) = answer.trim().parse::<usize>() {
                            if let Some(idx) = idx.checked_sub(1) {
                                if let Some(s) = sessions.get(idx) {
                                    if let Ok(msgs) = volt::session::load_messages(&sp, s.id).await
                                    {
                                        let mut state = agent.state().lock().await;
                                        state.session_id = s.id;
                                        state.messages = msgs;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            volt::tui::TuiChat::run(&agent).await?;
        }
        Commands::Workflow {
            pattern,
            agents,
            tasks,
            allow,
        } => {
            let mut specs = volt::orchestrator::parse_agent_specs(&agents)?;
            if allow {
                for spec in &mut specs {
                    spec.allow_all = true;
                }
            }
            let tasks: Vec<String> = serde_json::from_str(&tasks)?;
            let tools = volt::tools::register_all_tools().await;
            let orch = volt::orchestrator::Orchestrator::new(tools);
            let result = orch.run_workflow(&pattern, specs, tasks).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "steps": result.steps.iter().map(|s| serde_json::json!({
                        "agent": s.agent_name,
                        "success": s.success,
                        "duration_ms": s.duration_ms,
                        "output": s.output,
                    })).collect::<Vec<_>>(),
                    "final_output": result.final_output,
                    "total_duration_ms": result.total_duration_ms,
                }))?
            );
        }
        Commands::Eval { suite, model } => {
            let content = tokio::fs::read_to_string(&suite).await?;
            let suite_data: volt::eval::EvalSuite = serde_json::from_str(&content)?;

            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
            });
            let (provider, provider_kind) = orchestrator::build_provider(&model, "eval-agent");
            let tools = volt::tools::register_all_tools().await;
            let config = AgentConfig {
                name: "eval-agent".into(),
                model,
                provider: provider_kind,
                system_prompt: None,
                max_iterations: 15,
                temperature: 0.3,
                toolsets: vec!["builtin".into()],
                hidden: false,
                allow_all: true,
                enabled_context_kinds: volt::models::default_context_kinds(),
                essential_tools: volt::models::default_essential_tools(),
                context_kind_quotas: Default::default(),
            };
            let agent = Agent::new(config, provider, tools)
                .with_workspace(std::env::current_dir().unwrap_or_default());

            let summary = volt::eval::run_suite(&suite_data, &agent).await;
            volt::eval::print_summary(&summary);
        }
        Commands::McpServe => {
            let tools = volt::tools::register_all_tools().await;
            let server = MCPServer::new(tools);
            server.serve_stdio().await?;
        }
        Commands::ProvisionSkill { path } => {
            let embedder = EmbeddingClient::new_smart().await;
            let pool = db::connect(&settings.database_url).await?;
            let registry = volt::skills::SkillRegistry::new(Some(pool), Some(embedder.clone()));
            registry.compile_skill(&path, &embedder).await?;
            println!("Skill compiled from {:?} and stored in database.", path);
        }
        Commands::ListCatalogSkills { catalog_url } => {
            match volt::skills::catalog::fetch_catalog(catalog_url.as_deref()).await {
                Ok(catalog) => {
                    println!("Available skills ({}):", catalog.skills.len());
                    for skill in volt::skills::catalog::list_catalog(&catalog) {
                        println!(
                            "  {} v{} — {}",
                            skill.name, skill.version, skill.description
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Failed to fetch catalog: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::SearchCatalogSkills { query, catalog_url } => {
            match volt::skills::catalog::fetch_catalog(catalog_url.as_deref()).await {
                Ok(catalog) => {
                    let results = volt::skills::catalog::search_catalog(&catalog, &query);
                    if results.is_empty() {
                        println!("No skills found matching '{}'", query);
                    } else {
                        println!("Skills matching '{}' ({}):", query, results.len());
                        for skill in results {
                            println!(
                                "  {} v{} — {}",
                                skill.name, skill.version, skill.description
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to fetch catalog: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::InstallSkill { name, catalog_url } => {
            let embedder = EmbeddingClient::new_smart().await;
            let pool = db::connect(&settings.database_url).await?;
            match volt::skills::catalog::fetch_catalog(catalog_url.as_deref()).await {
                Ok(catalog) => {
                    match volt::skills::catalog::install_skill(&catalog, &name, &pool, &embedder)
                        .await
                    {
                        Ok(_) => println!("✓ Skill '{}' installed successfully.", name),
                        Err(e) => {
                            eprintln!("Failed to install skill '{}': {}", name, e);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to fetch catalog: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::ImportSkill { path, format, name } => {
            use volt::skills::importer;

            if !path.exists() {
                eprintln!("File not found: {:?}", path);
                std::process::exit(1);
            }

            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to read {:?}: {}", path, e);
                    std::process::exit(1);
                }
            };

            let source_fmt = match format.as_str() {
                "claude" => importer::SourceFormat::Claude,
                "cursor" => importer::SourceFormat::Cursor,
                "copilot" => importer::SourceFormat::Copilot,
                "opencode" => importer::SourceFormat::OpenCode,
                "markdown" => importer::SourceFormat::Markdown,
                _ => importer::detect_format(&path, &content),
            };

            if source_fmt == importer::SourceFormat::Volt {
                println!("✓ File is already a native Volt SKILL.md. Use `volt provision-skill --path {:?}` instead.", path);
                return Ok(());
            }

            let label = importer::format_label(&source_fmt);
            println!("Detected format: {}", label);

            let converted =
                importer::convert_to_volt_skill(&path, &content, &source_fmt, name.as_deref());

            // Write to temp file and compile
            let tmp_dir = std::env::temp_dir().join(format!("volt-import-{}", std::process::id()));
            std::fs::create_dir_all(&tmp_dir).ok();
            let tmp_path = tmp_dir.join("SKILL.md");
            std::fs::write(&tmp_path, &converted)?;

            let embedder = EmbeddingClient::new_smart().await;
            let pool = db::connect(&settings.database_url).await?;
            let registry = volt::skills::SkillRegistry::new(Some(pool), Some(embedder.clone()));

            match registry.compile_skill(&tmp_path, &embedder).await {
                Ok(_) => {
                    let manifest = volt::skills::parse_skill_manifest(&tmp_path).ok();
                    let skill_name = manifest
                        .as_ref()
                        .map(|m| m.name.as_str())
                        .unwrap_or("unknown");
                    println!(
                        "✓ Imported from {} as skill '{}' with RAG embedding.",
                        label, skill_name
                    );
                }
                Err(e) => {
                    eprintln!("Failed to compile imported skill: {}", e);
                    std::process::exit(1);
                }
            }

            std::fs::remove_dir_all(&tmp_dir).ok();
        }
        Commands::AgentChat { .. } => {
            eprintln!("AgentChat is deprecated — use AgentRun or AgentTui");
        }
    }

    Ok(())
}
