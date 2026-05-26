use crate::context::ContextStore;
use crate::embedding::EmbeddingClient;
use crate::llm::provider::TokenCallback;
use crate::llm::LLMProvider;
use crate::models::*;
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;
use crate::worker::SeedChannel;
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
    seed_channel: Option<SeedChannel>,
    cancel: Option<CancelToken>,
    on_token: Option<TokenCallback>,
    session_id: Option<uuid::Uuid>,
    sqlite_pool: Option<sqlx::SqlitePool>,
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
            seed_channel: None,
            cancel: None,
            on_token: None,
            session_id: None,
            sqlite_pool: None,
        }
    }

    pub fn with_memory(mut self, db: PgPool, embedder: EmbeddingClient) -> Self {
        self.db = Some(db);
        self.embedder = Some(embedder);
        self
    }

    pub fn with_memory_embedder_only(mut self, embedder: EmbeddingClient) -> Self {
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

    pub fn with_seed_channel(mut self, seed_channel: SeedChannel) -> Self {
        self.seed_channel = Some(seed_channel);
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

    pub fn with_session(mut self, session_id: uuid::Uuid, sqlite_pool: sqlx::SqlitePool) -> Self {
        self.session_id = Some(session_id);
        self.sqlite_pool = Some(sqlite_pool);
        self
    }

    pub async fn run(&self, input: &str) -> anyhow::Result<String> {
        // Load previous session messages for episodic memory
        if let (Some(sid), Some(pool)) = (self.session_id, &self.sqlite_pool) {
            match crate::session::load_messages(pool, sid).await {
                Ok(msgs) if !msgs.is_empty() => {
                    let mut state = self.state.lock().await;
                    state.messages.extend(msgs);
                    tracing::info!(
                        "[session] loaded {} messages from {}",
                        state.messages.len(),
                        sid
                    );
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("[session] failed to load messages: {}", e),
            }
        }

        self.push_user_message(input).await;

        for _iteration in 0..self.config.max_iterations {
            if self.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled by user"));
            }

            let context_embedding = self.build_context(input).await;
            let llm_messages = self.build_llm_messages().await;
            let llm_messages = self.compress_if_needed(llm_messages).await;

            let tool_defs = if let Some(ref emb) = context_embedding {
                let essential: Vec<&str> = self
                    .config
                    .essential_tools
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                self.tools.search_tools(emb, 8, &essential).await
            } else {
                self.tools.get_definitions().await
            };

            let model_ctx = crate::models::ModelContext::for_model(&self.config.model);
            let request = LLMRequest {
                model: self.config.model.clone(),
                messages: llm_messages,
                temperature: Some(self.config.temperature),
                max_tokens: Some(model_ctx.max_tokens),
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
                                let delay =
                                    std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
                                eprintln!(
                                    "\n\x1b[33m[API retry {}]\x1b[0m {} (retrying in {:?})",
                                    attempt + 1,
                                    e,
                                    delay
                                );
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
                state.total_prompt_tokens += usage.prompt_tokens;
                state.total_completion_tokens += usage.completion_tokens;
            }

            // Audit log: record this complete LLM turn (request + response) in ContextStore
            self.audit_turn(&request, &response, &state).await;

            if let Some(tool_calls) = &response.tool_calls {
                self.push_assistant_message(&mut state, &response, Some(tool_calls))
                    .await;
                self.execute_tool_calls(tool_calls, &mut state).await;
                // Build co-occurrence edges in ToolGraph for future retrieval
                if tool_calls.len() > 1 {
                    let names: Vec<String> = tool_calls.iter().map(|tc| tc.name.clone()).collect();
                    self.tools.record_co_occurrence(&names);
                }
            } else {
                self.push_assistant_message(&mut state, &response, None)
                    .await;
                self.store_memory(
                    input,
                    response.content.as_str(),
                    &state,
                    context_embedding.as_ref(),
                )
                .await;
                self.seed_episode_complete(input, response.content.as_str(), &state)
                    .await;
                drop(state);
                self.save_session_messages().await;
                return Ok(Arc::unwrap_or_clone(response.content));
            }
        }

        self.save_session_messages().await;
        Err(anyhow::anyhow!(
            "max iterations reached without final response"
        ))
    }

    /// Audit log: store the complete LLM turn (request + response) as a ContextEntry.
    /// Enables full traceability for EU AI Act Article 12 compliance.
    async fn save_session_messages(&self) {
        if let (Some(sid), Some(pool)) = (self.session_id, &self.sqlite_pool) {
            let state = self.state.lock().await;
            // Only save messages that are new (not loaded from session)
            // We track by checking if messages exceed what was originally loaded
            // Simple heuristic: save all messages — SQLite INSERT is idempotent via ON CONFLICT
            for msg in &state.messages {
                if let Err(e) = crate::session::save_message(pool, sid, msg).await {
                    tracing::warn!("[session] failed to save message: {}", e);
                }
            }
        }
    }

    async fn audit_turn(
        &self,
        request: &LLMRequest,
        response: &LLMResponse,
        state: &tokio::sync::MutexGuard<'_, AgentState>,
    ) {
        if let Some(ref store) = self.context_store {
            let prompt_text: String = request
                .messages
                .iter()
                .map(|m| format!("[{}]\n{}", m.role, m.content.as_str()))
                .collect::<Vec<_>>()
                .join("\n\n");
            let response_text = response.content.as_str();
            let tool_info: Vec<String> = response
                .tool_calls
                .as_ref()
                .map(|calls| {
                    calls
                        .iter()
                        .map(|tc| format!("{}={}", tc.name, tc.arguments))
                        .collect()
                })
                .unwrap_or_default();
            let audit = serde_json::json!({
                "model": request.model,
                "iteration": state.iteration,
                "session_id": state.session_id,
                "prompt_tokens": response.usage.as_ref().map(|u| u.prompt_tokens),
                "completion_tokens": response.usage.as_ref().map(|u| u.completion_tokens),
                "tool_calls": tool_info,
                "finish_reason": response.finish_reason,
            });
            store
                .add(
                    crate::context::ContextKind::AgentRun,
                    &format!(
                        "## Turn {}\n### Prompt\n{}\n### Response\n{}\n",
                        state.iteration, prompt_text, response_text
                    ),
                    audit,
                )
                .await;
        }
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
            let recent: Vec<&str> = s
                .messages
                .iter()
                .rev()
                .take(3)
                .map(|m| m.content.as_str())
                .collect();
            let mut parts: Vec<&str> = recent.into_iter().rev().collect();
            parts.push(input);
            parts.join("\n")
        };

        let context_embedding = if let Some(ref embedder) = self.embedder {
            embedder.embed_description(&context_query).await.ok()
        } else {
            None
        };

        // Retrieve relevant context per enabled kind for ablation control
        if let (Some(ref emb), Some(ref store)) = (&context_embedding, &self.context_store) {
            let kinds = &self.config.enabled_context_kinds;
            let per_kind_limit = 8_usize.div_ceil(kinds.len());
            let mut all_retrieved: Vec<crate::context::ContextEntry> = Vec::new();
            for kind in kinds {
                let mut kind_results = store.search(emb, per_kind_limit, Some(*kind), 0.25).await;
                all_retrieved.append(&mut kind_results);
            }
            // Re-rank globally by composite score and take top 8
            all_retrieved.sort_by(|a, b| {
                b.composite_score()
                    .partial_cmp(&a.composite_score())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let retrieved: Vec<_> = all_retrieved.into_iter().take(8).collect();

            if !retrieved.is_empty() {
                let blocks: Vec<String> = retrieved
                    .iter()
                    .map(|e| {
                        let tag = e.kind.as_str().replace("_", "-");
                        format!("<{tag}>\n{}\n</{tag}>", e.content)
                    })
                    .collect();
                let mut state = self.state.lock().await;
                // Remove stale context blocks from prior iterations
                state.messages.retain(|m| {
                    !(m.role == "system"
                        && (m.content.starts_with("<retrieved_context>")
                            || m.content.starts_with("<retrieved_skills>")))
                });
                state.messages.push(Message {
                    role: "system".into(),
                    content: Arc::new(format!(
                        "<retrieved_context>\n{}\n</retrieved_context>\n\nUse the above as reference only. Your task is below.",
                        blocks.join("\n")
                    )),
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
                let block: Vec<String> = matched
                    .iter()
                    .map(|s| format!("<skill name=\"{0}\">\n{1}\n</skill>", s.name, s.content))
                    .collect();
                if !block.is_empty() {
                    let mut state = self.state.lock().await;
                    state.messages.push(Message {
                        role: "system".into(),
                        content: Arc::new(format!(
                            "<retrieved_skills>\n{}\n</retrieved_skills>",
                            block.join("\n")
                        )),
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
                    let block: Vec<String> = memories
                        .iter()
                        .map(|m| format!("[{}] {}", m.kind, m.content))
                        .collect();
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
        let budget = model_ctx.max_context_tokens.saturating_sub(2048);
        let before = llm_messages.len();

        // Compute per-message token counts (already using tiktoken via ModelContext)
        let msg_tokens: Vec<u32> = llm_messages
            .iter()
            .map(|m| ModelContext::estimate_tokens(m.content.as_str()))
            .collect();
        let total_tokens: u32 = msg_tokens.iter().sum();

        if total_tokens <= budget {
            return llm_messages;
        }

        // Token-accurate rolling truncation: keep messages from the most recent
        // backward until the budget is exhausted. This replaces the old /10 heuristic
        // with real token counting, matching LightThinker++ (Apr 2026) and PRISM (May 2026).
        let mut running: u32 = 0;
        let mut keep_start = before;
        for (i, tok) in msg_tokens.iter().rev().enumerate() {
            if running + tok > budget {
                keep_start = before.saturating_sub(i);
                break;
            }
            running += tok;
        }

        let snapshot = self.state.lock().await;
        let recent = if keep_start < snapshot.messages.len() {
            snapshot.messages[keep_start..].to_vec()
        } else {
            snapshot.messages.clone()
        };

        let mut compressed: Vec<LLMMessage> = Vec::with_capacity(recent.len() + 1);
        if keep_start > 0 {
            compressed.push(LLMMessage {
                role: "system".into(),
                content: std::sync::Arc::new(format!(
                    "[Earlier conversation compressed: {} messages, ~{} tokens truncated. Key context follows.]",
                    keep_start,
                    msg_tokens[..keep_start].iter().sum::<u32>()
                )),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        for m in recent {
            compressed.push(LLMMessage {
                role: m.role,
                content: m.content,
                tool_calls: m.tool_calls,
                tool_call_id: m.tool_name,
            });
        }

        info!(
            "context compressed: {} messages -> {} (budget: {} tokens, keeping {} tokens)",
            before,
            compressed.len(),
            budget,
            running
        );
        compressed
    }

    async fn push_assistant_message(
        &self,
        state: &mut tokio::sync::MutexGuard<'_, AgentState>,
        response: &LLMResponse,
        tool_calls: Option<&Vec<ToolCall>>,
    ) {
        state.messages.push(Message {
            role: "assistant".into(),
            content: response.content.clone(),
            tool_calls: tool_calls.cloned(),
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        });
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: &[ToolCall],
        state: &mut tokio::sync::MutexGuard<'_, AgentState>,
    ) {
        // Check permissions upfront
        for tc in tool_calls {
            if self.is_cancelled() {
                return;
            }
            let needs_approval = self.tools.get_permission(&tc.name).await
                == PermissionLevel::Prompt
                && !self.config.allow_all
                && !state.allow_session;
            if needs_approval {
                eprintln!(
                    "\n\x1b[33m[approval]\x1b[0m tool '{}({:?})' requires approval.",
                    tc.name, tc.arguments
                );
                eprint!("Proceed? [y/N/a = always allow for this session] ");
                use std::io::Write;
                std::io::stderr().flush().ok();
                let answer = tokio::task::spawn_blocking(|| {
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf).ok();
                    buf.trim().to_lowercase()
                })
                .await
                .unwrap_or_default();
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
                }
            }
        }

        if self.is_cancelled() {
            return;
        }

        // Collect approved tool calls
        let approved: Vec<&ToolCall> = tool_calls
            .iter()
            .filter(|tc| {
                state
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.tool_name.as_deref() == Some(&tc.id))
                    .map(|m| !m.content.contains("skipped"))
                    .unwrap_or(true)
            })
            .collect();

        if approved.is_empty() {
            return;
        }

        // Execute approved tools concurrently
        let futures: Vec<_> = approved
            .iter()
            .map(|tc| {
                let tools = self.tools.clone();
                let name = tc.name.clone();
                let args = tc.arguments.clone();
                let id = tc.id.clone();
                let store = self.context_store.clone();
                let embedder = self.embedder.clone();
                info!("executing tool: {} with {}", name, args);
                async move {
                    let result = tools
                        .execute(&name, &args)
                        .await
                        .unwrap_or_else(|e| ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("tool error: {}", e)),
                            duration_ms: 0,
                        });

                    // rag_learn in background
                    if let (Some(ref store), Some(ref emb)) = (&store, &embedder) {
                        if let Ok(_emb) = emb.embed_description(&name).await {
                            let query = args.to_string();
                            store
                                .record_run(
                                    &query,
                                    &name,
                                    result.success,
                                    serde_json::json!({
                                        "tool_name": name,
                                        "duration_ms": result.duration_ms,
                                        "error": result.error,
                                    }),
                                )
                                .await;
                        }
                    }

                    let error_msg = result.error.clone();
                    let output = if result.success {
                        result.output.clone()
                    } else {
                        format!("error: {}", error_msg.unwrap_or_default())
                    };

                    (id, output, result)
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        for (id, output, result) in results {
            // Auto-seed artifact
            self.seed_artifact_if_applicable(&id, &result).await;

            state.messages.push(Message {
                role: "tool".into(),
                content: Arc::new(output.clone()),
                tool_calls: None,
                tool_result: Some(output),
                tool_name: Some(id),
                created_at: chrono::Utc::now(),
            });
        }
    }

    async fn store_memory(
        &self,
        input: &str,
        content: &str,
        state: &tokio::sync::MutexGuard<'_, AgentState>,
        existing_embedding: Option<&Vec<f32>>,
    ) {
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

    async fn seed_episode_complete(
        &self,
        input: &str,
        content: &str,
        state: &tokio::sync::MutexGuard<'_, AgentState>,
    ) {
        if let Some(ref ch) = self.seed_channel {
            let tools_used: Vec<String> = state
                .messages
                .iter()
                .filter(|m| m.tool_name.is_some())
                .filter_map(|m| m.tool_name.clone())
                .collect();
            let tools_used_dedup: Vec<String> = {
                let mut seen = std::collections::HashSet::new();
                tools_used
                    .into_iter()
                    .filter(|t| seen.insert(t.clone()))
                    .collect()
            };
            let resolution = content.chars().take(500).collect::<String>();
            ch.episode_complete(
                state.session_id,
                input,
                &resolution,
                tools_used_dedup,
                true,
                state.iteration,
            );
        }
    }

    async fn seed_artifact_if_applicable(&self, tool_name: &str, result: &ToolResult) {
        if let Some(ref ch) = self.seed_channel {
            if !result.success {
                return;
            }
            match tool_name {
                "write" | "edit" => {
                    let output = &result.output;
                    let file_path = output
                        .lines()
                        .find(|l| l.contains("Wrote to") || l.contains("Edited"))
                        .map(|l| l.to_string())
                        .unwrap_or_else(|| "unknown file".into());
                    let ext = std::path::Path::new(&file_path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("txt");
                    let language = match ext {
                        "rs" => "Rust",
                        "py" => "Python",
                        "js" | "ts" | "tsx" => "JavaScript/TypeScript",
                        "json" => "JSON",
                        "toml" | "yaml" | "yml" => "Config",
                        "md" => "Markdown",
                        "sql" => "SQL",
                        "html" => "HTML",
                        "css" => "CSS",
                        _ => ext,
                    };

                    #[cfg_attr(not(feature = "tools-ast"), allow(unused_mut))]
                    let mut description = result.output.clone();
                    #[cfg(feature = "tools-ast")]
                    if let Ok(content) = tokio::fs::read_to_string(&file_path).await {
                        if let Some(artifact) = crate::code_parser::parse_file(&file_path, &content)
                        {
                            if !artifact.functions.is_empty() {
                                description.push_str("\n\nFunctions: ");
                                description.push_str(&artifact.functions.join(", "));
                            }
                            if !artifact.classes.is_empty() {
                                description.push_str("\nClasses: ");
                                description.push_str(&artifact.classes.join(", "));
                            }
                            if !artifact.imports.is_empty() {
                                description.push_str("\nImports: ");
                                description.push_str(&artifact.imports.join(", "));
                            }
                        }
                    }

                    ch.artifact_created(&file_path, &description, language, tool_name);
                }
                "bash" => {
                    ch.artifact_created("shell_execution", &result.output, "shell", tool_name);
                }
                "csv_write" => {
                    ch.artifact_created("csv_data", &result.output, "csv", tool_name);
                }
                "create_pdf" => {
                    // Truncate PDF binary output to first 500 chars of description
                    let desc = result.output.chars().take(500).collect::<String>();
                    ch.artifact_created("pdf_document", &desc, "pdf", tool_name);
                }
                "screenshot" | "browser_screenshot" => {
                    ch.artifact_created("screenshot", &result.output, "image", tool_name);
                }
                "create_bar_chart" | "create_line_chart" => {
                    let desc = result.output.chars().take(300).collect::<String>();
                    ch.artifact_created("chart", &desc, "chart", tool_name);
                }
                _ => {}
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .map(|c| c.is_cancelled())
            .unwrap_or(false)
    }
}
