use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
use volt::agent::loop_rs::Agent;
use volt::config::Settings;
use volt::context::ContextStore;
use volt::db;
use volt::embedding::EmbeddingClient;
use volt::llm::anthropic::AnthropicProvider;
use volt::llm::LLMProvider;
use volt::llm::OpenAIProvider;
use volt::mcp::MCPServer;
use volt::models::*;
use volt::registry::{provision_manifest, RegistryClient};
use volt::tools::ToolRegistry;
use volt::{sandbox, validation, worker};

fn parse_context_kinds(input: &[String]) -> Vec<volt::context::ContextKind> {
    if input.is_empty() {
        return volt::models::default_context_kinds();
    }
    use volt::context::ContextKind;
    input
        .iter()
        .filter_map(|s| match s.to_lowercase().as_str() {
            "tool" => Some(ContextKind::Tool),
            "skill" => Some(ContextKind::Skill),
            "memory" => Some(ContextKind::Memory),
            "conversation" => Some(ContextKind::Conversation),
            "agent_run" | "agentrun" => Some(ContextKind::AgentRun),
            "artifact" => Some(ContextKind::Artifact),
            "system_prompt" | "systemprompt" => Some(ContextKind::SystemPrompt),
            "few_shot" | "fewshot" => Some(ContextKind::FewShot),
            "policy" => Some(ContextKind::Policy),
            "permission" => Some(ContextKind::Permission),
            "security" => Some(ContextKind::Security),
            "mcp_config" | "mcpconfig" => Some(ContextKind::MCPConfig),
            _ => {
                eprintln!("[warn] unknown context kind '{}', skipping", s);
                None
            }
        })
        .collect()
}

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
            let manifest = load_manifest(&manifest).await?;
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
            let manifest = load_manifest(&manifest).await?;
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
            let (provider, provider_kind) = build_provider(&model, "volt-agent");

            let cancel = volt::models::CancelToken::new();
            let c = cancel.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                eprintln!("\n[interrupt] shutting down...");
                c.cancel();
            });

            let embedder = EmbeddingClient::new_smart().await;
            let tools = setup_tools(Some(&embedder)).await;

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
                enabled_context_kinds: parse_context_kinds(&context_kinds),
                essential_tools: volt::models::default_essential_tools(),
                context_kind_quotas: Default::default(),
            };
            let config_quotas = config.context_kind_quotas.clone();
            let mut agent = Agent::new(config, provider, tools_for_agent)
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
            let skills = setup_skills(pool.clone(), Some(embedder_for_skills)).await;
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
                context_store.set_quotas(&config_quotas);
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

            tokio::spawn(seed_background(
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

            if let Some(ref sp) = sessions_pool {
                if let Ok(sessions) = volt::session::list_sessions(sp, 10).await {
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
                                    if let Ok(msgs) = volt::session::load_messages(sp, s.id).await {
                                        let mut state = agent.state.lock().await;
                                        state.session_id = s.id;
                                        state.messages = msgs;
                                        println!(
                                            "Resumed session '{}' with {} messages.",
                                            s.title,
                                            state.messages.len()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            println!("Volt agent chat - type /quit to exit");
            loop {
                print!("> ");
                use std::io::Write;
                std::io::stdout().flush()?;
                let input = tokio::task::spawn_blocking(|| {
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf).ok();
                    buf
                })
                .await
                .unwrap_or_default();
                let input = input.trim().to_string();
                if input.is_empty() || input == "/quit" {
                    break;
                }
                let _result = agent.run(&input).await?;
                println!();

                if let Some(ref sp) = sessions_pool {
                    let state = agent.state.lock().await;
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
        }
        Commands::AgentTui {
            model,
            allow,
            max_iterations,
        } => {
            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
            });
            let (provider, provider_kind) = build_provider(&model, "volt-agent");
            let tools = register_all_tools().await;
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
            let mut agent = Agent::new(config, provider, tools.clone());

            let context_store = ContextStore::new();
            let (seed_channel, seed_rx) = worker::create_seed_channel();
            agent = agent
                .with_context(context_store.clone())
                .with_seed_channel(seed_channel);

            let cancel_tui = CancelToken::new();
            worker::AutoSeedWorker::new(context_store.clone(), embedder.clone(), cancel_tui)
                .spawn(seed_rx);

            tokio::spawn(seed_background(
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
                                        let mut state = agent.state.lock().await;
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
            let tools = register_all_tools().await;
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
            let (provider, provider_kind) = build_provider(&model, "eval-agent");
            let tools = register_all_tools().await;
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
            let agent = Agent::new(config, provider, tools);

            let summary = volt::eval::run_suite(&suite_data, &agent).await;
            volt::eval::print_summary(&summary);
        }
        Commands::McpServe => {
            let tools = register_all_tools().await;
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

/// Spawn background seeding: workspace, tool intents, permissions, security.
async fn seed_background(
    store: Arc<volt::context::ContextStore>,
    embedder: volt::embedding::EmbeddingClient,
    tools: Arc<volt::tools::ToolRegistry>,
    sandbox: volt::models::SandboxPolicy,
) {
    let store_ref = &store;
    let embedder_ref = &embedder;
    let tools_ref = &tools;
    let sandbox_ref = &sandbox;
    worker::seed_from_workspace(store_ref, embedder_ref).await;
    worker::seed_tool_intents(store_ref, tools_ref, embedder_ref).await;
    worker::seed_permissions(store_ref, tools_ref, embedder_ref).await;
    worker::seed_security_policy(store_ref, sandbox_ref, embedder_ref).await;
}

fn build_provider(model: &str, agent_name: &str) -> (Box<dyn LLMProvider>, String) {
    use volt::orchestrator::{resolve_provider, ProviderKind};
    let route = resolve_provider(model);
    let kind_str = match route.kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAI => "openai",
    };
    let provider: Box<dyn LLMProvider> = match route.kind {
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(
            route.api_key,
            Some(route.base_url),
            agent_name.into(),
        )),
        ProviderKind::OpenAI => Box::new(OpenAIProvider::new(
            route.api_key,
            route.base_url,
            agent_name.into(),
        )),
    };
    (provider, kind_str.to_string())
}

async fn load_manifest(path: &PathBuf) -> anyhow::Result<RegistryManifest> {
    let body = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str::<RegistryManifest>(&body)?)
}

async fn setup_skills(
    pool: Option<sqlx::PgPool>,
    embedder: Option<EmbeddingClient>,
) -> Arc<volt::skills::SkillRegistry> {
    let mut registry = volt::skills::SkillRegistry::new(pool, embedder);
    if let Err(e) = registry.load_from_db().await {
        eprintln!("Warning: failed to load skills from database: {}", e);
    }
    Arc::new(registry)
}

async fn setup_tools(embedder: Option<&EmbeddingClient>) -> Arc<ToolRegistry> {
    let registry = register_all_tools().await;
    if let Some(emb) = embedder {
        registry.compute_embeddings(emb).await;
    }
    registry
}

async fn register_all_tools() -> Arc<ToolRegistry> {
    let registry = ToolRegistry::new();
    let minimal = std::env::var("VOLT_MINIMAL_TOOLS").is_ok();

    registry
        .register_with_permission(
            "bash",
            "Execute a shell command",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "shell command to run" }
                },
                "required": ["command"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let cmd = args["command"].as_str().unwrap_or("");
                    volt::tools::bash::execute_bash(cmd).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register_with_permission(
            "read",
            "Read a file from disk",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "file path to read" }
                },
                "required": ["path"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    volt::tools::read_tool::read_file(path).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register_with_permission(
            "write",
            "Write content to a file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "file path" },
                    "content": { "type": "string", "description": "content to write" }
                },
                "required": ["path", "content"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let content = args["content"].as_str().unwrap_or("");
                    volt::tools::write_tool::write_file(path, content).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register_with_permission(
            "edit",
            "Edit a file by replacing text",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "file path" },
                    "old_string": { "type": "string", "description": "text to replace" },
                    "new_string": { "type": "string", "description": "replacement text" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let old = args["old_string"].as_str().unwrap_or("");
                    let new = args["new_string"].as_str().unwrap_or("");
                    volt::tools::edit::edit_file(path, old, new).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register(
            "glob",
            "Find files matching a glob pattern",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "glob pattern" },
                    "base": { "type": "string", "description": "base directory" }
                },
                "required": ["pattern"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let pattern = args["pattern"].as_str().unwrap_or("*");
                    let base = args["base"].as_str().unwrap_or(".");
                    volt::tools::glob_tool::glob_files(pattern, base).await
                })
            }),
        )
        .await;

    registry
        .register(
            "grep",
            "Search file contents with regex",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "regex pattern" },
                    "path": { "type": "string", "description": "directory to search" }
                },
                "required": ["pattern"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let pattern = args["pattern"].as_str().unwrap_or("");
                    let path = args["path"].as_str().unwrap_or(".");
                    volt::tools::grep_tool::grep_files(pattern, path).await
                })
            }),
        )
        .await;

    registry
        .register_with_permission(
            "web_fetch",
            "Fetch a URL and return its content",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    volt::tools::web_tool::web_fetch(url).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register(
            "memory_append",
            "Append to persistent memory file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "description": "memory category" },
                    "content": { "type": "string", "description": "content to remember" }
                },
                "required": ["kind", "content"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let kind = args["kind"].as_str().unwrap_or("note");
                    let content = args["content"].as_str().unwrap_or("");
                    volt::tools::memory_tool::memory_append(kind, content).await
                })
            }),
        )
        .await;

    registry
        .register(
            "todo_add",
            "Add a task to the todo list",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "task description" }
                },
                "required": ["task"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let task = args["task"].as_str().unwrap_or("");
                    volt::tools::todo_tool::todo_add(task).await
                })
            }),
        )
        .await;

    let delegate_tools = registry.clone();
    let delegate_fn = {
        let dt = delegate_tools.clone();
        Arc::new(move |args: serde_json::Value| {
            let dt = dt.clone();
            Box::pin(async move {
                let task = args["task"].as_str().unwrap_or("");
                let context = args["context"].as_str().unwrap_or("");
                volt::tools::delegate::delegate_task(task, context, dt).await
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
        })
    };
    registry.register_with_permission("delegate", "Delegate a sub-task to a sub-agent and return its result", serde_json::json!({
        "type": "object",
        "properties": {
            "task": { "type": "string", "description": "task description for the sub-agent" },
            "context": { "type": "string", "description": "context and constraints from the parent agent" }
        },
        "required": ["task"]
    }), "builtin", delegate_fn, PermissionLevel::Prompt).await;

    let workflow_fn = {
        let wt = registry.clone();
        Arc::new(move |args: serde_json::Value| {
            let wt = wt.clone();
            Box::pin(async move {
                let pattern = args["pattern"].as_str().unwrap_or("parallel");
                let agents_json = args["agents"].as_str().unwrap_or("[]");
                let tasks_json = args["tasks"].as_str().unwrap_or("[]");
                let started = std::time::Instant::now();

                match volt::orchestrator::parse_agent_specs(agents_json) {
                    Ok(specs) => match serde_json::from_str::<Vec<String>>(tasks_json) {
                        Ok(tasks) => {
                            let orch = volt::orchestrator::Orchestrator::new(wt.clone());
                            match orch.run_workflow(pattern, specs, tasks).await {
                                Ok(result) => volt::models::ToolResult {
                                    success: true,
                                    output: result.final_output,
                                    error: None,
                                    duration_ms: started.elapsed().as_millis(),
                                },
                                Err(e) => volt::models::ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!("workflow error: {}", e)),
                                    duration_ms: started.elapsed().as_millis(),
                                },
                            }
                        }
                        Err(e) => volt::models::ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("invalid tasks JSON: {}", e)),
                            duration_ms: started.elapsed().as_millis(),
                        },
                    },
                    Err(e) => volt::models::ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("invalid agents JSON: {}", e)),
                        duration_ms: started.elapsed().as_millis(),
                    },
                }
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
        })
    };
    registry.register_with_permission("run_workflow", "Execute a multi-agent workflow (parallel or pipeline) and return combined results", serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "workflow pattern: 'parallel' or 'pipeline'" },
            "agents": { "type": "string", "description": "JSON array of agent specs, each with 'name' (required) and optional 'model', 'system_prompt', 'max_iterations', 'temperature'" },
            "tasks": { "type": "string", "description": "JSON array of task strings (one per agent for parallel, one per stage for pipeline)" }
        },
        "required": ["pattern", "agents", "tasks"]
    }), "builtin", workflow_fn, PermissionLevel::Prompt).await;

    registry
        .register_with_permission(
            "web_scrape",
            "Extract structured content from a URL using a CSS selector. Returns text content of all matching elements.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to scrape" },
                    "selector": { "type": "string", "description": "CSS selector to match elements" }
                },
                "required": ["url", "selector"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    let selector = args["selector"].as_str().unwrap_or("");
                    volt::tools::scrape_tool::web_scrape(url, selector).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register_with_permission(
            "web_scrape_all",
            "Fetch a URL and extract all human-readable content (headings, paragraphs, links). General-purpose page reading without needing a CSS selector.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch and extract" }
                },
                "required": ["url"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    volt::tools::scrape_tool::web_scrape_all(url).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register(
            "json_validate",
            "Validate JSON string and return its type (object, array, string, number, boolean, null).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON string to validate" }
                },
                "required": ["data"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let data = args["data"].as_str().unwrap_or("");
                    volt::tools::json_tool::json_validate(data).await
                })
            }),
        )
        .await;

    registry
        .register(
            "json_prettify",
            "Format JSON with custom indentation for readability.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON string to format" },
                    "indent": { "type": "integer", "description": "spaces per indent level (default: 2)" }
                },
                "required": ["data"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let data = args["data"].as_str().unwrap_or("");
                    let indent = args["indent"].as_u64().unwrap_or(2) as u8;
                    volt::tools::json_tool::json_prettify(data, indent).await
                })
            }),
        )
        .await;

    registry
        .register(
            "json_query",
            "Extract a value from JSON using a dot-separated path (e.g. 'store.book[0].title'). Supports nested objects and array indexing.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON string to query" },
                    "path": { "type": "string", "description": "dot-separated path with optional array indices (e.g. 'items[0].name')" }
                },
                "required": ["data", "path"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let data = args["data"].as_str().unwrap_or("");
                    let path = args["path"].as_str().unwrap_or("");
                    volt::tools::json_tool::json_query(data, path).await
                })
            }),
        )
        .await;

    if minimal {
        // Benchmark / minimal mode: only load essential tools to keep system prompt small
        return registry;
    }

    #[cfg(feature = "tools-screenshot")]
    registry
        .register_with_permission(
            "screenshot",
            "Capture a screenshot of the primary monitor. Returns a base64-encoded PNG image. Use this to see what's on screen.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            "builtin",
            Arc::new(|_args| {
                Box::pin(async move {
                    volt::tools::screenshot::capture_screenshot().await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry.register("create_bar_chart","Create a bar chart from labels and values, save as HTML.",
        serde_json::json!({"type":"object","properties":{"title":{"type":"string"},"labels":{"type":"array","items":{"type":"string"}},"values":{"type":"array","items":{"type":"number"}},"output_path":{"type":"string"}},"required":["title","labels","values","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["title"].as_str().unwrap_or("Chart");
            let l: Vec<String> = args["labels"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let v: Vec<f64> = args["values"].as_array().map(|a| a.iter().filter_map(|n| n.as_f64()).collect()).unwrap_or_default();
            let o = args["output_path"].as_str().unwrap_or("chart.html");
            volt::tools::chart_tool::create_bar_chart(t, l, v, o).await
        }))).await;

    registry.register("create_line_chart","Create a line chart from labels and values, save as HTML.",
        serde_json::json!({"type":"object","properties":{"title":{"type":"string"},"labels":{"type":"array","items":{"type":"string"}},"values":{"type":"array","items":{"type":"number"}},"output_path":{"type":"string"}},"required":["title","labels","values","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["title"].as_str().unwrap_or("Chart");
            let l: Vec<String> = args["labels"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let v: Vec<f64> = args["values"].as_array().map(|a| a.iter().filter_map(|n| n.as_f64()).collect()).unwrap_or_default();
            let o = args["output_path"].as_str().unwrap_or("chart.html");
            volt::tools::chart_tool::create_line_chart(t, l, v, o).await
        }))).await;
    #[cfg(feature = "tools-pdf")]
    registry.register_with_permission("create_pdf","Create a PDF document from text content.",
        serde_json::json!({"type":"object","properties":{"content":{"type":"string","description":"text content"},"output_path":{"type":"string","description":"output .pdf path"}},"required":["content","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let c = args["content"].as_str().unwrap_or(""); let o = args["output_path"].as_str().unwrap_or("output.pdf");
            volt::tools::pdf_tool::create_pdf(c, o).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register_with_permission("desktop_click","Click at screen coordinates.",
        serde_json::json!({"type":"object","properties":{"x":{"type":"integer"},"y":{"type":"integer"}},"required":["x","y"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let x = args["x"].as_i64().unwrap_or(0) as i32; let y = args["y"].as_i64().unwrap_or(0) as i32;
            volt::tools::desktop_tool::desktop_click(x, y).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register_with_permission("desktop_type","Type text at cursor position.",
        serde_json::json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["text"].as_str().unwrap_or("");
            volt::tools::desktop_tool::desktop_type(t).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register_with_permission("desktop_key","Press a key (enter, tab, escape, up, down, etc.).",
        serde_json::json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let k = args["key"].as_str().unwrap_or("");
            volt::tools::desktop_tool::desktop_key(k).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register("desktop_find_window","Find a window by title using Windows API.",
        serde_json::json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["title"].as_str().unwrap_or("");
            volt::tools::desktop_tool::desktop_find_window(t).await
        }))).await;

    #[cfg(feature = "tools-browser")]
    registry.register_with_permission("browser_navigate","Open a URL in headless Chrome and return the URL.",
        serde_json::json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let u = args["url"].as_str().unwrap_or("");
            volt::tools::browser_tool::browser_navigate(u).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-browser")]
    registry.register_with_permission("browser_extract","Open a URL and extract text via CSS selector.",
        serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"selector":{"type":"string"}},"required":["url","selector"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let u = args["url"].as_str().unwrap_or(""); let s = args["selector"].as_str().unwrap_or("");
            volt::tools::browser_tool::browser_extract(u, s).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-browser")]
    registry.register_with_permission("browser_screenshot","Open a URL and save a page screenshot.",
        serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"output_path":{"type":"string"}},"required":["url","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let u = args["url"].as_str().unwrap_or(""); let o = args["output_path"].as_str().unwrap_or("screenshot.png");
            volt::tools::browser_tool::browser_screenshot(u, o).await
        })), PermissionLevel::Prompt).await;

    registry
        .register(
            "csv_read",
            "Read a CSV file and return its contents as formatted rows. Supports flexible column counts and optional headers.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "path to CSV file" },
                    "has_header": { "type": "boolean", "description": "whether the CSV has a header row (default: true)" }
                },
                "required": ["path"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let has_header = args["has_header"].as_bool().unwrap_or(true);
                    volt::tools::csv_tool::csv_read(path, has_header).await
                })
            }),
        )
        .await;

    registry
        .register(
            "csv_write",
            "Write data to a CSV file. Provide data as comma-separated lines, first line is header if has_header is true.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "path to CSV file" },
                    "data": { "type": "string", "description": "CSV data, one row per line, comma-separated values" },
                    "has_header": { "type": "boolean", "description": "whether first line is a header row (default: true)" }
                },
                "required": ["path", "data"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let data = args["data"].as_str().unwrap_or("");
                    let has_header = args["has_header"].as_bool().unwrap_or(true);
                    volt::tools::csv_tool::csv_write(path, data, has_header).await
                })
            }),
        )
        .await;

    registry
        .register(
            "archive_extract",
            "Extract an archive file (tar.gz, tgz, tar, gz) to a destination directory.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "path to archive file" },
                    "dest": { "type": "string", "description": "destination directory to extract into" }
                },
                "required": ["path", "dest"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let dest = args["dest"].as_str().unwrap_or("");
                    volt::tools::archive_tool::archive_extract(path, dest).await
                })
            }),
        )
        .await;

    registry
        .register(
            "archive_create",
            "Create a tar or tar.gz archive from a list of source files/directories.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "output archive path" },
                    "sources": { "type": "array", "items": { "type": "string" }, "description": "list of files and directories to include" },
                    "format": { "type": "string", "description": "archive format: 'tar' or 'tar.gz' (default: 'tar.gz')" }
                },
                "required": ["path", "sources"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let sources: Vec<String> = args["sources"].as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    let format = args["format"].as_str().unwrap_or("tar.gz");
                    volt::tools::archive_tool::archive_create(path, &sources, format).await
                })
            }),
        )
        .await;

    // ── Git tools ─────────────────────────────────────────────────────────
    registry.register("git_status", "Show the working tree status (porcelain format).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        volt::tools::git_tool::git_status(repo).await
    }))).await;

    registry.register("git_diff_unstaged", "Show unstaged changes in the working directory.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        volt::tools::git_tool::git_diff_unstaged(repo).await
    }))).await;

    registry.register("git_diff_staged", "Show staged changes (diff --cached).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        volt::tools::git_tool::git_diff_staged(repo).await
    }))).await;

    registry.register("git_diff", "Show differences between branches or commits.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "target": { "type": "string", "description": "branch, commit, or range to diff against" }
        },
        "required": ["target"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let target = args["target"].as_str().unwrap_or("HEAD");
        volt::tools::git_tool::git_diff(repo, target).await
    }))).await;

    registry.register("git_commit", "Record changes to the repository.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "message": { "type": "string", "description": "commit message" }
        },
        "required": ["message"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let msg = args["message"].as_str().unwrap_or("");
        volt::tools::git_tool::git_commit(repo, msg).await
    }))).await;

    registry.register("git_add", "Add file contents to the staging area.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "files": { "type": "array", "items": { "type": "string" }, "description": "files to stage" }
        },
        "required": ["files"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let files: Vec<String> = args["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        volt::tools::git_tool::git_add(repo, &files).await
    }))).await;

    registry.register("git_reset", "Unstage all staged changes.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        volt::tools::git_tool::git_reset(repo).await
    }))).await;

    registry.register("git_log", "Show commit logs (oneline format).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "max_count": { "type": "number", "description": "maximum number of commits to show (default: 20)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let count = args["max_count"].as_u64().unwrap_or(20) as u32;
        volt::tools::git_tool::git_log(repo, count).await
    }))).await;

    registry.register("git_create_branch", "Create a new branch.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "branch": { "type": "string", "description": "name of the new branch" },
            "base": { "type": "string", "description": "optional base branch or commit" }
        },
        "required": ["branch"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let branch = args["branch"].as_str().unwrap_or("");
        let base = args["base"].as_str();
        volt::tools::git_tool::git_create_branch(repo, branch, base).await
    }))).await;

    registry.register("git_checkout", "Switch branches.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "branch": { "type": "string", "description": "branch to switch to" }
        },
        "required": ["branch"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let branch = args["branch"].as_str().unwrap_or("");
        volt::tools::git_tool::git_checkout(repo, branch).await
    }))).await;

    registry.register("git_show", "Show the contents of a commit.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "revision": { "type": "string", "description": "revision (commit hash, branch, tag)" }
        },
        "required": ["revision"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let rev = args["revision"].as_str().unwrap_or("HEAD");
        volt::tools::git_tool::git_show(repo, rev).await
    }))).await;

    registry.register("git_branch", "List git branches.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        volt::tools::git_tool::git_branch(repo).await
    }))).await;

    // ── Time tools ─────────────────────────────────────────────────────────
    registry.register("get_current_time", "Get the current time in a specific timezone.", serde_json::json!({
        "type": "object",
        "properties": {
            "timezone": { "type": "string", "description": "IANA timezone (e.g. 'America/New_York', 'UTC', 'Asia/Tokyo')" }
        },
        "required": ["timezone"]
    }), "utilities", Arc::new(|args| Box::pin(async move {
        let tz = args["timezone"].as_str().unwrap_or("UTC");
        volt::tools::time_tool::get_current_time(tz).await
    }))).await;

    registry
        .register(
            "convert_time",
            "Convert time between timezones.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "timezone": { "type": "string", "description": "source IANA timezone" },
                    "timezone_to": { "type": "string", "description": "target IANA timezone" }
                },
                "required": ["timezone", "timezone_to"]
            }),
            "utilities",
            Arc::new(|args| {
                Box::pin(async move {
                    let from = args["timezone"].as_str().unwrap_or("UTC");
                    let to = args["timezone_to"].as_str().unwrap_or("UTC");
                    volt::tools::time_tool::convert_time(from, to).await
                })
            }),
        )
        .await;

    // ── Sequential thinking ────────────────────────────────────────────────
    registry.register("sequentialthinking", "A detailed tool for dynamic and reflective problem-solving through structured thoughts. Use when the task requires careful reasoning, multi-step analysis, or exploring alternative solutions.", serde_json::json!({
        "type": "object",
        "properties": {
            "thought": { "type": "string", "description": "your current thought or reasoning step" },
            "next_thought_needed": { "type": "boolean", "description": "whether another thought step is needed" },
            "branch_id": { "type": "string", "description": "optional branch ID to explore alternative reasoning paths" },
            "branch_from_thought": { "type": "number", "description": "optional thought number to branch from" }
        },
        "required": ["thought", "next_thought_needed"]
    }), "reasoning", Arc::new(|args| Box::pin(async move {
        let thought = args["thought"].as_str().unwrap_or("");
        let next = args["next_thought_needed"].as_bool().unwrap_or(true);
        let branch_id = args["branch_id"].as_str();
        let branch_from = args["branch_from_thought"].as_u64().map(|n| n as u32);
        volt::tools::sequential_thinking::sequentialthinking(thought, next, branch_id, branch_from).await
    }))).await;

    registry
}
