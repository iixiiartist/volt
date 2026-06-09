use super::AgentMode;
use crate::agent::Agent;
use crate::context::ContextStore;
use crate::embedding::EmbeddingClient;
use crate::models::*;
use crate::session;
use crate::{orchestrator, worker};
use std::path::PathBuf;
use std::sync::Arc;

pub async fn run(options: AgentRunOptions) -> anyhow::Result<()> {
    let AgentRunOptions {
        input,
        model,
        allow,
        load_tools,
        context_kinds,
        mode,
        session_id,
        max_iterations,
        settings,
        use_mtp: _,
        use_cot: _,
        allow_write: _,
        framework: _,
        model_variant: _,
        quantization: _,
        blueprint,
        auto_blueprint,
        print,
        json,
        plan,
        worktree,
    } = options;

    // Validate output-mode flags.
    if print && json {
        anyhow::bail!("--print and --json are mutually exclusive");
    }

    // In print/json mode, suppress progress chatter. Eprintln is used for
    // diagnostics throughout this function; we route it via a no-op sink
    // when quiet mode is on.
    macro_rules! chat {
        ($($arg:tt)*) => {
            if !(print || json) {
                eprintln!($($arg)*);
            }
        };
    }

    let (provider, provider_kind) = orchestrator::build_provider(&model, "volt-agent");
    let cancel = CancelToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        c.cancel();
    });

    let embedder = EmbeddingClient::new_smart().await;
    let tools = crate::tools::setup_tools(Some(&embedder), Some(&settings.database_url)).await;

    if let Some(ref path) = load_tools {
        load_tool_stubs(path, &tools, &embedder).await;
    }

    let tools_for_agent = tools.clone();
    let tools_for_seed = tools.clone();
    let embedder_for_skills = embedder.clone();
    let embedder_for_worker = embedder.clone();
    let cancel_for_agent = cancel.clone();
    let cancel_for_worker = cancel.clone();

    let mode_profile = mode.parse::<AgentMode>().unwrap_or(AgentMode::Balanced);
    let enabled_kinds = if !context_kinds.is_empty() {
        crate::context::parse_context_kinds(&context_kinds)
    } else {
        mode_profile.context_kinds()
    };

    let model_name_for_envelope = model.clone();
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
        enabled_context_kinds: enabled_kinds,
        essential_tools: crate::models::default_essential_tools(),
        context_kind_quotas: Default::default(),
        use_mtp: options.use_mtp || settings.use_mtp,
        use_cot: options.use_cot || settings.use_cot,
        allow_write: options.allow_write || settings.allow_write,
        framework: options.framework.clone().or(settings.framework.clone()),
        model_variant: options
            .model_variant
            .clone()
            .or(settings.model_variant.clone()),
        quantization: options
            .quantization
            .clone()
            .or(settings.quantization.clone()),
        format_dialect: Default::default(),
        quirks: vec![],
        strict_mode: false,
        max_tools_per_turn: None,
        blueprint_path: None,
    };
    let config_quotas = config.context_kind_quotas.clone();
    // ── Blueprint selection: explicit > LLM-routed > none ────────
    let effective_blueprint = if let Some(ref bp) = blueprint {
        Some(bp.clone())
    } else if auto_blueprint {
        let blueprints = crate::agent::router::load_all_blueprints();
        if blueprints.is_empty() {
            chat!("[router] no blueprints found in blueprints/ directory");
            None
        } else {
            chat!(
                "[router] routing task across {} blueprint(s)...",
                blueprints.len()
            );
            match crate::agent::router::route_task(&input, &blueprints, &*provider).await {
                Some(bp) => {
                    chat!("[router] selected blueprint: {} ({})", bp.id, bp.name);
                    let paths = crate::agent::router::discover_blueprints();
                    paths
                        .iter()
                        .find(|p| p.file_stem().map(|s| s == bp.id.as_str()).unwrap_or(false))
                        .cloned()
                }
                None => {
                    chat!("[router] LLM could not determine best blueprint, using defaults");
                    None
                }
            }
        }
    } else {
        None
    };

    // Resolve the workspace. If `--worktree` was passed, create (or
    // reuse) a worktree for this session and use IT as the workspace
    // so the agent's file modifications are isolated.
    let workspace_root = if worktree {
        let cwd = std::env::current_dir().unwrap_or_default();
        match crate::commands::worktree::WorktreeManager::detect_repo_root(&cwd).await {
            Ok(Some(repo_root)) => {
                let mgr = crate::commands::worktree::WorktreeManager::new(repo_root);
                // The session id is generated a few lines below in the
                // SQLite block; for now synthesise a fresh uuid for the
                // worktree so the path is stable for the lifetime of
                // this run. If the session is later assigned a
                // different id (e.g. --session was passed), the
                // worktree will just be an extra one in .volt-worktrees.
                let wt_id = uuid::Uuid::new_v4();
                match mgr.create_for_session(wt_id).await {
                    Ok(info) => {
                        chat!(
                            "[worktree] isolated to {} (branch {})",
                            info.path.display(),
                            info.branch
                        );
                        info.path
                    }
                    Err(e) => {
                        chat!(
                            "[worktree] failed to create worktree: {} — falling back to cwd",
                            e
                        );
                        cwd
                    }
                }
            }
            Ok(None) => {
                chat!("[worktree] not in a git repository — --worktree ignored");
                std::env::current_dir().unwrap_or_default()
            }
            Err(e) => {
                chat!(
                    "[worktree] detect_repo_root failed: {} — falling back to cwd",
                    e
                );
                std::env::current_dir().unwrap_or_default()
            }
        }
    } else {
        std::env::current_dir().unwrap_or_default()
    };

    let mut agent = Agent::new(config, provider, tools_for_agent)
        .await
        .with_workspace(workspace_root)
        .with_cancel(cancel_for_agent)
        .with_stream(Arc::new(|token| {
            print!("{}", token);
        }));

    // Load hook registry from `.volt/hooks.toml` and `~/.volt/hooks.toml`.
    {
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut hook_registry = crate::agent::hooks::HookRegistry::from_default_paths(&cwd)
            .unwrap_or_else(|_| crate::agent::hooks::HookRegistry::empty());
        // session_id is set later in this flow; leave empty for now.
        hook_registry = hook_registry.with_session("", &agent.config().name);
        agent = agent.with_hooks(hook_registry);
    }

    if let Some(ref bp_path) = effective_blueprint {
        agent = agent.with_blueprint(bp_path.clone());
        chat!("[blueprint] loaded {}", bp_path.display());
    }

    let event_bus = crate::events::EventBus::new();
    agent = agent.with_event_bus(event_bus);

    let sessions_pool = {
        let session_db_path = PathBuf::from("volt_sessions.db");
        let sp = match session::open_sessions(&session_db_path).await {
            Ok(sp) => Some(sp),
            Err(e) => {
                chat!("[session] warning: {}", e);
                None
            }
        };
        if let Some(ref sqlite_pool) = sp {
            let sid = session_id
                .as_ref()
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .unwrap_or_else(uuid::Uuid::new_v4);
            let sess = Session {
                id: sid,
                agent_name: "volt-agent".into(),
                title: input.clone(),
                message_count: 0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            let _ = session::create_session(sqlite_pool, &sess).await;
            agent = agent.with_session(sid, sqlite_pool.clone());
            if session_id
                .as_ref()
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .is_none()
            {
                chat!("[session] created new session {}", sid);
            } else {
                chat!("[session] resumed session {}", sid);
            }
        }
        sp
    };

    let pool = match crate::db::connect(&settings.database_url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            chat!("[db] warning: {}. Running without memory.", e);
            None
        }
    };

    if let Some(ref p) = pool {
        agent = agent.with_memory(p.clone(), embedder.clone());
    } else {
        agent = agent.with_memory_embedder_only(embedder.clone());
    }

    if let Some(ref p) = pool {
        let failure_tracker = crate::tool_failure_tracker::ToolFailureTracker::new(Some(p.clone()));
        agent = agent.with_failure_tracker(failure_tracker);
    }

    let skills = crate::skills::setup_skills(pool.clone(), Some(embedder_for_skills)).await;
    agent = agent.with_skills(skills);

    let context_store = if let Some(ref p) = pool {
        let store = ContextStore::new_with_db(p.clone());
        match store.hydrate_from_db(2000).await {
            Ok(n) if n > 0 => chat!("[context] hydrated {} entries from DB", n),
            _ => {}
        }
        store
    } else {
        ContextStore::new()
    };

    if !config_quotas.is_empty() {
        context_store.set_quotas(&config_quotas).await;
    }

    if let Some(ref p) = pool {
        let skill_store = context_store.clone();
        let p_clone = p.clone();
        let emb_clone = embedder.clone();
        tokio::spawn(async move {
            worker::seed_skills_from_db(&skill_store, &p_clone, &emb_clone).await;
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

    chat!(
        "[mode] {} — {} context kinds",
        mode,
        mode_profile.context_kinds().len()
    );

    let started_at = std::time::Instant::now();
    // Plan-mode prefix: ask the model to output a plan as text before
    // executing any tools. Mirrors the TUI's `/plan` slash command.
    let plan_effective_input = if plan {
        format!(
            "[PLAN MODE — read-only]\n\
             You MUST respond with a numbered plan (steps, tools you would call, and the order \
             of operations) BEFORE invoking any tool. Wait for the user to approve the plan; do \
             not execute tools in this turn. After the plan, end your response.\n\n\
             USER REQUEST:\n{}",
            input
        )
    } else {
        input.clone()
    };
    let run_result = agent.run(&plan_effective_input).await;
    let elapsed = started_at.elapsed().as_secs_f32();

    // Capture the final state for the output envelope (token counts, model).
    let (total_p, total_c) = {
        let s = agent.state().lock().await;
        (s.total_prompt_tokens, s.total_completion_tokens)
    };
    let model_name = model_name_for_envelope;

    match run_result {
        Ok(result) => {
            emit_output(&result, json, print, total_p, total_c, elapsed, &model_name);
        }
        Err(e) => {
            let state = agent.state().lock().await;
            let last_text = state.messages.iter().rev().find_map(|m| {
                let c = m.content.trim();
                if !c.is_empty() {
                    Some(c.to_string())
                } else {
                    m.tool_result.as_ref().and_then(|r| {
                        if !r.trim().is_empty() {
                            Some(r.trim().to_string())
                        } else {
                            None
                        }
                    })
                }
            });
            match last_text {
                Some(text) => {
                    emit_output(&text, json, print, total_p, total_c, elapsed, &model_name);
                }
                None => {
                    // Truly empty: just print the error to stderr.
                    tracing::error!("error: {}", e);
                }
            }
        }
    }

    if let Some(ref sp) = sessions_pool {
        let state = agent.state().lock().await;
        let _ = session::create_session(
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
        let _ = session::save_session_messages_atomic(sp, state.session_id, &state.messages).await;
    }
    Ok(())
}

async fn load_tool_stubs(
    path: &str,
    tools: &Arc<crate::tools::ToolRegistry>,
    embedder: &EmbeddingClient,
) {
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
                    if name != "unknown" {
                        let name_owned = name.to_string();
                        let desc = fn_def["description"].as_str().unwrap_or("").to_string();
                        let schema = fn_def["parameters"].clone();
                        tools
                            .register(
                                name,
                                &desc,
                                schema,
                                "bfcl",
                                Arc::new(move |_args| {
                                    let msg = format!("[stub] {} called", name_owned);
                                    Box::pin(async move {
                                        ToolResult {
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
            tracing::info!("[tools] loaded {} BFCL stubs from {}", count, path);
            tools.compute_embeddings(embedder).await;
        }
        Err(e) => tracing::warn!("[tools] failed to load {}: {}", path, e),
    }
}

/// Emit the agent's final response text, honoring `--print` and `--json`.
/// Default mode prints the response and lets eprintln chatter through.
fn emit_output(
    result: &str,
    json_mode: bool,
    print_mode: bool,
    total_prompt_tokens: u64,
    total_completion_tokens: u64,
    elapsed_secs: f32,
    model: &str,
) {
    if json_mode {
        let envelope = json_envelope_ok(
            result,
            total_prompt_tokens,
            total_completion_tokens,
            elapsed_secs,
            model,
        );
        println!("{}", envelope);
    } else if print_mode {
        // Print only the response. No banner, no labels.
        print!("{}", result);
        use std::io::Write;
        let _ = std::io::stdout().flush();
    } else {
        println!("{}", result);
    }
}

fn json_envelope_ok(
    response: &str,
    total_prompt_tokens: u64,
    total_completion_tokens: u64,
    elapsed_secs: f32,
    model: &str,
) -> String {
    serde_json::json!({
        "ok": true,
        "response": response,
        "model": model,
        "tokens": {
            "prompt": total_prompt_tokens,
            "completion": total_completion_tokens,
        },
        "elapsed_secs": (elapsed_secs * 100.0).round() / 100.0,
    })
    .to_string()
}

pub struct AgentRunOptions {
    pub input: String,
    pub model: String,
    pub allow: bool,
    pub load_tools: Option<String>,
    pub context_kinds: Vec<String>,
    pub mode: String,
    pub session_id: Option<String>,
    pub max_iterations: Option<u32>,
    pub settings: crate::config::Settings,
    pub use_mtp: bool,
    pub use_cot: bool,
    pub allow_write: bool,
    pub framework: Option<String>,
    pub model_variant: Option<String>,
    pub quantization: Option<String>,
    pub blueprint: Option<PathBuf>,
    pub auto_blueprint: bool,
    /// Emit the response to stdout and suppress progress chatter.
    pub print: bool,
    /// Emit a single JSON envelope on stdout (one line) and suppress chatter.
    pub json: bool,
    /// When true, prefix the prompt with a planner directive that asks the
    /// model to output a plan as text before invoking any tool. Mirrors the
    /// `/plan` slash command in the TUI.
    pub plan: bool,
    /// When true, run the agent inside a fresh `git worktree` on a
    /// dedicated branch (`volt-session/<short-id>`). All file changes
    /// the agent makes are isolated to that worktree; the user can
    /// review the diff with `volt worktree list` / `volt worktree merge
    /// <id>` / `volt worktree clean <id>`.
    pub worktree: bool,
}

impl AgentRunOptions {
    pub fn model_or_default(model: Option<String>) -> String {
        model.unwrap_or_else(|| {
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
        })
    }
}
