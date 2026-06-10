// Chart, PDF, CSV, and Archive tools groups
use crate::register_tool;
use crate::tools::registry::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;

pub async fn register_chart_tools(registry: &Arc<ToolRegistry>) {
    let chart_schema = || {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "labels": { "type": "array", "items": { "type": "string" } },
                "values": { "type": "array", "items": { "type": "number" } },
                "output_path": { "type": "string" }
            },
            "required": ["title", "labels", "values", "output_path"]
        })
    };

    register_tool!(
        registry,
        "create_bar_chart",
        "Create a bar chart from labels and values, save as HTML.",
        chart_schema(),
        "builtin",
        |args: Value| async move {
            let t = args["title"].as_str().unwrap_or("Chart");
            let l: Vec<String> = args["labels"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let v: Vec<f64> = args["values"]
                .as_array()
                .map(|a| a.iter().filter_map(|n| n.as_f64()).collect())
                .unwrap_or_default();
            let o = args["output_path"].as_str().unwrap_or("chart.html");
            crate::tools::chart_tool::create_bar_chart(t, l, v, o).await
        }
    );

    register_tool!(
        registry,
        "create_line_chart",
        "Create a line chart from labels and values, save as HTML.",
        chart_schema(),
        "builtin",
        |args: Value| async move {
            let t = args["title"].as_str().unwrap_or("Chart");
            let l: Vec<String> = args["labels"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let v: Vec<f64> = args["values"]
                .as_array()
                .map(|a| a.iter().filter_map(|n| n.as_f64()).collect())
                .unwrap_or_default();
            let o = args["output_path"].as_str().unwrap_or("chart.html");
            crate::tools::chart_tool::create_line_chart(t, l, v, o).await
        }
    );
}

pub async fn register_pdf_tools(registry: &Arc<ToolRegistry>) {
    #[cfg(feature = "tools-pdf")]
    {
        use crate::attenuation::TrustLevel;
        use crate::models::PermissionLevel;
        register_tool_with_permission!(
            registry,
            "create_pdf",
            "Create a PDF document from text content.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "text content" },
                    "output_path": { "type": "string", "description": "output .pdf path" }
                },
                "required": ["content", "output_path"]
            }),
            "builtin",
            |args: Value| async move {
                let c = args["content"].as_str().unwrap_or("");
                let o = args["output_path"].as_str().unwrap_or("output.pdf");
                crate::tools::pdf_tool::create_pdf(c, o).await
            },
            PermissionLevel::Prompt,
            TrustLevel::Builtin
        );
    }
    #[cfg(not(feature = "tools-pdf"))]
    {
        let _ = registry;
    }
}

pub async fn register_csv_tools(registry: &Arc<ToolRegistry>) {
    register_tool!(
        registry,
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
        |args: Value| async move {
            let path = args["path"].as_str().unwrap_or("");
            let has_header = args["has_header"].as_bool().unwrap_or(true);
            crate::tools::csv_tool::csv_read(path, has_header).await
        }
    );

    register_tool!(
        registry,
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
        |args: Value| async move {
            let path = args["path"].as_str().unwrap_or("");
            let data = args["data"].as_str().unwrap_or("");
            let has_header = args["has_header"].as_bool().unwrap_or(true);
            crate::tools::csv_tool::csv_write(path, data, has_header).await
        }
    );
}

pub async fn register_archive_tools(registry: &Arc<ToolRegistry>) {
    register_tool!(
        registry,
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
        |args: Value| async move {
            let path = args["path"].as_str().unwrap_or("");
            let dest = args["dest"].as_str().unwrap_or("");
            crate::tools::archive_tool::archive_extract(path, dest).await
        }
    );

    register_tool!(
        registry,
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
        |args: Value| async move {
            let path = args["path"].as_str().unwrap_or("");
            let sources: Vec<String> = args["sources"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let format = args["format"].as_str().unwrap_or("tar.gz");
            crate::tools::archive_tool::archive_create(path, &sources, format).await
        }
    );
}
