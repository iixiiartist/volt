use crate::agent::tool_parser::parse_lossy_json;
use crate::llm::provider::TokenCallback;
use crate::llm::LLMProvider;
use crate::models::{
    AudioRequest, AudioResponse, ExecutedTool, LLMRequest, LLMResponse, ModelUsage,
    PromptTokensDetails, ToolCall, TtsRequest, Usage,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;

pub struct OpenAIProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    name: String,
}

impl OpenAIProvider {
    pub fn new(api_key: String, base_url: String, name: String) -> Self {
        Self {
            http: crate::http_client().clone(),
            api_key,
            base_url,
            name,
        }
    }

    pub fn new_with_client(http: reqwest::Client, api_key: String, base_url: String, name: String) -> Self {
        Self { http, api_key, base_url, name }
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

    // Build messages with optional vision content (image_url) support
    let messages: Vec<serde_json::Value> = request
        .messages
        .iter()
        .map(|m| {
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
        })
        .collect();

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "temperature": request.temperature.unwrap_or(0.7),
        "max_tokens": request.max_tokens.unwrap_or(4096),
        "stream": false
    });

    if let Some(ts) = tools {
        body["tools"] = json!(ts);
    }
    if let Some(v) = &request.reasoning_effort {
        body["reasoning_effort"] = json!(v);
    }
    if let Some(v) = &request.reasoning_format {
        body["reasoning_format"] = json!(v);
    }
    if let Some(v) = &request.include_reasoning {
        body["include_reasoning"] = json!(v);
    }
    if let Some(v) = &request.response_format {
        body["response_format"] = v.to_request_value();
    }
    if let Some(v) = &request.service_tier {
        body["service_tier"] = json!(v);
    }
    if let Some(v) = &request.search_settings {
        body["search_settings"] = v.clone();
    }
    if let Some(v) = &request.compound_custom {
        body["compound_custom"] = v.clone();
    }

    body
}

fn parse_usage(val: &serde_json::Value) -> Usage {
    Usage {
        prompt_tokens: val["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: val["completion_tokens"].as_u64().unwrap_or(0),
        total_tokens: val["total_tokens"].as_u64().unwrap_or(0),
        queue_time: val["queue_time"].as_f64(),
        total_time: val["total_time"].as_f64(),
        prompt_tokens_details: val["prompt_tokens_details"]
            .as_object()
            .map(|d| PromptTokensDetails {
                cached_tokens: d.get("cached_tokens").and_then(|v| v.as_u64()),
            }),
    }
}

fn parse_executed_tools(message: &serde_json::Value) -> Option<Vec<ExecutedTool>> {
    message["executed_tools"].as_array().map(|arr| {
        arr.iter()
            .map(|et| ExecutedTool {
                tool_type: et["type"].as_str().unwrap_or("").to_string(),
                arguments: et["arguments"].clone(),
                output: et["output"].clone(),
                search_results: et["search_results"].as_array().map(|sr| {
                    sr.iter()
                        .map(|r| crate::models::SearchResult {
                            title: r["title"].as_str().unwrap_or("").to_string(),
                            url: r["url"].as_str().unwrap_or("").to_string(),
                            content: r["content"].as_str().unwrap_or("").to_string(),
                            score: r["score"].as_f64().unwrap_or(0.0),
                        })
                        .collect()
                }),
            })
            .collect()
    })
}

fn audio_mime_from_ext(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "flac" => "audio/flac",
        "mp3" | "mpga" => "audio/mpeg",
        "mp4" => "audio/mp4",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

fn make_audio_part(data: Vec<u8>, filename: &str) -> reqwest::multipart::Part {
    let mime = audio_mime_from_ext(filename);
    let fallback_data = data.clone();
    reqwest::multipart::Part::bytes(data)
        .file_name(filename.to_string())
        .mime_str(mime)
        .unwrap_or_else(|_| reqwest::multipart::Part::bytes(fallback_data).file_name(filename.to_string()))
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

        let resp_val = req
            .json(&body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await?;

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

        let response = req
            .json(&body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await?;

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
        let mut usage_breakdown: Option<Vec<ModelUsage>> = None;
        let mut x_groq: Option<serde_json::Value> = None;

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
                                            prev.arguments = parse_lossy_json(&current_args_string);
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
                        usage = Some(parse_usage(u));
                    }
                    if let Some(ub) = val.get("usage_breakdown").and_then(|v| v.as_array()) {
                        usage_breakdown = Some(
                            ub.iter()
                                .filter_map(|m| {
                                    Some(ModelUsage {
                                        model: m["model"].as_str()?.to_string(),
                                        usage: parse_usage(&m["usage"]),
                                    })
                                })
                                .collect(),
                        );
                    }
                    if val.get("x_groq").is_some() {
                        x_groq = val.get("x_groq").cloned();
                    }
                }
            }
        }

        if let Some(mut tc) = current_tool_call {
            tc.arguments = parse_lossy_json(&current_args_string);
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
            usage_breakdown,
            executed_tools: None,
            system_fingerprint: None,
            x_groq,
        })
    }

    async fn transcribe(&self, audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        let url = format!("{}/audio/transcriptions", self.base_url.trim_end_matches('/'));
        let mut form = reqwest::multipart::Form::new()
            .part("file", make_audio_part(audio.file_data.clone(), &audio.file_name))
            .text("model", audio.model.clone());

        if let Some(ref lang) = audio.language {
            form = form.text("language", lang.clone());
        }
        if let Some(ref p) = audio.prompt {
            form = form.text("prompt", p.clone());
        }
        if let Some(ref rf) = audio.response_format {
            form = form.text("response_format", rf.clone());
        }
        if let Some(temp) = audio.temperature {
            form = form.text("temperature", temp.to_string());
        }

        let mut req = self.http.post(&url).multipart(form);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp_val = req
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await?;

        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("HTTP {}: {}", status.as_u16(), trunc);
        }

        let resp: serde_json::Value = resp_val.json().await?;
        Ok(AudioResponse {
            text: resp["text"].as_str().unwrap_or("").to_string(),
            x_groq: resp.get("x_groq").cloned(),
            segments: None,
            task: resp.get("task").and_then(|v| v.as_str()).map(String::from),
            language: resp.get("language").and_then(|v| v.as_str()).map(String::from),
            duration: resp.get("duration").and_then(|v| v.as_f64()),
        })
    }

    async fn translate(&self, audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        let url = format!("{}/audio/translations", self.base_url.trim_end_matches('/'));
        let form = reqwest::multipart::Form::new()
            .part("file", make_audio_part(audio.file_data.clone(), &audio.file_name))
            .text("model", audio.model.clone());

        let mut req = self.http.post(&url).multipart(form);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp_val = req
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await?;

        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("HTTP {}: {}", status.as_u16(), trunc);
        }

        let resp: serde_json::Value = resp_val.json().await?;
        Ok(AudioResponse {
            text: resp["text"].as_str().unwrap_or("").to_string(),
            x_groq: resp.get("x_groq").cloned(),
            segments: None,
            task: resp.get("task").and_then(|v| v.as_str()).map(String::from),
            language: resp.get("language").and_then(|v| v.as_str()).map(String::from),
            duration: resp.get("duration").and_then(|v| v.as_f64()),
        })
    }

    async fn synthesize(&self, tts: &TtsRequest) -> anyhow::Result<Vec<u8>> {
        let url = format!("{}/audio/speech", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": tts.model,
            "input": tts.input,
            "voice": tts.voice,
            "response_format": tts.response_format.as_deref().unwrap_or("wav"),
        });

        let mut req = self.http.post(&url);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp_val = req
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await?;

        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("HTTP {}: {}", status.as_u16(), trunc);
        }

        Ok(resp_val.bytes().await?.to_vec())
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
                    parse_lossy_json(args_val.as_str().unwrap_or("{}"))
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
        prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0),
        queue_time: u["queue_time"].as_f64(),
        total_time: u["total_time"].as_f64(),
        prompt_tokens_details: u["prompt_tokens_details"]
            .as_object()
            .map(|d| PromptTokensDetails {
                cached_tokens: d.get("cached_tokens").and_then(|v| v.as_u64()),
            }),
    });

    let usage_breakdown = resp["usage_breakdown"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    Some(ModelUsage {
                        model: m["model"].as_str()?.to_string(),
                        usage: parse_usage(&m["usage"]),
                    })
                })
                .collect()
        });

    let executed_tools = parse_executed_tools(message);

    Ok(LLMResponse {
        content,
        tool_calls,
        finish_reason: choice["finish_reason"].as_str().map(|s| s.to_string()),
        usage,
        usage_breakdown,
        executed_tools,
        system_fingerprint: resp["system_fingerprint"].as_str().map(|s| s.to_string()),
        x_groq: resp.get("x_groq").cloned(),
    })
}
