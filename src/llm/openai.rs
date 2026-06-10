use crate::agent::tool_parser::parse_lossy_json;
use crate::llm::poll_async;
use crate::llm::provider::{
    TokenCallback, DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE, LLM_HTTP_TIMEOUT, LLM_POLL_INTERVAL,
    LLM_POLL_MAX_ITERATIONS, LLM_POLL_TIMEOUT,
};
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

    /// Apply the standard `Authorization: Bearer <key>` header when an API key
    /// is configured. Returns the request builder unchanged otherwise.
    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            req
        } else {
            req.header("Authorization", format!("Bearer {}", self.api_key))
        }
    }
}

/// Build a strict JSON Schema for structured outputs that guarantees
/// the model's response conforms to Volt's tool call format.
/// When strict_mode is enabled, this schema is sent as response_format
/// instead of using the traditional tools array.
fn build_strict_response_schema(tools: &[crate::models::ToolDefinition]) -> serde_json::Value {
    // Build one branch per tool in an anyOf
    let tool_branches: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| {
            let mut args_schema = t.input_schema.clone();
            // Ensure additionalProperties: false for strictness
            if let Some(obj) = args_schema.as_object_mut() {
                obj.insert(
                    "additionalProperties".into(),
                    serde_json::Value::Bool(false),
                );
            }
            json!({
                "type": "object",
                "properties": {
                    "name": { "const": t.name },
                    "arguments": args_schema
                },
                "required": ["name", "arguments"],
                "additionalProperties": false
            })
        })
        .collect();

    let tool_calls_schema = if tool_branches.len() == 1 {
        json!({
            "type": "array",
            "items": tool_branches.into_iter().next().unwrap()
        })
    } else {
        json!({
            "type": "array",
            "items": {
                "anyOf": tool_branches
            }
        })
    };

    json!({
        "type": "object",
        "properties": {
            "reasoning": {
                "type": "string",
                "description": "Optional chain-of-thought or planning text before taking action"
            },
            "tool_calls": tool_calls_schema,
            "content": {
                "type": "string",
                "description": "Direct text response to the user. Use this when no tools are needed."
            }
        },
        "required": ["content"],
        "additionalProperties": false
    })
}

fn build_request_body(request: &LLMRequest) -> serde_json::Value {
    // When strict_mode is enabled, use structured outputs (json_schema response_format)
    // instead of the traditional tools array. This guarantees 100% schema conformance
    // and eliminates the need for client-side AST coercion / parse_lossy_json.
    let use_structured_output = request.strict_mode;

    let tools = if use_structured_output {
        None
    } else {
        request.tools.as_ref().map(|ts| {
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
        })
    };

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
                msg["tool_calls"] = json!(tcs
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments.to_string()
                            }
                        })
                    })
                    .collect::<Vec<_>>());
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
        "temperature": request.temperature.unwrap_or(DEFAULT_TEMPERATURE),
        "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": false
    });

    if let Some(ts) = tools {
        body["tools"] = json!(ts);
    }
    // Native structured outputs: guarantee schema conformance via API-level constraint
    if use_structured_output {
        if let Some(ref defs) = request.tools {
            let schema = build_strict_response_schema(defs);
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "volt_tool_calls",
                    "strict": true,
                    "schema": schema
                }
            });
        }
        // Force the model to generate a response (not just "stop" with empty content)
        body["tool_choice"] = json!("none");
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
        prompt_tokens_details: val["prompt_tokens_details"].as_object().map(|d| {
            PromptTokensDetails {
                cached_tokens: d.get("cached_tokens").and_then(|v| v.as_u64()),
                cache_creation_tokens: d.get("cache_creation_tokens").and_then(|v| v.as_u64()),
                cache_read_tokens: d.get("cache_read_tokens").and_then(|v| v.as_u64()),
            }
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
        .unwrap_or_else(|_| {
            reqwest::multipart::Part::bytes(fallback_data).file_name(filename.to_string())
        })
}

impl OpenAIProvider {}

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

        let req = self.apply_auth(self.http.post(&url));
        let resp_val = req.json(&body).timeout(LLM_HTTP_TIMEOUT).send().await?;

        let status = resp_val.status();

        // Async inference: 202 Accepted → poll for completion (NVIDIA NIM pattern)
        if status == reqwest::StatusCode::ACCEPTED {
            let async_resp: serde_json::Value = resp_val.json().await?;
            let request_id = async_resp["request_id"]
                .as_str()
                .or_else(|| async_resp["id"].as_str())
                .unwrap_or("")
                .to_string();
            if request_id.is_empty() {
                anyhow::bail!("async inference returned 202 but no request_id in response");
            }
            return poll_async::poll_async_inference(
                &self.http,
                &format!("{}/{}", url.trim_end_matches('/'), request_id),
                LLM_POLL_MAX_ITERATIONS,
                LLM_POLL_INTERVAL,
                LLM_POLL_TIMEOUT,
                |req| self.apply_auth(req),
                parse_openai_response,
            )
            .await;
        }

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

        let req = self.apply_auth(self.http.post(&url));

        let response = req.json(&body).timeout(LLM_HTTP_TIMEOUT).send().await?;

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
        let url = format!(
            "{}/audio/transcriptions",
            self.base_url.trim_end_matches('/')
        );
        let mut form = reqwest::multipart::Form::new()
            .part(
                "file",
                make_audio_part(audio.file_data.clone(), &audio.file_name),
            )
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

        let req = self.apply_auth(self.http.post(&url).multipart(form));

        let resp_val = req.timeout(LLM_HTTP_TIMEOUT).send().await?;

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
            language: resp
                .get("language")
                .and_then(|v| v.as_str())
                .map(String::from),
            duration: resp.get("duration").and_then(|v| v.as_f64()),
        })
    }

    async fn translate(&self, audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        let url = format!("{}/audio/translations", self.base_url.trim_end_matches('/'));
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                make_audio_part(audio.file_data.clone(), &audio.file_name),
            )
            .text("model", audio.model.clone());

        let req = self.apply_auth(self.http.post(&url).multipart(form));

        let resp_val = req.timeout(LLM_HTTP_TIMEOUT).send().await?;

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
            language: resp
                .get("language")
                .and_then(|v| v.as_str())
                .map(String::from),
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

        let req = self.apply_auth(self.http.post(&url));
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

/// Parse a structured output response (strict_mode) where the content field
/// contains a JSON object with tool_calls and content fields.
fn parse_structured_output(content_str: &str) -> Option<(Vec<ToolCall>, String)> {
    let parsed = serde_json::from_str::<serde_json::Value>(content_str).ok()?;
    let text_content = parsed.get("content")?.as_str()?.to_string();

    let calls: Vec<ToolCall> =
        if let Some(tool_calls) = parsed.get("tool_calls").and_then(|v| v.as_array()) {
            tool_calls
                .iter()
                .filter_map(|tc| {
                    let name = tc.get("name")?.as_str()?.to_string();
                    let args = tc.get("arguments")?.clone();
                    Some(ToolCall {
                        id: format!("call_{}", &uuid::Uuid::new_v4().to_string()[..8]),
                        name,
                        arguments: args,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

    Some((calls, text_content))
}

fn parse_openai_response(resp: serde_json::Value) -> anyhow::Result<LLMResponse> {
    let choice = &resp["choices"][0];
    let message = &choice["message"];
    let raw_content = message["content"].as_str().unwrap_or_default().to_string();

    // Check if this is a structured output response (strict_mode)
    // In strict mode, content is a JSON string with tool_calls + content fields
    let (tool_calls, content) = if let Some((calls, text)) = parse_structured_output(&raw_content) {
        (Some(calls), Arc::new(text))
    } else {
        // Traditional tool_call format
        let calls = message["tool_calls"].as_array().map(|arr| {
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
        (calls, Arc::new(raw_content))
    };

    let usage = resp["usage"].as_object().map(|u| Usage {
        prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        completion_tokens: u
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        queue_time: u.get("queue_time").and_then(|v| v.as_f64()),
        total_time: u.get("total_time").and_then(|v| v.as_f64()),
        prompt_tokens_details: u
            .get("prompt_tokens_details")
            .and_then(|v| v.as_object())
            .map(|d| PromptTokensDetails {
                cached_tokens: d.get("cached_tokens").and_then(|v| v.as_u64()),
                cache_creation_tokens: d.get("cache_creation_tokens").and_then(|v| v.as_u64()),
                cache_read_tokens: d.get("cache_read_tokens").and_then(|v| v.as_u64()),
            }),
    });

    let usage_breakdown = resp["usage_breakdown"].as_array().map(|arr| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LLMMessage, ToolDefinition};

    #[test]
    fn test_build_strict_response_schema_basic() {
        let tools = vec![
            ToolDefinition {
                name: "read".into(),
                description: "Read a file".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
                category: "builtin".into(),
            },
            ToolDefinition {
                name: "write".into(),
                description: "Write a file".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }),
                category: "builtin".into(),
            },
        ];

        let schema = build_strict_response_schema(&tools);

        // Top level is an object
        assert_eq!(schema["type"], "object");
        // Has properties: reasoning, tool_calls, content
        assert!(schema["properties"]["reasoning"].is_object());
        assert!(schema["properties"]["tool_calls"].is_object());
        assert!(schema["properties"]["content"].is_object());
        // Required includes content
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "content"));
        // additionalProperties is false
        assert_eq!(schema["additionalProperties"], false);

        // tool_calls is an array
        let tc_schema = &schema["properties"]["tool_calls"];
        assert_eq!(tc_schema["type"], "array");

        // items has anyOf with 2 branches (one per tool)
        let items = tc_schema["items"].as_object().unwrap();
        let any_of = items["anyOf"].as_array().unwrap();
        assert_eq!(any_of.len(), 2);

        // First branch has const name = "read"
        assert_eq!(any_of[0]["properties"]["name"]["const"], "read");
        // Second branch has const name = "write"
        assert_eq!(any_of[1]["properties"]["name"]["const"], "write");

        // Each branch has additionalProperties: false
        assert_eq!(any_of[0]["additionalProperties"], false);
        assert_eq!(any_of[1]["additionalProperties"], false);
    }

    #[test]
    fn test_build_strict_response_schema_single_tool() {
        let tools = vec![ToolDefinition {
            name: "bash".into(),
            description: "Run shell".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
            category: "builtin".into(),
        }];

        let schema = build_strict_response_schema(&tools);
        let tc_schema = &schema["properties"]["tool_calls"];

        // Single tool: no anyOf wrapper, just the single branch as items
        assert!(tc_schema["items"]["anyOf"].is_null());
        assert_eq!(tc_schema["items"]["properties"]["name"]["const"], "bash");
    }

    #[test]
    fn test_parse_structured_output_valid() {
        let json = r#"{"reasoning": "Need to read config", "tool_calls": [{"name": "read", "arguments": {"path": "config.toml"}}], "content": ""}"#;
        let (calls, text) = parse_structured_output(json).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read");
        assert_eq!(calls[0].arguments["path"], "config.toml");
        assert_eq!(text, "");
    }

    #[test]
    fn test_parse_structured_output_no_tools() {
        let json = r#"{"content": "Hello world"}"#;
        let (calls, text) = parse_structured_output(json).unwrap();
        assert!(calls.is_empty());
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_parse_structured_output_multiple_tools() {
        let json = r#"{"tool_calls": [{"name": "read", "arguments": {"path": "a.txt"}}, {"name": "write", "arguments": {"path": "b.txt", "content": "hi"}}], "content": ""}"#;
        let (calls, text) = parse_structured_output(json).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "read");
        assert_eq!(calls[1].name, "write");
        assert_eq!(text, "");
    }

    #[test]
    fn test_parse_structured_output_invalid_json() {
        assert!(parse_structured_output("not json").is_none());
        assert!(parse_structured_output("{}").is_none()); // missing content field
    }

    #[test]
    fn test_build_request_body_strict_mode() {
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: "Read".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
            category: "builtin".into(),
        }];

        let request = LLMRequest {
            model: "gpt-4o".into(),
            messages: vec![LLMMessage {
                role: "user".into(),
                content: Arc::new("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            strict_mode: true,
            tools: Some(tools),
            ..Default::default()
        };

        let body = build_request_body(&request);

        // Should NOT have tools array
        assert!(body.get("tools").is_none());
        // Should have response_format with json_schema
        let rf = body["response_format"].as_object().unwrap();
        assert_eq!(rf["type"], "json_schema");
        let js = rf["json_schema"].as_object().unwrap();
        assert_eq!(js["name"], "volt_tool_calls");
        assert_eq!(js["strict"], true);
        assert!(js["schema"].is_object());
        // Should have tool_choice: none
        assert_eq!(body["tool_choice"], "none");
    }

    #[test]
    fn test_build_request_body_non_strict_mode() {
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: "Read".into(),
            input_schema: serde_json::json!({ "type": "object" }),
            category: "builtin".into(),
        }];

        let request = LLMRequest {
            model: "gpt-4o".into(),
            messages: vec![LLMMessage {
                role: "user".into(),
                content: Arc::new("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            strict_mode: false,
            tools: Some(tools),
            ..Default::default()
        };

        let body = build_request_body(&request);

        // Should have tools array
        assert!(body["tools"].is_array());
        // Should NOT have response_format json_schema
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn test_build_strict_response_schema_injects_additional_properties_false() {
        let tools = vec![ToolDefinition {
            name: "write".into(),
            description: "Write".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
            category: "builtin".into(),
        }];

        let schema = build_strict_response_schema(&tools);
        let tc = &schema["properties"]["tool_calls"];
        let branch = &tc["items"];

        // Top-level branch enforces additionalProperties: false
        assert_eq!(branch["additionalProperties"], false);
        // The arguments schema should also have additionalProperties: false injected
        let args = &branch["properties"]["arguments"];
        assert_eq!(args["additionalProperties"], false);
    }

    #[test]
    fn test_build_strict_response_schema_exact_tool_names_as_const() {
        let tools = vec![
            ToolDefinition {
                name: "read".into(),
                description: "Read".into(),
                input_schema: serde_json::json!({"type": "object"}),
                category: "builtin".into(),
            },
            ToolDefinition {
                name: "write".into(),
                description: "Write".into(),
                input_schema: serde_json::json!({"type": "object"}),
                category: "builtin".into(),
            },
        ];

        let schema = build_strict_response_schema(&tools);
        let any_of = schema["properties"]["tool_calls"]["items"]["anyOf"]
            .as_array()
            .unwrap();

        assert_eq!(any_of.len(), 2);
        // Verify exact const values for tool names
        assert_eq!(any_of[0]["properties"]["name"]["const"], "read");
        assert_eq!(any_of[1]["properties"]["name"]["const"], "write");
    }

    #[test]
    fn test_build_request_body_strict_mode_with_explicit_response_format_override() {
        // When both strict_mode and an explicit response_format are set,
        // the explicit response_format takes precedence (applied after strict mode).
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: "Read".into(),
            input_schema: serde_json::json!({"type": "object"}),
            category: "builtin".into(),
        }];

        let request = LLMRequest {
            model: "gpt-4o".into(),
            messages: vec![LLMMessage {
                role: "user".into(),
                content: Arc::new("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            strict_mode: true,
            tools: Some(tools),
            response_format: Some(crate::models::ResponseFormat::JsonObject),
            ..Default::default()
        };

        let body = build_request_body(&request);
        // Explicit response_format overrides strict mode's json_schema
        assert_eq!(body["response_format"]["type"], "json_object");
        // tool_choice from strict mode should still be present
        assert_eq!(body["tool_choice"], "none");
    }

    #[test]
    fn test_build_request_body_strict_mode_empty_tools() {
        let request = LLMRequest {
            model: "gpt-4o".into(),
            messages: vec![LLMMessage {
                role: "user".into(),
                content: Arc::new("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            strict_mode: true,
            tools: Some(vec![]),
            ..Default::default()
        };

        let body = build_request_body(&request);
        // Even with empty tools, strict mode still generates a response_format
        // (tool_calls schema has an empty anyOf array)
        let rf = body["response_format"].as_object().unwrap();
        assert_eq!(rf["type"], "json_schema");
        let js = rf["json_schema"].as_object().unwrap();
        assert_eq!(js["name"], "volt_tool_calls");
        // tool_choice: none is still injected
        assert_eq!(body["tool_choice"], "none");
    }
}
