use crate::agent::Agent;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;

const SECRET_HEADER: &str = "x-volt-webhook-secret";

#[derive(Clone)]
pub struct WebhookChannel {
    port: u16,
    secret: String,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    reply: String,
    session_id: String,
}

struct AppState {
    agent: Arc<Agent>,
    secret: String,
}

impl WebhookChannel {
    pub fn new(port: u16, secret: String) -> Self {
        Self { port, secret }
    }
}

#[async_trait::async_trait]
impl crate::channels::Channel for WebhookChannel {
    async fn start(
        &self,
        agent: Arc<Agent>,
        mut shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let state = Arc::new(AppState {
            agent,
            secret: self.secret.clone(),
        });
        let app = Router::new()
            .route("/v1/chat", post(chat_handler))
            .with_state(state);

        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;
        tracing::info!("[webhook] listening on {}", addr);

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown.changed().await;
            })
            .await?;

        Ok(())
    }
}

async fn chat_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    if !state.secret.is_empty() {
        match headers.get(SECRET_HEADER).and_then(|v| v.to_str().ok()) {
            Some(s) if s == state.secret => {}
            _ => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    format!("missing or invalid {} header", SECRET_HEADER),
                ));
            }
        }
    }
    let session_id = req
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let reply = match state.agent.run(&req.message).await {
        Ok(s) => s,
        Err(e) => format!("agent error: {}", e),
    };
    Ok(Json(ChatResponse { reply, session_id }))
}
