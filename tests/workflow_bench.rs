use std::sync::Arc;
use std::time::Instant;
use volt::models::PermissionLevel;
use volt::orchestrator::{parse_agent_specs, Orchestrator};
use volt::tools::ToolRegistry;

fn _build_provider(model: &str) -> (Box<dyn volt::llm::LLMProvider>, String) {
    let route = volt::orchestrator::resolve_provider(model);
    let kind_str = match route.kind {
        volt::orchestrator::ProviderKind::Anthropic => "anthropic",
        volt::orchestrator::ProviderKind::OpenAI => "openai",
    };
    let provider: Box<dyn volt::llm::LLMProvider> = match route.kind {
        volt::orchestrator::ProviderKind::Anthropic => {
            Box::new(volt::llm::anthropic::AnthropicProvider::new(
                route.api_key,
                Some(route.base_url),
                "bench-agent".into(),
            ))
        }
        volt::orchestrator::ProviderKind::OpenAI => {
            Box::new(volt::llm::openai::OpenAIProvider::new(
                route.api_key,
                route.base_url,
                "bench-agent".into(),
            ))
        }
    };
    (provider, kind_str.to_string())
}

async fn register_all_tools() -> Arc<ToolRegistry> {
    let registry = ToolRegistry::new();

    registry
        .register_with_permission(
            "bash",
            "Execute a shell command",
            serde_json::json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
            "builtin",
            Arc::new(|args| Box::pin(async move {
                let cmd = args["command"].as_str().unwrap_or("");
                volt::tools::bash::execute_bash(cmd).await
            })),
            PermissionLevel::Prompt,
        )
        .await;
    registry
        .register_with_permission(
            "read",
            "Read a file from disk",
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            "builtin",
            Arc::new(|args| Box::pin(async move {
                let path = args["path"].as_str().unwrap_or("");
                volt::tools::read_tool::read_file(path).await
            })),
            PermissionLevel::Prompt,
        )
        .await;
    registry
        .register_with_permission(
            "write",
            "Write content to a file",
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            "builtin",
            Arc::new(|args| Box::pin(async move {
                let path = args["path"].as_str().unwrap_or("");
                let content = args["content"].as_str().unwrap_or("");
                volt::tools::write_tool::write_file(path, content).await
            })),
            PermissionLevel::Prompt,
        )
        .await;
    registry
        .register_with_permission(
            "edit",
            "Edit a file by replacing text",
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"}},"required":["path","old_string","new_string"]}),
            "builtin",
            Arc::new(|args| Box::pin(async move {
                let path = args["path"].as_str().unwrap_or("");
                let old = args["old_string"].as_str().unwrap_or("");
                let new_ = args["new_string"].as_str().unwrap_or("");
                volt::tools::edit::edit_file(path, old, new_).await
            })),
            PermissionLevel::Prompt,
        )
        .await;
    registry.register("glob","Find files matching a glob pattern",serde_json::json!({"type":"object","properties":{"pattern":{"type":"string"},"base":{"type":"string"}},"required":["pattern"]}),"builtin",Arc::new(|args| Box::pin(async move { let p=args["pattern"].as_str().unwrap_or("*"); let b=args["base"].as_str().unwrap_or("."); volt::tools::glob_tool::glob_files(p,b).await }))).await;
    registry.register("grep","Search file contents with regex",serde_json::json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern"]}),"builtin",Arc::new(|args| Box::pin(async move { let p=args["pattern"].as_str().unwrap_or(""); let path=args["path"].as_str().unwrap_or("."); volt::tools::grep_tool::grep_files(p,path).await }))).await;
    registry.register_with_permission("web_fetch","Fetch a URL and return its content",serde_json::json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),"builtin",Arc::new(|args| Box::pin(async move { let u=args["url"].as_str().unwrap_or(""); volt::tools::web_tool::web_fetch(u).await })),PermissionLevel::Prompt).await;
    registry.register("memory_append","Append to persistent memory file",serde_json::json!({"type":"object","properties":{"kind":{"type":"string"},"content":{"type":"string"}},"required":["kind","content"]}),"builtin",Arc::new(|args| Box::pin(async move { let k=args["kind"].as_str().unwrap_or("note"); let c=args["content"].as_str().unwrap_or(""); volt::tools::memory_tool::memory_append(k,c).await }))).await;
    registry.register("todo_add","Add a task to the todo list",serde_json::json!({"type":"object","properties":{"task":{"type":"string"}},"required":["task"]}),"builtin",Arc::new(|args| Box::pin(async move { let t=args["task"].as_str().unwrap_or(""); volt::tools::todo_tool::todo_add(t).await }))).await;
    let dt = registry.clone();
    registry.register_with_permission("delegate","Delegate a sub-task to a sub-agent",serde_json::json!({"type":"object","properties":{"task":{"type":"string"},"context":{"type":"string"}},"required":["task"]}),"builtin",Arc::new(move |args| { let dt=dt.clone(); Box::pin(async move { let t=args["task"].as_str().unwrap_or(""); let c=args["context"].as_str().unwrap_or(""); volt::tools::delegate::delegate_task(t,c,dt).await }) }),PermissionLevel::Prompt).await;
    registry.register_with_permission("web_scrape","Extract structured content from a URL using a CSS selector",serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"selector":{"type":"string"}},"required":["url","selector"]}),"builtin",Arc::new(|args| Box::pin(async move { let u=args["url"].as_str().unwrap_or(""); let s=args["selector"].as_str().unwrap_or(""); volt::tools::scrape_tool::web_scrape(u,s).await })),PermissionLevel::Prompt).await;
    registry.register_with_permission("web_scrape_all","Fetch a URL and extract all human-readable content",serde_json::json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),"builtin",Arc::new(|args| Box::pin(async move { let u=args["url"].as_str().unwrap_or(""); volt::tools::scrape_tool::web_scrape_all(u).await })),PermissionLevel::Prompt).await;
    registry.register("json_validate","Validate JSON string and return its type",serde_json::json!({"type":"object","properties":{"data":{"type":"string"}},"required":["data"]}),"builtin",Arc::new(|args| Box::pin(async move { let d=args["data"].as_str().unwrap_or(""); volt::tools::json_tool::json_validate(d).await }))).await;
    registry.register("json_prettify","Format JSON with custom indentation",serde_json::json!({"type":"object","properties":{"data":{"type":"string"},"indent":{"type":"integer"}},"required":["data"]}),"builtin",Arc::new(|args| Box::pin(async move { let d=args["data"].as_str().unwrap_or(""); let i=args["indent"].as_u64().unwrap_or(2)as u8; volt::tools::json_tool::json_prettify(d,i).await }))).await;
    registry.register("json_query","Extract a value from JSON using dot-separated path",serde_json::json!({"type":"object","properties":{"data":{"type":"string"},"path":{"type":"string"}},"required":["data","path"]}),"builtin",Arc::new(|args| Box::pin(async move { let d=args["data"].as_str().unwrap_or(""); let p=args["path"].as_str().unwrap_or(""); volt::tools::json_tool::json_query(d,p).await }))).await;
    registry.register("csv_read","Read a CSV file",serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"has_header":{"type":"boolean"}},"required":["path"]}),"builtin",Arc::new(|args| Box::pin(async move { let p=args["path"].as_str().unwrap_or(""); let h=args["has_header"].as_bool().unwrap_or(true); volt::tools::csv_tool::csv_read(p,h).await }))).await;
    registry.register("csv_write","Write data to a CSV file",serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"data":{"type":"string"},"has_header":{"type":"boolean"}},"required":["path","data"]}),"builtin",Arc::new(|args| Box::pin(async move { let p=args["path"].as_str().unwrap_or(""); let d=args["data"].as_str().unwrap_or(""); let h=args["has_header"].as_bool().unwrap_or(true); volt::tools::csv_tool::csv_write(p,d,h).await }))).await;
    registry.register("archive_extract","Extract an archive file",serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"dest":{"type":"string"}},"required":["path","dest"]}),"builtin",Arc::new(|args| Box::pin(async move { let p=args["path"].as_str().unwrap_or(""); let d=args["dest"].as_str().unwrap_or(""); volt::tools::archive_tool::archive_extract(p,d).await }))).await;
    registry.register("archive_create","Create a tar or tar.gz archive",serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"sources":{"type":"array","items":{"type":"string"}},"format":{"type":"string"}},"required":["path","sources"]}),"builtin",Arc::new(|args| Box::pin(async move { let p=args["path"].as_str().unwrap_or(""); let s:Vec<String>=args["sources"].as_array().map(|a|a.iter().filter_map(|v|v.as_str().map(String::from)).collect()).unwrap_or_default(); let f=args["format"].as_str().unwrap_or("tar.gz"); volt::tools::archive_tool::archive_create(p,&s,f).await }))).await;

    registry
}

#[tokio::test]
async fn test_all_tools_direct_benchmarks() {
    let _ = dotenvy::dotenv();
    let mut results = Vec::new();

    // ── 1. bash tool ──
    let started = Instant::now();
    let res =
        volt::tools::bash::execute_bash("echo 'hello world' && rustc --version && cargo --version")
            .await;
    results.push((
        "bash".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 2. read tool ──
    let started = Instant::now();
    let res = volt::tools::read_tool::read_file("Cargo.toml").await;
    results.push((
        "read_file".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 3. write tool ──
    let started = Instant::now();
    let res =
        volt::tools::write_tool::write_file("_bench_test_write.txt", "benchmark write test").await;
    results.push((
        "write_file".to_string(),
        started.elapsed().as_millis(),
        res.success,
        0,
    ));

    // ── 4. edit tool ──
    let started = Instant::now();
    let res =
        volt::tools::edit::edit_file("_bench_test_write.txt", "benchmark", "benchmark-edited")
            .await;
    results.push((
        "edit_file".to_string(),
        started.elapsed().as_millis(),
        res.success,
        0,
    ));

    // ── 5. glob tool ──
    let started = Instant::now();
    let res = volt::tools::glob_tool::glob_files("*.rs", "src/tools").await;
    results.push((
        "glob_files".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 6. grep tool ──
    let started = Instant::now();
    let res = volt::tools::grep_tool::grep_files("pub async fn", "src/tools").await;
    results.push((
        "grep_files".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 7. web_fetch tool ──
    let started = Instant::now();
    let res = volt::tools::web_tool::web_fetch("https://httpbin.org/get").await;
    results.push((
        "web_fetch".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 8. json_validate tool ──
    let started = Instant::now();
    let res = volt::tools::json_tool::json_validate(r#"{"name":"volt","version":"0.1.0"}"#).await;
    results.push((
        "json_validate".to_string(),
        started.elapsed().as_millis(),
        res.success,
        0,
    ));

    // ── 9. json_prettify tool ──
    let started = Instant::now();
    let res = volt::tools::json_tool::json_prettify(r#"{"a":1,"b":{"c":2}}"#, 2).await;
    results.push((
        "json_prettify".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 10. json_query tool ──
    let started = Instant::now();
    let res = volt::tools::json_tool::json_query(
        r#"{"store":{"book":[{"title":"Rust Book"}]}}"#,
        "store.book[0].title",
    )
    .await;
    results.push((
        "json_query".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 11. csv_write + csv_read ──
    let started = Instant::now();
    let csv_data = "name,role,model\nagent1,fs,gpt-4\nagent2,data,claude-3\nagent3,system,groq-llama\nagent4,web,gpt-4o\nagent5,memory,groq-mixtral";
    let w = volt::tools::csv_tool::csv_write("_bench_agents.csv", csv_data, true).await;
    let r = volt::tools::csv_tool::csv_read("_bench_agents.csv", true).await;
    results.push((
        "csv_readwrite".to_string(),
        started.elapsed().as_millis(),
        w.success && r.success,
        r.output.len(),
    ));

    // ── 12. archive_create + archive_extract ──
    let started = Instant::now();
    let arc = volt::tools::archive_tool::archive_create(
        "_bench_archive.tar.gz",
        &["_bench_test_write.txt".to_string()],
        "tar.gz",
    )
    .await;
    let ext =
        volt::tools::archive_tool::archive_extract("_bench_archive.tar.gz", "_bench_extracted")
            .await;
    results.push((
        "archive_create_extract".to_string(),
        started.elapsed().as_millis(),
        arc.success && ext.success,
        0,
    ));

    // ── 13. memory_append ──
    let started = Instant::now();
    let res = volt::tools::memory_tool::memory_append(
        "benchmark",
        "tool benchmark completed successfully",
    )
    .await;
    results.push((
        "memory_append".to_string(),
        started.elapsed().as_millis(),
        res.success,
        0,
    ));

    // ── 14. todo_add ──
    let started = Instant::now();
    let res = volt::tools::todo_tool::todo_add("Run comprehensive tool benchmarks").await;
    results.push((
        "todo_add".to_string(),
        started.elapsed().as_millis(),
        res.success,
        0,
    ));

    // ── 15. web_scrape (targeting a simple page) ──
    let started = Instant::now();
    let res = volt::tools::scrape_tool::web_scrape("https://httpbin.org/html", "h1").await;
    results.push((
        "web_scrape".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── 16. web_scrape_all ──
    let started = Instant::now();
    let res = volt::tools::scrape_tool::web_scrape_all("https://httpbin.org/html").await;
    results.push((
        "web_scrape_all".to_string(),
        started.elapsed().as_millis(),
        res.success,
        res.output.len(),
    ));

    // ── Print results ──
    println!("\n{}", "=".repeat(100));
    println!("{:^100}", "TOOL BENCHMARK RESULTS");
    println!("{}", "=".repeat(100));
    println!(
        "{:<30} {:>15} {:>15} {:>15}",
        "Tool", "Time (ms)", "Success", "Output Size"
    );
    println!("{}", "-".repeat(100));
    let mut total_ms = 0u128;
    let mut passed = 0u32;
    let mut failed = 0u32;
    for (name, ms, ok, size) in &results {
        let status = if *ok { "PASS" } else { "FAIL" };
        println!("{:<30} {:>15} {:>15} {:>15}", name, ms, status, size);
        total_ms += ms;
        if *ok {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    println!("{}", "-".repeat(100));
    println!(
        "{:<30} {:>15} {:>15}/{:>5} {:>15}",
        "TOTAL",
        total_ms,
        passed,
        passed + failed,
        ""
    );
    println!("{}\n", "=".repeat(100));

    // Cleanup temp files
    let _ = std::fs::remove_file("_bench_test_write.txt");
    let _ = std::fs::remove_file("_bench_agents.csv");
    let _ = std::fs::remove_file("_bench_archive.tar.gz");
    let _ = std::fs::remove_dir_all("_bench_extracted");

    assert!(passed > 0, "at least some tools should pass");
}

#[tokio::test]
async fn test_parallel_multi_agent_workflow() {
    dotenvy::dotenv().ok();
    // Override stale session env vars from .env
    if let Ok(content) = std::fs::read_to_string(".env") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                std::env::set_var(k.trim(), v.trim());
            }
        }
    }
    let tools = register_all_tools().await;

    let agents_json = r#"[
        {"name":"fs-agent","model":"llama-3.1-8b-instant","system_prompt":"You are a file system analysis agent. Use glob to find Rust source files and read them. Report findings concisely.","max_iterations":8,"temperature":0.1,"allow_all":true},
        {"name":"data-agent","model":"llama-3.1-8b-instant","system_prompt":"You are a data processing agent. You can create CSV data, read/write files, and format JSON. Report results concisely.","max_iterations":8,"temperature":0.1,"allow_all":true},
        {"name":"system-agent","model":"llama-3.1-8b-instant","system_prompt":"You are a system inspection agent. Use bash to run commands and read configuration files. Report versions and metrics concisely.","max_iterations":8,"temperature":0.1,"allow_all":true}
    ]"#;
    let tasks_json = r#"[
        "Use glob to find all .rs files in the src/tools/ directory, then read src/tools/mod.rs and list every registered tool module. Count them and report their names.",
        "Write a CSV file called test_agents.csv with columns: name, role, model. Add 5 rows of sample agents. Then read it back, validate and prettify the JSON output.",
        "Use bash to run 'rustc --version' and 'cargo --version'. Then read Cargo.toml and report the project name, version, description, and total number of dependencies."
    ]"#;

    let specs = parse_agent_specs(agents_json).expect("valid agent specs");
    let tasks: Vec<String> = serde_json::from_str(tasks_json).expect("valid tasks JSON");

    let orch = Orchestrator::new(tools);
    let started = Instant::now();
    let result = orch.run_workflow("parallel", specs, tasks).await;

    let total_duration = started.elapsed().as_millis();

    println!("\n{}", "=".repeat(100));
    println!("{:^100}", "PARALLEL MULTI-AGENT WORKFLOW RESULTS");
    println!("{}", "=".repeat(100));

    match result {
        Ok(wf_result) => {
            let total_prompt: u64 = wf_result.steps.iter().map(|s| s.prompt_tokens).sum();
            let total_completion: u64 = wf_result.steps.iter().map(|s| s.completion_tokens).sum();
            println!(
                "\nTotal workflow duration: {} ms",
                wf_result.total_duration_ms
            );
            println!("Steps completed: {}", wf_result.steps.len());
            println!(
                "Total tokens: {} prompt + {} completion = {} total",
                total_prompt,
                total_completion,
                total_prompt + total_completion
            );
            println!();
            for (i, step) in wf_result.steps.iter().enumerate() {
                let status = if step.success { "PASS" } else { "FAIL" };
                println!(
                    "  Step {}: [{}] {} ({} ms, {}P+{}C tokens)",
                    i + 1,
                    status,
                    step.agent_name,
                    step.duration_ms,
                    step.prompt_tokens,
                    step.completion_tokens
                );
                let preview: String = step.output.chars().take(200).collect();
                if !preview.is_empty() {
                    println!("    Output preview: {}", preview);
                }
                println!();
            }
            println!("{}", "-".repeat(100));
            println!(
                "Final output:\n{}",
                wf_result.final_output.chars().take(500).collect::<String>()
            );
            println!("{}", "-".repeat(100));
        }
        Err(e) => {
            println!("Workflow error: {}", e);
        }
    }

    println!("{}", "=".repeat(100));
    println!("Total benchmark time: {} ms\n", total_duration);

    // Cleanup
    let _ = std::fs::remove_file("test_agents.csv");
}

#[tokio::test]
async fn test_pipeline_workflow() {
    dotenvy::dotenv().ok();
    if let Ok(content) = std::fs::read_to_string(".env") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                std::env::set_var(k.trim(), v.trim());
            }
        }
    }
    let tools = register_all_tools().await;

    // Pipeline: Stage 1 discovers info, Stage 2 processes it
    let agents_json = r#"[
        {"name":"discover-agent","model":"llama-3.1-8b-instant","system_prompt":"You are a discovery agent. Read Cargo.toml and use glob/grep to find all registered tool names. List them.","max_iterations":6,"temperature":0.1,"allow_all":true},
        {"name":"report-agent","model":"llama-3.1-8b-instant","system_prompt":"You are a reporting agent. Take the previous agent's findings and create a formatted markdown summary. Use bash to count lines of code. Be concise.","max_iterations":6,"temperature":0.1,"allow_all":true}
    ]"#;
    let tasks_json = r#"[
        "Read Cargo.toml and report the project version, description. Then use glob to find all .rs files, grep to count total lines of Rust code, and list all tool modules from src/tools/mod.rs.",
        "Based on the previous results: {prev}, create a summary. Use bash to run 'cargo count' or 'find src -name \"*.rs\" | xargs wc -l' to verify code metrics."
    ]"#;

    let specs = parse_agent_specs(agents_json).expect("valid agent specs");
    let tasks: Vec<String> = serde_json::from_str(tasks_json).expect("valid tasks JSON");

    let orch = Orchestrator::new(tools);
    let started = Instant::now();
    let result = orch.run_workflow("pipeline", specs, tasks).await;

    let total_duration = started.elapsed().as_millis();

    println!("\n{}", "=".repeat(100));
    println!("{:^100}", "PIPELINE WORKFLOW RESULTS (Sequential Chaining)");
    println!("{}", "=".repeat(100));

    match result {
        Ok(wf_result) => {
            let total_prompt: u64 = wf_result.steps.iter().map(|s| s.prompt_tokens).sum();
            let total_completion: u64 = wf_result.steps.iter().map(|s| s.completion_tokens).sum();
            println!(
                "\nTotal workflow duration: {} ms",
                wf_result.total_duration_ms
            );
            println!("Steps completed: {}", wf_result.steps.len());
            println!(
                "Total tokens: {} prompt + {} completion = {} total",
                total_prompt,
                total_completion,
                total_prompt + total_completion
            );
            println!();
            for (i, step) in wf_result.steps.iter().enumerate() {
                let status = if step.success { "PASS" } else { "FAIL" };
                println!(
                    "  Stage {}: [{}] {} ({} ms, {}P+{}C tokens)",
                    i + 1,
                    status,
                    step.agent_name,
                    step.duration_ms,
                    step.prompt_tokens,
                    step.completion_tokens
                );
            }
            println!();
            println!("{}", "-".repeat(100));
            println!(
                "Final output:\n{}",
                wf_result.final_output.chars().take(800).collect::<String>()
            );
            println!("{}", "-".repeat(100));
        }
        Err(e) => {
            println!("Pipeline workflow error: {}", e);
        }
    }
    println!("{}", "=".repeat(100));
    println!("Total benchmark time: {} ms\n", total_duration);
}
