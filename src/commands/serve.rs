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
    response::{
        sse::{Event, Sse},
        Html, IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use dashmap::DashMap;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

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
    conversations: Arc<DashMap<String, ConversationState>>,
    tools: Arc<crate::tools::ToolRegistry>,
    embedder: EmbeddingClient,
    settings: crate::config::Settings,
    default_model: String,
    default_mode: String,
    default_allow: bool,
    default_max_iterations: u32,
}

#[allow(dead_code)]
struct ConversationState {
    agent: Option<Agent>,
    token_tx: broadcast::Sender<String>,
    busy: Arc<std::sync::atomic::AtomicBool>,
    model: String,
    mode: String,
    allow_all: bool,
    max_iterations: u32,
}

#[derive(Serialize)]
struct SessionResponse {
    session_id: String,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: String,
    model: Option<String>,
    mode: Option<String>,
    max_iterations: Option<u32>,
    allow_all: Option<bool>,
}

#[derive(Serialize)]
struct ChatResponse {
    session_id: String,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    session_id: String,
    session_id_short: String,
    model: String,
    mode: String,
    allow_all: bool,
    max_iterations: u32,
    messages: Vec<MessageView>,
}

struct MessageView {
    role: String,
    content: String,
    created_at: String,
}

async fn index_handler(State(state): State<AppState>) -> impl IntoResponse {
    let sid = uuid::Uuid::new_v4().to_string();
    state.conversations.insert(
        sid.clone(),
        ConversationState {
            agent: None,
            token_tx: broadcast::channel(1024).0,
            busy: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            model: state.default_model.clone(),
            mode: state.default_mode.clone(),
            allow_all: state.default_allow,
            max_iterations: state.default_max_iterations,
        },
    );
    let tpl = IndexTemplate {
        session_id: sid.clone(),
        session_id_short: sid[..8].to_string(),
        model: state.default_model.clone(),
        mode: state.default_mode.clone(),
        allow_all: state.default_allow,
        max_iterations: state.default_max_iterations,
        messages: Vec::new(),
    };
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("template error: {}", e)),
    )
}

async fn new_session_handler(State(state): State<AppState>) -> Json<SessionResponse> {
    let sid = uuid::Uuid::new_v4().to_string();
    state.conversations.insert(
        sid.clone(),
        ConversationState {
            agent: None,
            token_tx: broadcast::channel(1024).0,
            busy: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            model: state.default_model.clone(),
            mode: state.default_mode.clone(),
            allow_all: state.default_allow,
            max_iterations: state.default_max_iterations,
        },
    );
    Json(SessionResponse { session_id: sid })
}

async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let mut conv_entry = state
        .conversations
        .get_mut(&req.session_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "session not found".into()))?;

    if conv_entry.busy.load(std::sync::atomic::Ordering::Acquire) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "already processing a request".into(),
        ));
    }

    let model = req
        .model
        .as_deref()
        .unwrap_or(&conv_entry.model)
        .to_string();
    let mode = req.mode.as_deref().unwrap_or(&conv_entry.mode).to_string();
    let allow_all = req.allow_all.unwrap_or(conv_entry.allow_all);
    let max_iterations = req.max_iterations.unwrap_or(conv_entry.max_iterations);

    conv_entry.model = model.clone();
    conv_entry.mode = mode.clone();
    conv_entry.allow_all = allow_all;
    conv_entry.max_iterations = max_iterations;

    let sid_for_spawn = req.session_id.clone();
    let resp_sid = req.session_id.clone();
    let message = req.message.clone();

    let tools = state.tools.clone();
    let embedder = state.embedder.clone();
    let settings = state.settings.clone();
    let conversations = state.conversations.clone();

    conv_entry
        .busy
        .store(true, std::sync::atomic::Ordering::Release);
    let token_tx = conv_entry.token_tx.clone();

    tokio::spawn(async move {
        let result = run_agent_for_session(
            &sid_for_spawn,
            &conversations,
            &tools,
            &embedder,
            &settings,
            &model,
            &mode,
            allow_all,
            max_iterations,
            &message,
            &token_tx,
        )
        .await;
        if let Err(e) = result {
            let _ = token_tx.send(format!("[error: {}]", e));
        }
        let _ = token_tx.send("__done__".to_string());
        if let Some(entry) = conversations.get(&sid_for_spawn) {
            entry
                .busy
                .store(false, std::sync::atomic::Ordering::Release);
        }
    });

    Ok(Json(ChatResponse {
        session_id: resp_sid,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn run_agent_for_session(
    sid: &str,
    _conversations: &DashMap<String, ConversationState>,
    tools: &Arc<crate::tools::ToolRegistry>,
    embedder: &EmbeddingClient,
    settings: &crate::config::Settings,
    model: &str,
    mode: &str,
    allow_all: bool,
    max_iterations: u32,
    message: &str,
    token_tx: &broadcast::Sender<String>,
) -> anyhow::Result<()> {
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

    let token_tx_for_stream = token_tx.clone();
    let mut agent = Agent::new(config, provider, tools.clone())
        .with_workspace(std::env::current_dir().unwrap_or_default())
        .with_stream(Arc::new(move |token| {
            let _ = token_tx_for_stream.send(token.to_string());
        }));

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

    if let Ok(ref _sp) = crate::session::open_sessions(&PathBuf::from("volt_sessions.db")).await {
        let sid_uuid = uuid::Uuid::parse_str(sid).unwrap_or_else(|_| uuid::Uuid::new_v4());
        agent = agent.with_session(sid_uuid, _sp.clone());
    }

    match agent.run(message).await {
        Ok(result) => {
            if !result.is_empty() {
                let _ = token_tx.send(result);
            }
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
                    let _ = token_tx.send(text);
                }
                None => {
                    let _ = token_tx.send(format!("error: {}", e));
                }
            }
        }
    }

    Ok(())
}

async fn events_handler(
    State(state): State<AppState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    use futures::stream::StreamExt;

    let rx = if let Some(entry) = state.conversations.get(&session_id) {
        entry.token_tx.subscribe()
    } else {
        let (tx, _) = broadcast::channel(1);
        tx.subscribe()
    };

    let stream = BroadcastStream::new(rx).filter_map(|result| {
        let event = match result {
            Ok(msg) if msg == "__done__" => Ok(Event::default().data("").event("done")),
            Ok(msg) => Ok(Event::default().data(msg).event("token")),
            Err(_) => Ok(Event::default().data("").event("done")),
        };
        async move { Some(event) }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
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
        .route("/events/{session_id}", get(events_handler))
        .route("/session/new", post(new_session_handler))
        .with_state(state.clone());

    let addr = format!("127.0.0.1:{}", opts.port);
    eprintln!(
        "[web] Volt UI at http://{}  (model={}, mode={})",
        addr, state.default_model, state.default_mode
    );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
