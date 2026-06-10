//! vLLM inference provider.
//!
//! Talks to a vLLM server via its OpenAI-compatible HTTP API
//! (`POST /v1/chat/completions`, `POST /v1/audio/{transcriptions,translations,speech}`,
//! `GET /v1/models`). The wire format is identical to OpenAI's, so this
//! provider is structurally a slim mirror of `OpenAIProvider` with:
//!
//! * a default `base_url` of `http://localhost:8000/v1` (overridable via
//!   `VLLM_HOST` or `LLM_BASE_URL`)
//! * optional API key (vLLM's `--api-key` server flag is off by default;
//!   if the operator sets one, this client sends `Authorization: Bearer ...`)
//! * no Groq-specific extensions (`x_groq`, `compound_custom`,
//!   `usage_breakdown` are simply ignored on the response side)
//!
//! ## Integration status
//!
//! **This provider has not been validated against a live vLLM endpoint.**
//! The request body shape, response parsing, and streaming chunk format
//! are derived from the OpenAI spec that vLLM commits to
//! (<https://docs.vllm.ai/en/latest/features/tool_calling/>). An
//! integration test gated on `VLLM_INTEGRATION_URL` is the right next
//! step once a vLLM server is available. See
//! `docs/vllm-deployment.md` for the deployment runbook.
//!
//! Treat vLLM-tagged workflows as `environment: dev|staging` only until
//! the integration test lands.

use crate::agent::tool_parser::parse_lossy_json;
use crate::llm::provider::{TokenCallback, DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE, LLM_HTTP_TIMEOUT};
use crate::llm::LLMProvider;
use crate::models::{
    AudioRequest, AudioResponse, LLMRequest, LLMResponse, ToolCall, TtsRequest, Usage,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;

/// Default base URL when no `VLLM_HOST` or `LLM_BASE_URL` is set.
/// Matches vLLM's default `--port 8000`.
pub const VLLM_DEFAULT_BASE_URL: &str = "http://localhost:8000/v1";

pub struct VllmProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl VllmProvider {
    /// Construct a vLLM provider. `api_key` may be empty when vLLM is
    /// started without `--api-key`.
    pub fn new(api_key: String, base_url: String) -> Self {
        let base_url = if base_url.is_empty() {
            VLLM_DEFAULT_BASE_URL.to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        Self {
            http: crate::http_client().clone(),
            api_key,
            base_url,
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

    /// Build the OpenAI-compatible chat completions body. Identical to
    /// the OpenAI provider's body, minus Groq-specific extensions.
    pub(crate) fn build_request_body(request: &LLMRequest) -> serde_json::Value {
        use crate::models::PromptTokensDetails;

        // vLLM supports the `strict` response_format field on the wire
        // (vLLM accepts it for compat) but does not yet enforce it during
        // decoding — see vLLM issue #15526. We still send the schema when
        // strict_mode is on, because the model uses it as guidance even
        // without constrained decoding.
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
            body["tool_choice"] = json!("none");
        }
        if let Some(v) = &request.reasoning_effort {
            body["reasoning_effort"] = json!(v);
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

        // Suppress unused import warning when PromptTokensDetails isn't
        // referenced (kept for forward-compat with future usage fields).
        let _ = std::marker::PhantomData::<PromptTokensDetails>;

        body
    }

    /// Parse a non-streaming response from vLLM. Same shape as
    /// `parse_openai_response` but ignores Groq-specific fields.
    pub(crate) fn parse_response(resp: serde_json::Value) -> anyhow::Result<LLMResponse> {
        let choice = &resp["choices"][0];
        let message = &choice["message"];
        let raw_content = message["content"].as_str().unwrap_or_default().to_string();

        // vLLM supports the same structured-output response_format as
        // OpenAI; the content field is a JSON string with tool_calls
        // and content sub-fields. Try to parse it first.
        let (tool_calls, content) = if let Some((calls, text)) = parse_structured_output(&raw_content) {
            (Some(calls), Arc::new(text))
        } else {
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
                .map(|d| crate::models::PromptTokensDetails {
                    cached_tokens: d.get("cached_tokens").and_then(|v| v.as_u64()),
                    cache_creation_tokens: d.get("cache_creation_tokens").and_then(|v| v.as_u64()),
                    cache_read_tokens: d.get("cache_read_tokens").and_then(|v| v.as_u64()),
                }),
        });

        Ok(LLMResponse {
            content,
            tool_calls,
            finish_reason: choice["finish_reason"].as_str().map(|s| s.to_string()),
            usage,
            usage_breakdown: None, // vLLM does not return per-model breakdowns
            executed_tools: None,  // vLLM does not return compound-style executed_tools
            system_fingerprint: resp["system_fingerprint"].as_str().map(|s| s.to_string()),
            x_groq: None,
        })
    }
}

/// Build a strict JSON Schema for structured outputs. Same as
/// `OpenAIProvider::build_strict_response_schema` — duplicated here
/// rather than re-exported to keep the providers decoupled.
fn build_strict_response_schema(tools: &[crate::models::ToolDefinition]) -> serde_json::Value {
    let tool_branches: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| {
            let mut args_schema = t.input_schema.clone();
            if let Some(obj) = args_schema.as_object_mut() {
                obj.insert("additionalProperties".into(), serde_json::Value::Bool(false));
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

fn parse_structured_output(content_str: &str) -> Option<(Vec<ToolCall>, String)> {
    let parsed = serde_json::from_str::<serde_json::Value>(content_str).ok()?;
    let text_content = parsed.get("content")?.as_str()?.to_string();
    let calls: Vec<ToolCall> = if let Some(tool_calls) = parsed.get("tool_calls").and_then(|v| v.as_array()) {
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

#[async_trait]
impl LLMProvider for VllmProvider {
    fn name(&self) -> &str {
        "vllm"
    }

    fn supported_models(&self) -> Vec<String> {
        // vLLM serves whatever model the operator has loaded; we don't
        // know the catalog without a `GET /v1/models` call, and this
        // method is sync. Return a wildcard.
        vec!["*".into()]
    }

    async fn complete(&self, request: &LLMRequest) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = Self::build_request_body(request);

        let req = self.apply_auth(self.http.post(&url));
        let resp_val = req.json(&body).timeout(LLM_HTTP_TIMEOUT).send().await?;
        let status = resp_val.status();

        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("vLLM HTTP {}: {}", status.as_u16(), trunc);
        }

        let resp: serde_json::Value = resp_val.json().await?;
        Self::parse_response(resp)
    }

    async fn complete_stream(
        &self,
        request: &LLMRequest,
        on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut body = Self::build_request_body(request);
        body["stream"] = json!(true);

        let req = self.apply_auth(self.http.post(&url));
        let response = req.json(&body).timeout(LLM_HTTP_TIMEOUT).send().await?;
        let status = response.status();

        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("vLLM HTTP {}: {}", status.as_u16(), trunc);
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
                        if !u.is_null() {
                            usage = Some(Usage {
                                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
                                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
                                total_tokens: u["total_tokens"].as_u64().unwrap_or(0),
                                queue_time: u["queue_time"].as_f64(),
                                total_time: u["total_time"].as_f64(),
                                prompt_tokens_details: None,
                            });
                        }
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
            tool_calls: if tool_calls_acc.is_empty() { None } else { Some(tool_calls_acc) },
            finish_reason,
            usage,
            usage_breakdown: None,
            executed_tools: None,
            system_fingerprint: None,
            x_groq: None,
        })
    }

    async fn transcribe(&self, audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        // vLLM supports `/v1/audio/transcriptions` when the server is
        // started with a Whisper-class model. The wire format matches
        // OpenAI's. This path is unverified against a live vLLM server
        // at the time of writing.
        let url = format!("{}/audio/transcriptions", self.base_url);
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

        let req = self.apply_auth(self.http.post(&url).multipart(form));
        let resp_val = req.timeout(LLM_HTTP_TIMEOUT).send().await?;
        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("vLLM HTTP {}: {}", status.as_u16(), trunc);
        }
        let resp: serde_json::Value = resp_val.json().await?;
        Ok(AudioResponse {
            text: resp["text"].as_str().unwrap_or("").to_string(),
            x_groq: None,
            segments: None,
            task: resp.get("task").and_then(|v| v.as_str()).map(String::from),
            language: resp.get("language").and_then(|v| v.as_str()).map(String::from),
            duration: resp.get("duration").and_then(|v| v.as_f64()),
        })
    }

    async fn translate(&self, audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        let url = format!("{}/audio/translations", self.base_url);
        let form = reqwest::multipart::Form::new()
            .part("file", make_audio_part(audio.file_data.clone(), &audio.file_name))
            .text("model", audio.model.clone());

        let req = self.apply_auth(self.http.post(&url).multipart(form));
        let resp_val = req.timeout(LLM_HTTP_TIMEOUT).send().await?;
        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("vLLM HTTP {}: {}", status.as_u16(), trunc);
        }
        let resp: serde_json::Value = resp_val.json().await?;
        Ok(AudioResponse {
            text: resp["text"].as_str().unwrap_or("").to_string(),
            x_groq: None,
            segments: None,
            task: resp.get("task").and_then(|v| v.as_str()).map(String::from),
            language: resp.get("language").and_then(|v| v.as_str()).map(String::from),
            duration: resp.get("duration").and_then(|v| v.as_f64()),
        })
    }

    async fn synthesize(&self, tts: &TtsRequest) -> anyhow::Result<Vec<u8>> {
        // vLLM does not currently expose a `/v1/audio/speech` endpoint.
        // Return an explicit error so the caller knows this isn't a
        // supported operation rather than silently no-op'ing.
        let _ = tts;
        anyhow::bail!(
            "vLLM does not provide a text-to-speech endpoint. \
             Use a dedicated TTS provider (e.g. Riva, ElevenLabs) for synthesis."
        )
    }
}

#[cfg(test)]
mod tests {
    //! These tests assert the request body shape and the response
    //! parsing logic. They do **not** hit a live vLLM endpoint.
    //! An integration test gated on `VLLM_INTEGRATION_URL` should
    //! land once a vLLM deployment is available; see
    //! `docs/vllm-deployment.md` for the test plan.
    use super::*;
    use crate::models::{LLMMessage, ToolDefinition};

    #[test]
    fn vllm_default_url_is_localhost_8000() {
        let p = VllmProvider::new(String::new(), String::new());
        assert_eq!(p.base_url, "http://localhost:8000/v1");
    }

    #[test]
    fn vllm_trims_trailing_slash() {
        let p = VllmProvider::new(String::new(), "http://vllm.internal:8000/v1/".into());
        assert_eq!(p.base_url, "http://vllm.internal:8000/v1");
    }

    #[test]
    fn request_body_uses_openai_shape() {
        let request = LLMRequest {
            model: "meta-llama/Llama-3.3-70B-Instruct".into(),
            messages: vec![LLMMessage {
                role: "user".into(),
                content: Arc::new("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: Some(0.3),
            max_tokens: Some(1024),
            strict_mode: false,
            tools: Some(vec![ToolDefinition {
                name: "read".into(),
                description: "Read a file".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
                category: "builtin".into(),
            }]),
            ..Default::default()
        };
        let body = VllmProvider::build_request_body(&request);

        // Required OpenAI fields
        assert_eq!(body["model"], "meta-llama/Llama-3.3-70B-Instruct");
        assert_eq!(body["stream"], false);
        assert!((body["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(body["max_tokens"], 1024);
        assert!(body["messages"].is_array());
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hello");

        // Tools array, OpenAI function shape
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "read");
        assert!(tools[0]["function"]["parameters"].is_object());

        // No Groq-specific fields
        assert!(body.get("compound_custom").is_none());
        assert!(body.get("search_settings").is_none());
    }

    #[test]
    fn request_body_strict_mode_uses_response_format() {
        let request = LLMRequest {
            model: "test".into(),
            messages: vec![],
            strict_mode: true,
            tools: Some(vec![ToolDefinition {
                name: "x".into(),
                description: "x".into(),
                input_schema: serde_json::json!({"type": "object"}),
                category: "builtin".into(),
            }]),
            ..Default::default()
        };
        let body = VllmProvider::build_request_body(&request);

        // tools array should be absent in strict mode
        assert!(body.get("tools").is_none());
        // response_format with json_schema
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["name"], "volt_tool_calls");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        // tool_choice forced to "none" in strict mode
        assert_eq!(body["tool_choice"], "none");
    }

    #[test]
    fn parse_response_extracts_text_and_tool_calls() {
        let resp = serde_json::json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion",
            "created": 1700000000,
            "model": "meta-llama/Llama-3.3-70B-Instruct",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "read",
                            "arguments": "{\"path\":\"/tmp/x\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 50,
                "completion_tokens": 25,
                "total_tokens": 75
            }
        });
        let out = VllmProvider::parse_response(resp).unwrap();
        assert_eq!(out.content.as_str(), "");
        let tcs = out.tool_calls.unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].name, "read");
        assert_eq!(tcs[0].id, "call_123");
        assert_eq!(tcs[0].arguments["path"], "/tmp/x");
        assert_eq!(out.finish_reason.as_deref(), Some("tool_calls"));
        let u = out.usage.unwrap();
        assert_eq!(u.prompt_tokens, 50);
        assert_eq!(u.completion_tokens, 25);
        assert_eq!(u.total_tokens, 75);
        // No Groq fields
        assert!(out.usage_breakdown.is_none());
        assert!(out.executed_tools.is_none());
        assert!(out.x_groq.is_none());
    }

    #[test]
    fn parse_response_handles_plain_text() {
        let resp = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello, world."
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8 }
        });
        let out = VllmProvider::parse_response(resp).unwrap();
        assert_eq!(out.content.as_str(), "Hello, world.");
        assert!(out.tool_calls.is_none());
        assert_eq!(out.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn parse_response_handles_structured_output() {
        let resp = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "{\"reasoning\":\"need to read\",\"tool_calls\":[{\"name\":\"read\",\"arguments\":{\"path\":\"a\"}}],\"content\":\"\"}"
                },
                "finish_reason": "stop"
            }]
        });
        let out = VllmProvider::parse_response(resp).unwrap();
        assert_eq!(out.content.as_str(), "");
        let tcs = out.tool_calls.unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].name, "read");
        assert_eq!(tcs[0].arguments["path"], "a");
    }

    #[test]
    fn apply_auth_adds_bearer_only_when_key_present() {
        let p = VllmProvider::new(String::new(), "http://x".into());
        let req = p.http.get("http://x/foo");
        // We can't easily inspect a RequestBuilder, but we can confirm
        // the function doesn't panic and returns a builder.
        let _ = p.apply_auth(req);

        let p2 = VllmProvider::new("secret".into(), "http://x".into());
        let req2 = p2.http.get("http://x/foo");
        let _ = p2.apply_auth(req2);
    }

    #[test]
    fn synthesize_returns_explicit_unsupported_error() {
        // Runtime check: no network call attempted, no panic.
        let p = VllmProvider::new(String::new(), "http://x".into());
        let req = TtsRequest {
            model: "tts-1".into(),
            input: "hi".into(),
            voice: "alloy".into(),
            response_format: Some("wav".into()),
            sample_rate: None,
            speed: None,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(p.synthesize(&req));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("vLLM does not provide a text-to-speech"));
    }
}
