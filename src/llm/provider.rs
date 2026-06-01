use crate::models::{AudioRequest, AudioResponse, LLMRequest, LLMResponse, TtsRequest};
use async_trait::async_trait;
use std::sync::Arc;

pub type TokenCallback = Arc<dyn Fn(&str) + Send + Sync>;

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
