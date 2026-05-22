use crate::embedding::EmbeddingClient;
use crate::llm::LLMProvider;
use crate::llm::provider::TokenCallback;
use crate::models::*;
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

pub struct Agent {
    pub config: AgentConfig,
    pub state: Arc<Mutex<AgentState>>,
    provider: Box<dyn LLMProvider>,
    tools: Arc<ToolRegistry>,
    db: Option<PgPool>,
    embedder: Option<EmbeddingClient>,
    skills: Option<Arc<SkillRegistry>>,
    cancel: Option<CancelToken>,
    on_token: Option<TokenCallback>,
}

impl Agent {
    pub fn new(
        config: AgentConfig,
        provider: Box<dyn LLMProvider>,
        tools: Arc<ToolRegistry>,
    ) -> Self {
        let state = AgentState {
            id: uuid::Uuid::new_v4(),
            name: config.name.clone(),
            session_id: uuid::Uuid::new_v4(),
            iteration: 0,
            messages: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        Self {
            config,
            state: Arc::new(Mutex::new(state)),
            provider,
            tools,
            db: None,
            embedder: None,
            skills: None,
            cancel: None,
            on_token: None,
        }
    }

    pub fn with_memory(mut self, db: PgPool, embedder: EmbeddingClient) -> Self {
        self.db = Some(db);
        self.embedder = Some(embedder);
        self
    }

    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    pub fn with_stream(mut self, on_token: TokenCallback) -> Self {
        self.on_token = Some(on_token);
        self
    }

    pub async fn run(&self, input: &str) -> anyhow::Result<String> {
        let mut state = self.state.lock().await;
        state.messages.push(Message {
            role: "user".into(),
            content: input.into(),
            tool_calls: None,
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        });
        drop(state);

        for _iteration in 0..self.config.max_iterations {
            if self.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled by user"));
            }

            // ── Unified RAG: embed context, then search tools + skills + memory ──
            let context_query = {
                let s = self.state.lock().await;
                let recent: Vec<&str> = s.messages.iter().rev().take(3).map(|m| m.content.as_str()).collect();
                let mut parts: Vec<&str> = recent.into_iter().rev().collect();
                parts.push(input);
                parts.join("\n")
            };

            let context_embedding = if let Some(ref embedder) = self.embedder {
                embedder.embed_description(&context_query).await.ok()
            } else {
                None
            };

            let skill_context = if let (Some(ref emb), Some(ref skills)) = (&context_embedding, &self.skills) {
                let matched = skills.search(emb, 3).await;
                if !matched.is_empty() {
                    let block: Vec<String> = matched.iter().map(|s| format!("[{0}]\n{1}", s.name, s.content)).collect();
                    Some(format!("## Relevant Skills\n{}", block.join("\n---\n")))
                } else { None }
            } else { None };

            let memory_context = if let (Some(ref emb), Some(ref db)) = (&context_embedding, &self.db) {
                if let Ok(memories) = crate::db::search_memories(db, emb, 5, None).await {
                    if !memories.is_empty() {
                        let block: Vec<String> = memories.iter().map(|m| format!("[{}] {}", m.kind, m.content)).collect();
                        Some(format!("## Relevant memories\n{}", block.join("\n")))
                    } else { None }
                } else { None }
            } else { None };

            let state_snapshot = self.state.lock().await;
            let mut llm_messages: Vec<LLMMessage> = Vec::new();

            if let Some(ref ctx) = skill_context {
                llm_messages.push(LLMMessage {
                    role: "system".into(),
                    content: ctx.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            if let Some(ref ctx) = memory_context {
                llm_messages.push(LLMMessage {
                    role: "system".into(),
                    content: ctx.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }

            for m in &state_snapshot.messages {
                let mut msg = LLMMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                };
                if let Some(tcs) = &m.tool_calls {
                    msg.tool_calls = Some(tcs.clone());
                }
                if let Some(tid) = &m.tool_name {
                    msg.tool_call_id = Some(tid.clone());
                }
                llm_messages.push(msg);
            }

            let model_ctx = ModelContext::for_model(&self.config.model);
            let total_tokens: u32 = llm_messages
                .iter()
                .map(|m| ModelContext::estimate_tokens(&m.content))
                .sum();

            if total_tokens > model_ctx.max_context_tokens.saturating_sub(2048) {
                let max_keep = model_ctx.max_context_tokens.saturating_sub(2048) as usize / 10;
                let before = llm_messages.len();
                llm_messages = crate::agent::context::compress_context(&state_snapshot.messages, max_keep)
                    .into_iter()
                    .map(|m| LLMMessage {
                        role: m.role,
                        content: m.content,
                        tool_calls: m.tool_calls,
                        tool_call_id: m.tool_name,
                    })
                    .collect();
                info!(
                    "context compressed: {} messages -> {} (est {} tokens)",
                    before,
                    llm_messages.len(),
                    ModelContext::estimate_tokens(
                        &llm_messages.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("")
                    )
                );
            }

            drop(state_snapshot);

            let tool_defs = if let Some(ref emb) = context_embedding {
                let defs = self.tools.search_tools(emb, 8, &["read", "glob", "grep", "web_fetch"]).await;
                defs
            } else {
                self.tools.get_definitions().await
            };
            let request = LLMRequest {
                model: self.config.model.clone(),
                messages: llm_messages,
                temperature: Some(self.config.temperature),
                max_tokens: Some(4096),
                stop: None,
                tools: Some(tool_defs),
                stream: false,
            };

            let response = if let Some(ref on_token) = self.on_token {
                let tok = on_token.clone();
                self.provider.complete_stream(&request, tok).await
            } else {
                self.provider.complete(&request).await
            };

            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    if self.is_cancelled() {
                        return Err(anyhow::anyhow!("cancelled by user"));
                    }
                    return Err(e);
                }
            };

            let mut state = self.state.lock().await;
            state.iteration += 1;
            state.updated_at = chrono::Utc::now();

            if let Some(tool_calls) = &response.tool_calls {
                state.messages.push(Message {
                    role: "assistant".into(),
                    content: response.content.clone(),
                    tool_calls: Some(tool_calls.clone()),
                    tool_result: None,
                    tool_name: None,
                    created_at: chrono::Utc::now(),
                });

                for tc in tool_calls {
                    if self.is_cancelled() {
                        return Err(anyhow::anyhow!("cancelled by user"));
                    }
                    info!("executing tool: {} with {:?}", tc.name, tc.arguments);

                    if self.tools.get_permission(&tc.name).await == PermissionLevel::Prompt {
                        eprintln!("\n\x1b[33m[approval]\x1b[0m tool '{}({:?})' requires approval.", tc.name, tc.arguments);
                        eprint!("Proceed? [y/N] ");
                        use std::io::Write;
                        std::io::stderr().flush().ok();
                        let mut answer = String::new();
                        std::io::stdin().read_line(&mut answer).ok();
                        if !answer.trim().eq_ignore_ascii_case("y") {
                            info!("tool '{}' rejected by user", tc.name);
                            state.messages.push(Message {
                                role: "tool".into(),
                                content: "skipped: rejected by user".into(),
                                tool_calls: None,
                                tool_result: Some("skipped: rejected by user".into()),
                                tool_name: Some(tc.name.clone()),
                                created_at: chrono::Utc::now(),
                            });
                            continue;
                        }
                    }

                    let result = self
                        .tools
                        .execute(&tc.name, &tc.arguments)
                        .await
                        .unwrap_or_else(|e| ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("tool error: {}", e)),
                            duration_ms: 0,
                        });

                    let output = if result.success {
                        result.output
                    } else {
                        format!("error: {}", result.error.unwrap_or_default())
                    };

                    state.messages.push(Message {
                        role: "tool".into(),
                        content: output.clone(),
                        tool_calls: None,
                        tool_result: Some(output),
                        tool_name: Some(tc.name.clone()),
                        created_at: chrono::Utc::now(),
                    });
                }
            } else {
                state.messages.push(Message {
                    role: "assistant".into(),
                    content: response.content.clone(),
                    tool_calls: None,
                    tool_result: None,
                    tool_name: None,
                    created_at: chrono::Utc::now(),
                });

                if let (Some(ref db), Some(ref embedder)) = (&self.db, &self.embedder) {
                    if let Ok(embedding) = embedder.embed_description(input).await {
                        let summary = response.content.chars().take(500).collect::<String>();
                        let _ = crate::db::store_memory(
                            db,
                            "conversation",
                            &summary,
                            &embedding,
                            Some(state.session_id),
                        )
                        .await;
                    }
                }

                return Ok(response.content);
            }
        }

        Err(anyhow::anyhow!(
            "max iterations reached without final response"
        ))
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::openai::OpenAIProvider;
    use crate::tools::ToolRegistry;

    #[tokio::test]
    async fn test_agent_creation() {
        let tools = ToolRegistry::new();
        let provider = Box::new(OpenAIProvider::new(
            "test-key".into(),
            "https://example.com/v1".into(),
            "test".into(),
        ));
        let config = AgentConfig {
            name: "test-agent".into(),
            model: "test-model".into(),
            provider: "test".into(),
            system_prompt: None,
            max_iterations: 1,
            temperature: 0.0,
            toolsets: vec!["builtin".into()],
            hidden: false,
        };
        let agent = Agent::new(config, provider, tools);
        let state = agent.state.lock().await;
        assert_eq!(state.name, "test-agent");
        assert_eq!(state.iteration, 0);
        assert!(state.messages.is_empty());
    }

    #[tokio::test]
    async fn test_agent_with_memory() {
        let tools = ToolRegistry::new();
        let provider = Box::new(OpenAIProvider::new(
            "test-key".into(),
            "https://example.com/v1".into(),
            "test".into(),
        ));
        let config = AgentConfig {
            name: "memory-agent".into(),
            model: "test-model".into(),
            provider: "test".into(),
            system_prompt: None,
            max_iterations: 1,
            temperature: 0.0,
            toolsets: vec!["builtin".into()],
            hidden: false,
        };
        let agent = Agent::new(config, provider, tools);
        assert!(agent.db.is_none());
        assert!(agent.embedder.is_none());
    }

    #[tokio::test]
    async fn test_agent_max_iterations() {
        let tools = ToolRegistry::new();
        let provider = Box::new(OpenAIProvider::new(
            "test-key".into(),
            "https://example.com/v1".into(),
            "test".into(),
        ));
        let config = AgentConfig {
            name: "iter-agent".into(),
            model: "test-model".into(),
            provider: "test".into(),
            system_prompt: None,
            max_iterations: 0,
            temperature: 0.0,
            toolsets: vec!["builtin".into()],
            hidden: false,
        };
        let agent = Agent::new(config, provider, tools);
        let result = agent.run("hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max iterations"));
    }

    #[tokio::test]
    async fn test_agent_cancellation() {
        let tools = ToolRegistry::new();
        let provider = Box::new(OpenAIProvider::new(
            "test-key".into(),
            "https://example.com/v1".into(),
            "test".into(),
        ));
        let config = AgentConfig {
            name: "cancel-agent".into(),
            model: "test-model".into(),
            provider: "test".into(),
            system_prompt: None,
            max_iterations: 5,
            temperature: 0.0,
            toolsets: vec!["builtin".into()],
            hidden: false,
        };
        let cancel = CancelToken::new();
        cancel.cancel();
        let agent = Agent::new(config, provider, tools).with_cancel(cancel);
        let result = agent.run("hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cancelled"));
    }
}
