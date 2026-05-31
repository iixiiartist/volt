#[cfg(feature = "tools-local-embeddings")]
use std::sync::Mutex;

#[cfg(feature = "tools-local-embeddings")]
use ort::ep::{CPU, CUDA, DirectML, OpenVINO};

#[cfg(feature = "tools-local-embeddings")]
use ort::session::builder::GraphOptimizationLevel;

#[cfg(feature = "tools-local-embeddings")]
use ort::session::Session;

#[cfg(feature = "tools-local-embeddings")]
use ort::value::{DynTensorValueType, Tensor};

#[cfg(feature = "tools-local-embeddings")]
use ndarray::Array2;

#[cfg(feature = "tools-local-embeddings")]
use tokenizers::Tokenizer;

/// Local embedding engine using ort (ONNX Runtime) with hardware-accelerated
/// Execution Provider fallback chain: OpenVINO → DirectML → CUDA → CPU.
///
/// Requires the `tools-local-embeddings` feature flag and MSVC toolchain.
///
/// Default model: Xenova/bge-small-en-v1.5 (384d, int8 quantized ONNX, ~60MB).
/// Output is padded to 1024d by normalize_dims().
///
/// Override with:
///   VOLT_ONNX_MODEL_DIR  — path to directory with model.onnx + tokenizer.json
///   EMBEDDING_MODEL      — HuggingFace model ID (default: Xenova/bge-small-en-v1.5)
#[cfg(feature = "tools-local-embeddings")]
pub struct LocalEmbedder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

#[cfg(feature = "tools-local-embeddings")]
impl LocalEmbedder {
    /// Load an ONNX embedding model from local path or HuggingFace hub.
    pub fn load() -> anyhow::Result<Self> {
        let model_dir = resolve_model_dir()?;
        let onnx_path = find_onnx_file(&model_dir)?;

        let tokenizer_path = model_dir.join("tokenizer.json");
        if !tokenizer_path.exists() {
            anyhow::bail!(
                "tokenizer.json not found at {}. Download the full model directory.",
                tokenizer_path.display()
            );
        }
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer load: {}", e))?;

        // Auto-configures the global ONNX Runtime environment if not yet set.
        let mut builder = Session::builder().map_err(ort_err)?
            .with_optimization_level(GraphOptimizationLevel::Level3).map_err(ort_err)?
            .with_intra_threads(2).map_err(ort_err)?
            .with_execution_providers([
                OpenVINO::default().build(),
                DirectML::default().build(),
                CUDA::default().build(),
                CPU::default().build(),
            ]).map_err(ort_err)?;
        let session = builder.commit_from_file(&onnx_path)?;

        tracing::info!(
            "ORT embedder loaded: {} ({}) — EP chain: OpenVINO → DirectML → CUDA → CPU",
            onnx_path.file_name().unwrap_or_default().to_string_lossy(),
            model_dir.display(),
        );

        // Log which EPs are available on this system
        log_ep_availability();

        Ok(Self { session: Mutex::new(session), tokenizer })
    }

    /// Same as `embed()` but runs on tokio's blocking thread pool
    /// so it doesn't starve the async runtime.
    pub async fn embed_async(self: std::sync::Arc<Self>, text: String) -> anyhow::Result<Vec<f32>> {
        tokio::task::spawn_blocking(move || self.embed(&text))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {}", e))?
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

        let input_ids = Array2::from_shape_vec((1, seq_len), token_ids)?;
        let attention_mask = Array2::from_shape_vec((1, seq_len), attn_mask.clone())?;
        let token_type_ids = Array2::from_shape_vec((1, seq_len), token_type)?;

        let input_ids_tensor = Tensor::from_array(input_ids)?;
        let attention_mask_tensor = Tensor::from_array(attention_mask)?;
        let token_type_ids_tensor = Tensor::from_array(token_type_ids)?;

        let (seq, hidden, data) = {
            let mut session_lock = self.session.lock().unwrap();
            let outputs = session_lock.run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor
            ])?;
            let last_hidden_dyn = outputs[0]
                .downcast_ref::<DynTensorValueType>()
                .map_err(|e| anyhow::anyhow!("output is not a tensor: {}", e))?;
            let last_hidden = last_hidden_dyn
                .try_extract_array::<f32>()
                .map_err(|e| anyhow::anyhow!("failed to extract f32 array: {}", e))?;

            let shape = last_hidden.shape();
            if shape.len() != 3 || shape[0] != 1 {
                anyhow::bail!(
                    "unexpected last_hidden_state shape: {:?}, expected (1, seq, hidden)",
                    shape
                );
            }
            let seq = shape[1];
            let hidden = shape[2];
            let data = last_hidden.as_slice().ok_or_else(|| {
                anyhow::anyhow!("tensor data is not contiguous")
            })?.to_vec();
            (seq, hidden, data)
        };

        let mask_sum: i64 = attn_mask.iter().sum();
        if mask_sum == 0 {
            anyhow::bail!("empty input after tokenization");
        }

        let mut pooled = vec![0.0f32; hidden];
        for t in 0..seq {
            let scale = attn_mask[t] as f32 / mask_sum as f32;
            for h in 0..hidden {
                pooled[h] += data[t * hidden + h] * scale;
            }
        }

        let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut pooled {
                *v /= norm;
            }
        }

        Ok(pooled)
    }

    /// Batch-embed multiple texts in a single ONNX forward pass.
    /// All texts are padded to the same sequence length.
    pub fn batch_embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let batch_size = texts.len();
        let mut encodings = Vec::with_capacity(batch_size);
        let mut per_seq_masks: Vec<Vec<i64>> = Vec::with_capacity(batch_size);
        for text in texts {
            let encoding = self
                .tokenizer
                .encode(text.as_str(), true)
                .map_err(|e| anyhow::anyhow!("tokenize: {}", e))?;
            let mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&v| v as i64)
                .collect();
            per_seq_masks.push(mask);
            encodings.push(encoding);
        }

        let max_len = encodings.iter().map(|e| e.len()).max().unwrap_or(1);

        let mut input_ids_data = Vec::with_capacity(batch_size * max_len);
        let mut attn_mask_data = Vec::with_capacity(batch_size * max_len);
        let mut token_type_data = Vec::with_capacity(batch_size * max_len);

        for encoding in &encodings {
            let seq_len = encoding.len();
            let ids: Vec<i64> = encoding
                .get_ids()
                .iter()
                .map(|&v| v as i64)
                .collect();
            let mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&v| v as i64)
                .collect();

            input_ids_data.extend(&ids);
            input_ids_data.extend(vec![0i64; max_len - seq_len]);
            attn_mask_data.extend(&mask);
            attn_mask_data.extend(vec![0i64; max_len - seq_len]);
            token_type_data.extend(vec![0i64; max_len]);
        }

        let input_ids = Array2::from_shape_vec((batch_size, max_len), input_ids_data)?;
        let attention_mask = Array2::from_shape_vec((batch_size, max_len), attn_mask_data)?;
        let token_type_ids = Array2::from_shape_vec((batch_size, max_len), token_type_data)?;

        let input_ids_tensor = Tensor::from_array(input_ids)?;
        let attention_mask_tensor = Tensor::from_array(attention_mask)?;
        let token_type_ids_tensor = Tensor::from_array(token_type_ids)?;

        let (max_seq, hidden, data) = {
            let mut session_lock = self.session.lock().unwrap();
            let outputs = session_lock.run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor
            ])?;
            let last_hidden_dyn = outputs[0]
                .downcast_ref::<DynTensorValueType>()
                .map_err(|e| anyhow::anyhow!("output is not a tensor: {}", e))?;
            let last_hidden = last_hidden_dyn
                .try_extract_array::<f32>()
                .map_err(|e| anyhow::anyhow!("failed to extract f32 array: {}", e))?;

            let shape = last_hidden.shape();
            if shape.len() != 3 || shape[0] != batch_size {
                anyhow::bail!(
                    "unexpected last_hidden_state shape: {:?}, expected ({}, seq, hidden)",
                    shape,
                    batch_size
                );
            }
            let max_seq = shape[1];
            let hidden = shape[2];
            let data = last_hidden.as_slice().ok_or_else(|| {
                anyhow::anyhow!("tensor data is not contiguous")
            })?.to_vec();
            (max_seq, hidden, data)
        };

        let mut results = Vec::with_capacity(batch_size);
        for b in 0..batch_size {
            let seq = encodings[b].len();
            let mask_sum: i64 = per_seq_masks[b].iter().sum();
            if mask_sum == 0 {
                anyhow::bail!("empty input at batch index {}", b);
            }
            let mut pooled = vec![0.0f32; hidden];
            for t in 0..seq {
                let scale = per_seq_masks[b][t] as f32 / mask_sum as f32;
                for h in 0..hidden {
                    pooled[h] += data[b * max_seq * hidden + t * hidden + h] * scale;
                }
            }
            let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut pooled {
                    *v /= norm;
                }
            }
            results.push(pooled);
        }

        Ok(results)
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
    if let Ok(dir) = std::env::var("VOLT_ONNX_MODEL_DIR") {
        let p = std::path::PathBuf::from(&dir);
        if p.exists() {
            return Ok(p);
        }
        anyhow::bail!("VOLT_ONNX_MODEL_DIR set to {} but directory not found", dir);
    }

    let model_id =
        std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "Xenova/bge-small-en-v1.5".into());

    let api = hf_hub::api::sync::Api::new()?;
    let repo = api.model(model_id);

    let tokenizer_path = repo.get("tokenizer.json")?;
    let model_dir = tokenizer_path
        .parent()
        .unwrap_or(&tokenizer_path)
        .to_path_buf();

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

/// Map non-Send BuilderResult errors to `anyhow::Error`.
#[cfg(feature = "tools-local-embeddings")]
fn ort_err<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow::anyhow!("ORT: {}", e)
}

/// Log which hardware execution providers are actually available by
/// checking loaded native DLLs via Win32 API (available on MSVC).
#[cfg(feature = "tools-local-embeddings")]
fn log_ep_availability() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleA(lpModuleName: *const i8) -> isize;
    }
    let check = |name: &str| {
        let cstr = std::ffi::CString::new(name).ok();
        match cstr {
            Some(s) => unsafe { GetModuleHandleA(s.as_ptr()) != 0 },
            None => false,
        }
    };
    let directml = check("DirectML.dll");
    let openvino = check("openvino.dll");
    let cuda = check("cudart64_12.dll");
    tracing::info!(
        "ORT EP availability — DirectML: {}, OpenVINO: {}, CUDA: {}",
        if directml { "YES" } else { "no" },
        if openvino { "YES" } else { "no" },
        if cuda { "YES" } else { "no" },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    #[cfg(feature = "tools-local-embeddings")]
    fn benchmark_ort_inference() {
        let start = Instant::now();
        let embedder = LocalEmbedder::load().expect("failed to load ONNX model");
        let load_ms = start.elapsed().as_millis();
        eprintln!("LOAD: {} ms", load_ms);

        let texts = vec![
            "What is the capital of France?".to_string(),
            "Explain quantum computing in simple terms.".to_string(),
            "Write a Python function to sort a list.".to_string(),
            "Summarize the theory of relativity.".to_string(),
            "How does machine learning work?".to_string(),
        ];

        for (i, text) in texts.iter().enumerate() {
            let t0 = Instant::now();
            let vec = embedder.embed(text).expect("embed failed");
            let elapsed = t0.elapsed().as_micros();
            eprintln!("EMBED[{}]: {} µs, dim={}", i, elapsed, vec.len());
        }

        // Batch
        let t0 = Instant::now();
        let results = embedder.batch_embed(&texts).expect("batch embed failed");
        let batch_elapsed = t0.elapsed().as_micros();
        eprintln!(
            "BATCH[{}]: {} µs, dim={}",
            results.len(),
            batch_elapsed,
            results[0].len()
        );
    }
}