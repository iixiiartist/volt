// LLM tools group (LiteRT-LM, llama.cpp, MTP)
use crate::models::ToolResult;
use crate::tools::registry::ToolRegistry;
use std::sync::Arc;

pub async fn register_llm_tools(registry: &Arc<ToolRegistry>) {
    // LiteRT-LM local inference tool
    let litertlm_path = std::env::var("VOLT_TOOL_BIN_DIR")
        .map(|d| std::path::PathBuf::from(d).join("litert_lm.exe"))
        .unwrap_or_else(|_| std::path::PathBuf::from("litert_lm.exe"));
    registry.register(
        "litertlm",
        "Run local inference via LiteRT-LM (Gemma-4 E4B, etc). Usage: {'model': 'path/to/model', 'prompt': '...', 'max_tokens': 256}",
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
        Arc::new(move |args| {
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
        }),
    ).await;

    // llama.cpp local inference tool
    let llamacpp_path = std::env::var("VOLT_TOOL_BIN_DIR")
        .map(|d| std::path::PathBuf::from(d).join("llama.exe"))
        .unwrap_or_else(|_| std::path::PathBuf::from("llama.exe"));
    registry.register(
        "llamacpp",
        "Run local inference via llama.cpp (Gemma-4 31B, etc). Usage: {'model': 'path/to/model.gguf', 'prompt': '...', 'context_size': 4096}",
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
        Arc::new(move |args| {
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
        }),
    ).await;

    // MTP (Multimodal Token Prediction) tool
    registry.register(
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
        Arc::new(|args| Box::pin(async move {
            let _draft = args["draft_model"].as_str().unwrap_or("");
            let full = args["full_model"].as_str().unwrap_or("");
            let prompt = args["prompt"].as_str().unwrap_or("");
            let framework = args["framework"].as_str().unwrap_or("litertlm");
            let draft_path = std::env::var("VOLT_TOOL_BIN_DIR")
                .map(|d| std::path::PathBuf::from(d).join(if framework == "litertlm" { "litert_lm.exe" } else { "llama.exe" }))
                .unwrap_or_else(|_| std::path::PathBuf::from(if framework == "litertlm" { "litert_lm.exe" } else { "llama.exe" }));
            let full_path = draft_path.clone();
            let tool = crate::tools::mtp::MtpTool::new(draft_path, full_path, framework.to_string());
            match tool.run_with_draft(full, prompt).await {
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
        })),
    ).await;
}
