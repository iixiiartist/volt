use crate::orchestrator::{parse_agent_specs, Orchestrator};

pub async fn run(
    pattern: String,
    agents: String,
    tasks: String,
    allow: bool,
) -> anyhow::Result<()> {
    let mut specs = parse_agent_specs(&agents)?;
    if allow {
        for spec in &mut specs {
            spec.allow_all = true;
        }
    }
    let tasks: Vec<String> = serde_json::from_str(&tasks)?;
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
