use crate::agent::preset;

pub async fn cmd_list() {
    let presets = preset::list_presets();
    if presets.is_empty() {
        println!("No agent presets found.");
        println!("  Add .toml files to: presets/");
        return;
    }
    println!("Available agents:");
    for (name, path) in &presets {
        if let Some((_, p)) = preset::load_preset(name) {
            let model = p
                .agent
                .as_ref()
                .and_then(|a| a.model.as_deref())
                .unwrap_or("?");
            println!("  {}  ({})  [{}]", name, model, path.display());
        } else {
            println!("  {}  [{}]", name, path.display());
        }
    }
}

pub async fn cmd_run_interactive(worktree: bool) -> anyhow::Result<()> {
    let presets = preset::list_presets();
    if presets.is_empty() {
        println!("No agent presets found. Create files in: presets/");
        println!("Example: presets/gemma4-e4b.toml");
        return Ok(());
    }

    println!("Available agents:");
    for (i, (name, _)) in presets.iter().enumerate() {
        let model = preset::load_preset(name)
            .and_then(|(_, p)| p.agent?.model)
            .unwrap_or_else(|| "?".into());
        println!("  {}. {} ({})", i + 1, name, model);
    }
    println!();
    print!("Select agent [1-{}]: ", presets.len());
    use std::io::Write;
    std::io::stdout().flush()?;
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    let idx: usize = buf.trim().parse().unwrap_or(1);
    let idx = if idx > 0 { idx - 1 } else { 0 };
    let (name, _) = presets.get(idx).unwrap_or(&presets[0]).clone();

    let (_, p) = preset::load_preset(&name)
        .ok_or_else(|| anyhow::anyhow!("preset '{}' failed to load", name))?;
    let model_s = p
        .agent
        .as_ref()
        .and_then(|a| a.model.as_deref())
        .unwrap_or("?")
        .to_string();
    let max_iter = p.agent.as_ref().and_then(|a| a.max_iterations);
    let allow = p.agent.as_ref().and_then(|a| a.allow).unwrap_or(true);

    if let Some(ref env) = p.env {
        for (k, v) in env {
            std::env::set_var(k, v);
        }
    }

    println!("Using agent: {} ({})", name, model_s);
    println!("Enter your query (or Ctrl+C to cancel):");
    print!("> ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim().to_string();
    if input.is_empty() {
        return Ok(());
    }

    let settings = crate::config::Settings::from_env()?;
    super::agent_run::run(super::agent_run::AgentRunOptions {
        input,
        model: model_s,
        allow,
        load_tools: None,
        context_kinds: Vec::new(),
        mode: "balanced".into(),
        session_id: None,
        max_iterations: max_iter,
        settings,
        use_mtp: false,
        use_cot: false,
        allow_write: false,
        framework: None,
        model_variant: None,
        quantization: None,
        blueprint: None,
        auto_blueprint: false,
        print: false,
        json: false,
        plan: false,
        worktree,
    })
    .await
}
