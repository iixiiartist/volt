use crate::context::ContextStore;
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
    context_store: Option<Arc<ContextStore>>,
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
            context_injected: false,
            allow_session: false,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
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
            context_store: None,
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

    pub fn with_context(mut self, context_store: Arc<ContextStore>) -> Self {
        self.context_store = Some(context_store);
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
        self.push_user_message(input).await;

        for _iteration in 0..self.config.max_iterations {
            if self.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled by user"));
            }

            let context_embedding = self.build_context(input).await;
            let llm_messages = self.build_llm_messages().await;
            let llm_messages = self.compress_if_needed(llm_messages).await;

            let tool_defs = if let Some(ref emb) = context_embedding {
                self.tools.search_tools(emb, 8, &["read", "glob", "grep", "web_fetch"]).await
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

            let response = 'retry: loop {
                let max_retries = 3;
                for attempt in 0..max_retries {
                    if self.is_cancelled() {
                        return Err(anyhow::anyhow!("cancelled by user"));
                    }
                    let result = if let Some(ref on_token) = self.on_token {
                        let tok = on_token.clone();
                        self.provider.complete_stream(&request, tok).await
                    } else {
                        self.provider.complete(&request).await
                    };
                    match result {
                        Ok(r) => break 'retry r,
                        Err(e) => {
                            if attempt + 1 < max_retries {
                                let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
                                eprintln!("\n\x1b[33m[API retry {}]\x1b[0m {} (retrying in {:?})", attempt + 1, e, delay);
                                tokio::time::sleep(delay).await;
                            } else {
                                if self.is_cancelled() {
                                    return Err(anyhow::anyhow!("cancelled by user"));
                                }
                                eprintln!("\n\x1b[31m[API Error]\x1b[0m {}", e);
                                return Err(e);
                            }
                        }
                    }
                }
            };

            if self.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled by user"));
            }

            let mut state = self.state.lock().await;
            state.iteration += 1;
            state.updated_at = chrono::Utc::now();
            if let Some(ref usage) = response.usage {
                state.total_prompt_tokens += usage.prompt_tokens as u64;
                state.total_completion_tokens += usage.completion_tokens as u64;
            }

            if let Some(tool_calls) = &response.tool_calls {
                self.push_assistant_message(&mut state, &response, Some(tool_calls)).await;
                self.execute_tool_calls(tool_calls, &mut state).await;
            } else {
                self.push_assistant_message(&mut state, &response, None).await;
                self.store_memory(input, response.content.as_str(), &state, context_embedding.as_ref()).await;
                return Ok(Arc::unwrap_or_clone(response.content));
            }
        }

        Err(anyhow::anyhow!("max iterations reached without final response"))
    }

    async fn push_user_message(&self, input: &str) {
        let mut state = self.state.lock().await;
        state.messages.push(Message {
            role: "user".into(),
            content: Arc::new(input.to_string()),
            tool_calls: None,
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        });
    }

    async fn build_context(&self, input: &str) -> Option<Vec<f32>> {
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

        // Always retrieve relevant context via unified ContextStore (no one-shot flag)
        if let (Some(ref emb), Some(ref store)) = (&context_embedding, &self.context_store) {
            let retrieved = store.search(emb, 8, None, 0.25).await;
            if !retrieved.is_empty() {
                let blocks: Vec<String> = retrieved.iter().map(|e| {
                    format!("[{}]\n{}", e.kind.as_str(), e.content)
                }).collect();
                let mut state = self.state.lock().await;
                state.messages.push(Message {
                    role: "system".into(),
                    content: Arc::new(format!("## Relevant Context\n{}", blocks.join("\n---\n"))),
                    tool_calls: None,
                    tool_result: None,
                    tool_name: None,
                    created_at: chrono::Utc::now(),
                });
            }
        }

        // Also retrieve skills from the dedicated registry for backward compat
        if let (Some(ref emb), Some(ref skills)) = (&context_embedding, &self.skills) {
            let matched = skills.search(emb, 3).await;
            if !matched.is_empty() {
                let block: Vec<String> = matched.iter().map(|s| format!("[{0}]\n{1}", s.name, s.content)).collect();
                if !block.is_empty() {
                    let mut state = self.state.lock().await;
                    state.messages.push(Message {
                        role: "system".into(),
                        content: Arc::new(format!("## Relevant Skills\n{}", block.join("\n---\n"))),
                        tool_calls: None,
                        tool_result: None,
                        tool_name: None,
                        created_at: chrono::Utc::now(),
                    });
                }
            }
        }

        // DB memories — still here for backward compat with pgvector store
        if let (Some(ref emb), Some(ref db)) = (&context_embedding, &self.db) {
            if let Ok(memories) = crate::db::search_memories(db, emb, 5, None).await {
                if !memories.is_empty() {
                    let block: Vec<String> = memories.iter().map(|m| format!("[{}] {}", m.kind, m.content)).collect();
                    let ctx = format!("## Relevant memories\n{}", block.join("\n"));
                    let mut state = self.state.lock().await;
                    state.messages.push(Message {
                        role: "system".into(),
                        content: Arc::new(ctx),
                        tool_calls: None,
                        tool_result: None,
                        tool_name: None,
                        created_at: chrono::Utc::now(),
                    });
                }
            }
        }

        context_embedding
    }

    async fn build_llm_messages(&self) -> Vec<LLMMessage> {
        let state_snapshot = self.state.lock().await;
        let mut llm_messages: Vec<LLMMessage> = Vec::new();

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
        llm_messages
    }

    async fn compress_if_needed(&self, llm_messages: Vec<LLMMessage>) -> Vec<LLMMessage> {
        let model_ctx = ModelContext::for_model(&self.config.model);
        let total_tokens: u32 = llm_messages
            .iter()
                .map(|m| ModelContext::estimate_tokens(m.content.as_str()))
            .sum();

        if total_tokens <= model_ctx.max_context_tokens.saturating_sub(2048) {
            return llm_messages;
        }

        let snapshot = self.state.lock().await;
        let max_keep = model_ctx.max_context_tokens.saturating_sub(2048) as usize / 10;
        let before = llm_messages.len();
        let compressed: Vec<LLMMessage> = crate::agent::context::compress_context(&snapshot.messages, max_keep)
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
            compressed.len(),
            ModelContext::estimate_tokens(
                &compressed.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("")
            )
        );
        compressed
    }

    async fn push_assistant_message(&self, state: &mut tokio::sync::MutexGuard<'_, AgentState>, response: &LLMResponse, tool_calls: Option<&Vec<ToolCall>>) {
        state.messages.push(Message {
            role: "assistant".into(),
            content: response.content.clone(),
            tool_calls: tool_calls.cloned(),
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        });
    }

    async fn execute_tool_calls(&self, tool_calls: &[ToolCall], state: &mut tokio::sync::MutexGuard<'_, AgentState>) {
        for tc in tool_calls {
            if self.is_cancelled() {
                return;
            }
            info!("executing tool: {} with {:?}", tc.name, tc.arguments);

            let needs_approval = self.tools.get_permission(&tc.name).await == PermissionLevel::Prompt
                && !self.config.allow_all
                && !state.allow_session;
            if needs_approval {
                eprintln!("\n\x1b[33m[approval]\x1b[0m tool '{}({:?})' requires approval.", tc.name, tc.arguments);
                eprint!("Proceed? [y/N/a = always allow for this session] ");
                use std::io::Write;
                std::io::stderr().flush().ok();
                let answer = tokio::task::spawn_blocking(|| {
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf).ok();
                    buf.trim().to_lowercase()
                }).await.unwrap_or_default();
                let approved = answer == "y" || answer == "a";
                if answer == "a" {
                    state.allow_session = true;
                }
                if !approved {
                    info!("tool '{}' rejected by user", tc.name);
                    state.messages.push(Message {
                        role: "tool".into(),
                        content: Arc::new("skipped: rejected by user".to_string()),
                        tool_calls: None,
                        tool_result: Some("skipped: rejected by user".into()),
                        tool_name: Some(tc.id.clone()),
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

            // rag_learn: record this tool execution outcome
            if let (Some(ref store), Some(ref emb)) = (&self.context_store, &self.embedder) {
                if let Ok(_emb) = emb.embed_description(&tc.name).await {
                    let query = tc.arguments.to_string();
                    store.record_run(&query, &tc.name, result.success, serde_json::json!({
                        "tool_name": tc.name,
                        "duration_ms": result.duration_ms,
                        "error": result.error,
                    })).await;
                }
            }

            let output = if result.success {
                result.output
            } else {
                format!("error: {}", result.error.unwrap_or_default())
            };

            state.messages.push(Message {
                role: "tool".into(),
                content: Arc::new(output.clone()),
                tool_calls: None,
                tool_result: Some(output),
                tool_name: Some(tc.id.clone()),
                created_at: chrono::Utc::now(),
            });
        }
    }

    async fn store_memory(&self, input: &str, content: &str, state: &tokio::sync::MutexGuard<'_, AgentState>, existing_embedding: Option<&Vec<f32>>) {
        if let (Some(ref db), Some(ref embedder)) = (&self.db, &self.embedder) {
            let embedding = match existing_embedding {
                Some(emb) => Ok(emb.clone()),
                None => embedder.embed_description(input).await,
            };
            if let Ok(embedding) = embedding {
                let summary = content.chars().take(500).collect::<String>();
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
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false)
    }
}
