use super::AgentMode;
use crate::agent::loop_rs::Agent;
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
    } = options;

    let (provider, provider_kind) = orchestrator::build_provider(&model, "volt-agent");
    let cancel = CancelToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        c.cancel();
    });

    let embedder = EmbeddingClient::new_smart().await;
    let tools = crate::tools::setup_tools(Some(&embedder)).await;

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
    };
    let config_quotas = config.context_kind_quotas.clone();
    let mut agent = Agent::new(config, provider, tools_for_agent)
        .with_workspace(std::env::current_dir().unwrap_or_default())
        .with_cancel(cancel_for_agent)
        .with_stream(Arc::new(|token| {
            print!("{}", token);
        }));

    let sessions_pool = {
        let session_db_path = PathBuf::from("volt_sessions.db");
        let sp = match session::open_sessions(&session_db_path).await {
            Ok(sp) => Some(sp),
            Err(e) => {
                eprintln!("[session] warning: {}", e);
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
                eprintln!("[session] created new session {}", sid);
            } else {
                eprintln!("[session] resumed session {}", sid);
            }
        }
        sp
    };

    let pool = match crate::db::connect(&settings.database_url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            eprintln!("[db] warning: {}. Running without memory.", e);
            None
        }
    };

    if let Some(ref p) = pool {
        agent = agent.with_memory(p.clone(), embedder.clone());
    } else {
        agent = agent.with_memory_embedder_only(embedder.clone());
    }

    let skills = crate::skills::setup_skills(pool.clone(), Some(embedder_for_skills)).await;
    agent = agent.with_skills(skills);

    let context_store = if let Some(ref p) = pool {
        let store = ContextStore::new_with_db(p.clone());
        match store.hydrate_from_db(2000).await {
            Ok(n) if n > 0 => eprintln!("[context] hydrated {} entries from DB", n),
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

    if let Ok(pool) = crate::db::connect(&settings.database_url).await {
        context_store.set_db(pool.clone());
        agent = agent.with_memory(pool.clone(), embedder.clone());
        let skill_store = context_store.clone();
        let skill_emb = embedder;
        tokio::spawn(async move {
            worker::seed_skills_from_db(&skill_store, &pool, &skill_emb).await;
        });
    }

    eprintln!(
        "[mode] {} — {} context kinds",
        mode,
        mode_profile.context_kinds().len()
    );

    match agent.run(&input).await {
        Ok(result) => println!("{}", result),
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
                Some(text) => println!("{}", text),
                None => eprintln!("error: {}", e),
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
            eprintln!("[tools] loaded {} BFCL stubs from {}", count, path);
            tools.compute_embeddings(embedder).await;
        }
        Err(e) => eprintln!("[tools] failed to load {}: {}", path, e),
    }
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
}

impl AgentRunOptions {
    pub fn model_or_default(model: Option<String>) -> String {
        model.unwrap_or_else(|| {
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
        })
    }
}
