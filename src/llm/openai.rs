use crate::llm::provider::TokenCallback;
use crate::llm::LLMProvider;
use crate::models::*;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

pub struct OpenAIProvider {
    http: Client,
    api_key: String,
    base_url: String,
    name: String,
}

impl OpenAIProvider {
    pub fn new(api_key: String, base_url: String, name: String) -> Self {
        Self {
            http: crate::http_client(300),
            api_key,
            base_url,
            name,
        }
    }
}

fn build_request_body(request: &LLMRequest) -> serde_json::Value {
    let tools = request.tools.as_ref().map(|ts| {
        ts.iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema
                    }
                })
            })
            .collect::<Vec<_>>()
    });

    let mut body = json!({
        "model": request.model,
        "messages": request.messages.iter().map(|m| {
            let mut msg = json!({
                "role": m.role,
                "content": m.content.as_str()
            });
            if let Some(tcs) = &m.tool_calls {
                msg["tool_calls"] = json!(tcs.iter().map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments.to_string()
                        }
                    })
                }).collect::<Vec<_>>());
            }
            if let Some(tid) = &m.tool_call_id {
                msg["tool_call_id"] = json!(tid);
            }
            msg
        }).collect::<Vec<_>>(),
        "temperature": request.temperature.unwrap_or(0.7),
        "max_tokens": request.max_tokens.unwrap_or(4096),
        "stream": false
    });

    if let Some(ts) = tools {
        body["tools"] = json!(ts);
    }

    body
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["*".into()]
    }

    async fn complete(&self, request: &LLMRequest) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = build_request_body(request);

        let mut req = self.http.post(&url);

        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp_val = req.json(&body).send().await?;

        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("HTTP {}: {}", status.as_u16(), trunc);
        }

        let resp: serde_json::Value = resp_val.json().await?;

        parse_openai_response(resp)
    }

    async fn complete_stream(
        &self,
        request: &LLMRequest,
        on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut body = build_request_body(request);
        body["stream"] = json!(true);

        let mut req = self.http.post(&url);

        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let response = req.json(&body).send().await?;

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("HTTP {}: {}", status.as_u16(), trunc);
        }

        let mut full_content = String::new();
        let mut tool_calls_acc: Vec<ToolCall> = Vec::new();
        let mut current_tool_call: Option<ToolCall> = None;
        let mut current_args_string = String::new();
        let mut finish_reason: Option<String> = None;
        let mut usage: Option<Usage> = None;

        let mut line_buffer = String::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            line_buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_idx) = line_buffer.find('\n') {
                let line = line_buffer[..newline_idx].trim().to_string();
                line_buffer.drain(..=newline_idx);

                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(choice) = val["choices"][0].as_object() {
                        if let Some(delta) = choice.get("delta").and_then(|d| d.as_object()) {
                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                if !content.is_empty() {
                                    full_content.push_str(content);
                                    on_token(content);
                                }
                            }
                            if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                for tc in tcs {
                                    let id = tc["id"].as_str().unwrap_or("").to_string();
                                    let name =
                                        tc["function"]["name"].as_str().unwrap_or("").to_string();
                                    let args = tc["function"]["arguments"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();

                                    if !id.is_empty() {
                                        if let Some(mut prev) = current_tool_call.take() {
                                            prev.arguments =
                                                serde_json::from_str(&current_args_string)
                                                    .unwrap_or_default();
                                            tool_calls_acc.push(prev);
                                        }
                                        current_args_string = args;
                                        current_tool_call = Some(ToolCall {
                                            id,
                                            name,
                                            arguments: serde_json::Value::Null,
                                        });
                                    } else if !args.is_empty() {
                                        current_args_string.push_str(&args);
                                    }
                                }
                            }
                        }
                        if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                            finish_reason = Some(fr.to_string());
                        }
                    }
                    if let Some(u) = val.get("usage") {
                        usage = Some(Usage {
                            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
                        });
                    }
                }
            }
        }

        if let Some(mut tc) = current_tool_call {
            tc.arguments = serde_json::from_str(&current_args_string).unwrap_or_default();
            tool_calls_acc.push(tc);
        }

        Ok(LLMResponse {
            content: Arc::new(full_content),
            tool_calls: if tool_calls_acc.is_empty() {
                None
            } else {
                Some(tool_calls_acc)
            },
            finish_reason,
            usage,
        })
    }
}

fn parse_openai_response(resp: serde_json::Value) -> anyhow::Result<LLMResponse> {
    let choice = &resp["choices"][0];
    let message = &choice["message"];
    let content = Arc::new(message["content"].as_str().unwrap_or_default().to_string());

    let tool_calls = message["tool_calls"].as_array().map(|arr| {
        arr.iter()
            .map(|tc| {
                let args_val = &tc["function"]["arguments"];
                let arguments = if args_val.is_string() {
                    serde_json::from_str(args_val.as_str().unwrap_or("{}")).unwrap_or_default()
                } else {
                    args_val.clone()
                };
                ToolCall {
                    id: tc["id"].as_str().unwrap_or("").to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments,
                }
            })
            .collect()
    });

    let usage = resp["usage"].as_object().map(|u| Usage {
        prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
    });

    Ok(LLMResponse {
        content,
        tool_calls,
        finish_reason: choice["finish_reason"].as_str().map(|s| s.to_string()),
        usage,
    })
}
