// LLM tools group (LiteRT-LM, llama.cpp, MTP)
use crate::models::ToolResult;
use crate::register_tool;
use crate::tools::registry::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;
fn tool_bin_path(binary_name: &str) -> std::path::PathBuf {
    std::env::var("VOLT_TOOL_BIN_DIR")
        .map(|d| std::path::PathBuf::from(d).join(binary_name))
        .unwrap_or_else(|_| std::path::PathBuf::from(binary_name))
}

fn local_llm_enabled() -> bool {
    std::env::var("VOLT_ENABLE_LOCAL_LLM_TOOLS").as_deref() == Ok("1")
}

pub async fn register_llm_tools(registry: &Arc<ToolRegistry>) {
    // LiteRT-LM local inference tool
    let litertlm_path = tool_bin_path("litert_lm.exe");
    if local_llm_enabled() && litertlm_path.exists() {
        registry
            .register(
                "litertlm",
                "[LOCAL ONLY] Run a local LiteRT-LM inference binary (e.g., Gemma-4 E4B). Do NOT use this for answering user questions — you are the LLM, answer directly. Only use this if you specifically need to run a separate local model.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "model": { "type": "string", "description": "path to model file" },
                        "prompt": { "type": "string", "description": "prompt text" },
                        "max_tokens": { "type": "integer", "description": "max tokens to generate" }
                    },
                    "required": ["model", "prompt"]
                }),
                "llm",
                std::sync::Arc::new({
                    let litertlm_path = litertlm_path.clone();
                    move |args: Value| {
                        let litertlm_path = litertlm_path.clone();
                        Box::pin(async move {
                            let model = args["model"].as_str().unwrap_or("");
                            let prompt = args["prompt"].as_str().unwrap_or("");
                            let max_tokens = args["max_tokens"].as_u64().unwrap_or(256) as u32;
                            let tool = crate::tools::litertlm::LiteRTTool::new(litertlm_path);
                            match tool.run(model, prompt, max_tokens).await {
                                Ok(result) => ToolResult {
                                    success: true,
                                    output: result,
                                    error: None,
                                    duration_ms: 0,
                                },
                                Err(e) => ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!("LiteRT-LM error: {}", e)),
                                    duration_ms: 0,
                                },
                            }
                        })
                    }
                }),
            )
            .await;
    }

    // llama.cpp local inference tool
    let llamacpp_path = tool_bin_path("llama.exe");
    if local_llm_enabled() && llamacpp_path.exists() {
        registry
            .register(
                "llamacpp",
                "[LOCAL ONLY] Run a local llama.cpp inference binary (e.g., Gemma-4 31B). Do NOT use this for answering user questions — you are the LLM, answer directly. Only use this if you specifically need to run a separate local model.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "model": { "type": "string", "description": "path to GGUF model file" },
                        "prompt": { "type": "string", "description": "prompt text" },
                        "context_size": { "type": "integer", "description": "context size (default 4096)" }
                    },
                    "required": ["model", "prompt"]
                }),
                "llm",
                std::sync::Arc::new({
                    let llamacpp_path = llamacpp_path.clone();
                    move |args: Value| {
                        let llamacpp_path = llamacpp_path.clone();
                        Box::pin(async move {
                            let model = args["model"].as_str().unwrap_or("");
                            let prompt = args["prompt"].as_str().unwrap_or("");
                            let context_size = args["context_size"].as_u64().unwrap_or(4096) as u32;
                            let tool = crate::tools::llamacpp::LlamaCppTool::new(llamacpp_path);
                            match tool.run(model, prompt, context_size).await {
                                Ok(result) => ToolResult {
                                    success: true,
                                    output: result,
                                    error: None,
                                    duration_ms: 0,
                                },
                                Err(e) => ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!("llama.cpp error: {}", e)),
                                    duration_ms: 0,
                                },
                            }
                        })
                    }
                }),
            )
            .await;
    }

    // MTP (Multimodal Token Prediction) tool
    if local_llm_enabled()
        && (tool_bin_path("litert_lm.exe").exists() || tool_bin_path("llama.exe").exists())
    {
        register_tool!(
            registry,
            "mtp",
            "Run MTP using a draft model + full model. Usage: {'draft_model': 'path/to/draft', 'full_model': 'path/to/full', 'prompt': '...', 'framework': 'litertlm|llamacpp'}",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "draft_model": { "type": "string", "description": "path to draft model" },
                    "full_model": { "type": "string", "description": "path to full model" },
                    "prompt": { "type": "string", "description": "prompt text" },
                    "framework": { "type": "string", "description": "framework to use: litertlm or llamacpp" }
                },
                "required": ["draft_model", "full_model", "prompt", "framework"]
            }),
            "llm",
            |args: Value| async move {
                let draft = args["draft_model"].as_str().unwrap_or("");
                let full = args["full_model"].as_str().unwrap_or("");
                let prompt = args["prompt"].as_str().unwrap_or("");
                let framework = args["framework"].as_str().unwrap_or("litertlm");
                let binary_name = if framework == "litertlm" { "litert_lm.exe" } else { "llama.exe" };
                let draft_binary = tool_bin_path(binary_name);
                let full_binary = draft_binary.clone();
                let tool = crate::tools::mtp::MtpTool::new(draft_binary, full_binary, framework.to_string());
                match tool.run_with_draft(draft, full, prompt).await {
                    Ok(result) => ToolResult {
                        success: true,
                        output: result,
                        error: None,
                        duration_ms: 0,
                    },
                    Err(e) => ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("MTP error: {}", e)),
                        duration_ms: 0,
                    },
                }
            }
        );
    }
}
