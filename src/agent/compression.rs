use super::Agent;
use crate::models::{LLMMessage, Message, ModelContext};
use std::sync::Arc;
use tracing::info;

impl Agent {
    pub(super) async fn compress_if_needed(
        &self,
        llm_messages: Vec<LLMMessage>,
    ) -> Vec<LLMMessage> {
        let model_ctx = ModelContext::for_model(&self.config.model);
        let budget = (model_ctx.max_context_tokens as f64 * 0.80) as u32;
        let before = llm_messages.len();

        let msg_tokens: Vec<u32> = llm_messages
            .iter()
            .map(|m| ModelContext::estimate_tokens(m.content.as_str()))
            .collect();
        let total_tokens: u32 = msg_tokens.iter().sum();

        if total_tokens <= budget {
            return llm_messages;
        }

        let mut system_total: u32 = 0;
        let mut conversation_indices: Vec<usize> = Vec::new();
        for (i, msg) in llm_messages.iter().enumerate() {
            if msg.role == "system" {
                system_total += msg_tokens[i];
            } else {
                conversation_indices.push(i);
            }
        }

        if system_total >= budget.saturating_sub(512) {
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
                let truncated: Vec<&Message> = snapshot.messages
                    [..keep_start.min(snapshot.messages.len())]
                    .iter()
                    .collect();
                if let Some(ref store) = self.context_store {
                    self.seed_truncated_context(store, &truncated).await;
                }
                compressed.push(LLMMessage {
                    role: "system".into(),
                    content: Arc::new(format!(
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
                "context compressed (fallback): {} messages -> {} (budget: {} tokens, keeping {} tokens)",
                before, compressed.len(), budget, running
            );
            return compressed;
        }

        let conv_budget = budget.saturating_sub(system_total);

        let mut conv_running: u32 = 0;
        let mut conv_keep: Vec<(usize, u32)> = Vec::new();

        for &idx in conversation_indices.iter().rev() {
            let tok = msg_tokens[idx];
            if conv_running + tok > conv_budget {
                break;
            }
            conv_running += tok;
            conv_keep.push((idx, tok));
        }
        conv_keep.reverse();

        let total_conversation: u32 = conversation_indices.iter().map(|&i| msg_tokens[i]).sum();
        let compressed_conv_count = conversation_indices.len().saturating_sub(conv_keep.len());

        if compressed_conv_count > 0 {
            if let Some(ref store) = self.context_store {
                let truncated_indices: Vec<&usize> = conversation_indices
                    .iter()
                    .filter(|idx| !conv_keep.iter().any(|(ki, _)| ki == *idx))
                    .collect();
                let truncated_msgs: Vec<&LLMMessage> = truncated_indices
                    .iter()
                    .filter_map(|&&idx| llm_messages.get(idx))
                    .collect();
                self.seed_truncated_context_llm(store, &truncated_msgs)
                    .await;
            }
        }

        let mut compressed: Vec<LLMMessage> = Vec::with_capacity(before);
        for msg in &llm_messages {
            if msg.role == "system" {
                compressed.push(msg.clone());
            }
        }
        if compressed_conv_count > 0 {
            compressed.push(LLMMessage {
                role: "system".into(),
                content: Arc::new(format!(
                    "[Conversation summary: {} conversation turns compressed (~{} tokens). Keeping the {} most recent turns ({}/{} tokens).]",
                    compressed_conv_count,
                    total_conversation.saturating_sub(conv_running),
                    conv_keep.len(),
                    conv_running,
                    conv_budget,
                )),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        for (idx, _) in &conv_keep {
            let msg = &llm_messages[*idx];
            compressed.push(msg.clone());
        }

        info!(
            "context compressed (selective): {} messages -> {} (budget: {} tokens, system: {}, conv kept: {} tokens)",
            before, compressed.len(), budget, system_total, conv_running
        );
        compressed
    }

    async fn seed_truncated_context(
        &self,
        store: &crate::context::ContextStore,
        truncated: &[&Message],
    ) {
        if truncated.is_empty() {
            return;
        }
        let topic_hint = truncated
            .iter()
            .take(3)
            .filter_map(|m| {
                let c = m.content.trim();
                if !c.is_empty() && c.len() > 10 {
                    Some(c.chars().take(120).collect::<String>())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" | ");
        let summary = format!(
            "[Compressed conversation segment: {} messages. Topics: {}]",
            truncated.len(),
            topic_hint
        );
        let session_id = self
            .session_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "unknown".into());
        let truncated_text: String = truncated
            .iter()
            .map(|m| format!("[{}] {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(ref pool) = self.db {
            let _ = store
                .seed_truncated_history_persistent(&session_id, truncated_text, summary, pool)
                .await;
        } else {
            store
                .add(
                    crate::context::ContextKind::Conversation,
                    &summary,
                    serde_json::json!({}),
                )
                .await;
        }
    }

    async fn seed_truncated_context_llm(
        &self,
        store: &crate::context::ContextStore,
        truncated: &[&LLMMessage],
    ) {
        if truncated.is_empty() {
            return;
        }
        let topic_hint = truncated
            .iter()
            .take(3)
            .filter_map(|m| {
                let c = m.content.trim();
                if !c.is_empty() && c.len() > 10 {
                    Some(c.chars().take(120).collect::<String>())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" | ");
        let summary = format!(
            "[Compressed conversation segment: {} messages. Topics: {}]",
            truncated.len(),
            topic_hint
        );
        let session_id = self
            .session_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "unknown".into());
        let truncated_text: String = truncated
            .iter()
            .map(|m| format!("[{}] {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(ref pool) = self.db {
            let _ = store
                .seed_truncated_history_persistent(&session_id, truncated_text, summary, pool)
                .await;
        } else {
            store
                .add(
                    crate::context::ContextKind::Conversation,
                    &summary,
                    serde_json::json!({}),
                )
                .await;
        }
    }
}

#[cfg(test)]
mod compression_tests {
    use super::*;
    use crate::context::ContextKind;
    use crate::models::{AgentConfig, LLMMessage};
    use crate::test_utils::MockLLMProvider;
    use crate::tools::ToolRegistry;
    use std::sync::Arc;

    fn small_context_config() -> AgentConfig {
        AgentConfig {
            name: "compress-test".into(),
            model: "llama-3-8b".into(),
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
        Agent::new(small_context_config(), provider, tools).await
    }

    fn make_msg(role: &str, content: &str) -> LLMMessage {
        LLMMessage {
            role: role.into(),
            content: Arc::new(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[tokio::test]
    async fn test_compress_under_budget_returns_unchanged() {
        let agent = create_agent().await;
        let msgs = vec![
            make_msg("system", "You are a helpful assistant."),
            make_msg("user", "Hello!"),
            make_msg("assistant", "Hi there!"),
        ];
        let result = agent.compress_if_needed(msgs.clone()).await;
        assert_eq!(result.len(), msgs.len());
        assert_eq!(result[1].content.as_str(), "Hello!");
    }

    #[tokio::test]
    async fn test_compress_empty_messages() {
        let agent = create_agent().await;
        let result = agent.compress_if_needed(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_compress_selective_triggers_on_over_budget() {
        let agent = create_agent().await;
        let mut msgs = vec![make_msg("system", "You are a helpful AI assistant.")];
        for i in 0..15 {
            let content = format!(
                "This is message number {} with enough text to consume significant token budget. ",
                i
            ) + &"The quick brown fox jumps over the lazy dog. ".repeat(80);
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            msgs.push(make_msg(role, &content));
        }
        let compressed = agent.compress_if_needed(msgs).await;
        assert!(!compressed.is_empty());
        assert!(compressed.iter().any(|m| m.role == "system"));
    }

    #[tokio::test]
    async fn test_compress_preserves_system_messages() {
        let agent = create_agent().await;
        let mut msgs = vec![
            make_msg("system", "System instruction one."),
            make_msg("system", "System instruction two."),
        ];
        let large_content = "user data padding. ".repeat(3000);
        msgs.push(make_msg("user", &large_content));
        msgs.push(make_msg("assistant", "ok"));

        let compressed = agent.compress_if_needed(msgs).await;
        let system_count = compressed.iter().filter(|m| m.role == "system").count();
        assert!(
            system_count >= 2,
            "should preserve at least both original system messages"
        );
    }

    #[tokio::test]
    async fn test_compress_fallback_system_exceeds_budget() {
        let agent = create_agent().await;
        {
            let mut state = agent.state.lock().await;
            state.messages.push(crate::models::Message {
                id: uuid::Uuid::new_v4(),
                parent_message_id: None,
                role: "system".into(),
                content: Arc::new("large system content. ".repeat(3000)),
                tool_calls: None,
                tool_result: None,
                tool_name: None,
                created_at: chrono::Utc::now(),
            });
            state.messages.push(crate::models::Message {
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

        let msgs = vec![
            make_msg("system", &"large system content. ".repeat(3000)),
            make_msg("user", "hello"),
        ];

        let compressed = agent.compress_if_needed(msgs).await;
        assert!(!compressed.is_empty());
    }

    #[tokio::test]
    async fn test_compress_single_message_no_op() {
        let agent = create_agent().await;
        let msgs = vec![make_msg("system", "only one message")];
        let result = agent.compress_if_needed(msgs.clone()).await;
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_compress_seed_truncated_context_empty_no_panic() {
        let agent = create_agent().await;
        let store = crate::context::ContextStore::new();
        agent.seed_truncated_context(&store, &[]).await;
    }

    #[tokio::test]
    async fn test_compress_seed_truncated_context_llm_empty_no_panic() {
        let agent = create_agent().await;
        let store = crate::context::ContextStore::new();
        agent.seed_truncated_context_llm(&store, &[]).await;
    }
}
