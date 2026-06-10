use crate::agent::tool_parser::parse_lossy_json;
use crate::llm::provider::{TokenCallback, DEFAULT_MAX_TOKENS, LLM_HTTP_TIMEOUT};
use crate::llm::LLMProvider;
use crate::models::{LLMRequest, LLMResponse, ToolCall, Usage};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;

const ANTHROPIC_API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    name: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: Option<String>, name: String) -> Self {
        Self {
            http: crate::http_client().clone(),
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".into()),
            name,
        }
    }
}

/// Build Anthropic Messages API format with prompt caching support.
///
/// System messages are collected into an array of text blocks with
/// `cache_control: {"type": "ephemeral"}` on the FIRST system block.
/// The LAST content block of the LAST message also gets `cache_control`
/// to create a rolling prefix cache for multi-turn agent loops.
fn build_messages(request: &LLMRequest) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut system_blocks: Vec<serde_json::Value> = Vec::new();
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for msg in &request.messages {
        if msg.role == "system" {
            system_blocks.push(json!({"type": "text", "text": msg.content.as_str()}));
            continue;
        }
        if msg.role == "tool" && msg.tool_call_id.is_some() {
            messages.push(json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id,
                    "content": msg.content.as_str()
                }]
            }));
        } else if let Some(tcs) = &msg.tool_calls {
            let mut content: Vec<serde_json::Value> = Vec::new();
            if !msg.content.is_empty() {
                content.push(json!({"type": "text", "text": msg.content.as_str()}));
            }
            for tc in tcs {
                content.push(json!({
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.name,
                    "input": tc.arguments
                }));
            }
            messages.push(json!({"role": "assistant", "content": content}));
        } else {
            messages.push(json!({"role": msg.role, "content": msg.content.as_str()}));
        }
    }

    // Add cache_control to the first system block so the static prefix is cached
    if !system_blocks.is_empty() {
        if let Some(first) = system_blocks.first_mut() {
            if let Some(obj) = first.as_object_mut() {
                obj.insert("cache_control".into(), json!({"type": "ephemeral"}));
            }
        }
    }

    // Add cache_control to the last content block of the last message
    // This creates a rolling cache: on the next turn, everything up to this
    // point is a cached prefix, and only new content needs fresh processing.
    if let Some(last_msg) = messages.last_mut() {
        if let Some(content) = last_msg.get_mut("content") {
            if content.is_string() {
                // Convert string content to array of text blocks
                let text = content.as_str().unwrap_or("").to_string();
                *content = json!([
                    {"type": "text", "text": text, "cache_control": {"type": "ephemeral"}}
                ]);
            } else if let Some(arr) = content.as_array_mut() {
                if let Some(last_block) = arr.last_mut() {
                    if let Some(obj) = last_block.as_object_mut() {
                        obj.insert("cache_control".into(), json!({"type": "ephemeral"}));
                    }
                }
            }
        }
    }

    (messages, system_blocks)
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "claude-3-5-sonnet-20241022".into(),
            "claude-3-5-haiku-20241022".into(),
            "claude-3-opus-20240229".into(),
        ]
    }

    async fn complete(&self, request: &LLMRequest) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let (messages, system_blocks) = build_messages(request);

        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.input_schema}))
                .collect::<Vec<_>>()
        });

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            "messages": messages
        });
        if !system_blocks.is_empty() {
            body["system"] = json!(system_blocks);
        }
        if let Some(ts) = &tools {
            body["tools"] = json!(ts);
        }

        let resp: serde_json::Value = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            // Array system blocks with cache_control require 2023-06-01 or newer.
            // 2023-06-01 is the stable version that supports both array system
            // blocks and prompt caching.
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        parse_anthropic_response(resp)
    }

    async fn complete_stream(
        &self,
        request: &LLMRequest,
        on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let (messages, system_blocks) = build_messages(request);

        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.input_schema}))
                .collect::<Vec<_>>()
        });

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            "stream": true,
            "messages": messages
        });
        if !system_blocks.is_empty() {
            body["system"] = json!(system_blocks);
        }
        if let Some(ts) = &tools {
            body["tools"] = json!(ts);
        }

        let fut = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .json(&body)
            .send();

        let response = tokio::time::timeout(LLM_HTTP_TIMEOUT, fut)
            .await
            .map_err(|_| anyhow::anyhow!("anthropic request timed out after 300s"))?
            .map_err(|e| anyhow::anyhow!("anthropic request failed: {}", e))?
            .error_for_status()
            .map_err(|e| anyhow::anyhow!("anthropic HTTP error: {}", e))?;

        let mut full_content = String::new();
        let mut tool_calls_acc: Vec<ToolCall> = Vec::new();
        let mut current_tool_call: Option<ToolCall> = None;
        let mut stop_reason: Option<String> = None;
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut cache_read_tokens = 0u64;
        let mut cache_create_tokens = 0u64;

        let mut stream = response.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                let line = line.trim();
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                    match val["type"].as_str() {
                        Some("content_block_delta") => {
                            if let Some(text) = val["delta"]["text"].as_str() {
                                full_content.push_str(text);
                                on_token(text);
                            }
                            if let Some(input) = val["delta"]["partial_json"].as_str() {
                                let id = val["delta"]
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let name = val["delta"]
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if !name.is_empty() {
                                    if let Some(prev) = current_tool_call.take() {
                                        tool_calls_acc.push(prev);
                                    }
                                    current_tool_call = Some(ToolCall {
                                        id,
                                        name,
                                        arguments: parse_lossy_json(input),
                                    });
                                } else if let Some(ref mut current) = current_tool_call {
                                    let additional = parse_lossy_json(input);
                                    if let Some(existing) = current.arguments.as_object_mut() {
                                        if let Some(additional_obj) = additional.as_object() {
                                            for (k, v) in additional_obj {
                                                existing.insert(k.clone(), v.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some("content_block_start")
                            if val["content_block"]["type"] == "tool_use" =>
                        {
                            if let Some(prev) = current_tool_call.take() {
                                tool_calls_acc.push(prev);
                            }
                        }
                        Some("message_delta") => {
                            stop_reason =
                                val["delta"]["stop_reason"].as_str().map(|s| s.to_string());
                            if let Some(u) = val.get("usage") {
                                output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
                            }
                        }
                        Some("message_start") => {
                            if let Some(u) = val.get("message").and_then(|m| m.get("usage")) {
                                input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                                cache_read_tokens =
                                    u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                                cache_create_tokens =
                                    u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some(tc) = current_tool_call {
            tool_calls_acc.push(tc);
        }

        Ok(LLMResponse {
            content: Arc::new(full_content),
            tool_calls: if tool_calls_acc.is_empty() {
                None
            } else {
                Some(tool_calls_acc)
            },
            finish_reason: stop_reason,
            usage: Some(Usage {
                prompt_tokens: input_tokens,
                completion_tokens: output_tokens,
                total_tokens: input_tokens + output_tokens,
                queue_time: None,
                total_time: None,
                prompt_tokens_details: Some(crate::models::PromptTokensDetails {
                    cached_tokens: if cache_read_tokens > 0 {
                        Some(cache_read_tokens)
                    } else {
                        None
                    },
                    cache_creation_tokens: if cache_create_tokens > 0 {
                        Some(cache_create_tokens)
                    } else {
                        None
                    },
                    cache_read_tokens: if cache_read_tokens > 0 {
                        Some(cache_read_tokens)
                    } else {
                        None
                    },
                }),
            }),
            usage_breakdown: None,
            executed_tools: None,
            system_fingerprint: None,
            x_groq: None,
        })
    }
}

fn parse_anthropic_response(resp: serde_json::Value) -> anyhow::Result<LLMResponse> {
    let content_blocks = resp["content"].as_array();
    let empty = vec![];
    let content_blocks = content_blocks.unwrap_or(&empty);
    let mut content = String::new();
    let mut tool_calls = Vec::new();

    for block in content_blocks {
        match block["type"].as_str() {
            Some("text") => content.push_str(block["text"].as_str().unwrap_or("")),
            Some("tool_use") => tool_calls.push(ToolCall {
                id: block["id"].as_str().unwrap_or("").to_string(),
                name: block["name"].as_str().unwrap_or("").to_string(),
                arguments: block["input"].clone(),
            }),
            _ => {}
        }
    }

    let usage = resp["usage"].as_object().map(|u| {
        let cache_read = u
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cache_create = u
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Usage {
            prompt_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            completion_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            total_tokens: (u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                + u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)),
            queue_time: None,
            total_time: None,
            prompt_tokens_details: Some(crate::models::PromptTokensDetails {
                cached_tokens: if cache_read > 0 {
                    Some(cache_read)
                } else {
                    None
                },
                cache_creation_tokens: if cache_create > 0 {
                    Some(cache_create)
                } else {
                    None
                },
                cache_read_tokens: if cache_read > 0 {
                    Some(cache_read)
                } else {
                    None
                },
            }),
        }
    });

    Ok(LLMResponse {
        content: Arc::new(content),
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        finish_reason: resp["stop_reason"].as_str().map(|s| s.to_string()),
        usage,
        usage_breakdown: None,
        executed_tools: None,
        system_fingerprint: None,
        x_groq: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LLMMessage;
    use std::sync::Arc;

    #[test]
    fn test_build_messages_collects_system_blocks() {
        let request = LLMRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![
                LLMMessage {
                    role: "system".into(),
                    content: Arc::new("You are Volt.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "system".into(),
                    content: Arc::new("Context from RAG.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "user".into(),
                    content: Arc::new("Hello".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            ..Default::default()
        };

        let (messages, system_blocks) = build_messages(&request);

        // System blocks collected into array
        assert_eq!(system_blocks.len(), 2);
        assert_eq!(system_blocks[0]["type"], "text");
        assert_eq!(system_blocks[0]["text"], "You are Volt.");
        assert_eq!(system_blocks[1]["text"], "Context from RAG.");

        // First system block has cache_control
        assert_eq!(system_blocks[0]["cache_control"]["type"], "ephemeral");
        // Second system block does NOT have cache_control
        assert!(system_blocks[1].get("cache_control").is_none());

        // Messages should not contain system role
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_build_messages_rolling_cache_on_last_message() {
        let request = LLMRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![
                LLMMessage {
                    role: "system".into(),
                    content: Arc::new("You are Volt.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "user".into(),
                    content: Arc::new("Query 1".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "assistant".into(),
                    content: Arc::new("Answer 1".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "user".into(),
                    content: Arc::new("Query 2".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            ..Default::default()
        };

        let (messages, _system) = build_messages(&request);

        // Last message should have cache_control on its content
        let last = messages.last().unwrap();
        let content = last["content"].as_array().unwrap();
        let last_block = content.last().unwrap();
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_parse_anthropic_response_with_cache_usage() {
        let resp = serde_json::json!({
            "content": [{"type": "text", "text": "Hello"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 80,
                "cache_creation_input_tokens": 20
            }
        });

        let parsed = parse_anthropic_response(resp).unwrap();
        let usage = parsed.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(
            usage.prompt_tokens_details.as_ref().unwrap().cached_tokens,
            Some(80)
        );
    }

    #[test]
    fn test_build_messages_rolling_cache_on_tool_use_last_message() {
        // When the last message is an assistant message containing tool_calls,
        // cache_control should be placed on the LAST content block (the tool_use block).
        let request = LLMRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![
                LLMMessage {
                    role: "system".into(),
                    content: Arc::new("You are Volt.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "user".into(),
                    content: Arc::new("Do something.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "assistant".into(),
                    content: Arc::new("".into()),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_1".into(),
                        name: "read".into(),
                        arguments: serde_json::json!({"path": "test.txt"}),
                    }]),
                    tool_call_id: None,
                },
            ],
            ..Default::default()
        };

        let (messages, _system) = build_messages(&request);

        let last = messages.last().unwrap();
        assert_eq!(last["role"], "assistant");
        let content = last["content"].as_array().unwrap();
        // Should be [tool_use] since assistant content is empty
        assert_eq!(content.len(), 1);
        let last_block = content.last().unwrap();
        assert_eq!(last_block["type"], "tool_use");
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_build_messages_rolling_cache_on_mixed_content_last_message() {
        // When the last assistant message has both text and tool_calls,
        // cache_control should be on the LAST block (the final tool_use).
        let request = LLMRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![
                LLMMessage {
                    role: "user".into(),
                    content: Arc::new("Do two things.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "assistant".into(),
                    content: Arc::new("Sure.".into()),
                    tool_calls: Some(vec![
                        ToolCall {
                            id: "call_1".into(),
                            name: "read".into(),
                            arguments: serde_json::json!({"path": "a.txt"}),
                        },
                        ToolCall {
                            id: "call_2".into(),
                            name: "write".into(),
                            arguments: serde_json::json!({"path": "b.txt", "content": "hi"}),
                        },
                    ]),
                    tool_call_id: None,
                },
            ],
            ..Default::default()
        };

        let (messages, _system) = build_messages(&request);

        let last = messages.last().unwrap();
        let content = last["content"].as_array().unwrap();
        // Should be: text block + 2 tool_use blocks
        assert_eq!(content.len(), 3);
        // First block is text without cache_control
        assert_eq!(content[0]["type"], "text");
        assert!(content[0].get("cache_control").is_none());
        // Last block is tool_use with cache_control
        let last_block = content.last().unwrap();
        assert_eq!(last_block["type"], "tool_use");
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_build_messages_only_system_blocks() {
        // Edge case: only system messages, no user/assistant.
        // All system blocks are collected; first gets cache_control.
        // No regular messages means no rolling cache on last message.
        let request = LLMRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![
                LLMMessage {
                    role: "system".into(),
                    content: Arc::new("You are Volt.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "system".into(),
                    content: Arc::new("Be helpful.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            ..Default::default()
        };

        let (messages, system_blocks) = build_messages(&request);
        assert!(messages.is_empty());
        assert_eq!(system_blocks.len(), 2);
        assert_eq!(system_blocks[0]["cache_control"]["type"], "ephemeral");
        assert!(system_blocks[1].get("cache_control").is_none());
    }

    #[test]
    fn test_build_messages_no_system_messages_rolling_cache_still_applied() {
        // Even without system blocks, the last content block of the last
        // message should still receive cache_control.
        let request = LLMRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![
                LLMMessage {
                    role: "user".into(),
                    content: Arc::new("Hello.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "assistant".into(),
                    content: Arc::new("Hi there.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: "user".into(),
                    content: Arc::new("Query.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            ..Default::default()
        };

        let (messages, system_blocks) = build_messages(&request);
        assert!(system_blocks.is_empty());
        let last = messages.last().unwrap();
        let content = last["content"].as_array().unwrap();
        let last_block = content.last().unwrap();
        assert_eq!(last_block["type"], "text");
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
    }
}
