use crate::agent::loop_rs::Agent;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Channel: Send + Sync {
    async fn start(&self,
        agent: Arc<Agent>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_channel_trait_exists() {
        // Compile-time assertion
    }
}
