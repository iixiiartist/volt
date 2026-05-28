#[cfg(feature = "tools-local-embeddings")]
use tract_onnx::prelude::*;

#[cfg(feature = "tools-local-embeddings")]
use tokenizers::Tokenizer;

/// Local embedding engine using tract (pure Rust ONNX inference).
/// Requires the `tools-local-embeddings` feature flag.
///
/// Default model: Xenova/bge-large-en-v1.5 (1024d, BERT-large architecture)
/// Downloads ~337MB (int8 quantized ONNX) + tokenizer on first use
/// (cached to ~/.cache/huggingface).
///
/// Override with:
///   VOLT_ONNX_MODEL_DIR  — path to directory with model.onnx + tokenizer.json
///   EMBEDDING_MODEL      — HuggingFace model ID (default: Xenova/bge-large-en-v1.5)
#[cfg(feature = "tools-local-embeddings")]
type TractPlan = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

#[cfg(feature = "tools-local-embeddings")]
pub struct LocalEmbedder {
    model: TractPlan,
    tokenizer: Tokenizer,
}

#[cfg(feature = "tools-local-embeddings")]
impl LocalEmbedder {
    /// Load an ONNX embedding model from local path or HuggingFace hub.
    pub fn load() -> anyhow::Result<Self> {
        let model_dir = resolve_model_dir()?;
        let onnx_path = find_onnx_file(&model_dir)?;

        let model = tract_onnx::onnx()
            .model_for_path(&onnx_path)?
            .into_optimized()?
            .into_runnable()?;

        let tokenizer_path = model_dir.join("tokenizer.json");
        if !tokenizer_path.exists() {
            anyhow::bail!(
                "tokenizer.json not found at {}. Download the full model directory.",
                tokenizer_path.display()
            );
        }
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer load: {}", e))?;

        tracing::info!(
            "ONNX embedder loaded: {} ({})",
            onnx_path.file_name().unwrap_or_default().to_string_lossy(),
            model_dir.display(),
        );

        Ok(Self { model, tokenizer })
    }

    /// Embed text to a 1024-dimensional vector (mean pooling + L2 normalize).
    pub fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenize: {}", e))?;

        let seq_len = encoding.len();
        let token_ids: Vec<i64> = encoding.get_ids().iter().map(|&v| v as i64).collect();
        let attn_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&v| v as i64)
            .collect();
        let token_type: Vec<i64> = vec![0i64; seq_len];

        let input_ids_arr = ndarray::Array2::from_shape_vec((1, seq_len), token_ids)?;
        let attn_mask_arr = ndarray::Array2::from_shape_vec((1, seq_len), attn_mask.clone())?;
        let token_type_arr = ndarray::Array2::from_shape_vec((1, seq_len), token_type)?;

        // Build inputs for BERT encoder: [input_ids, attention_mask, token_type_ids]
        let mut inputs: tract_onnx::prelude::TVec<TValue> = Default::default();
        inputs.push(Tensor::from(input_ids_arr.into_dyn()).into());
        inputs.push(Tensor::from(attn_mask_arr.into_dyn()).into());
        inputs.push(Tensor::from(token_type_arr.into_dyn()).into());

        let outputs = self.model.run(inputs)?;

        // Find the last_hidden_state output (BERT encoder output)
        let last_hidden = find_last_hidden_state(&outputs)?;
        let shape = last_hidden.shape();
        if shape.len() != 3 || shape[0] != 1 || shape[2] != 1024 {
            anyhow::bail!(
                "unexpected last_hidden_state shape: {:?}, expected (1, _, 1024)",
                shape
            );
        }
        let seq = shape[1];
        let hidden = shape[2];

        // Mean pooling: average over non-padded tokens
        let mask_sum: i64 = attn_mask.iter().sum();
        if mask_sum == 0 {
            anyhow::bail!("empty input after tokenization");
        }

        let mut pooled = vec![0.0f32; hidden];
        for t in 0..seq {
            let scale = attn_mask[t] as f32 / mask_sum as f32;
            for h in 0..hidden {
                pooled[h] += last_hidden[[0, t, h]] * scale;
            }
        }

        // L2 normalize
        let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut pooled {
                *v /= norm;
            }
        }

        Ok(pooled)
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

// ─── Helpers (feature-gated) ──────────────────────────────────────

#[cfg(feature = "tools-local-embeddings")]
fn resolve_model_dir() -> anyhow::Result<std::path::PathBuf> {
    // 1. Explicit local path via env var
    if let Ok(dir) = std::env::var("VOLT_ONNX_MODEL_DIR") {
        let p = std::path::PathBuf::from(&dir);
        if p.exists() {
            return Ok(p);
        }
        anyhow::bail!("VOLT_ONNX_MODEL_DIR set to {} but directory not found", dir);
    }

    // 2. Default: download from HuggingFace hub via hf-hub
    let model_id =
        std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "Xenova/bge-large-en-v1.5".into());

    let api = hf_hub::api::sync::Api::new()?;
    let repo = api.model(model_id);

    // Download tokenizer.json first
    let tokenizer_path = repo.get("tokenizer.json")?;
    let model_dir = tokenizer_path
        .parent()
        .unwrap_or(&tokenizer_path)
        .to_path_buf();

    // Download the ONNX model file
    let _onnx_path = find_onnx_in_repo(&repo)?;

    Ok(model_dir)
}

#[cfg(feature = "tools-local-embeddings")]
fn find_onnx_file(dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let candidates = [
        dir.join("onnx/model_quantized.onnx"),
        dir.join("onnx/model_int8.onnx"),
        dir.join("onnx/model_q4f16.onnx"),
        dir.join("onnx/model.onnx"),
        dir.join("model.onnx"),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    anyhow::bail!(
        "no ONNX model file found in {}. Expected one of: model_quantized.onnx, model_int8.onnx, model.onnx (in onnx/ or root)",
        dir.display()
    )
}

#[cfg(feature = "tools-local-embeddings")]
fn find_onnx_in_repo(repo: &hf_hub::api::sync::ApiRepo) -> anyhow::Result<std::path::PathBuf> {
    let onnx_variants = [
        "onnx/model_quantized.onnx",
        "onnx/model_int8.onnx",
        "onnx/model_q4f16.onnx",
        "onnx/model.onnx",
        "model.onnx",
    ];
    for variant in &onnx_variants {
        if let Ok(path) = repo.get(variant) {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "could not download ONNX model (tried: {})",
        onnx_variants.join(", ")
    )
}

#[cfg(feature = "tools-local-embeddings")]
fn find_last_hidden_state<'a>(
    outputs: &'a tract_onnx::prelude::TVec<TValue>,
) -> anyhow::Result<ndarray::ArrayViewD<'a, f32>> {
    for output in outputs.iter() {
        if let Ok(view) = output.to_array_view::<f32>() {
            if view.shape().len() == 3 {
                return Ok(view);
            }
        }
    }
    anyhow::bail!("no 3D (batch, seq, hidden) float32 output tensor found in model outputs")
}
