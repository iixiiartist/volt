use crate::agent::tool_parser::parse_lossy_json;
use crate::llm::provider::TokenCallback;
use crate::llm::LLMProvider;
use crate::models::{LLMRequest, LLMResponse, ToolCall, Usage};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;

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

fn build_messages(request: &LLMRequest) -> (Vec<serde_json::Value>, Option<String>) {
    let mut system: Option<String> = None;
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for msg in &request.messages {
        if msg.role == "system" {
            system = Some(msg.content.as_str().to_string());
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

    (messages, system)
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
        let (messages, system) = build_messages(request);

        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.input_schema}))
                .collect::<Vec<_>>()
        });

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "messages": messages
        });
        if let Some(s) = system {
            body["system"] = json!(s);
        }
        if let Some(ts) = &tools {
            body["tools"] = json!(ts);
        }

        let resp: serde_json::Value = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
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
        let (messages, system) = build_messages(request);

        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.input_schema}))
                .collect::<Vec<_>>()
        });

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "stream": true,
            "messages": messages
        });
        if let Some(s) = system {
            body["system"] = json!(s);
        }
        if let Some(ts) = &tools {
            body["tools"] = json!(ts);
        }

        let fut = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send();

        let response = tokio::time::timeout(std::time::Duration::from_secs(300), fut)
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
                prompt_tokens_details: None,
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

    let usage = resp["usage"].as_object().map(|u| Usage {
        prompt_tokens: u["input_tokens"].as_u64().unwrap_or(0),
        completion_tokens: u["output_tokens"].as_u64().unwrap_or(0),
        total_tokens: (u["input_tokens"].as_u64().unwrap_or(0)
            + u["output_tokens"].as_u64().unwrap_or(0)),
        queue_time: None,
        total_time: None,
        prompt_tokens_details: None,
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
