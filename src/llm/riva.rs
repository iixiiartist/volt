use crate::llm::provider::{TokenCallback, AUDIO_HTTP_TIMEOUT};
use crate::llm::LLMProvider;
use crate::models::{AudioRequest, AudioResponse, LLMRequest, LLMResponse, TtsRequest};
use async_trait::async_trait;

/// NVIDIA Riva speech/audio provider.
/// Supports speech-to-text (transcribe), speech-to-text translate, and text-to-speech (synthesize).
/// Chat completion methods return errors — Riva is an audio-only API.
pub struct RivaProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl RivaProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            http: crate::http_client().clone(),
            api_key,
            base_url: "https://riva.api.nvidia.com/v1".into(),
        }
    }

    pub fn new_with_base(api_key: String, base_url: String) -> Self {
        Self {
            http: crate::http_client().clone(),
            api_key,
            base_url,
        }
    }
}

#[async_trait]
impl LLMProvider for RivaProvider {
    fn name(&self) -> &str {
        "riva"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["riva-tts".into(), "riva-stt".into()]
    }

    async fn complete(&self, _request: &LLMRequest) -> anyhow::Result<LLMResponse> {
        anyhow::bail!("Riva provider does not support chat completions (audio/speech only)")
    }

    async fn complete_stream(
        &self,
        _request: &LLMRequest,
        _on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse> {
        anyhow::bail!(
            "Riva provider does not support streaming chat completions (audio/speech only)"
        )
    }

    async fn transcribe(&self, audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        let url = format!("{}/speech/recognize", self.base_url.trim_end_matches('/'));
        let mime = match audio
            .file_name
            .rsplit('.')
            .next()
            .unwrap_or("wav")
            .to_lowercase()
            .as_str()
        {
            "flac" => "audio/flac",
            "mp3" | "mpga" => "audio/mpeg",
            "mp4" | "m4a" => "audio/mp4",
            "ogg" => "audio/ogg",
            "webm" => "audio/webm",
            _ => "audio/wav",
        };

        let form = reqwest::multipart::Form::new()
            .part(
                "audio",
                reqwest::multipart::Part::bytes(audio.file_data.clone())
                    .file_name(audio.file_name.clone())
                    .mime_str(mime)
                    .unwrap_or_else(|_| {
                        reqwest::multipart::Part::bytes(audio.file_data.clone())
                            .file_name(audio.file_name.clone())
                    }),
            )
            .text(
                "config",
                serde_json::json!({
                    "encoding": "LINEAR_PCM",
                    "sample_rate_hertz": 16000,
                    "language_code": audio.language.as_deref().unwrap_or("en-US"),
                    "max_alternatives": 1,
                    "profanity_filter": false,
                })
                .to_string(),
            );

        let mut req = self.http.post(&url).multipart(form);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp_val = req.timeout(AUDIO_HTTP_TIMEOUT).send().await?;

        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("Riva transcribe HTTP {}: {}", status.as_u16(), trunc);
        }

        let resp: serde_json::Value = resp_val.json().await?;
        let text = resp["results"]
            .as_array()
            .and_then(|r| r.first())
            .and_then(|alt| alt["alternatives"].as_array())
            .and_then(|a| a.first())
            .and_then(|t| t["transcript"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(AudioResponse {
            text,
            x_groq: None,
            segments: None,
            task: Some("transcribe".into()),
            language: audio.language.clone(),
            duration: None,
        })
    }

    async fn translate(&self, audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        // Riva does not have a separate translate endpoint; delegate to transcribe
        self.transcribe(audio).await
    }

    async fn synthesize(&self, tts: &TtsRequest) -> anyhow::Result<Vec<u8>> {
        let url = format!("{}/speech/synthesis", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "text": tts.input,
            "voice": tts.voice,
            "language_code": "en-US",
            "sample_rate_hertz": tts.sample_rate.unwrap_or(24000),
            "encoding": "LINEAR_PCM",
            "audio_config": {
                "audio_encoding": tts.response_format.as_deref().unwrap_or("LINEAR_PCM"),
                "sample_rate_hertz": tts.sample_rate.unwrap_or(24000),
            }
        });

        let mut req = self.http.post(&url).json(&body);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp_val = req.timeout(AUDIO_HTTP_TIMEOUT).send().await?;

        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("Riva synthesize HTTP {}: {}", status.as_u16(), trunc);
        }

        Ok(resp_val.bytes().await?.to_vec())
    }
}
