use crate::models::ToolResult;
use std::time::Instant;

pub async fn sleep_until(target_time: &str) -> ToolResult {
    let started = Instant::now();
    let target = match chrono::DateTime::parse_from_rfc3339(target_time) {
        Ok(t) => t.with_timezone(&chrono::Utc),
        Err(_) => {
            // Try parsing as UTC-only RFC 3339 (ends with Z)
            match chrono::DateTime::parse_from_rfc3339(&format!(
                "{}Z",
                target_time.trim_end_matches('Z')
            )) {
                Ok(t) => t.with_timezone(&chrono::Utc),
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("invalid timestamp '{}': {}", target_time, e)),
                        duration_ms: started.elapsed().as_millis(),
                    };
                }
            }
        }
    };

    let now = chrono::Utc::now();
    if target <= now {
        return ToolResult {
            success: true,
            output: format!("target time {} is in the past; no sleep needed", target),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        };
    }

    let duration = target - now;
    let secs = duration.num_seconds();
    if secs > 86_400 {
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!(
                "refusing to sleep for {} seconds (>{}-hour max)",
                secs, 24
            )),
            duration_ms: started.elapsed().as_millis(),
        };
    }

    tokio::time::sleep(std::time::Duration::from_secs(secs.max(0) as u64)).await;

    ToolResult {
        success: true,
        output: format!("slept until {} ({}s)", target, secs),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}
