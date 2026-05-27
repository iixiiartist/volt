use super::AgentMode;
use crate::agent::loop_rs::Agent;
use crate::context::ContextStore;
use crate::embedding::EmbeddingClient;
use crate::models::*;
use crate::tui::TuiChat;
use crate::{db, session, worker};

pub async fn run(options: AgentTuiOptions) -> anyhow::Result<()> {
    let AgentTuiOptions {
        model,
        allow,
        max_iterations,
        mode,
        settings,
    } = options;

    let (provider, provider_kind) = crate::orchestrator::build_provider(&model, "volt-agent");
    let tools = crate::tools::register_all_tools().await;
    let embedder = EmbeddingClient::new_smart().await;
    tools.compute_embeddings(&embedder).await;

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
    };
    let mut agent = Agent::new(config, provider, tools.clone())
        .with_workspace(std::env::current_dir().unwrap_or_default());

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

    TuiChat::run(&agent).await?;
    Ok(())
}

pub struct AgentTuiOptions {
    pub model: String,
    pub allow: bool,
    pub max_iterations: Option<u32>,
    pub mode: String,
    pub settings: crate::config::Settings,
}

impl AgentTuiOptions {
    pub fn model_or_default(model: Option<String>) -> String {
        model.unwrap_or_else(|| {
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
        })
    }
}
