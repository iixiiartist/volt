use crate::models::*;
use async_trait::async_trait;
use std::sync::Arc;

pub type TokenCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn complete(&self, request: &LLMRequest) -> anyhow::Result<LLMResponse>;

    async fn complete_stream(
        &self,
        request: &LLMRequest,
        on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse>;

    fn name(&self) -> &str;
    fn supported_models(&self) -> Vec<String>;
}
