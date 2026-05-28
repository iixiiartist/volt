use crate::orchestrator::{parse_agent_specs, Orchestrator};
use std::path::PathBuf;

pub async fn run(
    pattern: String,
    agents: Option<String>,
    tasks: Option<String>,
    agents_file: Option<PathBuf>,
    tasks_file: Option<PathBuf>,
    allow: bool,
) -> anyhow::Result<()> {
    let agents_json = match (agents, agents_file) {
        (Some(j), None) => j,
        (None, Some(f)) => std::fs::read_to_string(&f).map_err(|e| anyhow::anyhow!("read agents file: {}", e))?,
        (Some(_), Some(_)) => anyhow::bail!("provide --agents OR --agents-file, not both"),
        (None, None) => anyhow::bail!("provide either --agents or --agents-file"),
    };
    let tasks_json = match (tasks, tasks_file) {
        (Some(j), None) => j,
        (None, Some(f)) => std::fs::read_to_string(&f).map_err(|e| anyhow::anyhow!("read tasks file: {}", e))?,
        (Some(_), Some(_)) => anyhow::bail!("provide --tasks OR --tasks-file, not both"),
        (None, None) => anyhow::bail!("provide either --tasks or --tasks-file"),
    };
    let mut specs = parse_agent_specs(&agents_json)?;
    if allow {
        for spec in &mut specs {
            spec.allow_all = true;
        }
    }
    let tasks: Vec<String> = serde_json::from_str(&tasks_json)?;
    let tools = crate::tools::register_all_tools().await;
    let orch = Orchestrator::new(tools).await;
    let result = orch.run_workflow(&pattern, specs, tasks).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "steps": result.steps.iter().map(|s| serde_json::json!({
                "agent": s.agent_name, "success": s.success,
                "duration_ms": s.duration_ms, "output": s.output,
            })).collect::<Vec<_>>(),
            "final_output": result.final_output, "total_duration_ms": result.total_duration_ms,
        }))?
    );
    Ok(())
}
