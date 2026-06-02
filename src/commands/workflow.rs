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
        (None, Some(f)) => {
            std::fs::read_to_string(&f).map_err(|e| anyhow::anyhow!("read agents file: {}", e))?
        }
        (Some(_), Some(_)) => anyhow::bail!("provide --agents OR --agents-file, not both"),
        (None, None) => anyhow::bail!("provide either --agents or --agents-file"),
    };
    let tasks_json = match (tasks, tasks_file) {
        (Some(j), None) => j,
        (None, Some(f)) => {
            std::fs::read_to_string(&f).map_err(|e| anyhow::anyhow!("read tasks file: {}", e))?
        }
        (Some(_), Some(_)) => anyhow::bail!("provide --tasks OR --tasks-file, not both"),
        (None, None) => anyhow::bail!("provide either --tasks or --tasks-file"),
    };

    let tools = crate::tools::register_all_tools().await;
    let orch = Orchestrator::new(tools).await;

    let result = if pattern == "dag" {
        // For DAG pattern: agents_json is the DAG definition, tasks_json[0] is the initial input
        let initial_input = serde_json::from_str::<Vec<String>>(&tasks_json)?
            .first()
            .cloned()
            .unwrap_or_default();
        if allow {
            // allow-all is a no-op for DAG mode — individual agent specs control their own permissions
        }
        orch.run_dag(&agents_json, &initial_input).await?
    } else {
        let mut specs = parse_agent_specs(&agents_json)?;
        if allow {
            for spec in &mut specs {
                spec.allow_all = true;
            }
        }
        let tasks: Vec<String> = serde_json::from_str(&tasks_json)?;
        orch.run_workflow(&pattern, specs, tasks).await?
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "steps": result.steps.iter().map(|s| serde_json::json!({
                "agent": s.agent_name, "success": s.success,
                "duration_ms": s.duration_ms, "prompt_tokens": s.prompt_tokens,
                "completion_tokens": s.completion_tokens, "output": s.output,
                "error": s.error,
            })).collect::<Vec<_>>(),
            "final_output": result.final_output, "total_duration_ms": result.total_duration_ms,
        }))?
    );
    Ok(())
}
