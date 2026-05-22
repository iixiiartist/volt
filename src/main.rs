use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
use volt::agent::loop_rs::Agent;
use volt::config::Settings;
use volt::db;
use volt::embedding::EmbeddingClient;
use volt::llm::OpenAIProvider;
use volt::mcp::MCPServer;
use volt::models::*;
use volt::registry::{provision_manifest, RegistryClient};
use volt::tools::ToolRegistry;
use volt::{sandbox, validation};

#[derive(Parser, Debug)]
#[command(name = "volt")]
#[command(about = "Volt — agent tool runtime and registry CLI", long_about = None)]
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
    },

    /// Start an interactive agent chat session.
    AgentChat {
        #[arg(long)]
        model: Option<String>,
    },

    /// Start the MCP stdio server for tool access.
    McpServe,

    /// Start interactive TUI chat session.
    AgentTui {
        #[arg(long)]
        model: Option<String>,
    },

    /// Multi-agent workflow: parallel, pipeline, or supervisor.
    Workflow {
        #[arg(long)]
        pattern: String,
        #[arg(long)]
        agents: String,
        #[arg(long)]
        tasks: String,
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
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
            let embedder = EmbeddingClient::with_provider(
                settings.embedding_api_key.clone(),
                settings.embedding_model.clone(),
                settings.embedding_provider.clone(),
                settings.embedding_endpoint.clone(),
            );
            let result = provision_manifest(&pool, &embedder, manifest, marketplace_verified).await?;
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
            let embedder = EmbeddingClient::with_provider(
                settings.embedding_api_key.clone(),
                settings.embedding_model.clone(),
                settings.embedding_provider.clone(),
                settings.embedding_endpoint.clone(),
            );
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
                .map(|p| serde_json::from_str(p).unwrap_or(serde_json::json!({})))
                .unwrap_or(serde_json::json!({}));
            let tool_info = db::get_tool_by_name(&pool, &tool).await?;
            let tool_id = tool_info.as_ref().map(|t| t.id);
            let source = db::get_tool_source(&pool, &tool).await?;
            let execution_id = Uuid::new_v4();

            match source {
                Some(code) => {
                    let sandbox_cmd = format!("echo '{}' | python3 -c \"{}\"", tool_params, code);
                    let result = sandbox::run_command(&sandbox_cmd, &settings.sandbox_policy).await?;
                    let output_val = serde_json::from_str::<serde_json::Value>(&result.stdout)
                        .unwrap_or(serde_json::json!({ "raw": result.stdout }));
                    let status = if result.status == "ok" { "success" } else { "failed" };

                    db::record_execution(
                        &pool,
                        tool_id,
                        &tool,
                        &tool_params,
                        &output_val,
                        status,
                        if result.status != "ok" { Some(&result.stderr) } else { None },
                        result.duration_ms as i32,
                        execution_id,
                    )
                    .await?;

                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "execution_id": execution_id.to_string(),
                        "status": status,
                        "output": output_val,
                        "duration_ms": result.duration_ms,
                    }))?);
                }
                None => {
                    anyhow::bail!("tool '{}' not found; provision it first", tool);
                }
            }
        }
        Commands::Sandbox { command, timeout_ms } => {
            let policy = SandboxPolicy {
                timeout_ms: timeout_ms.unwrap_or(settings.sandbox_policy.timeout_ms),
                max_stdout_bytes: settings.sandbox_policy.max_stdout_bytes,
                working_dir: settings.sandbox_policy.working_dir,
            };
            let result = sandbox::run_command(&command, &policy).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::AgentRun { input, model } => {
            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL")
                    .unwrap_or_else(|_| "phi4-mini:3.8b".into())
            });
            let api_key = std::env::var("NVIDIA_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            let base_url = std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".into());

            let cancel = volt::models::CancelToken::new();
            let c = cancel.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                eprintln!("\n[interrupt] shutting down...");
                c.cancel();
            });

            let provider = Box::new(OpenAIProvider::new(api_key, base_url, "nvidia".into()));
            let embedder = EmbeddingClient::with_provider(
                settings.embedding_api_key.clone(),
                settings.embedding_model.clone(),
                settings.embedding_provider.clone(),
                settings.embedding_endpoint.clone(),
            );
            let tools = setup_tools(Some(&embedder)).await;
            let config = AgentConfig {
                name: "volt-agent".into(),
                model,
                provider: "nvidia".into(),
                system_prompt: None,
                max_iterations: 25,
                temperature: 0.3,
                toolsets: vec!["builtin".into()],
                hidden: false,
            };
            let mut agent = Agent::new(config, provider, tools)
                .with_cancel(cancel)
                .with_stream(std::sync::Arc::new(|token| {
                    print!("{}", token);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }));
            let pool = db::connect(&settings.database_url).await.ok();
            if let Some(ref p) = pool {
                agent = agent.with_memory(p.clone(), embedder.clone());
            }
            let skills = setup_skills(pool, Some(embedder)).await;
            agent = agent.with_skills(skills);
            println!();
            match agent.run(&input).await {
                Ok(result) => {
                    println!();
                    println!("{}", result);
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                }
            }
        }
        Commands::AgentChat { model } => {
            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL")
                    .unwrap_or_else(|_| "phi4-mini:3.8b".into())
            });
            let api_key = std::env::var("NVIDIA_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            let base_url = std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".into());

            let cancel = volt::models::CancelToken::new();
            let c = cancel.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                eprintln!("\n[interrupt] shutting down...");
                c.cancel();
            });

            let provider = Box::new(OpenAIProvider::new(api_key, base_url, "nvidia".into()));
            let tools = register_all_tools().await;
            let config = AgentConfig {
                name: "volt-agent".into(),
                model,
                provider: "nvidia".into(),
                system_prompt: None,
                max_iterations: 25,
                temperature: 0.3,
                toolsets: vec!["builtin".into()],
                hidden: false,
            };
            let mut agent = Agent::new(config, provider, tools)
                .with_cancel(cancel)
                .with_stream(std::sync::Arc::new(|token| {
                    print!("{}", token);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }));
            if let Ok(pool) = db::connect(&settings.database_url).await {
                let embedder = EmbeddingClient::with_provider(
                    settings.embedding_api_key.clone(),
                    settings.embedding_model.clone(),
                    settings.embedding_provider.clone(),
                    settings.embedding_endpoint.clone(),
                );
                agent = agent.with_memory(pool, embedder);
            }

            let sessions_pool = volt::session::open_sessions(
                &std::path::PathBuf::from("volt_sessions.db"),
            )
            .await
            .ok();

            if let Some(ref sp) = sessions_pool {
                if let Ok(sessions) = volt::session::list_sessions(sp, 10).await {
                    if !sessions.is_empty() {
                        println!("Past sessions:");
                        for (i, s) in sessions.iter().enumerate() {
                            println!("  {}. {} ({} msgs, {})", i + 1, s.title, s.message_count, s.created_at.format("%b %d %H:%M"));
                        }
                        print!("Resume a session? [1-{}/N] ", sessions.len());
                        use std::io::Write;
                        std::io::stdout().flush()?;
                        let mut answer = String::new();
                        std::io::stdin().read_line(&mut answer).ok();
                        if let Ok(idx) = answer.trim().parse::<usize>() {
                            if let Some(s) = sessions.get(idx.wrapping_sub(1)) {
                                if let Ok(msgs) = volt::session::load_messages(sp, s.id).await {
                                    let mut state = agent.state.lock().await;
                                    state.session_id = s.id;
                                    state.messages = msgs;
                                    println!("Resumed session '{}' with {} messages.", s.title, state.messages.len());
                                }
                            }
                        }
                    }
                }
            }

            println!("Volt agent chat — type /quit to exit");
            let mut input = String::new();
            loop {
                input.clear();
                print!("> ");
                use std::io::Write;
                std::io::stdout().flush()?;
                std::io::stdin().read_line(&mut input)?;
                let input = input.trim();
                if input.is_empty() || input == "/quit" {
                    break;
                }
                let result = agent.run(input).await?;
                println!("{}", result);

                if let Some(ref sp) = sessions_pool {
                    let state = agent.state.lock().await;
                    let _ = volt::session::create_session(
                        sp,
                        &Session {
                            id: state.session_id,
                            agent_name: state.name.clone(),
                            title: input.chars().take(60).collect(),
                            message_count: state.messages.len() as u32,
                            created_at: state.created_at,
                            updated_at: state.updated_at,
                        },
                    )
                    .await;
                    for msg in state.messages.iter().rev().take(2) {
                        let _ = volt::session::save_message(sp, state.session_id, msg).await;
                    }
                }
            }
        }
        Commands::AgentTui { model } => {
            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL")
                    .unwrap_or_else(|_| "phi4-mini:3.8b".into())
            });
            let api_key = std::env::var("NVIDIA_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            let base_url = std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".into());

            let provider = Box::new(OpenAIProvider::new(api_key, base_url, "nvidia".into()));
            let tools = register_all_tools().await;
            let config = AgentConfig {
                name: "volt-agent".into(),
                model,
                provider: "nvidia".into(),
                system_prompt: None,
                max_iterations: 25,
                temperature: 0.3,
                toolsets: vec!["builtin".into()],
                hidden: false,
            };
            let mut agent = Agent::new(config, provider, tools);
            if let Ok(pool) = db::connect(&settings.database_url).await {
                let embedder = EmbeddingClient::with_provider(
                    settings.embedding_api_key.clone(),
                    settings.embedding_model.clone(),
                    settings.embedding_provider.clone(),
                    settings.embedding_endpoint.clone(),
                );
                agent = agent.with_memory(pool, embedder);
            }

            if let Ok(sp) = volt::session::open_sessions(
                &std::path::PathBuf::from("volt_sessions.db"),
            ).await {
                if let Ok(sessions) = volt::session::list_sessions(&sp, 10).await {
                    if !sessions.is_empty() {
                        println!("Past sessions:");
                        for (i, s) in sessions.iter().enumerate() {
                            println!("  {}. {} ({} msgs, {})", i + 1, s.title, s.message_count, s.created_at.format("%b %d %H:%M"));
                        }
                        print!("Resume a session? [1-{}/N] ", sessions.len());
                        use std::io::Write;
                        std::io::stdout().flush()?;
                        let mut answer = String::new();
                        std::io::stdin().read_line(&mut answer).ok();
                        if let Ok(idx) = answer.trim().parse::<usize>() {
                            if let Some(s) = sessions.get(idx.wrapping_sub(1)) {
                                if let Ok(msgs) = volt::session::load_messages(&sp, s.id).await {
                                    let mut state = agent.state.lock().await;
                                    state.session_id = s.id;
                                    state.messages = msgs;
                                }
                            }
                        }
                    }
                }
            }

            volt::tui::TuiChat::run(&agent).await?;
        }
        Commands::Workflow { pattern, agents, tasks } => {
            let specs = volt::orchestrator::parse_agent_specs(&agents)?;
            let tasks: Vec<String> = serde_json::from_str(&tasks)?;
            let tools = register_all_tools().await;
            let orch = volt::orchestrator::Orchestrator::new(tools);
            let result = orch.run_workflow(&pattern, specs, tasks).await?;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "steps": result.steps.iter().map(|s| serde_json::json!({
                    "agent": s.agent_name,
                    "success": s.success,
                    "duration_ms": s.duration_ms,
                    "output": s.output,
                })).collect::<Vec<_>>(),
                "final_output": result.final_output,
                "total_duration_ms": result.total_duration_ms,
            }))?);
        }
        Commands::Eval { suite, model } => {
            let content = tokio::fs::read_to_string(&suite).await?;
            let suite_data: volt::eval::EvalSuite = serde_json::from_str(&content)?;

            let model = model.unwrap_or_else(|| {
                std::env::var("LLM_MODEL")
                    .unwrap_or_else(|_| "phi4-mini:3.8b".into())
            });
            let api_key = std::env::var("NVIDIA_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            let base_url = std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".into());

            let provider = Box::new(volt::llm::OpenAIProvider::new(api_key, base_url, "nvidia".into()));
            let tools = register_all_tools().await;
            let config = AgentConfig {
                name: "eval-agent".into(),
                model,
                provider: "nvidia".into(),
                system_prompt: None,
                max_iterations: 15,
                temperature: 0.3,
                toolsets: vec!["builtin".into()],
                hidden: false,
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
            let embedder = EmbeddingClient::with_provider(
                settings.embedding_api_key.clone(),
                settings.embedding_model.clone(),
                settings.embedding_provider.clone(),
                settings.embedding_endpoint.clone(),
            );
            let pool = db::connect(&settings.database_url).await?;
            let registry = volt::skills::SkillRegistry::new(Some(pool), Some(embedder.clone()));
            registry.compile_skill(&path, &embedder).await?;
            println!("Skill compiled from {:?} and stored in database.", path);
        }
    }

    Ok(())
}

async fn load_manifest(path: &PathBuf) -> anyhow::Result<RegistryManifest> {
    let body = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str::<RegistryManifest>(&body)?)
}

async fn setup_skills(pool: Option<sqlx::PgPool>, embedder: Option<EmbeddingClient>) -> Arc<volt::skills::SkillRegistry> {
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

    registry.register_with_permission("bash", "Execute a shell command", serde_json::json!({
        "type": "object",
        "properties": {
            "command": { "type": "string", "description": "shell command to run" }
        },
        "required": ["command"]
    }), "builtin", Arc::new(|args| {
        let cmd = args["command"].as_str().unwrap_or("");
        volt::tools::bash::execute_bash(cmd)
    }), PermissionLevel::Prompt).await;

    registry.register("read", "Read a file from disk", serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "file path to read" }
        },
        "required": ["path"]
    }), "builtin", Arc::new(|args| {
        let path = args["path"].as_str().unwrap_or("");
        volt::tools::read_tool::read_file(path)
    })).await;

    registry.register_with_permission("write", "Write content to a file", serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "file path" },
            "content": { "type": "string", "description": "content to write" }
        },
        "required": ["path", "content"]
    }), "builtin", Arc::new(|args| {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        volt::tools::write_tool::write_file(path, content)
    }), PermissionLevel::Prompt).await;

    registry.register_with_permission("edit", "Edit a file by replacing text", serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "file path" },
            "old_string": { "type": "string", "description": "text to replace" },
            "new_string": { "type": "string", "description": "replacement text" }
        },
        "required": ["path", "old_string", "new_string"]
    }), "builtin", Arc::new(|args| {
        let path = args["path"].as_str().unwrap_or("");
        let old = args["old_string"].as_str().unwrap_or("");
        let new = args["new_string"].as_str().unwrap_or("");
        volt::tools::edit::edit_file(path, old, new)
    }), PermissionLevel::Prompt).await;

    registry.register("glob", "Find files matching a glob pattern", serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "glob pattern" },
            "base": { "type": "string", "description": "base directory" }
        },
        "required": ["pattern"]
    }), "builtin", Arc::new(|args| {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        let base = args["base"].as_str().unwrap_or(".");
        volt::tools::glob_tool::glob_files(pattern, base)
    })).await;

    registry.register("grep", "Search file contents with regex", serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "regex pattern" },
            "path": { "type": "string", "description": "directory to search" }
        },
        "required": ["pattern"]
    }), "builtin", Arc::new(|args| {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let path = args["path"].as_str().unwrap_or(".");
        volt::tools::grep_tool::grep_files(pattern, path)
    })).await;

    registry.register("web_fetch", "Fetch a URL and return its content", serde_json::json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "description": "URL to fetch" }
        },
        "required": ["url"]
    }), "builtin", Arc::new(|args| {
        let url = args["url"].as_str().unwrap_or("");
        volt::tools::web_tool::web_fetch(url)
    })).await;

    registry.register("memory_append", "Append to persistent memory file", serde_json::json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "description": "memory category" },
            "content": { "type": "string", "description": "content to remember" }
        },
        "required": ["kind", "content"]
    }), "builtin", Arc::new(|args| {
        let kind = args["kind"].as_str().unwrap_or("note");
        let content = args["content"].as_str().unwrap_or("");
        volt::tools::memory_tool::memory_append(kind, content)
    })).await;

    registry.register("todo_add", "Add a task to the todo list", serde_json::json!({
        "type": "object",
        "properties": {
            "task": { "type": "string", "description": "task description" }
        },
        "required": ["task"]
    }), "builtin", Arc::new(|args| {
        let task = args["task"].as_str().unwrap_or("");
        volt::tools::todo_tool::todo_add(task)
    })).await;

    let delegate_tools = registry.clone();
    registry.register("delegate", "Delegate a sub-task to a sub-agent and return its result", serde_json::json!({
        "type": "object",
        "properties": {
            "task": { "type": "string", "description": "task description for the sub-agent" },
            "context": { "type": "string", "description": "context and constraints from the parent agent" }
        },
        "required": ["task"]
    }), "builtin", Arc::new(move |args| {
        let task = args["task"].as_str().unwrap_or("");
        let context = args["context"].as_str().unwrap_or("");
        volt::tools::delegate::delegate_task(task, context, delegate_tools.clone())
    })).await;

    let workflow_tools = registry.clone();
    registry.register("run_workflow", "Execute a multi-agent workflow (parallel or pipeline) and return combined results", serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "workflow pattern: 'parallel' or 'pipeline'" },
            "agents": { "type": "string", "description": "JSON array of agent specs, each with 'name' (required) and optional 'model', 'system_prompt', 'max_iterations', 'temperature'" },
            "tasks": { "type": "string", "description": "JSON array of task strings (one per agent for parallel, one per stage for pipeline)" }
        },
        "required": ["pattern", "agents", "tasks"]
    }), "builtin", Arc::new(move |args| {
        let pattern = args["pattern"].as_str().unwrap_or("parallel");
        let agents_json = args["agents"].as_str().unwrap_or("[]");
        let tasks_json = args["tasks"].as_str().unwrap_or("[]");
        let started = std::time::Instant::now();

        match volt::orchestrator::parse_agent_specs(agents_json) {
            Ok(specs) => {
                match serde_json::from_str::<Vec<String>>(tasks_json) {
                    Ok(tasks) => {
                        let orch = volt::orchestrator::Orchestrator::new(workflow_tools.clone());
                        let handle = tokio::runtime::Handle::current();
                        match tokio::task::block_in_place(|| {
                            handle.block_on(orch.run_workflow(pattern, specs, tasks))
                        }) {
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
                }
            }
            Err(e) => volt::models::ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("invalid agents JSON: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            },
        }
    })).await;

    registry
}