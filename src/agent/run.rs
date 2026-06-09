use super::{Agent, MAX_TOOL_OUTPUT_CHARS};
use crate::agent::prompt::build_system_prompt;
use crate::agent::prompt_builder;
use crate::models::{
    LLMMessage, LLMRequest, LLMResponse, Message, PermissionLevel, ToolCall, ToolResult,
};
use std::sync::Arc;
use uuid::Uuid;

impl Agent {

    pub async fn run(&self, input: &str) -> anyhow::Result<String> {
        // PreRun hooks (one-shot side effects; not allowed to block).
        if let Some(registry) = &self.hook_registry {
            registry.run_pre_run().await;
        }
        self.setup_session_and_prompt(input).await;
        self.push_user_message(input).await;

        let result = self.run_iteration_loop(input).await;

        // PostRun hooks fire whether the run succeeded or failed.
        if let Some(registry) = &self.hook_registry {
            registry.run_post_run().await;
        }
        result
    }

    /// Run the agent exactly once: one LLM call, no iteration loop.
    /// Returns the first non-empty text content. Use this for simple
    /// single-shot questions (the 60% case) where the agent loop is
    /// pure overhead. ~10x cheaper and faster than `run()`.
    pub async fn run_once(&self, input: &str) -> anyhow::Result<String> {
        if let Some(registry) = &self.hook_registry {
            registry.run_pre_run().await;
        }
        self.setup_session_and_prompt(input).await;
        self.push_user_message(input).await;
        // Single LLM call — no iteration, no tool execution.
        let messages = self.build_llm_messages().await;
        let request = LLMRequest {
            model: self.config.model.clone(),
            messages,
            temperature: Some(self.config.temperature),
            max_tokens: None,
            stop: None,
            tools: None,
            stream: false,
            ..Default::default()
        };
        let response = self.provider.complete(&request).await?;
        Ok(response.content.to_string())
    }

    /// Phase 1: session load, system-prompt install, precision-mode check.
    /// Runs before any LLM call so the agent starts with consistent context.
    async fn setup_session_and_prompt(&self, input: &str) {
        let is_precision = self.is_precision_mode();
        if !is_precision {
            if let (Some(sid), Some(pool)) = (self.session_id, &self.sqlite_pool) {
                match crate::session::load_messages(pool, sid).await {
                    Ok(msgs) if !msgs.is_empty() => {
                        let mut state = self.state.lock().await;
                        // CRITICAL: clear any prior in-memory conversation
                        // before loading the new session's history. The
                        // webui reuses the same `Agent` across sessions
                        // and chats, so without this every new session
                        // would inherit the previous session's messages
                        // — including prior failed runs, stale tool
                        // outputs, and any messages that don't belong to
                        // the current session_id.
                        state.messages.clear();
                        state.last_saved_message_idx = 0;
                        state.messages.extend(msgs);
                        tracing::info!(
                            "[session] loaded {} messages for {}",
                            state.messages.len(),
                            sid
                        );
                    }
                    Ok(_) => {
                        // No messages in DB for this session — clear any
                        // stale in-memory state from a prior session.
                        let mut state = self.state.lock().await;
                        state.messages.clear();
                        state.last_saved_message_idx = 0;
                    }
                    Err(e) => tracing::warn!("[session] failed to load messages: {}", e),
                }
            }
        }

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

    /// Phase 2: the main tool-using loop. Runs up to `config.max_iterations`,
    /// each iteration calling the LLM, executing any returned tool calls, and
    /// feeding results back. Returns the final answer (or an error if cancelled
    /// or no answer was produced within the budget).
    async fn run_iteration_loop(&self, input: &str) -> anyhow::Result<String> {
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
                let limit = if self
                    .config
                    .quirks
                    .contains(&crate::agent::blueprint::ModelQuirk::SchemaLimitTen)
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
                strict_mode: self.config.strict_mode,
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
                            let is_auth_err = err_str.contains("401")
                                || err_str.contains("403")
                                || err_str.contains("Unauthorized")
                                || err_str.contains("Forbidden");
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
                            let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
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
                tracing::warn!(
                    "[agent] iter {}: usage.prompt={} completion={} content_len={} tool_calls={}",
                    state.iteration,
                    usage.prompt_tokens,
                    usage.completion_tokens,
                    response.content.len(),
                    response.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0)
                );
                state.total_prompt_tokens += usage.prompt_tokens;
                state.total_completion_tokens += usage.completion_tokens;
            } else {
                tracing::warn!(
                    "[agent] iter {}: NO USAGE! content_len={} tool_calls={}",
                    state.iteration,
                    response.content.len(),
                    response.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0)
                );
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
                    let arg_answer = final_call
                        .arguments
                        .get("answer")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    // Multi-stage fallback for empty `answer`:
                    //   1. Model's natural-language content in this turn.
                    //   2. Any non-empty string field in the tool-call JSON.
                    //   3. The most recent non-empty assistant message in
                    //      the conversation history. Some models emit the
                    //      answer in a prior turn and then "confirm" with
                    //      a redundant final_answer({}) call.
                    //   4. The most recent non-empty tool message (last
                    //      resort for models that put the answer in tool
                    //      output rather than final_answer).
                    let answer = if arg_answer.trim().is_empty() {
                        let fallback = response.content.as_str().trim().to_string();
                        if !fallback.is_empty() {
                            fallback
                        } else {
                            final_call
                                .arguments
                                .as_object()
                                .and_then(|obj| {
                                    obj.values()
                                        .find_map(|v| v.as_str().map(|s| s.trim().to_string()))
                                })
                                .filter(|s| !s.is_empty())
                                .unwrap_or_else(|| {
                                    state
                                        .messages
                                        .iter()
                                        .rev()
                                        .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
                                        .map(|m| m.content.as_str().trim().to_string())
                                        .unwrap_or_default()
                                })
                        }
                    } else {
                        arg_answer
                    };
                    tracing::warn!(
                        "[agent] final_answer received: arg_len={} content_len={} recovered_len={}",
                        final_call
                            .arguments
                            .get("answer")
                            .and_then(|v| v.as_str())
                            .map(|s| s.len())
                            .unwrap_or(0),
                        response.content.len(),
                        answer.len()
                    );
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
                if self
                    .config
                    .quirks
                    .contains(&crate::agent::blueprint::ModelQuirk::MissingFinalAnswer)
                    && !content.trim().is_empty()
                {
                    tracing::warn!(
                        "MissingFinalAnswer quirk triggered: wrapping text as final_answer"
                    );
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

        // Iteration budget exhausted — try to recover the last assistant or
        // tool message as a best-effort answer, otherwise surface a hard
        // error. `finalize_response` handles the post-run hook and the
        // empty-content branch uniformly.
        self.finalize_response().await
    }

    /// Phase 3: end-of-run cleanup. Saves the session delta, scans message
    /// history for a last non-empty assistant (or tool) text, and returns
    /// it. Returns Err if the loop exited without producing any text and
    /// nothing useful is recoverable.
    async fn finalize_response(&self) -> anyhow::Result<String> {
        self.save_session_messages_delta().await;
        let state = self.state.lock().await;
        // Prefer the latest non-empty assistant message, then fall back
        // to the latest non-empty tool message. Some models (notably the
        // small 8B) emit the answer as the *content* of an assistant
        // message but ALSO call final_answer with empty arguments. We
        // want that content.
        let last_answer = state
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
            .or_else(|| {
                state
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "tool" && !m.content.trim().is_empty())
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

        // Run UserPromptSubmit hooks. They cannot block the prompt (that
        // would be hostile), but they can inject context that is appended
        // to the user message and visible to the model.
        let mut effective_input = safe_input.clone();
        if let Some(registry) = &self.hook_registry {
            let outcome = registry.run_user_prompt_submit(&effective_input).await;
            let ctx = outcome.merged_context();
            if !ctx.trim().is_empty() {
                effective_input.push_str("\n\n[hook context]\n");
                effective_input.push_str(&ctx);
            }
        }

        let mut state = self.state.lock().await;
        let parent_id = crate::models::Message::last_id(&state.messages);
        state.messages.push(Message {
            id: Uuid::new_v4(),
            parent_message_id: parent_id,
            role: "user".into(),
            content: Arc::new(effective_input),
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
            match embedder.embed_description(&context_query).await {
                Ok(e) => Some(e),
                Err(e) => {
                    tracing::warn!("[build_context] embedder failed: {}", e);
                    None
                }
            }
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
        // Keep the conversation lean: only the last N turns go into
        // the LLM prompt verbatim. Older turns live in the
        // `Conversation` context kind and are retrieved on demand via
        // RAG in `build_context` (semantic search). This is the
        // "Everything-as-RAG" pattern — stuffing the full history
        // into every request blows past the 8B's effective working
        // context after a few turns.
        const MAX_RECENT_TURNS: usize = 6;
        const MAX_RECENT_MESSAGES: usize = 12;
        let total = state_snapshot.messages.len();
        let start = total.saturating_sub(MAX_RECENT_MESSAGES);
        // Find the boundary of the last MAX_RECENT_TURNS user/assistant
        // exchanges so we don't cut mid-tool-call.
        let mut cut = start;
        let mut user_turns = 0;
        for (i, m) in state_snapshot.messages.iter().enumerate().skip(start) {
            if m.role == "user" {
                user_turns += 1;
                if user_turns > MAX_RECENT_TURNS {
                    cut = i;
                    break;
                }
            }
        }
        let window: &[crate::models::Message] = &state_snapshot.messages[cut..];
        let linearized = crate::models::linearize_messages(window);

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
        if cut > 0 {
            tracing::info!(
                "[build_llm_messages] trimmed {} older messages from LLM context (relying on RAG for recall)",
                cut
            );
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
        mut allow_session: bool,
    ) -> Vec<(String, String, String, ToolResult)> {
        for tc in tool_calls {
            if self.is_cancelled() {
                return Vec::new();
            }
            if !self.needs_approval(&tc.name).await || allow_session || self.config.allow_all {
                continue;
            }
            if self.request_approval(&tc.name, &tc.arguments).await {
                allow_session = true;
            }
        }

        if self.is_cancelled() {
            return Vec::new();
        }

        let allowed = self.filter_by_capability(tool_calls).await;
        let failure_filtered = self.filter_by_failure_tracker(&allowed).await;
        let mut results = self.run_tools_parallel(&failure_filtered).await;

        // Surface skipped (failure-tracker warnings) and (denied) tool calls.
        self.append_skipped(&mut results, &allowed, &failure_filtered);
        self.publish_executed_events(&results);
        if allow_session {
            self.state.lock().await.allow_session = true;
        }
        results
    }

    async fn needs_approval(&self, tool_name: &str) -> bool {
        self.tools.get_permission(tool_name).await == PermissionLevel::Prompt
    }

    /// Ask the user (or TUI callback) for permission. Returns `true` if the
    /// user chose to allow all tools for the rest of the session.
    async fn request_approval(&self, name: &str, args: &serde_json::Value) -> bool {
        let decision = if let Some(approval_fn) = &self.approval_fn {
            approval_fn(name, args).await
        } else {
            eprintln!(
                "\n\x1b[33m[approval]\x1b[0m tool '{}({:?})' requires approval.",
                name, args
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
            match answer.as_str() {
                "y" | "yes" => crate::agent::ApprovalDecision::AllowOnce,
                "a" => crate::agent::ApprovalDecision::AllowSession,
                _ => crate::agent::ApprovalDecision::Deny,
            }
        };
        matches!(decision, crate::agent::ApprovalDecision::AllowSession)
    }

    /// Drop any tool call that has no valid capability token for its required scope.
    async fn filter_by_capability(&self, tool_calls: &[ToolCall]) -> Vec<ToolCall> {
        let mut allowed = Vec::with_capacity(tool_calls.len());
        for tc in tool_calls {
            let scope = crate::capability::tool_required_scope(&tc.name);
            let tokens = self.capability_manager.list_tokens().await;
            let has_valid = tokens.iter().any(|t| {
                t.scope == scope
                    && t.remaining > 0
                    && chrono::Utc::now() <= t.expires_at
            });
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
        allowed
    }

    /// Drop any tool call that the failure tracker has flagged.
    async fn filter_by_failure_tracker(&self, tool_calls: &[ToolCall]) -> Vec<ToolCall> {
        let Some(tracker) = self.failure_tracker.as_ref() else {
            return tool_calls.to_vec();
        };
        let mut kept = Vec::with_capacity(tool_calls.len());
        for tc in tool_calls {
            if let Some(warning) = tracker.should_avoid(&tc.name).await {
                tracing::info!("[failure-tracker] skipping '{}': {}", tc.name, warning);
                continue;
            }
            kept.push(tc.clone());
        }
        kept
    }

    /// Execute a batch of tool calls in parallel. Runs Pre/PostToolUse hooks,
    /// wraps output, records the run in the context store, and returns the
    /// `(name, id, output, result)` tuples.
    async fn run_tools_parallel(
        &self,
        tool_calls: &[ToolCall],
    ) -> Vec<(String, String, String, ToolResult)> {
        let cap_mgr = self.capability_manager.clone();
        let hook_registry = self.hook_registry.clone();
        let futures: Vec<_> = tool_calls
            .iter()
            .map(|tc| self.run_single_tool(tc, &cap_mgr, hook_registry.as_ref()))
            .collect();
        futures::future::join_all(futures).await
    }

    #[allow(clippy::type_complexity)]
    fn run_single_tool<'a>(
        &'a self,
        tc: &'a ToolCall,
        cap_mgr: &'a Arc<crate::capability::CapabilityManager>,
        hook_registry: Option<&'a crate::agent::hooks::HookRegistry>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = (String, String, String, ToolResult)> + Send + 'a>,
    > {
        let tools = self.tools.clone();
        let context_store = self.context_store.clone();
        let embedder = self.embedder.clone();
        Box::pin(async move {
            let name = tc.name.clone();
            let args = tc.arguments.clone();
            let id = tc.id.clone();
            tracing::debug!("executing tool: {} with {}", name, args);

            let mut effective_args = args.clone();
            let mut blocked_reason: Option<String> = None;
            if let Some(registry) = hook_registry {
                match registry.run_pre_tool_use(&name, &args).await {
                    crate::agent::hooks::PreToolDecision::Allow => {}
                    crate::agent::hooks::PreToolDecision::Block { reason } => {
                        blocked_reason = Some(reason);
                    }
                    crate::agent::hooks::PreToolDecision::ModifyArgs { args: new_args } => {
                        effective_args = new_args;
                    }
                }
            }

            let (effective_name, output, result) = if let Some(reason) = blocked_reason {
                (
                    name.clone(),
                    format!("[hook blocked] {}", reason),
                    ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("blocked by hook: {}", reason)),
                        duration_ms: 0,
                    },
                )
            } else {
                let result = tools
                    .execute_gated(&name, &effective_args, cap_mgr)
                    .await
                    .unwrap_or_else(|e| ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("tool error: {}", e)),
                        duration_ms: 0,
                    });

                if let (Some(ref store), Some(ref emb)) = (&context_store, &embedder) {
                    if emb.embed_description(&name).await.is_ok() {
                        store
                            .record_run(
                                &effective_args.to_string(),
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

                let mut post_output = output;
                if let Some(registry) = hook_registry {
                    let outcome = registry
                        .run_post_tool_use(
                            &name,
                            &effective_args,
                            &post_output,
                            result.success,
                            result.duration_ms as u64,
                        )
                        .await;
                    let ctx = outcome.merged_context();
                    if !ctx.trim().is_empty() {
                        post_output.push_str("\n\n[hook context]\n");
                        post_output.push_str(&ctx);
                    }
                }

                (name.clone(), post_output, result)
            };

            (effective_name, id, output, result)
        })
    }

    /// Add failure-tracker `skipped` warnings back into the result list as
    /// synthetic `ToolResult::error` entries, so the LLM can see why a
    /// tool was rejected.
    fn append_skipped(
        &self,
        results: &mut Vec<(String, String, String, ToolResult)>,
        allowed: &[ToolCall],
        failure_filtered: &[ToolCall],
    ) {
        let filtered_ids: std::collections::HashSet<&str> =
            failure_filtered.iter().map(|tc| tc.id.as_str()).collect();
        for tc in allowed {
            if filtered_ids.contains(tc.id.as_str()) {
                continue;
            }
            let warning = self
                .failure_tracker
                .as_ref()
                .and_then(|t| futures::executor::block_on(t.should_avoid(&tc.name)));
            if let Some(warning) = warning {
                results.push((
                    tc.name.clone(),
                    tc.id.clone(),
                    warning.clone(),
                    ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(warning),
                        duration_ms: 0,
                    },
                ));
            }
        }
    }

    /// Publish `ToolExecuted` events for every result.
    fn publish_executed_events(&self, results: &[(String, String, String, ToolResult)]) {
        for (name, _, _, result) in results {
            if let Some(ref bus) = self.event_bus {
                bus.publish(crate::events::Event::ToolExecuted {
                    tool_name: name.clone(),
                    success: result.success,
                });
            }
        }
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
