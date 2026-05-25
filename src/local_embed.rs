#[cfg(feature = "tools-local-embeddings")]
use candle_core::{Device, Tensor};
#[cfg(feature = "tools-local-embeddings")]
use candle_transformers::models::bert::{BertModel, Config};
#[cfg(feature = "tools-local-embeddings")]
use tokenizers::Tokenizer;

/// Local embedding engine using HuggingFace candle.
/// Requires the `tools-local-embeddings` feature flag.
/// Downloads ~130MB BGE-small-en-v1.5 model on first use (cached to ~/.cache/huggingface).
#[cfg(feature = "tools-local-embeddings")]
pub struct LocalEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

#[cfg(feature = "tools-local-embeddings")]
impl LocalEmbedder {
    /// Load BGE-small-en-v1.5 from HuggingFace hub. Cached after first download.
    pub fn load() -> anyhow::Result<Self> {
        let device = Device::Cpu;
        let api = hf_hub::api::sync::Api::new()?;
        let repo = api.model("BAAI/bge-small-en-v1.5".to_string());
        let tokenizer_path = repo.get("tokenizer.json")?;
        let model_path = repo.get("model.safetensors")?;
        let config_path = repo.get("config.json")?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer load: {}", e))?;

        let config: Config = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
        let vb = candle_nn::VarBuilder::from_pth(&model_path)?;
        let model = BertModel::new(&device, &config, vb)?;

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    /// Embed text to a 384-dimensional vector (mean pooling).
    pub fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let tokens = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenize: {}", e))?;
        let token_ids = Tensor::new(tokens.get_ids(), &self.device)?;
        let token_type_ids = Tensor::new(tokens.get_type_ids(), &self.device)?;
        let attention_mask = Tensor::new(tokens.get_attention_mask(), &self.device)?;

        let output = self.model.forward(
            &token_ids.unsqueeze(0)?,
            &token_type_ids.unsqueeze(0)?,
            Some(&attention_mask.unsqueeze(0)?),
        )?;

        // Mean pooling
        let pooled = output.mean(1)?;
        let normalized = pooled.broadcast_div(&pooled.sqr()?.sum(1)?.sqrt()?.unsqueeze(1)?)?;
        Ok(normalized.to_vec1()?)
    }
}

/// Stub for when feature is disabled.
#[cfg(not(feature = "tools-local-embeddings"))]
pub struct LocalEmbedder;

#[cfg(not(feature = "tools-local-embeddings"))]
impl LocalEmbedder {
    pub fn load() -> anyhow::Result<Self> {
        anyhow::bail!("local embeddings not compiled (enable tools-local-embeddings feature)")
    }
}
