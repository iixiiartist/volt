use crate::attenuation::TrustLevel;
use crate::models::{PermissionLevel, ToolResult};
use crate::tools::ToolRegistry;
use std::sync::Arc;

pub async fn setup_tools(
    embedder: Option<&crate::embedding::EmbeddingClient>,
    database_url: Option<&str>,
) -> Arc<ToolRegistry> {
    let registry = crate::tools::register_all_tools().await;
    if let Some(url) = database_url {
        if let Ok(pool) = crate::db::connect(url).await {
            if let Ok(rows) = crate::db::list_tools_with_schema(&pool).await {
                for row in rows {
                    let name = row.tool_name.clone();
                    let desc = row.description.clone();
                    let schema = row.parameter_schema.clone();
                    let code = row.source_code.clone();
                    registry
                        .register_with_permission(
                            &name,
                            &desc,
                            schema,
                            "installed",
                            Arc::new(move |args| {
                                let code = code.clone();
                                Box::pin(async move {
                                    let stdin = args.to_string();
                                    let policy = crate::models::SandboxPolicy {
                                        timeout_ms: 300_000,
                                        max_stdout_bytes: 10_485_760,
                                        working_dir: None,
                                    };
                                    let result = crate::sandbox::run_command_direct(
                                        "python3",
                                        &["-c", &code],
                                        Some(&stdin),
                                        &policy,
                                    )
                                    .await;
                                    let output_val = serde_json::from_str(&result.stdout)
                                        .unwrap_or_else(
                                            |_| serde_json::json!({"raw": result.stdout}),
                                        );
                                    ToolResult {
                                        success: result.status == "ok",
                                        output: output_val.to_string(),
                                        error: if result.status == "ok" {
                                            None
                                        } else {
                                            Some(result.stderr.clone())
                                        },
                                        duration_ms: result.duration_ms,
                                    }
                                })
                            }),
                            PermissionLevel::Allow,
                            TrustLevel::Installed,
                        )
                        .await;
                }
            }
        }
    }
    if let Some(emb) = embedder {
        if std::env::var("VOLT_SKIP_TOOL_EMBEDDINGS").is_ok() {
            tracing::info!("[tools] skipping compute_embeddings (VOLT_SKIP_TOOL_EMBEDDINGS=1)");
        } else {
            registry.compute_embeddings(emb).await;
        }
    }
    registry
}

pub async fn register_all_tools() -> Arc<ToolRegistry> {
    let registry = ToolRegistry::new();
    let minimal = std::env::var("VOLT_MINIMAL_TOOLS").is_ok();

    // ── Phase 1: Core tools (always registered) ──────────────────────────
    crate::tools::groups::core::register_core_tools(&registry).await;
    crate::tools::groups::web::register_web_tools(&registry).await;
    crate::tools::groups::memory::register_memory_tools(&registry).await;
    crate::tools::groups::data::register_csv_tools(&registry).await;
    crate::tools::groups::data::register_archive_tools(&registry).await;
    crate::tools::groups::git::register_git_tools(&registry).await;
    crate::tools::groups::time_sequential::register_time_tools(&registry).await;
    crate::tools::groups::time_sequential::register_sequential_tools(&registry).await;
    crate::tools::groups::llm::register_llm_tools(&registry).await;

    // ── Phase 2: Dynamic tools (require registry capture) ──────────────────
    register_delegate_and_workflow(&registry).await;

    // ── Phase 3: Feature-gated / extended tools ───────────────────────────
    if !minimal {
        crate::tools::groups::data::register_chart_tools(&registry).await;
        crate::tools::groups::data::register_pdf_tools(&registry).await;
        crate::tools::groups::desktop::register_desktop_tools(&registry).await;
        crate::tools::groups::browser::register_browser_tools(&registry).await;
    }

    // ── Phase 4: NVIDIA Cloud Functions (requires NVIDIA_API_KEY) ──────────
    if std::env::var("NVIDIA_API_KEY").is_ok() || std::env::var("NVCF_API_KEY").is_ok() {
        crate::tools::nvidia_cloud_functions::register_nvidia_cloud_functions(&registry).await;
    }

    // ── Phase 5: Ollama Cloud web tools (requires OLLAMA_API_KEY) ──────────
    if std::env::var("OLLAMA_API_KEY").is_ok() {
        crate::tools::ollama_web_tools::register_ollama_web_tools(&registry).await;
    }

    // ── Phase 6: CLI gateway (requires VOLT_ENABLE_CLI_TOOLS=1) ─────────
    if std::env::var("VOLT_ENABLE_CLI_TOOLS").as_deref() == Ok("1") {
        crate::tools::cli_tools::register_cli_tools(&registry).await;
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    /// Serialize env-var-dependent tests to avoid race conditions.
    static ENV_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn reset_env() {
        unsafe {
            std::env::remove_var("VOLT_ENABLE_CLI_TOOLS");
            std::env::remove_var("VOLT_ENABLE_LOCAL_LLM_TOOLS");
            std::env::remove_var("VOLT_MINIMAL_TOOLS");
            std::env::remove_var("VOLT_TOOL_BIN_DIR");
            std::env::remove_var("NVIDIA_API_KEY");
            std::env::remove_var("NVCF_API_KEY");
            std::env::remove_var("OLLAMA_API_KEY");
        }
    }

    #[tokio::test]
    async fn test_cli_exec_gated_without_env() {
        let _lock = ENV_MUTEX
            .lock()
            .expect("ENV_MUTEX poisoned; this is a test-only lock");
        reset_env();

        let registry = register_all_tools().await;
        let def = registry.get_definition("cli_exec").await;
        assert!(
            def.is_none(),
            "cli_exec should be gated when VOLT_ENABLE_CLI_TOOLS is unset"
        );
    }

    #[tokio::test]
    async fn test_cli_query_gated_without_env() {
        let _lock = ENV_MUTEX
            .lock()
            .expect("ENV_MUTEX poisoned; this is a test-only lock");
        reset_env();

        let registry = register_all_tools().await;
        let def = registry.get_definition("cli_query").await;
        assert!(
            def.is_none(),
            "cli_query should be gated when VOLT_ENABLE_CLI_TOOLS is unset"
        );
    }

    #[tokio::test]
    async fn test_cli_tools_available_with_env() {
        let _lock = ENV_MUTEX
            .lock()
            .expect("ENV_MUTEX poisoned; this is a test-only lock");
        reset_env();
        unsafe { std::env::set_var("VOLT_ENABLE_CLI_TOOLS", "1") }

        let registry = register_all_tools().await;
        assert!(
            registry.get_definition("cli_exec").await.is_some(),
            "cli_exec should be available when VOLT_ENABLE_CLI_TOOLS=1"
        );
        assert!(
            registry.get_definition("cli_query").await.is_some(),
            "cli_query should be available when VOLT_ENABLE_CLI_TOOLS=1"
        );
    }

    #[tokio::test]
    async fn test_local_llm_tools_gated_without_env() {
        let _lock = ENV_MUTEX
            .lock()
            .expect("ENV_MUTEX poisoned; this is a test-only lock");
        reset_env();

        let registry = register_all_tools().await;
        assert!(
            registry.get_definition("litertlm").await.is_none(),
            "litertlm should be gated when VOLT_ENABLE_LOCAL_LLM_TOOLS is unset"
        );
        assert!(
            registry.get_definition("llamacpp").await.is_none(),
            "llamacpp should be gated when VOLT_ENABLE_LOCAL_LLM_TOOLS is unset"
        );
        assert!(
            registry.get_definition("mtp").await.is_none(),
            "mtp should be gated when VOLT_ENABLE_LOCAL_LLM_TOOLS is unset"
        );
    }

    #[tokio::test]
    async fn test_local_llm_tools_available_with_env_and_binaries() {
        let _lock = ENV_MUTEX
            .lock()
            .expect("ENV_MUTEX poisoned; this is a test-only lock");
        reset_env();

        let temp_dir = tempfile::tempdir().unwrap();
        let bin_dir = temp_dir.path();

        // Create fake binaries so the existence check passes
        std::fs::write(bin_dir.join("litert_lm.exe"), "").unwrap();
        std::fs::write(bin_dir.join("llama.exe"), "").unwrap();

        unsafe {
            std::env::set_var("VOLT_ENABLE_LOCAL_LLM_TOOLS", "1");
            std::env::set_var("VOLT_TOOL_BIN_DIR", bin_dir.to_str().unwrap());
        }

        let registry = register_all_tools().await;
        assert!(
            registry.get_definition("litertlm").await.is_some(),
            "litertlm should be available when gate is set and binary exists"
        );
        assert!(
            registry.get_definition("llamacpp").await.is_some(),
            "llamacpp should be available when gate is set and binary exists"
        );
        assert!(
            registry.get_definition("mtp").await.is_some(),
            "mtp should be available when gate is set and at least one binary exists"
        );
    }

    #[tokio::test]
    async fn test_minimal_mode_excludes_extended_tools() {
        let _lock = ENV_MUTEX
            .lock()
            .expect("ENV_MUTEX poisoned; this is a test-only lock");
        reset_env();
        unsafe { std::env::set_var("VOLT_MINIMAL_TOOLS", "1") }

        let registry = register_all_tools().await;
        // Minimal mode should exclude chart, pdf, desktop, browser tools
        assert!(
            registry.get_definition("create_bar_chart").await.is_none(),
            "create_bar_chart should be excluded in minimal mode"
        );
        assert!(
            registry.get_definition("create_pdf").await.is_none(),
            "create_pdf should be excluded in minimal mode"
        );
    }
}

async fn register_delegate_and_workflow(registry: &Arc<ToolRegistry>) {
    // delegate
    let delegate_tools = registry.clone();
    let delegate_fn = {
        let dt = delegate_tools.clone();
        Arc::new(move |args: serde_json::Value| {
            let dt = dt.clone();
            Box::pin(async move {
                let task = args["task"].as_str().unwrap_or("");
                let context = args["context"].as_str().unwrap_or("");
                crate::tools::delegate::delegate_task(task, context, dt).await
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
        })
    };
    registry
        .register_with_permission(
            "delegate",
            "Delegate a sub-task to a sub-agent and return its result",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "task description for the sub-agent" },
                    "context": { "type": "string", "description": "context and constraints from the parent agent" }
                },
                "required": ["task"]
            }),
            "builtin",
            delegate_fn,
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;

    // run_workflow
    let workflow_fn = {
        let wt = registry.clone();
        Arc::new(move |args: serde_json::Value| {
            let wt = wt.clone();
            Box::pin(async move {
                let pattern = args["pattern"].as_str().unwrap_or("parallel");
                let agents_json = args["agents"].as_str().unwrap_or("[]");
                let tasks_json = args["tasks"].as_str().unwrap_or("[]");
                let started = std::time::Instant::now();

                match crate::orchestrator::parse_agent_specs(agents_json) {
                    Ok(specs) => match serde_json::from_str::<Vec<String>>(tasks_json) {
                        Ok(tasks) => {
                            let orch = crate::orchestrator::Orchestrator::new(wt.clone()).await;
                            match orch.run_workflow(pattern, specs, tasks).await {
                                Ok(result) => ToolResult {
                                    success: true,
                                    output: result.final_output,
                                    error: None,
                                    duration_ms: started.elapsed().as_millis(),
                                },
                                Err(e) => ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!("workflow error: {}", e)),
                                    duration_ms: started.elapsed().as_millis(),
                                },
                            }
                        }
                        Err(e) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("invalid tasks JSON: {}", e)),
                            duration_ms: started.elapsed().as_millis(),
                        },
                    },
                    Err(e) => ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("invalid agents JSON: {}", e)),
                        duration_ms: started.elapsed().as_millis(),
                    },
                }
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
        })
    };
    registry
        .register_with_permission(
            "run_workflow",
            "Execute a multi-agent workflow (parallel or pipeline) and return combined results",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "workflow pattern: 'parallel' or 'pipeline'" },
                    "agents": { "type": "string", "description": "JSON array of agent specs, each with 'name' (required) and optional 'model', 'system_prompt', 'max_iterations', 'temperature'" },
                    "tasks": { "type": "string", "description": "JSON array of task strings (one per agent for parallel, one per stage for pipeline)" }
                },
                "required": ["pattern", "agents", "tasks"]
            }),
            "builtin",
            workflow_fn,
            PermissionLevel::Prompt,
            TrustLevel::Builtin,
        )
        .await;
}
