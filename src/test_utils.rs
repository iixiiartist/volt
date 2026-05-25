use crate::llm::provider::TokenCallback;
use crate::models::*;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;

pub struct MockLLMProvider {
    pub responses: Mutex<Vec<anyhow::Result<LLMResponse>>>,
    pub requests: Mutex<Vec<LLMRequest>>,
}

impl MockLLMProvider {
    pub fn new(responses: Vec<anyhow::Result<LLMResponse>>) -> Self {
        Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn tool_result(content: &str) -> anyhow::Result<LLMResponse> {
        Ok(LLMResponse {
            content: Arc::new(content.to_string()),
            tool_calls: None,
            finish_reason: Some("stop".into()),
            usage: None,
        })
    }

    pub fn tool_calls(tools: Vec<ToolCall>) -> anyhow::Result<LLMResponse> {
        Ok(LLMResponse {
            content: Arc::new(String::new()),
            tool_calls: Some(tools),
            finish_reason: Some("tool_calls".into()),
            usage: None,
        })
    }
}

#[async_trait]
impl crate::llm::LLMProvider for MockLLMProvider {
    async fn complete(&self, request: &LLMRequest) -> anyhow::Result<LLMResponse> {
        self.requests.lock().unwrap().push(request.clone());
        self.responses.lock().unwrap().pop().unwrap_or_else(|| {
            Ok(LLMResponse {
                content: Arc::new("mock response".into()),
                tool_calls: None,
                finish_reason: Some("stop".into()),
                usage: None,
            })
        })
    }

    async fn complete_stream(
        &self,
        request: &LLMRequest,
        _on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse> {
        self.complete(request).await
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".into()]
    }
}

pub async fn test_tool_registry() -> Arc<crate::tools::ToolRegistry> {
    let registry = crate::tools::ToolRegistry::new();

    let echo_fn: crate::tools::ToolFn = Arc::new(|_args: serde_json::Value| {
        Box::pin(async move {
            crate::models::ToolResult {
                success: true,
                output: "echo done".into(),
                error: None,
                duration_ms: 0,
            }
        })
    });
    registry
        .register(
            "echo",
            "Echo tool",
            serde_json::json!({"type":"object"}),
            "test",
            echo_fn,
        )
        .await;
    registry
}
