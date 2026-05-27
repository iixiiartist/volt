use crate::config::Settings;
use crate::db;
use crate::models::SandboxPolicy;
use crate::sandbox;
use uuid::Uuid;

pub async fn init_db(database_url: &str) -> anyhow::Result<()> {
    let pool = db::connect(database_url).await?;
    db::init_schema(&pool).await?;
    println!("schema initialized");
    Ok(())
}

pub async fn list_tools(database_url: &str) -> anyhow::Result<()> {
    let pool = db::connect(database_url).await?;
    let tools = db::list_tools(&pool).await?;
    let output = serde_json::to_string_pretty(&tools)?;
    println!("{}", output);
    Ok(())
}

pub async fn history(limit: i64, database_url: &str) -> anyhow::Result<()> {
    let pool = db::connect(database_url).await?;
    let records = db::list_executions(&pool, limit).await?;
    let output = serde_json::to_string_pretty(&records)?;
    println!("{}", output);
    Ok(())
}

pub async fn execute(
    tool: String,
    params: Option<String>,
    settings: &Settings,
) -> anyhow::Result<()> {
    let pool = db::connect(&settings.database_url).await?;
    let tool_params: serde_json::Value = params
        .as_deref()
        .map(|p| {
            serde_json::from_str(p).unwrap_or_else(|e| {
                eprintln!(
                    "[cli] warning: invalid JSON params '{}': {}. Using empty object.",
                    p, e
                );
                serde_json::json!({})
            })
        })
        .unwrap_or(serde_json::json!({}));
    let tool_info = db::get_tool_by_name(&pool, &tool).await?;
    let tool_id = tool_info.as_ref().map(|t| t.id);
    let source: Option<String> = db::get_tool_source(&pool, &tool).await?;
    let execution_id = Uuid::new_v4();

    match source {
        Some(code) => {
            let stdin_input = tool_params.to_string();
            let result = sandbox::run_command_direct(
                "python3",
                &["-c", &code],
                Some(&stdin_input),
                &settings.sandbox_policy,
            )
            .await;
            let output_val: serde_json::Value = serde_json::from_str(&result.stdout)
                .unwrap_or(serde_json::json!({ "raw": result.stdout }));
            let status = if result.status == "ok" {
                "success"
            } else {
                "failed"
            };
            db::record_execution(
                &pool,
                tool_id,
                &tool,
                &tool_params,
                &output_val,
                status,
                if result.status != "ok" {
                    Some(&result.stderr)
                } else {
                    None
                },
                result.duration_ms as i32,
                execution_id,
            )
            .await?;
            let json_output = serde_json::to_string_pretty(&serde_json::json!({
                "execution_id": execution_id.to_string(), "status": status,
                "output": output_val, "duration_ms": result.duration_ms,
            }))?;
            println!("{}", json_output);
        }
        None => anyhow::bail!("tool '{}' not found; provision it first", tool),
    }
    Ok(())
}

pub async fn sandbox_command(
    command: String,
    timeout_ms: Option<u64>,
    settings: &Settings,
) -> anyhow::Result<()> {
    let policy = SandboxPolicy {
        timeout_ms: timeout_ms.unwrap_or(settings.sandbox_policy.timeout_ms),
        max_stdout_bytes: settings.sandbox_policy.max_stdout_bytes,
        working_dir: settings.sandbox_policy.working_dir.clone(),
    };
    let result = sandbox::run_command(&command, &policy).await?;
    let output = serde_json::to_string_pretty(&result)?;
    println!("{}", output);
    Ok(())
}

pub async fn validate_manifest(manifest: std::path::PathBuf) -> anyhow::Result<()> {
    let manifest = crate::registry::load_manifest(&manifest).await?;
    let report = crate::validation::validate_manifest(&manifest);
    let output = serde_json::to_string_pretty(&report)?;
    println!("{}", output);
    if !report.accepted {
        std::process::exit(2);
    }
    Ok(())
}
