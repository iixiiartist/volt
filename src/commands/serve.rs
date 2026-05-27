use super::AgentMode;
use crate::agent::loop_rs::Agent;
use crate::context::ContextStore;
use crate::embedding::EmbeddingClient;
use crate::models::*;
use crate::{db, orchestrator, worker};
use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub model: String,
    pub allow: bool,
    pub max_iterations: u32,
    pub mode: String,
    pub port: u16,
    pub settings: crate::config::Settings,
}

impl ServeOptions {
    pub fn model_or_default(model: Option<String>) -> String {
        model.unwrap_or_else(|| {
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
        })
    }
}

#[derive(Clone)]
struct AppState {
    conversations: Arc<DashMap<String, Arc<Mutex<ConversationState>>>>,
    tools: Arc<crate::tools::ToolRegistry>,
    embedder: EmbeddingClient,
    settings: crate::config::Settings,
    default_model: String,
    default_mode: String,
    default_allow: bool,
    default_max_iterations: u32,
}

struct ConversationState {
    model: String,
}

#[derive(Serialize)]
struct SessionResponse {
    session_id: String,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: String,
}

#[derive(Serialize)]
struct ChatResponse {
    reply: String,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    count: Option<u32>,
}

#[derive(Serialize)]
struct SearchResponse {
    result: String,
    error: Option<String>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    session_id: String,
    session_id_short: String,
    model: String,
    mode: String,
}

async fn index_handler(State(state): State<AppState>) -> impl IntoResponse {
    let sid = uuid::Uuid::new_v4().to_string();
    state.conversations.insert(
        sid.clone(),
        Arc::new(Mutex::new(ConversationState {
            model: state.default_model.clone(),
        })),
    );
    let tpl = IndexTemplate {
        session_id: sid.clone(),
        session_id_short: sid[..8].to_string(),
        model: state.default_model.clone(),
        mode: state.default_mode.clone(),
    };
    Html(tpl.render().unwrap_or_else(|e| format!("template error: {}", e)))
}

async fn new_session_handler(State(state): State<AppState>) -> Json<SessionResponse> {
    let sid = uuid::Uuid::new_v4().to_string();
    state.conversations.insert(
        sid.clone(),
        Arc::new(Mutex::new(ConversationState {
            model: state.default_model.clone(),
        })),
    );
    Json(SessionResponse { session_id: sid })
}

async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let model = state.default_model.clone();
    let mode = state.default_mode.clone();
    let allow_all = state.default_allow;
    let max_iterations = state.default_max_iterations;

    eprintln!(
        "[web chat] session={} msg='{}' model={} mode={}",
        &req.session_id[..8],
        &req.message[..40.min(req.message.len())],
        model,
        mode
    );

    let tools = state.tools.clone();
    let embedder = state.embedder.clone();
    let settings = state.settings.clone();

    match run_agent(
        &tools,
        &embedder,
        &settings,
        &model,
        &mode,
        allow_all,
        max_iterations,
        &req.message,
    )
    .await
    {
        Ok(reply) => {
            eprintln!(
                "[web chat] ok session={} reply_len={}",
                &req.session_id[..8],
                reply.len()
            );
            Ok(Json(ChatResponse {
                reply,
                error: None,
            }))
        }
        Err(e) => {
            eprintln!("[web chat] error session={}: {}", &req.session_id[..8], e);
            Ok(Json(ChatResponse {
                reply: String::new(),
                error: Some(format!("{}", e)),
            }))
        }
    }
}

async fn run_agent(
    tools: &Arc<crate::tools::ToolRegistry>,
    embedder: &EmbeddingClient,
    settings: &crate::config::Settings,
    model: &str,
    mode: &str,
    allow_all: bool,
    max_iterations: u32,
    message: &str,
) -> anyhow::Result<String> {
    let (provider, provider_kind) = orchestrator::build_provider(model, "volt-web");

    let mode_profile = mode.parse::<AgentMode>().unwrap_or(AgentMode::Balanced);

    let config = AgentConfig {
        name: "volt-web".into(),
        model: model.to_string(),
        provider: provider_kind,
        system_prompt: None,
        max_iterations,
        temperature: 0.3,
        toolsets: vec!["builtin".into()],
        hidden: false,
        allow_all,
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

    let cancel_worker = CancelToken::new();
    worker::AutoSeedWorker::new(context_store.clone(), embedder.clone(), cancel_worker)
        .spawn(seed_rx);

    let cs_for_seed = context_store.clone();
    let tools_for_seed = tools.clone();
    let embedder_for_seed = embedder.clone();
    let sandbox_policy = settings.sandbox_policy.clone();
    tokio::spawn(async move {
        worker::seed_background(
            cs_for_seed,
            embedder_for_seed,
            tools_for_seed,
            sandbox_policy,
        )
        .await;
    });

    if let Ok(pool) = db::connect(&settings.database_url).await {
        context_store.set_db(pool.clone());
        agent = agent.with_memory(pool.clone(), embedder.clone());
        let cs_for_skills = context_store.clone();
        let skill_pool = pool.clone();
        let skill_emb = embedder.clone();
        tokio::spawn(async move {
            worker::seed_skills_from_db(&cs_for_skills, &skill_pool, &skill_emb).await;
        });
    } else {
        agent = agent.with_memory_embedder_only(embedder.clone());
    }

    match agent.run(message).await {
        Ok(result) => {
            eprintln!("[web agent] OK, result_len={}", result.len());
            Ok(result)
        }
        Err(e) => {
            eprintln!("[web agent] error: {:?}", e);
            let state = agent.state().lock().await;
            let last_text = state.messages.iter().rev().find_map(|m| {
                let c = m.content.trim();
                if !c.is_empty() {
                    Some(format!("[fallback] {}", c.to_string()))
                } else {
                    m.tool_result
                        .as_ref()
                        .and_then(|r| if !r.trim().is_empty() { Some(r.to_string()) } else { None })
                }
            });
            match last_text {
                Some(text) => Ok(text),
                None => Err(e),
            }
        }
    }
}

async fn research_handler(
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    eprintln!("[web research] query='{}'", req.query);
    match crate::tools::you_tools::web_search(
        &req.query,
        req.count,
        None,
        None,
    )
    .await
    {
        ToolResult {
            success: true,
            output,
            error: _,
            duration_ms,
        } => {
            eprintln!("[web research] ok duration_ms={}", duration_ms);
            Ok(Json(SearchResponse {
                result: output,
                error: None,
            }))
        }
        ToolResult {
            success: false,
            output: _,
            error: Some(err),
            duration_ms,
        } => {
            eprintln!("[web research] error duration_ms={}: {}", duration_ms, err);
            Ok(Json(SearchResponse {
                result: String::new(),
                error: Some(err),
            }))
        }
        ToolResult {
            success: false,
            output: _,
            error: _,
            duration_ms,
        } => {
            eprintln!("[web research] error duration_ms={}", duration_ms);
            Ok(Json(SearchResponse {
                result: String::new(),
                error: Some("search failed".into()),
            }))
        }
    }
}

pub async fn serve(opts: ServeOptions) -> anyhow::Result<()> {
    let tools = crate::tools::register_all_tools().await;
    let embedder = EmbeddingClient::new_smart().await;
    tools.compute_embeddings(&embedder).await;

    let state = AppState {
        conversations: Arc::new(DashMap::new()),
        tools,
        embedder,
        settings: opts.settings,
        default_model: opts.model,
        default_mode: opts.mode,
        default_allow: opts.allow,
        default_max_iterations: opts.max_iterations,
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/chat", post(chat_handler))
        .route("/session/new", post(new_session_handler))
        .route("/research", post(research_handler))
        .with_state(state.clone());

    let addr = format!("127.0.0.1:{}", opts.port);
    eprintln!(
        "[web] Volt UI at http://{}  (model={}, mode={})",
        addr, state.default_model, state.default_mode
    );
    eprintln!(
        "[web] GROQ_API_KEY length: {}",
        std::env::var("GROQ_API_KEY").unwrap_or_default().len()
    );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("[web] listening on {}", addr);
    match axum::serve(listener, app).await {
        Ok(()) => eprintln!("[web] server shut down"),
        Err(e) => eprintln!("[web] server error: {}", e),
    }

    Ok(())
}
