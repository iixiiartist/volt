use std::path::PathBuf;
use crate::agent::loop_rs::Agent;
use crate::models::*;

pub async fn run(suite: PathBuf, model: Option<String>) -> anyhow::Result<()> {
    let model = model.unwrap_or_else(|| std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into()));
    let content = tokio::fs::read_to_string(&suite).await?;
    let suite_data: crate::eval::EvalSuite = serde_json::from_str(&content)?;
    let (provider, provider_kind) = crate::orchestrator::build_provider(&model, "eval-agent");
    let tools = crate::tools::register_all_tools().await;
    let config = AgentConfig {
        name: "eval-agent".into(), model, provider: provider_kind,
        system_prompt: None, max_iterations: 15, temperature: 0.3,
        toolsets: vec!["builtin".into()], hidden: false, allow_all: true,
        enabled_context_kinds: crate::models::default_context_kinds(),
        essential_tools: crate::models::default_essential_tools(),
        context_kind_quotas: Default::default(),
    };
    let agent = Agent::new(config, provider, tools)
        .with_workspace(std::env::current_dir().unwrap_or_default());
    let summary = crate::eval::run_suite(&suite_data, &agent).await;
    crate::eval::print_summary(&summary);
    Ok(())
}
