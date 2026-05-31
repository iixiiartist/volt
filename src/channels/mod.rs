use crate::agent::Agent;
use async_trait::async_trait;
use std::sync::Arc;

pub mod telegram;
pub mod webhook;

#[async_trait]
pub trait Channel: Send + Sync {
    async fn start(
        &self,
        agent: Arc<Agent>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()>;
}
