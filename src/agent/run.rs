use super::{Agent, MAX_TOOL_OUTPUT_CHARS};
use crate::agent::cot;
use crate::agent::prompt::build_system_prompt;
use crate::agent::prompt_builder;
use crate::models::{
    LLMMessage, LLMRequest, LLMResponse, Message, PermissionLevel, ToolCall, ToolResult,
};
use std::sync::Arc;
use uuid::Uuid;

impl Agent {
    pub async fn run(&self, input: &str) -> anyhow::Result<String> {
        let is_precision = self.is_precision_mode();
        if !is_precision {
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
        }

        {
            let mut state = self.state.lock().await;
            let current_prompt = if self.config.framework.is_some() {
                prompt_builder::build_prompt(
                    &build_system_prompt(&self.config, self.workspace.as_deref()),
                    input,
                    None,
                    None,
                )
            } else {
                build_system_prompt(&self.config, self.workspace.as_deref())
            };
            let existing_idx = state.messages.iter().position(|m| m.role == "system");
            match existing_idx {
                Some(idx) => {
                    if state.messages[idx].content.as_ref() != &current_prompt {
                        state.messages[idx].content = Arc::new(current_prompt);
                        tracing::info!("[system] replaced stale system prompt on session resume");
                    }
                }
                None => {
                    state.messages.insert(
                        0,
                        Message {
                            id: Uuid::new_v4(),
                            parent_message_id: None,
                            role: "system".into(),
                            content: Arc::new(current_prompt),
                            tool_calls: None,
                            tool_result: None,
                            tool_name: None,
                            created_at: chrono::Utc::now(),
                        },
                    );
                }
            }
        }

        if self.config.use_cot {
            let tool_names: Vec<String> = self
                .tools
                .get_definitions()
                .await
                .iter()
                .map(|d| d.name.clone())
                .collect();
            let plan_prompt = cot::planning_prompt(input, &tool_names);
            let planning_messages = vec![LLMMessage {
                role: "user".to_string(),
                content: Arc::new(plan_prompt),
                tool_calls: None,
                tool_call_id: None,
            }];
            let _model_ctx = crate::models::ModelContext::for_model(&self.config.model);
            let plan_request = LLMRequest {
                model: self.config.model.clone(),
                messages: planning_messages,
                temperature: Some(0.2),
                max_tokens: Some(512),
                stop: None,
                tools: None,
                stream: false,
                ..Default::default()
            };
            if let Ok(plan_response) = self.provider.complete(&plan_request).await {
                let plan_text = plan_response.content.as_str();
                let plan_steps = cot::parse_plan(plan_text);
                if !plan_steps.is_empty() {
                    let plan_summary = plan_steps
                        .iter()
                        .map(|(n, desc, _tool)| format!("{}. {}", n, desc))
                        .collect::<Vec<_>>()
                        .join("\n");
                    tracing::info!("[CoT] plan generated:\n{}", plan_summary);
                    let state = self.state.lock().await;
                    crate::tools::memory_tool::memory_append("plan", &plan_summary).await;
                    drop(state);
                }
            }
        }

        self.push_user_message(input).await;

        for _iteration in 0..self.config.max_iterations {
            tokio::task::yield_now().await;

            if self.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled by user"));
            }

            let context_result = self.build_context(input).await;
            let llm_messages = self.build_llm_messages().await;
            let llm_messages = self.compress_if_needed(llm_messages).await;

            let (context_embedding, query_text) = match context_result {
                Some((e, q)) => (Some(e), Some(q)),
                None => (None, None),
            };

            let tool_defs = if let Some(ref emb) = context_embedding {
                let essential: Vec<&str> = self
                    .config
                    .essential_tools
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                let limit = if self.config.quirks.contains(&crate::agent::blueprint::ModelQuirk::SchemaLimitTen)
                    && !self.config.strict_mode
                {
                    10
                } else {
                    8
                };
                self.tools
                    .search_tools(emb, limit, &essential, query_text.as_deref())
                    .await
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
                ..Default::default()
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
                            let err_str = e.to_string();
                            // Fast-fail on auth errors — retrying will never succeed
                            let is_auth_err = err_str.contains("401") || err_str.contains("403") || err_str.contains("Unauthorized") || err_str.contains("Forbidden");
                            if is_auth_err || attempt + 1 >= max_retries {
                                if !is_auth_err {
                                    if self.is_cancelled() {
                                        return Err(anyhow::anyhow!("cancelled by user"));
                                    }
                                    eprintln!("\n\x1b[31m[API Error]\x1b[0m {}", e);
                                    return Err(e);
                                }
                                eprintln!("\n\x1b[31m[API Auth Error]\x1b[0m {} — aborting (retrying would not help)", e);
                                return Err(e);
                            }
                            let delay =
                                std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
                            eprintln!(
                                "\n\x1b[33m[API retry {}]\x1b[0m {} (retrying in {:?})",
                                attempt + 1,
                                e,
                                delay
                            );
                            tokio::time::sleep(delay).await;
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

            self.save_checkpoint(
                state.iteration,
                &state.messages,
                state.total_prompt_tokens,
                state.total_completion_tokens,
            )
            .await;

            self.audit_turn(&request, &response, &state).await;

            if let Some(tool_calls) = &response.tool_calls {
                // ── Quirk pre-processing ────────────────────────────────
                for tc in tool_calls.iter() {
                    let mut args = tc.arguments.clone();
                    crate::agent::tool_parser::coerce_quirks(&mut args, &self.config.quirks);
                }

                // ── max_tools_per_turn enforcement ─────────────────────
                if let Some(max) = self.config.max_tools_per_turn {
                    if tool_calls.len() > max
                        && !tool_calls.iter().any(|tc| tc.name == "final_answer")
                    {
                        let overflow: Vec<_> = tool_calls
                            .iter()
                            .skip(max)
                            .map(|tc| tc.name.clone())
                            .collect();
                        let err_msg = format!(
                            "Only {} tool call(s) allowed per turn. You sent {}: {}. Please retry with at most {} call(s).",
                            max, tool_calls.len(), overflow.join(", "), max
                        );
                        self.push_assistant_message(&mut state, &response, Some(tool_calls))
                            .await;
                        let parent_id = crate::models::Message::last_id(&state.messages);
                        state.messages.push(Message {
                            id: Uuid::new_v4(),
                            parent_message_id: parent_id,
                            role: "tool".into(),
                            content: Arc::new(err_msg.clone()),
                            tool_calls: None,
                            tool_result: Some(err_msg),
                            tool_name: Some("max_tools_per_turn".into()),
                            created_at: chrono::Utc::now(),
                        });
                        drop(state);
                        continue;
                    }
                }

                {
                    let defs = self.tools.get_definitions().await;
                    let def_map: std::collections::HashMap<&str, &crate::models::ToolDefinition> =
                        defs.iter().map(|d| (d.name.as_str(), d)).collect();
                    let validation_errors =
                        crate::agent::tool_parser::validate_tool_calls(tool_calls, |name| {
                            def_map.get(name).copied()
                        });
                    if !validation_errors.is_empty() {
                        self.push_assistant_message(&mut state, &response, Some(tool_calls))
                            .await;
                        for (idx, err) in &validation_errors {
                            let tool_name = &tool_calls[*idx].name;
                            let parent_id = crate::models::Message::last_id(&state.messages);
                            state.messages.push(Message {
                                id: Uuid::new_v4(),
                                parent_message_id: parent_id,
                                role: "tool".into(),
                                content: Arc::new(
                                    crate::agent::tool_parser::build_validation_error_message(
                                        tool_name, err,
                                    ),
                                ),
                                tool_calls: None,
                                tool_result: Some(err.clone()),
                                tool_name: Some("validation_error".into()),
                                created_at: chrono::Utc::now(),
                            });
                        }
                        self.save_checkpoint(
                            state.iteration,
                            &state.messages,
                            state.total_prompt_tokens,
                            state.total_completion_tokens,
                        )
                        .await;
                        drop(state);
                        continue;
                    }
                }

                if let Some(final_call) = tool_calls.iter().find(|tc| tc.name == "final_answer") {
                    let answer = final_call
                        .arguments
                        .get("answer")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.push_assistant_message(&mut state, &response, Some(tool_calls))
                        .await;
                    drop(state);
                    self.save_session_messages_delta().await;
                    return Ok(answer);
                }

                self.push_assistant_message(&mut state, &response, Some(tool_calls))
                    .await;
                let allow_session = state.allow_session;
                drop(state);
                let tool_results = self.execute_tool_calls(tool_calls, allow_session).await;
                if tool_calls.len() > 1 {
                    let names: Vec<String> = tool_calls.iter().map(|tc| tc.name.clone()).collect();
                    self.tools.record_co_occurrence(&names);
                }
                let mut state = self.state.lock().await;
                let mut had_data_tool = false;
                for (tool_name, call_id, output, result) in tool_results {
                    self.seed_artifact_if_applicable(&tool_name, &result).await;
                    let is_data_tool = matches!(
                        tool_name.as_str(),
                        "web_search"
                            | "web_fetch"
                            | "web_scrape"
                            | "web_scrape_all"
                            | "you_research"
                            | "you_contents"
                            | "bash"
                            | "read"
                            | "csv_read"
                            | "grep"
                            | "browser_extract"
                    );
                    if is_data_tool && result.success && !output.trim().is_empty() {
                        had_data_tool = true;
                    }
                    let msg_content = if output.len() > MAX_TOOL_OUTPUT_CHARS {
                        let rid = format!("ref_{}_turn_{}", tool_name, state.iteration);
                        let snippet = if output.len() > 500 {
                            let mut idx = 500;
                            while !output.is_char_boundary(idx) && idx > 0 {
                                idx -= 1;
                            }
                            &output[..idx]
                        } else {
                            &output
                        };
                        self.tool_output_buffer
                            .lock()
                            .await
                            .insert(rid.clone(), output.clone());
                        format!(
                            "[Tool output: {} ({} chars)]\n{}\n\n[Reference: {} — use `get_tool_output` with ref_id=\"{}\" to inspect full output]",
                            tool_name, output.len(), snippet, rid, rid
                        )
                    } else {
                        output.clone()
                    };
                    let parent_id = crate::models::Message::last_id(&state.messages);
                    state.messages.push(Message {
                        id: Uuid::new_v4(),
                        parent_message_id: parent_id,
                        role: "tool".into(),
                        content: Arc::new(msg_content),
                        tool_calls: None,
                        tool_result: Some(output),
                        tool_name: Some(call_id),
                        created_at: chrono::Utc::now(),
                    });
                }
                if had_data_tool {
                    let parent_id = crate::models::Message::last_id(&state.messages);
                    state.messages.push(Message {
                        id: Uuid::new_v4(),
                        parent_message_id: parent_id,
                        role: "system".into(),
                        content: Arc::new(
                            "You just received data from a tool. If this data is worth keeping or the user asked you to save it, call `write(path, content)` now with the data as content and the destination file path. You have a `write` tool for this purpose.".into(),
                        ),
                        tool_calls: None,
                        tool_result: None,
                        tool_name: None,
                        created_at: chrono::Utc::now(),
                    });
                }
            } else {
                // ── MissingFinalAnswer quirk ──────────────────────────
                let content = response.content.as_str();
                if self.config.quirks.contains(&crate::agent::blueprint::ModelQuirk::MissingFinalAnswer)
                    && !content.trim().is_empty()
                {
                    tracing::warn!("MissingFinalAnswer quirk triggered: wrapping text as final_answer");
                    let answer = content.trim().to_string();
                    let synthetic = ToolCall {
                        id: Uuid::new_v4().to_string(),
                        name: "final_answer".into(),
                        arguments: serde_json::json!({"answer": answer}),
                    };
                    self.push_assistant_message(&mut state, &response, Some(&vec![synthetic]))
                        .await;
                    drop(state);
                    self.save_session_messages_delta().await;
                    return Ok(answer);
                }

                // ── Normal text-only return ──────────────────────────
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
                self.save_session_messages_delta().await;
                return Ok(Arc::unwrap_or_clone(response.content));
            }
        }

        self.save_session_messages_delta().await;
        let state = self.state.lock().await;
        let last_answer = state
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant" && !m.content.is_empty())
            .or_else(|| {
                state
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "tool" && !m.content.is_empty())
            })
            .map(|m| m.content.as_str().to_string())
            .unwrap_or_default();
        if !last_answer.is_empty() {
            Ok(last_answer)
        } else {
            Err(anyhow::anyhow!(
                "max iterations reached without final response"
            ))
        }
    }

    async fn save_session_messages_delta(&self) {
        if let (Some(sid), Some(pool)) = (self.session_id, &self.sqlite_pool) {
            let mut state = self.state.lock().await;
            let last = state.last_saved_message_idx;
            let current = state.messages.len();
            if last >= current {
                return;
            }
            let mut tx = match pool.begin().await {
                Ok(tx) => tx,
                Err(e) => {
                    tracing::warn!("[session] begin tx failed: {}", e);
                    return;
                }
            };
            for (i, msg) in state.messages[last..current].iter().enumerate() {
                let position = last + i;
                if let Err(e) = sqlx::query(
                    "INSERT INTO messages (session_id, position_index, role, content, tool_calls, tool_result, tool_name, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(session_id, position_index) DO UPDATE SET content = excluded.content, tool_calls = excluded.tool_calls, tool_result = excluded.tool_result, tool_name = excluded.tool_name"
                )
                .bind(sid.to_string())
                .bind(position as i64)
                .bind(&msg.role)
                .bind(msg.content.as_str())
                .bind(msg.tool_calls.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
                .bind(msg.tool_result.as_ref())
                .bind(msg.tool_name.as_ref())
                .bind(msg.created_at.to_rfc3339())
                .execute(&mut *tx)
                .await
                {
                    tracing::warn!("[session] failed to save message: {}", e);
                }
            }
            if let Err(e) = tx.commit().await {
                tracing::warn!("[session] commit failed: {}", e);
            } else {
                state.last_saved_message_idx = current;
            }
        }
    }

    async fn save_checkpoint(
        &self,
        iteration: u32,
        messages: &[crate::models::Message],
        prompt_tokens: u64,
        completion_tokens: u64,
    ) {
        if let (Some(sid), Some(pool)) = (self.session_id, &self.sqlite_pool) {
            let data = crate::session::CheckpointData {
                session_id: sid,
                iteration,
                messages: messages.to_vec(),
                token_prompt: prompt_tokens,
                token_completion: completion_tokens,
            };

            if let Some(ref journal) = self.checkpoint_journal {
                journal.push(data).await;
                return;
            }

            let state_hash = crate::session::compute_state_hash(messages);
            if let Err(msg) =
                crate::session::check_circuit_breaker(pool, sid, iteration, &state_hash).await
            {
                tracing::error!("[circuit-breaker] {}", msg);
                return;
            }

            if let Err(e) = crate::session::save_checkpoint(pool, &data).await {
                tracing::warn!("[checkpoint] failed: {}", e);
            }
        }
    }

    async fn audit_turn(
        &self,
        request: &LLMRequest,
        response: &LLMResponse,
        state: &tokio::sync::MutexGuard<'_, crate::models::AgentState>,
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
        let safe_input = if std::env::var("VOLT_LEAK_DETECTOR").ok().as_deref() != Some("false") {
            let ld = crate::leak_detector::LeakDetector::new();
            let result = ld.scan(input);
            if !result.found.is_empty() {
                tracing::warn!(
                    "[leak detector] redacted {} secrets from user input",
                    result.found.len()
                );
            }
            result.redacted_text
        } else {
            input.to_string()
        };
        let mut state = self.state.lock().await;
        let parent_id = crate::models::Message::last_id(&state.messages);
        state.messages.push(Message {
            id: Uuid::new_v4(),
            parent_message_id: parent_id,
            role: "user".into(),
            content: Arc::new(safe_input),
            tool_calls: None,
            tool_result: None,
            tool_name: None,
            created_at: chrono::Utc::now(),
        });
    }

    async fn build_context(&self, input: &str) -> Option<(Vec<f32>, String)> {
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

        if let (Some(ref emb), Some(ref store)) = (&context_embedding, &self.context_store) {
            // strict_mode: exclude Tool and Skill from RAG retrieval
            let kinds: Vec<_> = if self.config.strict_mode {
                self.config
                    .enabled_context_kinds
                    .iter()
                    .filter(|k| {
                        **k != crate::context::ContextKind::Tool
                            && **k != crate::context::ContextKind::Skill
                    })
                    .collect()
            } else {
                self.config.enabled_context_kinds.iter().collect()
            };
            let per_kind_limit = 8_usize.div_ceil(kinds.len());
            let mut all_retrieved: Vec<crate::context::ContextEntry> = Vec::new();
            for kind in kinds {
                let mut kind_results = store
                    .search(emb, per_kind_limit, Some(*kind), 0.25, Some(&context_query))
                    .await;
                all_retrieved.append(&mut kind_results);
            }
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
                        format!("## {tag}\n{}", e.content)
                    })
                    .collect();
                let mut state = self.state.lock().await;
                state.messages.retain(|m| {
                    !(m.role == "system"
                        && (m.content.starts_with("## Retrieved context")
                            || m.content.starts_with("## Retrieved skills")
                            || m.content.starts_with("<retrieved_context>")
                            || m.content.starts_with("<retrieved_skills>")))
                });
                let insert_idx = state
                    .messages
                    .iter()
                    .position(|m| m.role != "system")
                    .unwrap_or(state.messages.len());
                let parent_id = crate::models::Message::last_id(&state.messages);
                state.messages.insert(
                    insert_idx,
                    Message {
                        id: Uuid::new_v4(),
                        parent_message_id: parent_id,
                        role: "system".into(),
                        content: Arc::new(format!(
                            "## Retrieved context\n{}\n\nDO NOT repeat or echo the above context. Respond to the user's request directly.",
                            blocks.join("\n\n")
                        )),
                        tool_calls: None,
                        tool_result: None,
                        tool_name: None,
                        created_at: chrono::Utc::now(),
                    },
                );
            }
        }

        if !self.is_precision_mode() {
            if let (Some(ref emb), Some(ref skills)) = (&context_embedding, &self.skills) {
                let matched = skills.search(emb, 3).await;
                if !matched.is_empty() {
                    let block: Vec<String> = matched
                        .iter()
                        .map(|s| format!("### Skill: {0}\n{1}", s.name, s.content))
                        .collect();
                    if !block.is_empty() {
                        let mut state = self.state.lock().await;
                        let parent_id = crate::models::Message::last_id(&state.messages);
                        state.messages.push(Message {
                            id: Uuid::new_v4(),
                            parent_message_id: parent_id,
                            role: "system".into(),
                            content: Arc::new(format!(
                                "## Retrieved skills\n{}\n\nDO NOT repeat or echo the above context.",
                                block.join("\n\n")
                            )),
                            tool_calls: None,
                            tool_result: None,
                            tool_name: None,
                            created_at: chrono::Utc::now(),
                        });
                    }
                }
            }
        }

        if !self.is_precision_mode() {
            if let (Some(ref emb), Some(ref db)) = (&context_embedding, &self.db) {
                if let Ok(memories) = crate::db::search_memories(db, emb, 5, None).await {
                    if !memories.is_empty() {
                        let block: Vec<String> = memories
                            .iter()
                            .map(|m| format!("[{}] {}", m.kind, m.content))
                            .collect();
                        let ctx = format!("## Relevant memories\n{}", block.join("\n"));
                        let mut state = self.state.lock().await;
                        let parent_id = crate::models::Message::last_id(&state.messages);
                        state.messages.push(Message {
                            id: Uuid::new_v4(),
                            parent_message_id: parent_id,
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
        }

        context_embedding.map(|e| (e, context_query))
    }

    #[expect(dead_code)]
    async fn push_message(
        state: &mut tokio::sync::MutexGuard<'_, crate::models::AgentState>,
        role: impl Into<String>,
        content: impl Into<String>,
    ) {
        let parent_id = crate::models::Message::last_id(&state.messages);
        let msg = crate::models::Message::new(role, content).with_parent_option(parent_id);
        state.messages.push(msg);
    }

    async fn build_llm_messages(&self) -> Vec<LLMMessage> {
        let state_snapshot = self.state.lock().await;
        let mut llm_messages: Vec<LLMMessage> = Vec::new();
        let linearized = crate::models::linearize_messages(&state_snapshot.messages);

        for m in linearized {
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

    async fn push_assistant_message(
        &self,
        state: &mut tokio::sync::MutexGuard<'_, crate::models::AgentState>,
        response: &LLMResponse,
        tool_calls: Option<&Vec<ToolCall>>,
    ) {
        let parent_id = crate::models::Message::last_id(&state.messages);
        state.messages.push(Message {
            id: Uuid::new_v4(),
            parent_message_id: parent_id,
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
        allow_session: bool,
    ) -> Vec<(String, String, String, ToolResult)> {
        let mut allow_session = allow_session;
        for tc in tool_calls {
            if self.is_cancelled() {
                return Vec::new();
            }
            let needs_approval = self.tools.get_permission(&tc.name).await
                == PermissionLevel::Prompt
                && !self.config.allow_all
                && !allow_session;
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
                if answer == "a" {
                    allow_session = true;
                }
            }
        }

        if self.is_cancelled() {
            return Vec::new();
        }

        let mut allowed = Vec::new();
        for tc in tool_calls {
            let scope = crate::capability::tool_required_scope(&tc.name);
            let tokens = self.capability_manager.list_tokens().await;
            let has_valid = tokens
                .iter()
                .any(|t| t.scope == scope && t.remaining > 0 && chrono::Utc::now() <= t.expires_at);
            if !has_valid {
                tracing::warn!(
                    "[capability] no valid token for {:?} — skipping tool '{}'",
                    scope,
                    tc.name
                );
                continue;
            }
            allowed.push(tc.clone());
        }

        let mut skipped = Vec::new();
        let mut failure_filtered = Vec::new();
        for tc in &allowed {
            if let Some(ref tracker) = self.failure_tracker {
                if let Some(warning) = tracker.should_avoid(&tc.name).await {
                    skipped.push((tc.name.clone(), tc.id.clone(), warning));
                    continue;
                }
            }
            failure_filtered.push(tc.clone());
        }

        let cap_mgr = self.capability_manager.clone();
        let futures: Vec<_> = failure_filtered
            .iter()
            .map(|tc| {
                let tools = self.tools.clone();
                let cap_mgr = cap_mgr.clone();
                let name = tc.name.clone();
                let args = tc.arguments.clone();
                let id = tc.id.clone();
                let store = self.context_store.clone();
                let embedder = self.embedder.clone();
                tracing::debug!("executing tool: {} with {}", name, args);
                async move {
                    let result = tools
                        .execute_gated(&name, &args, &cap_mgr)
                        .await
                        .unwrap_or_else(|e| ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("tool error: {}", e)),
                            duration_ms: 0,
                        });

                    if let (Some(ref store), Some(ref emb)) = (&store, &embedder) {
                        if let Ok(_emb) = emb.embed_description(&name).await {
                            store
                                .record_run(
                                    &args.to_string(),
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
                    let raw_output = if result.success {
                        result.output.clone()
                    } else {
                        format!("error: {}", error_msg.unwrap_or_default())
                    };

                    let output = if std::env::var("VOLT_WRAP_TOOL_OUTPUT").ok().as_deref()
                        != Some("false")
                    {
                        crate::safety_layer::wrap_tool_output(&name, &raw_output)
                    } else {
                        raw_output
                    };

                    (name, id, output, result)
                }
            })
            .collect();

        let mut results: Vec<(String, String, String, ToolResult)> =
            futures::future::join_all(futures).await;

        for (name, id, warning) in skipped {
            results.push((
                name,
                id,
                warning.clone(),
                ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(warning),
                    duration_ms: 0,
                },
            ));
        }

        for (name, _, _, ref result) in &results {
            if let Some(ref bus) = self.event_bus {
                bus.publish(crate::events::Event::ToolExecuted {
                    tool_name: name.clone(),
                    success: result.success,
                });
            }
        }

        if allow_session {
            let mut state = self.state.lock().await;
            state.allow_session = true;
        }

        results
    }

    async fn store_memory(
        &self,
        input: &str,
        content: &str,
        state: &tokio::sync::MutexGuard<'_, crate::models::AgentState>,
        existing_embedding: Option<&Vec<f32>>,
    ) {
        if let (Some(ref db), Some(ref embedder)) = (&self.db, &self.embedder) {
            let embedding = match existing_embedding {
                Some(emb) => Ok(emb.clone()),
                None => embedder.embed_description(input).await,
            };
            if let Ok(embedding) = embedding {
                let summary = content.chars().take(500).collect::<String>();
                if let Err(e) = crate::db::store_memory(
                    db,
                    "conversation",
                    &summary,
                    &embedding,
                    Some(state.session_id),
                )
                .await
                {
                    tracing::warn!("[memory] store_memory failed: {}", e);
                }
            }
        }
    }

    async fn seed_episode_complete(
        &self,
        input: &str,
        content: &str,
        state: &tokio::sync::MutexGuard<'_, crate::models::AgentState>,
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

#[cfg(test)]
mod run_tests {
    use super::*;
    use crate::context::ContextKind;
    use crate::models::AgentConfig;
    use crate::test_utils::MockLLMProvider;
    use crate::tools::ToolRegistry;
    use std::sync::Arc;

    fn test_config() -> AgentConfig {
        AgentConfig {
            name: "run-test".into(),
            model: "test-model".into(),
            provider: "mock".into(),
            system_prompt: None,
            max_iterations: 3,
            temperature: 0.7,
            toolsets: vec!["core".into()],
            hidden: false,
            allow_all: true,
            enabled_context_kinds: vec![ContextKind::Tool, ContextKind::Artifact],
            essential_tools: vec!["read".into()],
            context_kind_quotas: std::collections::HashMap::new(),
            use_mtp: false,
            use_cot: false,
            allow_write: true,
            framework: None,
            model_variant: None,
            quantization: None,
            format_dialect: Default::default(),
            quirks: vec![],
            strict_mode: false,
            max_tools_per_turn: None,
            blueprint_path: None,
        }
    }

    async fn create_agent() -> Agent {
        let provider = Box::new(MockLLMProvider::new(vec![]));
        let tools = ToolRegistry::new();
        Agent::new(test_config(), provider, tools).await
    }

    #[tokio::test]
    async fn test_store_memory_no_db_graceful() {
        let agent = create_agent().await;
        let state = agent.state.lock().await;
        // Should not panic when db and embedder are None
        agent
            .store_memory("test input", "test output", &state, None)
            .await;
    }

    #[tokio::test]
    async fn test_save_checkpoint_no_session_graceful() {
        let agent = create_agent().await;
        let messages = vec![];
        // Should not panic when session_id and sqlite_pool are None
        agent.save_checkpoint(0, &messages, 0, 0).await;
    }

    #[tokio::test]
    async fn test_is_cancelled_no_token() {
        let agent = create_agent().await;
        assert!(!agent.is_cancelled());
    }

    #[tokio::test]
    async fn test_is_cancelled_with_token() {
        let agent = create_agent().await;
        let token = crate::models::CancelToken::new();
        let agent = agent.with_cancel(token.clone());
        assert!(!agent.is_cancelled());
    }

    #[tokio::test]
    async fn test_is_cancelled_true_after_cancel() {
        let agent = create_agent().await;
        let token = crate::models::CancelToken::new();
        let agent = agent.with_cancel(token.clone());
        token.cancel();
        assert!(agent.is_cancelled());
    }

    #[tokio::test]
    async fn test_save_session_messages_delta_no_session() {
        let agent = create_agent().await;
        // Should not panic when no session configured
        agent.save_session_messages_delta().await;
    }

    #[tokio::test]
    async fn test_save_session_messages_delta_empty_state() {
        let agent = create_agent().await;
        // Push a message into state first
        {
            let mut state = agent.state.lock().await;
            state.last_saved_message_idx = 0;
        }
        // last == current, should be no-op
        agent.save_session_messages_delta().await;
    }

    #[tokio::test]
    async fn test_build_llm_messages_empty() {
        let agent = create_agent().await;
        let messages = agent.build_llm_messages().await;
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_build_llm_messages_with_state() {
        let agent = create_agent().await;
        {
            let mut state = agent.state.lock().await;
            state.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                parent_message_id: None,
                role: "user".into(),
                content: Arc::new("hello".into()),
                tool_calls: None,
                tool_result: None,
                tool_name: None,
                created_at: chrono::Utc::now(),
            });
        }
        let messages = agent.build_llm_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content.as_str(), "hello");
    }

    #[tokio::test]
    async fn test_audit_turn_no_context_store() {
        let agent = create_agent().await;
        let request = LLMRequest {
            model: "test".into(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
            stop: None,
            tools: None,
            stream: false,
            ..Default::default()
        };
        let response = LLMResponse {
            content: Arc::new("test response".into()),
            tool_calls: None,
            finish_reason: Some("stop".into()),
            usage: None,
            usage_breakdown: None,
            executed_tools: None,
            system_fingerprint: None,
            x_groq: None,
        };
        let state = agent.state.lock().await;
        // Should not panic when context_store is None
        agent.audit_turn(&request, &response, &state).await;
    }

    #[tokio::test]
    async fn test_seed_artifact_no_seed_channel() {
        let agent = create_agent().await;
        let result = ToolResult {
            success: true,
            output: "Wrote to /tmp/test.rs".into(),
            error: None,
            duration_ms: 0,
        };
        // Should not panic when seed_channel is None
        agent.seed_artifact_if_applicable("write", &result).await;
    }

    #[tokio::test]
    async fn test_seed_artifact_failed_result_skipped() {
        let agent = create_agent().await;
        let result = ToolResult {
            success: false,
            output: "error".into(),
            error: Some("failed".into()),
            duration_ms: 0,
        };
        // Should not panic - early return on !success
        agent.seed_artifact_if_applicable("write", &result).await;
    }

    #[tokio::test]
    async fn test_seed_episode_complete_no_channel() {
        let agent = create_agent().await;
        let state = agent.state.lock().await;
        agent.seed_episode_complete("input", "output", &state).await;
    }

    #[tokio::test]
    async fn test_push_user_message_leak_detector() {
        let agent = create_agent().await;
        // Temporarily enable leak detector
        std::env::set_var("VOLT_LEAK_DETECTOR", "true");
        agent.push_user_message("test input without secrets").await;
        let state = agent.state.lock().await;
        assert!(state
            .messages
            .iter()
            .any(|m| m.role == "user" && m.content.as_str() == "test input without secrets"));
    }

    #[tokio::test]
    async fn test_build_context_no_embedder() {
        let agent = create_agent().await;
        let result = agent.build_context("test input").await;
        // Should return None when no embedder configured
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_execute_tool_calls_empty() {
        let agent = create_agent().await;
        let results = agent.execute_tool_calls(&[], false).await;
        assert!(results.is_empty());
    }
}
