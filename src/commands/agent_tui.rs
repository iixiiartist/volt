use super::AgentMode;
use crate::agent::Agent;
use crate::context::ContextStore;
use crate::embedding::EmbeddingClient;
use crate::llm::provider::TokenCallback;
use crate::models::*;
use crate::{db, session, worker};
use reedline::ExternalPrinter;
use std::sync::Arc;

pub async fn run(options: AgentTuiOptions) -> anyhow::Result<()> {
    let AgentTuiOptions {
        model,
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
    } = options;

    let (provider, provider_kind) = crate::orchestrator::try_build_provider(&model, "volt-agent")
        .map_err(|e| anyhow::anyhow!("{}\n{}", e, e.hint()))?;
    let embedder = EmbeddingClient::new_smart().await;
    let tools = crate::tools::setup_tools(Some(&embedder), None).await;

    let mode_profile = mode.parse::<AgentMode>().unwrap_or(AgentMode::Balanced);

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
        enabled_context_kinds: mode_profile.context_kinds(),
        essential_tools: crate::models::default_essential_tools(),
        context_kind_quotas: Default::default(),
        use_mtp,
        use_cot,
        allow_write,
        framework,
        model_variant,
        quantization,
        format_dialect: Default::default(),
        quirks: vec![],
        strict_mode: false,
        max_tools_per_turn: None,
        blueprint_path: None,
    };
    let workspace_root = if worktree {
        let cwd = std::env::current_dir().unwrap_or_default();
        match crate::commands::worktree::WorktreeManager::detect_repo_root(&cwd).await {
            Ok(Some(repo_root)) => {
                let mgr = crate::commands::worktree::WorktreeManager::new(repo_root);
                let wt_id = uuid::Uuid::new_v4();
                match mgr.create_for_session(wt_id).await {
                    Ok(info) => {
                        tracing::info!(
                            "[worktree] isolated to {} (branch {})",
                            info.path.display(),
                            info.branch
                        );
                        info.path
                    }
                    Err(e) => {
                        tracing::warn!("[worktree] failed: {} - using cwd", e);
                        cwd
                    }
                }
            }
            Ok(None) => {
                tracing::info!("[worktree] not in a git repo - --worktree ignored");
                std::env::current_dir().unwrap_or_default()
            }
            Err(e) => {
                tracing::warn!("[worktree] detect failed: {} - using cwd", e);
                std::env::current_dir().unwrap_or_default()
            }
        }
    } else {
        std::env::current_dir().unwrap_or_default()
    };

    let mut agent = Agent::new(config, provider, tools.clone())
        .await
        .with_workspace(workspace_root);

    let context_store = ContextStore::new();
    let (seed_channel, seed_rx) = worker::create_seed_channel();
    agent = agent
        .with_context(context_store.clone())
        .with_seed_channel(seed_channel);

    let cancel_tui = CancelToken::new();
    worker::AutoSeedWorker::new(context_store.clone(), embedder.clone(), cancel_tui).spawn(seed_rx);

    tokio::spawn(worker::seed_background(
        context_store.clone(),
        embedder.clone(),
        tools.clone(),
        settings.sandbox_policy.clone(),
        None,
    ));

    if let Ok(pool) = db::connect(&settings.database_url).await {
        context_store.set_db(pool.clone());
        agent = agent.with_memory(pool.clone(), embedder.clone());
        let skill_emb = embedder;
        tokio::spawn(async move {
            worker::seed_skills_from_db(&context_store, &pool, &skill_emb).await;
        });
    } else {
        agent = agent.with_memory_embedder_only(embedder);
    }

    println!(
        "[mode] {} — {} context kinds",
        mode,
        mode_profile.context_kinds().len()
    );

    if let Ok(sp) = session::open_sessions(&std::path::PathBuf::from("volt_sessions.db")).await {
        if let Ok(sessions) = session::list_sessions(&sp, 10).await {
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
                    if let Some(s) = idx.checked_sub(1).and_then(|i| sessions.get(i)) {
                        if let Ok(msgs) = session::load_messages(&sp, s.id).await {
                            let mut state = agent.state().lock().await;
                            state.session_id = s.id;
                            state.messages = msgs;
                        }
                    }
                }
            }
        }
    }

    // Build the agent with a streaming callback that prints tokens to an
    // ExternalPrinter. The TUI owns the printer and uses reedline as the
    // line editor; streamed tokens are printed above the input row in
    // real time.
    let printer = ExternalPrinter::<String>::default();
    let printer_for_cb = printer.clone();
    let on_token: TokenCallback = Arc::new(move |token: &str| {
        let _ = printer_for_cb.print(token.to_string());
    });

    // Wire the per-tool approval callback so the TUI can render an
    // approval widget instead of falling back to the stdin prompt.
    let (approval_tx, approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let approval_fn = crate::tui::approval_callback_for(approval_tx);
    // Load hook registry from `.volt/hooks.toml` and `~/.volt/hooks.toml`.
    // Missing files are not errors — they just mean no hooks configured.
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut hook_registry = crate::agent::hooks::HookRegistry::from_default_paths(&cwd)
        .unwrap_or_else(|_| crate::agent::hooks::HookRegistry::empty());
    let agent_name = agent.config().name.clone();
    // session_id is set later (in resume/new session flow) — leave empty
    // string here; registry will use its default placeholder.
    hook_registry = hook_registry.with_session("", &agent_name);
    let agent = Arc::new(
        agent
            .with_stream(on_token)
            .with_approval(approval_fn)
            .with_hooks(hook_registry),
    );

    let tui = crate::tui::TuiChat::new_with_approval(agent, tools, printer, approval_rx);
    tui.run().await?;
    Ok(())
}

pub struct AgentTuiOptions {
    pub model: String,
    pub allow: bool,
    pub max_iterations: Option<u32>,
    pub mode: String,
    pub settings: crate::config::Settings,
    pub use_mtp: bool,
    pub use_cot: bool,
    pub allow_write: bool,
    pub framework: Option<String>,
    pub model_variant: Option<String>,
    pub quantization: Option<String>,
    /// When true, run inside a fresh `git worktree` so file changes are
    /// isolated to a branch. See `--worktree` on `volt agent-run`.
    pub worktree: bool,
}

impl AgentTuiOptions {
    /// Same resolution order as `AgentRunOptions::model_or_default`.
    pub fn model_or_default(model: Option<String>) -> String {
        if let Some(m) = model {
            if !m.trim().is_empty() {
                return m;
            }
        }
        if let Ok(m) = std::env::var("LLM_MODEL") {
            if !m.trim().is_empty() {
                return m;
            }
        }
        if let Ok(m) = std::env::var("LLM_DEFAULT_MODEL") {
            if !m.trim().is_empty() {
                return m;
            }
        }
        let inv = crate::llm::detect_providers();
        for p in inv.active() {
            if let Some(default) = p.default_model {
                return default.to_string();
            }
        }
        String::new()
    }
}
