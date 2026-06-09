use crate::agent::tool_parser::parse_lossy_json;
use crate::llm::provider::{TokenCallback, LLM_HTTP_TIMEOUT, LLM_POLL_INTERVAL, LLM_POLL_MAX_ITERATIONS, LLM_POLL_TIMEOUT};
use crate::llm::LLMProvider;
use crate::models::{
    AudioRequest, AudioResponse, LLMRequest, LLMResponse, ToolCall, TtsRequest, Usage,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;

pub struct OllamaProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            http: crate::http_client().clone(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn build_chat_body(request: &LLMRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = json!({
                    "role": m.role,
                    "content": m.content.as_str()
                });
                if let Some(tcs) = &m.tool_calls {
                    let ollama_tcs: Vec<serde_json::Value> = tcs
                        .iter()
                        .map(|tc| {
                            json!({
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments
                                }
                            })
                        })
                        .collect();
                    msg["tool_calls"] = json!(ollama_tcs);
                }
                msg
            })
            .collect();

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": false,
            "options": {
                "temperature": request.temperature.unwrap_or(0.7),
                "num_predict": request.max_tokens.unwrap_or(4096) as i64,
            }
        });

        if let Some(tools) = &request.tools {
            let ollama_tools: Vec<serde_json::Value> = tools
                .iter()
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
                .collect();
            body["tools"] = json!(ollama_tools);
        }

        // Ollama native think/reasoning support
        if let Some(ref effort) = request.reasoning_effort {
            body["think"] = json!(effort);
        }

        // Structured output
        if let Some(ref fmt) = request.response_format {
            body["format"] = fmt.to_request_value();
        }

        body
    }
}

fn parse_ollama_response(val: serde_json::Value) -> anyhow::Result<LLMResponse> {
    let message = &val["message"];
    let content = message["content"].as_str().unwrap_or_default().to_string();

    let tool_calls = message["tool_calls"].as_array().map(|arr| {
        arr.iter()
            .map(|tc| {
                let fn_obj = &tc["function"];
                let args_val = &fn_obj["arguments"];
                ToolCall {
                    id: String::new(),
                    name: fn_obj["name"].as_str().unwrap_or("").to_string(),
                    arguments: if args_val.is_string() {
                        parse_lossy_json(args_val.as_str().unwrap_or("{}"))
                    } else {
                        args_val.clone()
                    },
                }
            })
            .collect()
    });

    let usage = Usage {
        prompt_tokens: val["prompt_eval_count"].as_u64().unwrap_or(0),
        completion_tokens: val["eval_count"].as_u64().unwrap_or(0),
        total_tokens: val["prompt_eval_count"].as_u64().unwrap_or(0)
            + val["eval_count"].as_u64().unwrap_or(0),
        queue_time: None,
        total_time: val["total_duration"].as_f64().map(|d| d / 1_000_000_000.0),
        prompt_tokens_details: None,
    };

    Ok(LLMResponse {
        content: Arc::new(content),
        tool_calls,
        finish_reason: val["done_reason"].as_str().map(|s| s.to_string()),
        usage: Some(usage),
        usage_breakdown: None,
        executed_tools: None,
        system_fingerprint: None,
        x_groq: None,
    })
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["*".into()]
    }

    async fn complete(&self, request: &LLMRequest) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/api/chat", self.base_url);
        let body = Self::build_chat_body(request);

        let mut req = self.http.post(&url).json(&body);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp_val = req
            .timeout(LLM_HTTP_TIMEOUT)
            .send()
            .await?;

        let status = resp_val.status();

        // Async inference (202)
        if status == reqwest::StatusCode::ACCEPTED {
            let async_resp: serde_json::Value = resp_val.json().await?;
            let request_id = async_resp["request_id"]
                .as_str()
                .or_else(|| async_resp["id"].as_str())
                .unwrap_or("")
                .to_string();
            if request_id.is_empty() {
                anyhow::bail!("ollama async returned 202 but no request_id");
            }
            return crate::llm::poll_async::poll_async_inference(
                &self.http,
                &format!("{}/chat/{}", url.trim_end_matches('/'), request_id),
                LLM_POLL_MAX_ITERATIONS,
                LLM_POLL_INTERVAL,
                LLM_POLL_TIMEOUT,
                |req| {
                    if self.api_key.is_empty() {
                        req
                    } else {
                        req.header("Authorization", format!("Bearer {}", self.api_key))
                    }
                },
                parse_ollama_response,
            ).await;
        }

        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("ollama HTTP {}: {}", status.as_u16(), trunc);
        }

        let resp: serde_json::Value = resp_val.json().await?;
        parse_ollama_response(resp)
    }

    async fn complete_stream(
        &self,
        request: &LLMRequest,
        on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/api/chat", self.base_url);
        let mut body = Self::build_chat_body(request);
        body["stream"] = json!(true);

        let mut req = self.http.post(&url).json(&body);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let response = req
            .timeout(LLM_HTTP_TIMEOUT)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("ollama HTTP {}: {}", status.as_u16(), trunc);
        }

        let mut full_content = String::new();
        let mut tool_calls_acc: Vec<ToolCall> = Vec::new();
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

                if line.is_empty() {
                    continue;
                }

                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(msg) = val.get("message") {
                        if let Some(content) = msg["content"].as_str() {
                            if !content.is_empty() {
                                full_content.push_str(content);
                                on_token(content);
                            }
                        }
                        if let Some(tcs) = msg["tool_calls"].as_array() {
                            for tc in tcs {
                                let fn_obj = &tc["function"];
                                tool_calls_acc.push(ToolCall {
                                    id: String::new(),
                                    name: fn_obj["name"].as_str().unwrap_or("").to_string(),
                                    arguments: fn_obj["arguments"].clone(),
                                });
                            }
                        }
                    }
                    if val["done"].as_bool().unwrap_or(false) {
                        finish_reason = val["done_reason"].as_str().map(|s| s.to_string());
                        usage = Some(Usage {
                            prompt_tokens: val["prompt_eval_count"].as_u64().unwrap_or(0),
                            completion_tokens: val["eval_count"].as_u64().unwrap_or(0),
                            total_tokens: val["prompt_eval_count"].as_u64().unwrap_or(0)
                                + val["eval_count"].as_u64().unwrap_or(0),
                            queue_time: None,
                            total_time: None,
                            prompt_tokens_details: None,
                        });
                    }
                }
            }
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
            usage_breakdown: None,
            executed_tools: None,
            system_fingerprint: None,
            x_groq: None,
        })
    }

    async fn transcribe(&self, _audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        anyhow::bail!("ollama does not support audio transcription via native API (use OpenAI-compatible endpoint instead)")
    }

    async fn synthesize(&self, _tts: &TtsRequest) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!(
            "ollama does not support TTS via native API (use OpenAI-compatible endpoint instead)"
        )
    }
}
