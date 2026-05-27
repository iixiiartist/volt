use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;

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

impl WebhookChannel {
    pub fn new(port: u16, secret: String) -> Self {
        Self { port, secret }
    }

    pub async fn serve(&self,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let state = Arc::new(self.clone());
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
    State(state): State<Arc<WebhookChannel>>,
    req: Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    // For now, return a placeholder. Agent wiring requires full initialization.
    let reply = format!(
        "Webhook received: '{}'. Agent integration pending.",
        req.message
    );
    let session_id = req.session_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    Ok(Json(ChatResponse { reply, session_id }))
}
