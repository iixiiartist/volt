use crate::models::{AudioRequest, AudioResponse, LLMRequest, LLMResponse, TtsRequest};
use async_trait::async_trait;
use std::sync::Arc;

pub type TokenCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Default timeout for LLM HTTP requests. The agent loop in `agent/run.rs` is
/// 300s per iteration, so requests must complete within that budget.
pub const LLM_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Timeout for short, bounded LLM sub-requests (status polls, async-result probes).
pub const LLM_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Number of retry iterations when polling a long-running operation.
pub const LLM_POLL_MAX_ITERATIONS: u32 = 60;

/// Interval between poll iterations.
pub const LLM_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Default `max_tokens` for LLM requests when the caller doesn't specify.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Default sampling temperature.
pub const DEFAULT_TEMPERATURE: f32 = 0.7;

/// Timeout for audio synthesis / recognition HTTP requests (Riva, Whisper).
pub const AUDIO_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn complete(&self, request: &LLMRequest) -> anyhow::Result<LLMResponse>;

    async fn complete_stream(
        &self,
        request: &LLMRequest,
        on_token: TokenCallback,
    ) -> anyhow::Result<LLMResponse>;

    fn name(&self) -> &str;
    fn supported_models(&self) -> Vec<String>;

    /// Transcribe audio to text (speech-to-text). Default: returns error.
    async fn transcribe(&self, _audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        anyhow::bail!("audio transcription not supported by this provider")
    }

    /// Translate audio to English text. Default: returns error.
    async fn translate(&self, _audio: &AudioRequest) -> anyhow::Result<AudioResponse> {
        anyhow::bail!("audio translation not supported by this provider")
    }

    /// Synthesize text to speech. Default: returns error.
    async fn synthesize(&self, _tts: &TtsRequest) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("text-to-speech not supported by this provider")
    }
}
