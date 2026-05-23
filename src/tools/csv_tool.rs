use crate::models::ToolResult;
use std::time::Instant;

pub async fn csv_read(path: &str, has_header: bool) -> ToolResult {
    let started = Instant::now();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return ToolResult {
            success: false, output: String::new(), error: Some(format!("read failed: {}", e)), duration_ms: started.elapsed().as_millis(),
        },
    };

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(has_header)
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers: Vec<String> = reader.headers().ok().map(|h| h.iter().map(|s| s.to_string()).collect()).unwrap_or_default();
    let mut records = Vec::new();
    for result in reader.records() {
        match result {
            Ok(r) => {
                let row: Vec<String> = r.iter().map(|s| s.to_string()).collect();
                records.push(row);
            }
            Err(e) => {
                records.push(vec![format!("<parse error: {}>", e)]);
            }
        }
    }

    let mut out = String::new();
    if !headers.is_empty() {
        out.push_str(&format!("Headers: {}\n", headers.join(" | ")));
        out.push_str(&format!("---\n"));
    }
    out.push_str(&format!("{} rows\n", records.len()));
    for (i, row) in records.iter().enumerate().take(100) {
        out.push_str(&format!("{}: {}\n", i + 1, row.join(" | ")));
    }
    if records.len() > 100 {
        out.push_str(&format!("... and {} more rows\n", records.len() - 100));
    }

    ToolResult {
        success: true,
        output: out,
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}

pub async fn csv_write(path: &str, data: &str, has_header: bool) -> ToolResult {
    let started = Instant::now();

    let lines: Vec<&str> = data.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return ToolResult {
            success: false, output: String::new(), error: Some("no data provided".into()), duration_ms: started.elapsed().as_millis(),
        };
    }

    let wtr = std::fs::File::create(path);
    let mut writer = csv::WriterBuilder::new()
        .has_headers(has_header)
        .from_writer(match wtr {
            Ok(f) => f,
            Err(e) => return ToolResult {
                success: false, output: String::new(), error: Some(format!("create failed: {}", e)), duration_ms: started.elapsed().as_millis(),
            },
        });

    let rows: Vec<Vec<&str>> = lines.iter().map(|l| l.split(',').map(|s| s.trim()).collect()).collect();

    if has_header && !rows.is_empty() {
        if let Err(e) = writer.write_record(&rows[0]) {
            return ToolResult {
                success: false, output: String::new(), error: Some(format!("write header failed: {}", e)), duration_ms: started.elapsed().as_millis(),
            };
        }
        for row in &rows[1..] {
            if let Err(e) = writer.write_record(row) {
                return ToolResult {
                    success: false, output: String::new(), error: Some(format!("write row failed: {}", e)), duration_ms: started.elapsed().as_millis(),
                };
            }
        }
    } else {
        for row in &rows {
            if let Err(e) = writer.write_record(row) {
                return ToolResult {
                    success: false, output: String::new(), error: Some(format!("write row failed: {}", e)), duration_ms: started.elapsed().as_millis(),
                };
            }
        }
    }

    match writer.flush() {
        Ok(_) => ToolResult {
            success: true,
            output: format!("Wrote {} rows to {}", if has_header { rows.len() - 1 } else { rows.len() }, path),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false, output: String::new(), error: Some(format!("flush failed: {}", e)), duration_ms: started.elapsed().as_millis(),
        },
    }
}