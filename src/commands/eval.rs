use crate::agent::Agent;
use crate::models::*;
use std::path::PathBuf;

pub async fn run(suite: PathBuf, model: Option<String>) -> anyhow::Result<()> {
    let model = model
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("LLM_MODEL").ok().filter(|s| !s.trim().is_empty()))
        .or_else(|| std::env::var("LLM_DEFAULT_MODEL").ok().filter(|s| !s.trim().is_empty()))
        .or_else(|| {
            let inv = crate::llm::detect_providers();
            let defaults: Vec<String> = inv
                .active()
                .filter_map(|p| p.default_model.map(|m| m.to_string()))
                .collect();
            defaults.into_iter().next()
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no model configured. Pass --model, set LLM_MODEL in .env, \
                 or run `volt config` to choose a provider."
            )
        })?;
    let content = tokio::fs::read_to_string(&suite).await?;
    let suite_data: crate::eval::EvalSuite = serde_json::from_str(&content)?;
    let (provider, provider_kind) = crate::orchestrator::try_build_provider(&model, "eval-agent")
        .map_err(|e| anyhow::anyhow!("{}\n{}", e, e.hint()))?;
    let tools = crate::tools::register_all_tools().await;
    let config = AgentConfig {
        name: "eval-agent".into(),
        model,
        provider: provider_kind,
        system_prompt: None,
        max_iterations: 15,
        temperature: 0.3,
        toolsets: vec!["builtin".into()],
        hidden: false,
        allow_all: true,
        enabled_context_kinds: crate::models::default_context_kinds(),
        essential_tools: crate::models::default_essential_tools(),
        context_kind_quotas: Default::default(),
        use_mtp: false,
        use_cot: false,
        allow_write: false,
        framework: None,
        model_variant: None,
        quantization: None,
        format_dialect: Default::default(),
        quirks: vec![],
        strict_mode: false,
        max_tools_per_turn: None,
        blueprint_path: None,
    };
    let agent = Agent::new(config, provider, tools)
        .await
        .with_workspace(std::env::current_dir().unwrap_or_default());
    let summary = crate::eval::run_suite(&suite_data, &agent).await;
    crate::eval::print_summary(&summary);
    Ok(())
}
