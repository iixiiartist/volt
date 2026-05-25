use crate::models::ToolResult;
use std::time::Instant;

pub async fn json_validate(data: &str) -> ToolResult {
    let started = Instant::now();
    match serde_json::from_str::<serde_json::Value>(data) {
        Ok(v) => {
            let kind = match &v {
                serde_json::Value::Object(_) => "object",
                serde_json::Value::Array(_) => "array",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::Bool(_) => "boolean",
                serde_json::Value::Null => "null",
            };
            ToolResult {
                success: true,
                output: format!("Valid JSON ({})", kind),
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("invalid JSON: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn json_prettify(data: &str, indent: u8) -> ToolResult {
    let started = Instant::now();
    let v: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("invalid JSON: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };
    let indent_str = " ".repeat(indent as usize);
    match serde_json::to_string_pretty(&v) {
        Ok(s) => {
            let reindented = s.replace("  ", &indent_str);
            ToolResult {
                success: true,
                output: reindented,
                error: None,
                duration_ms: started.elapsed().as_millis(),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("serialize failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn json_query(data: &str, path: &str) -> ToolResult {
    let started = Instant::now();
    let v: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("invalid JSON: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };

    let parts: Vec<&str> = path
        .trim_start_matches('.')
        .split('.')
        .filter(|s| !s.is_empty())
        .collect();
    let mut current = &v;

    for part in parts {
        let indexed = if part.contains('[') && part.ends_with(']') {
            let bracket = part.find('[').unwrap();
            let name = &part[..bracket];
            let idx_str = &part[bracket + 1..part.len() - 1];
            let idx: usize = match idx_str.parse() {
                Ok(i) => i,
                Err(_) => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("invalid index '{}'", idx_str)),
                        duration_ms: started.elapsed().as_millis(),
                    }
                }
            };
            match current.get(name).and_then(|v| v.get(idx)) {
                Some(v) => v,
                None => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("path '{}' not found at '{}'", path, part)),
                        duration_ms: started.elapsed().as_millis(),
                    }
                }
            }
        } else {
            match current.get(part) {
                Some(v) => v,
                None => {
                    return ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("path '{}' not found at '{}'", path, part)),
                        duration_ms: started.elapsed().as_millis(),
                    }
                }
            }
        };
        current = indexed;
    }

    let out = serde_json::to_string_pretty(current).unwrap_or_else(|_| "null".to_string());
    ToolResult {
        success: true,
        output: out,
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}
