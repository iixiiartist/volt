use crate::models::{PermissionLevel, ToolResult};
use crate::tools::ToolRegistry;
use std::sync::Arc;

pub async fn setup_tools(
    embedder: Option<&crate::embedding::EmbeddingClient>,
) -> Arc<ToolRegistry> {
    let registry = crate::tools::register_all_tools().await;
    if let Some(emb) = embedder {
        registry.compute_embeddings(emb).await;
    }
    registry
}

pub async fn register_all_tools() -> Arc<ToolRegistry> {
    let registry = ToolRegistry::new();
    let minimal = std::env::var("VOLT_MINIMAL_TOOLS").is_ok();
    let bfcl_mode = std::env::var("VOLT_BFCL_MODE").is_ok();

    if !bfcl_mode {
        registry
            .register_with_permission(
                "bash",
                "Execute a shell command",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "shell command to run" }
                    },
                    "required": ["command"]
                }),
                "builtin",
                Arc::new(|args| {
                    Box::pin(async move {
                        let cmd = args["command"].as_str().unwrap_or("");
                        crate::tools::bash::execute_bash(cmd).await
                    })
                }),
                PermissionLevel::Prompt,
            )
            .await;
    }

    registry
        .register_with_permission(
            "read",
            "Read a file from disk",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "file path to read" }
                },
                "required": ["path"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    crate::tools::read_tool::read_file(path).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register(
            "glob",
            "Find files matching a glob pattern",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "glob pattern" },
                    "base": { "type": "string", "description": "base directory" }
                },
                "required": ["pattern"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let pattern = args["pattern"].as_str().unwrap_or("*");
                    let base = args["base"].as_str().unwrap_or(".");
                    crate::tools::glob_tool::glob_files(pattern, base).await
                })
            }),
        )
        .await;

    registry
        .register(
            "grep",
            "Search file contents with regex",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "regex pattern" },
                    "path": { "type": "string", "description": "directory to search" }
                },
                "required": ["pattern"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let pattern = args["pattern"].as_str().unwrap_or("");
                    let path = args["path"].as_str().unwrap_or(".");
                    crate::tools::grep_tool::grep_files(pattern, path).await
                })
            }),
        )
        .await;

    registry
        .register_with_permission(
            "web_fetch",
            "Fetch a URL and return its content",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    crate::tools::web_tool::web_fetch(url).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register(
            "memory_append",
            "Append to persistent memory file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "description": "memory category" },
                    "content": { "type": "string", "description": "content to remember" }
                },
                "required": ["kind", "content"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let kind = args["kind"].as_str().unwrap_or("note");
                    let content = args["content"].as_str().unwrap_or("");
                    crate::tools::memory_tool::memory_append(kind, content).await
                })
            }),
        )
        .await;

    registry
        .register(
            "todo_add",
            "Add a task to the todo list",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "task description" }
                },
                "required": ["task"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let task = args["task"].as_str().unwrap_or("");
                    crate::tools::todo_tool::todo_add(task).await
                })
            }),
        )
        .await;

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
    registry.register_with_permission("delegate", "Delegate a sub-task to a sub-agent and return its result", serde_json::json!({
        "type": "object",
        "properties": {
            "task": { "type": "string", "description": "task description for the sub-agent" },
            "context": { "type": "string", "description": "context and constraints from the parent agent" }
        },
        "required": ["task"]
    }), "builtin", delegate_fn, PermissionLevel::Prompt).await;

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
                            let orch = crate::orchestrator::Orchestrator::new(wt.clone());
                            match orch.run_workflow(pattern, specs, tasks).await {
                                Ok(result) => crate::models::ToolResult {
                                    success: true,
                                    output: result.final_output,
                                    error: None,
                                    duration_ms: started.elapsed().as_millis(),
                                },
                                Err(e) => crate::models::ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!("workflow error: {}", e)),
                                    duration_ms: started.elapsed().as_millis(),
                                },
                            }
                        }
                        Err(e) => crate::models::ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("invalid tasks JSON: {}", e)),
                            duration_ms: started.elapsed().as_millis(),
                        },
                    },
                    Err(e) => crate::models::ToolResult {
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
    registry.register_with_permission("run_workflow", "Execute a multi-agent workflow (parallel or pipeline) and return combined results", serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "workflow pattern: 'parallel' or 'pipeline'" },
            "agents": { "type": "string", "description": "JSON array of agent specs, each with 'name' (required) and optional 'model', 'system_prompt', 'max_iterations', 'temperature'" },
            "tasks": { "type": "string", "description": "JSON array of task strings (one per agent for parallel, one per stage for pipeline)" }
        },
        "required": ["pattern", "agents", "tasks"]
    }), "builtin", workflow_fn, PermissionLevel::Prompt).await;

    registry
        .register_with_permission(
            "web_scrape",
            "Extract structured content from a URL using a CSS selector. Returns text content of all matching elements.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to scrape" },
                    "selector": { "type": "string", "description": "CSS selector to match elements" }
                },
                "required": ["url", "selector"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    let selector = args["selector"].as_str().unwrap_or("");
                    crate::tools::scrape_tool::web_scrape(url, selector).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register_with_permission(
            "web_scrape_all",
            "Fetch a URL and extract all human-readable content (headings, paragraphs, links). General-purpose page reading without needing a CSS selector.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch and extract" }
                },
                "required": ["url"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let url = args["url"].as_str().unwrap_or("");
                    crate::tools::scrape_tool::web_scrape_all(url).await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry
        .register(
            "json_validate",
            "Validate JSON string and return its type (object, array, string, number, boolean, null).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON string to validate" }
                },
                "required": ["data"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let data = args["data"].as_str().unwrap_or("");
                    crate::tools::json_tool::json_validate(data).await
                })
            }),
        )
        .await;

    registry
        .register(
            "json_prettify",
            "Format JSON with custom indentation for readability.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON string to format" },
                    "indent": { "type": "integer", "description": "spaces per indent level (default: 2)" }
                },
                "required": ["data"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let data = args["data"].as_str().unwrap_or("");
                    let indent = args["indent"].as_u64().unwrap_or(2) as u8;
                    crate::tools::json_tool::json_prettify(data, indent).await
                })
            }),
        )
        .await;

    registry
        .register(
            "json_query",
            "Extract a value from JSON using a dot-separated path (e.g. 'store.book[0].title'). Supports nested objects and array indexing.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON string to query" },
                    "path": { "type": "string", "description": "dot-separated path with optional array indices (e.g. 'items[0].name')" }
                },
                "required": ["data", "path"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let data = args["data"].as_str().unwrap_or("");
                    let path = args["path"].as_str().unwrap_or("");
                    crate::tools::json_tool::json_query(data, path).await
                })
            }),
        )
        .await;

    // ── you.com web search ────────────────────────────────────────────────
    if !bfcl_mode {
        registry
            .register(
                "web_search",
            "Search the web for real-time information using you.com Search API. Returns structured results with URLs, titles, snippets, and optional full-page content via livecrawl. Use this when you need current information from the internet.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" },
                    "count": { "type": "integer", "description": "Number of results (1-100, default 10)", "default": 10 },
                    "freshness": { "type": "string", "description": "Recency filter: day, week, month, year, or YYYY-MM-DDtoYYYY-MM-DD", "enum": ["day", "week", "month", "year"], "default": null },
                    "livecrawl": { "type": "string", "description": "Fetch full page content: web, news, or all", "enum": ["web", "news", "all"], "default": null }
                },
                "required": ["query"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let query = args["query"].as_str().unwrap_or("");
                    let count = args["count"].as_u64().map(|c| c as u32);
                    let freshness = args["freshness"].as_str();
                    let livecrawl = args["livecrawl"].as_str();
                    crate::tools::you_tools::web_search(query, count, freshness, livecrawl).await
                })
            }),
        )
        .await;

        // ── you.com research (deep research) ──────────────────────────────────
        registry
        .register(
            "you_research",
            "Deep research via you.com Research API. Runs multiple searches, reads sources, and synthesizes a thorough, well-cited answer. Use for complex questions requiring multi-step research.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "The research question or topic" },
                    "research_effort": { "type": "string", "description": "Research depth: lite, standard, deep, or exhaustive", "enum": ["lite", "standard", "deep", "exhaustive"], "default": "standard" }
                },
                "required": ["input"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let input = args["input"].as_str().unwrap_or("");
                    let effort = args["research_effort"].as_str();
                    crate::tools::you_tools::you_research(input, effort).await
                })
            }),
        )
        .await;

        // ── you.com contents ──────────────────────────────────────────────────
        registry
        .register(
            "you_contents",
            "Fetch clean Markdown or HTML content from specific URLs using you.com Contents API. Takes a list of URLs and returns structured page content. Use when you already have URLs and need their full text content.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "urls": { "type": "array", "items": { "type": "string" }, "description": "List of URLs to fetch content from" },
                    "format": { "type": "string", "description": "Content format: markdown or html", "enum": ["markdown", "html"], "default": "markdown" }
                },
                "required": ["urls"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let urls: Vec<String> = args["urls"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let fmt = args["format"].as_str();
                    crate::tools::you_tools::you_contents(&urls, fmt).await
                })
            }),
        )
        .await;
    }

    registry
        .register(
            "final_answer",
            "Submit your final answer and terminate. Call this when you have determined the answer to the user's question.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "answer": {
                        "type": "string",
                        "description": "The final answer to the question"
                    }
                },
                "required": ["answer"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let answer = args["answer"].as_str().unwrap_or("");
                    crate::tools::final_answer::final_answer(answer).await
                })
            }),
        )
        .await;

    if minimal {
        // Benchmark / minimal mode: only load essential tools to keep system prompt small
        return registry;
    }

    #[cfg(feature = "tools-screenshot")]
    registry
        .register_with_permission(
            "screenshot",
            "Capture a screenshot of the primary monitor. Returns a base64-encoded PNG image. Use this to see what's on screen.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            "builtin",
            Arc::new(|_args| {
                Box::pin(async move {
                    crate::tools::screenshot::capture_screenshot().await
                })
            }),
            PermissionLevel::Prompt,
        )
        .await;

    registry.register("create_bar_chart","Create a bar chart from labels and values, save as HTML.",
        serde_json::json!({"type":"object","properties":{"title":{"type":"string"},"labels":{"type":"array","items":{"type":"string"}},"values":{"type":"array","items":{"type":"number"}},"output_path":{"type":"string"}},"required":["title","labels","values","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["title"].as_str().unwrap_or("Chart");
            let l: Vec<String> = args["labels"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let v: Vec<f64> = args["values"].as_array().map(|a| a.iter().filter_map(|n| n.as_f64()).collect()).unwrap_or_default();
            let o = args["output_path"].as_str().unwrap_or("chart.html");
            crate::tools::chart_tool::create_bar_chart(t, l, v, o).await
        }))).await;

    registry.register("create_line_chart","Create a line chart from labels and values, save as HTML.",
        serde_json::json!({"type":"object","properties":{"title":{"type":"string"},"labels":{"type":"array","items":{"type":"string"}},"values":{"type":"array","items":{"type":"number"}},"output_path":{"type":"string"}},"required":["title","labels","values","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["title"].as_str().unwrap_or("Chart");
            let l: Vec<String> = args["labels"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let v: Vec<f64> = args["values"].as_array().map(|a| a.iter().filter_map(|n| n.as_f64()).collect()).unwrap_or_default();
            let o = args["output_path"].as_str().unwrap_or("chart.html");
            crate::tools::chart_tool::create_line_chart(t, l, v, o).await
        }))).await;
    #[cfg(feature = "tools-pdf")]
    registry.register_with_permission("create_pdf","Create a PDF document from text content.",
        serde_json::json!({"type":"object","properties":{"content":{"type":"string","description":"text content"},"output_path":{"type":"string","description":"output .pdf path"}},"required":["content","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let c = args["content"].as_str().unwrap_or(""); let o = args["output_path"].as_str().unwrap_or("output.pdf");
            crate::tools::pdf_tool::create_pdf(c, o).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register_with_permission("desktop_click","Click at screen coordinates.",
        serde_json::json!({"type":"object","properties":{"x":{"type":"integer"},"y":{"type":"integer"}},"required":["x","y"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let x = args["x"].as_i64().unwrap_or(0) as i32; let y = args["y"].as_i64().unwrap_or(0) as i32;
            crate::tools::desktop_tool::desktop_click(x, y).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register_with_permission("desktop_type","Type text at cursor position.",
        serde_json::json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["text"].as_str().unwrap_or("");
            crate::tools::desktop_tool::desktop_type(t).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register_with_permission("desktop_key","Press a key (enter, tab, escape, up, down, etc.).",
        serde_json::json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let k = args["key"].as_str().unwrap_or("");
            crate::tools::desktop_tool::desktop_key(k).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-desktop")]
    registry.register("desktop_find_window","Find a window by title using Windows API.",
        serde_json::json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let t = args["title"].as_str().unwrap_or("");
            crate::tools::desktop_tool::desktop_find_window(t).await
        }))).await;

    #[cfg(feature = "tools-browser")]
    registry.register_with_permission("browser_navigate","Open a URL in headless Chrome and return the URL.",
        serde_json::json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let u = args["url"].as_str().unwrap_or("");
            crate::tools::browser_tool::browser_navigate(u).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-browser")]
    registry.register_with_permission("browser_extract","Open a URL and extract text via CSS selector.",
        serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"selector":{"type":"string"}},"required":["url","selector"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let u = args["url"].as_str().unwrap_or(""); let s = args["selector"].as_str().unwrap_or("");
            crate::tools::browser_tool::browser_extract(u, s).await
        })), PermissionLevel::Prompt).await;

    #[cfg(feature = "tools-browser")]
    registry.register_with_permission("browser_screenshot","Open a URL and save a page screenshot.",
        serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"output_path":{"type":"string"}},"required":["url","output_path"]}),"builtin",
        Arc::new(|args| Box::pin(async move {
            let u = args["url"].as_str().unwrap_or(""); let o = args["output_path"].as_str().unwrap_or("screenshot.png");
            crate::tools::browser_tool::browser_screenshot(u, o).await
        })), PermissionLevel::Prompt).await;

    registry
        .register(
            "csv_read",
            "Read a CSV file and return its contents as formatted rows. Supports flexible column counts and optional headers.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "path to CSV file" },
                    "has_header": { "type": "boolean", "description": "whether the CSV has a header row (default: true)" }
                },
                "required": ["path"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let has_header = args["has_header"].as_bool().unwrap_or(true);
                    crate::tools::csv_tool::csv_read(path, has_header).await
                })
            }),
        )
        .await;

    registry
        .register(
            "csv_write",
            "Write data to a CSV file. Provide data as comma-separated lines, first line is header if has_header is true.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "path to CSV file" },
                    "data": { "type": "string", "description": "CSV data, one row per line, comma-separated values" },
                    "has_header": { "type": "boolean", "description": "whether first line is a header row (default: true)" }
                },
                "required": ["path", "data"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let data = args["data"].as_str().unwrap_or("");
                    let has_header = args["has_header"].as_bool().unwrap_or(true);
                    crate::tools::csv_tool::csv_write(path, data, has_header).await
                })
            }),
        )
        .await;

    registry
        .register(
            "archive_extract",
            "Extract an archive file (tar.gz, tgz, tar, gz) to a destination directory.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "path to archive file" },
                    "dest": { "type": "string", "description": "destination directory to extract into" }
                },
                "required": ["path", "dest"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let dest = args["dest"].as_str().unwrap_or("");
                    crate::tools::archive_tool::archive_extract(path, dest).await
                })
            }),
        )
        .await;

    registry
        .register(
            "archive_create",
            "Create a tar or tar.gz archive from a list of source files/directories.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "output archive path" },
                    "sources": { "type": "array", "items": { "type": "string" }, "description": "list of files and directories to include" },
                    "format": { "type": "string", "description": "archive format: 'tar' or 'tar.gz' (default: 'tar.gz')" }
                },
                "required": ["path", "sources"]
            }),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let path = args["path"].as_str().unwrap_or("");
                    let sources: Vec<String> = args["sources"].as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    let format = args["format"].as_str().unwrap_or("tar.gz");
                    crate::tools::archive_tool::archive_create(path, &sources, format).await
                })
            }),
        )
        .await;

    // ── Git tools ─────────────────────────────────────────────────────────
    registry.register("git_status", "Show the working tree status (porcelain format).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_status(repo).await
    }))).await;

    registry.register("git_diff_unstaged", "Show unstaged changes in the working directory.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_diff_unstaged(repo).await
    }))).await;

    registry.register("git_diff_staged", "Show staged changes (diff --cached).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_diff_staged(repo).await
    }))).await;

    registry.register("git_diff", "Show differences between branches or commits.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "target": { "type": "string", "description": "branch, commit, or range to diff against" }
        },
        "required": ["target"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let target = args["target"].as_str().unwrap_or("HEAD");
        crate::tools::git_tool::git_diff(repo, target).await
    }))).await;

    registry.register("git_commit", "Record changes to the repository.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "message": { "type": "string", "description": "commit message" }
        },
        "required": ["message"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let msg = args["message"].as_str().unwrap_or("");
        crate::tools::git_tool::git_commit(repo, msg).await
    }))).await;

    registry.register("git_add", "Add file contents to the staging area.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "files": { "type": "array", "items": { "type": "string" }, "description": "files to stage" }
        },
        "required": ["files"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let files: Vec<String> = args["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        crate::tools::git_tool::git_add(repo, &files).await
    }))).await;

    registry.register("git_reset", "Unstage all staged changes.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_reset(repo).await
    }))).await;

    registry.register("git_log", "Show commit logs (oneline format).", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "max_count": { "type": "number", "description": "maximum number of commits to show (default: 20)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let count = args["max_count"].as_u64().unwrap_or(20) as u32;
        crate::tools::git_tool::git_log(repo, count).await
    }))).await;

    registry.register("git_create_branch", "Create a new branch.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "branch": { "type": "string", "description": "name of the new branch" },
            "base": { "type": "string", "description": "optional base branch or commit" }
        },
        "required": ["branch"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let branch = args["branch"].as_str().unwrap_or("");
        let base = args["base"].as_str();
        crate::tools::git_tool::git_create_branch(repo, branch, base).await
    }))).await;

    registry.register("git_checkout", "Switch branches.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "branch": { "type": "string", "description": "branch to switch to" }
        },
        "required": ["branch"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let branch = args["branch"].as_str().unwrap_or("");
        crate::tools::git_tool::git_checkout(repo, branch).await
    }))).await;

    registry.register("git_show", "Show the contents of a commit.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" },
            "revision": { "type": "string", "description": "revision (commit hash, branch, tag)" }
        },
        "required": ["revision"]
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        let rev = args["revision"].as_str().unwrap_or("HEAD");
        crate::tools::git_tool::git_show(repo, rev).await
    }))).await;

    registry.register("git_branch", "List git branches.", serde_json::json!({
        "type": "object",
        "properties": {
            "repo_path": { "type": "string", "description": "path to git repository (default: current dir)" }
        }
    }), "git", Arc::new(|args| Box::pin(async move {
        let repo = args["repo_path"].as_str().unwrap_or(".");
        crate::tools::git_tool::git_branch(repo).await
    }))).await;

    // ── Time tools ─────────────────────────────────────────────────────────
    registry.register("get_current_time", "Get the current time in a specific timezone.", serde_json::json!({
        "type": "object",
        "properties": {
            "timezone": { "type": "string", "description": "IANA timezone (e.g. 'America/New_York', 'UTC', 'Asia/Tokyo')" }
        },
        "required": ["timezone"]
    }), "utilities", Arc::new(|args| Box::pin(async move {
        let tz = args["timezone"].as_str().unwrap_or("UTC");
        crate::tools::time_tool::get_current_time(tz).await
    }))).await;

    registry
        .register(
            "convert_time",
            "Convert time between timezones.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "timezone": { "type": "string", "description": "source IANA timezone" },
                    "timezone_to": { "type": "string", "description": "target IANA timezone" }
                },
                "required": ["timezone", "timezone_to"]
            }),
            "utilities",
            Arc::new(|args| {
                Box::pin(async move {
                    let from = args["timezone"].as_str().unwrap_or("UTC");
                    let to = args["timezone_to"].as_str().unwrap_or("UTC");
                    crate::tools::time_tool::convert_time(from, to).await
                })
            }),
        )
        .await;

    // ── Sequential thinking ────────────────────────────────────────────────
    registry.register("sequentialthinking", "A detailed tool for dynamic and reflective problem-solving through structured thoughts. Use when the task requires careful reasoning, multi-step analysis, or exploring alternative solutions.", serde_json::json!({
        "type": "object",
        "properties": {
            "thought": { "type": "string", "description": "your current thought or reasoning step" },
            "next_thought_needed": { "type": "boolean", "description": "whether another thought step is needed" },
            "branch_id": { "type": "string", "description": "optional branch ID to explore alternative reasoning paths" },
            "branch_from_thought": { "type": "number", "description": "optional thought number to branch from" }
        },
        "required": ["thought", "next_thought_needed"]
    }), "reasoning", Arc::new(|args| Box::pin(async move {
        let thought = args["thought"].as_str().unwrap_or("");
        let next = args["next_thought_needed"].as_bool().unwrap_or(true);
        let branch_id = args["branch_id"].as_str();
        let branch_from = args["branch_from_thought"].as_u64().map(|n| n as u32);
        crate::tools::sequential_thinking::sequentialthinking(thought, next, branch_id, branch_from).await
    }))).await;

    registry
}
