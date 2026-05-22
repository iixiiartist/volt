use crate::models::ToolResult;
use std::time::Instant;

pub fn web_fetch(url: &str) -> ToolResult {
    let started = Instant::now();
    match reqwest::blocking::get(url) {
        Ok(resp) => {
            let status = resp.status();
            match resp.text() {
                Ok(body) => ToolResult {
                    success: status.is_success(),
                    output: body,
                    error: if status.is_success() { None } else { Some(format!("HTTP {}", status)) },
                    duration_ms: started.elapsed().as_millis(),
                },
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("body read failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("fetch failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
