use crate::models::{ToolCall, ToolDefinition};
use serde_json::Value;

/// Validate a single tool call's arguments against its tool definition schema.
/// Returns Ok(()) or a descriptive error message that can be fed back to the LLM.
pub fn validate_tool_call(tool_call: &ToolCall, tool_def: &ToolDefinition) -> Result<(), String> {
    let schema = &tool_def.input_schema;
    let args = &tool_call.arguments;

    // Top-level must be an object
    if let Some(obj_type) = schema.get("type").and_then(|v| v.as_str()) {
        if obj_type == "object" && !args.is_object() {
            return Err(format!(
                "argument must be a JSON object, got {}",
                json_type_name(args)
            ));
        }
    }

    let args_obj = match args.as_object() {
        Some(obj) => obj,
        None => return Ok(()), // non-object args pass through
    };

    // Check required properties
    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        for req in required {
            if let Some(key) = req.as_str() {
                if !args_obj.contains_key(key) {
                    return Err(format!(
                        "missing required argument '{}' for tool '{}'",
                        key, tool_call.name
                    ));
                }
            }
        }
    }

    // Validate property types if properties schema exists
    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        for (prop_name, prop_schema) in properties {
            if let Some(value) = args_obj.get(prop_name) {
                if let Some(err) = validate_value(value, prop_schema, prop_name) {
                    return Err(err);
                }
            }
        }
    }

    Ok(())
}

fn validate_value(value: &Value, schema: &Value, path: &str) -> Option<String> {
    let expected_type = schema.get("type").and_then(|v| v.as_str())?;

    let type_ok = match expected_type {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || (value.is_f64() && value.as_f64().is_some_and(|f| f.fract() == 0.0))
        }
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true, // unknown type passes
    };

    if !type_ok {
        return Some(format!(
            "argument '{}' expects type '{}', got '{}' (value: {})",
            path,
            expected_type,
            json_type_name(value),
            value
        ));
    }

    // Recursive checks for nested schemas
    if let Some(items) = schema.get("items") {
        if let Some(arr) = value.as_array() {
            for (i, item) in arr.iter().enumerate() {
                let item_path = format!("{}[{}]", path, i);
                if let Some(err) = validate_value(item, items, &item_path) {
                    return Some(err);
                }
            }
        }
    }

    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        if let Some(obj) = value.as_object() {
            for (prop_name, prop_schema) in properties {
                if let Some(val) = obj.get(prop_name) {
                    let prop_path = format!("{}.{}", path, prop_name);
                    if let Some(err) = validate_value(val, prop_schema, &prop_path) {
                        return Some(err);
                    }
                }
            }
        }
    }

    if let Some(allowed) = schema.get("enum").and_then(|v| v.as_array()) {
        if !allowed.iter().any(|a| a == value) {
            return Some(format!(
                "argument '{}' has value {} which is not in allowed enum {:?}",
                path, value, allowed
            ));
        }
    }

    None
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Validate all tool calls against their registered definitions.
/// Returns a list of (index, error_message) for invalid calls.
pub fn validate_tool_calls<'a>(
    tool_calls: &[ToolCall],
    get_definition: impl Fn(&str) -> Option<&'a ToolDefinition>,
) -> Vec<(usize, String)> {
    tool_calls
        .iter()
        .enumerate()
        .filter_map(|(i, tc)| {
            let def = get_definition(&tc.name)?;
            validate_tool_call(tc, def).err().map(|err| (i, err))
        })
        .collect()
}

/// Build an error message for the LLM describing what went wrong with a tool call.
pub fn build_validation_error_message(tool_name: &str, error: &str) -> String {
    format!(
        "Tool call validation error for '{}': {}. Please correct the arguments and try again.",
        tool_name, error
    )
}

/// Attempt to parse JSON, with automatic repair of common LLM output issues:
/// - Truncated brackets (missing closing `}` or `]`)
/// - Trailing commas before closing brackets
/// - Single-quoted keys/values (treated as unquoted and repaired)
pub fn parse_lossy_json(input: &str) -> serde_json::Value {
    // 1. Direct parse
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(input) {
        return val;
    }

    // 2. Try with trailing comma removal + bracket closure
    let repaired = try_repair_json(input);
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&repaired) {
        return val;
    }

    // 3. Try extracting any JSON object from the text
    if let Some(start) = input.find('{') {
        let candidate = &input[start..];
        // Try progressively shorter suffixes
        for len in (candidate.len().saturating_sub(50)..candidate.len()).rev() {
            let sub = &candidate[..=len];
            let sub_repaired = try_repair_json(sub);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&sub_repaired) {
                return val;
            }
        }
    }

    // 4. Last resort: extract any key-value pairs with a simple regex
    serde_json::Value::Object(extract_kv_pairs(input))
}

fn try_repair_json(input: &str) -> String {
    let trimmed = input.trim();

    // Count opening vs closing braces/brackets
    let open_braces = trimmed.matches('{').count();
    let close_braces = trimmed.matches('}').count();
    let open_brackets = trimmed.matches('[').count();
    let close_brackets = trimmed.matches(']').count();

    // Remove trailing commas before closing brackets
    let no_trailing_commas = {
        let mut s = trimmed.to_string();
        // Remove comma before }
        while s.contains(",}") {
            s = s.replace(",}", "}");
        }
        // Remove comma before ]
        while s.contains(",]") {
            s = s.replace(",]", "]");
        }
        s
    };

    // Add missing closing braces/brackets
    let mut result = no_trailing_commas;
    for _ in 0..open_braces.saturating_sub(close_braces) {
        result.push('}');
    }
    for _ in 0..open_brackets.saturating_sub(close_brackets) {
        result.push(']');
    }

    result
}

fn extract_kv_pairs(input: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    // Simple heuristic: find all `"key": value` or `key: value` patterns
    for cap in input.lines() {
        let trimmed = cap.trim();
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            let val_str = trimmed[colon_pos + 1..].trim().trim_end_matches(',').trim();
            if !key.is_empty() {
                let val = val_str.trim_matches('"').trim_matches('\'');
                map.insert(key, serde_json::Value::String(val.to_string()));
            }
        }
    }
    map
}

/// Coerce model quirk artifacts in tool call arguments **before** schema validation.
/// Recursively walks the JSON value and applies fixes:
/// - StringifiedBooleans: `"true"` / `"false"` strings → `true` / `false` booleans
pub fn coerce_quirks(args: &mut serde_json::Value, quirks: &[crate::agent::blueprint::ModelQuirk]) {
    if quirks.is_empty() {
        return;
    }
    coerce_quirks_inner(args, quirks);
}

#[allow(clippy::collapsible_match)]
fn coerce_quirks_inner(
    value: &mut serde_json::Value,
    quirks: &[crate::agent::blueprint::ModelQuirk],
) {
    match value {
        serde_json::Value::String(s) => {
            let s_clone = s.clone();
            if quirks.contains(&crate::agent::blueprint::ModelQuirk::StringifiedBooleans) {
                if s_clone == "true" {
                    *value = serde_json::Value::Bool(true);
                    return;
                } else if s_clone == "false" {
                    *value = serde_json::Value::Bool(false);
                    return;
                }
            }
            if quirks.contains(&crate::agent::blueprint::ModelQuirk::StringifiedIntegers) {
                if let Ok(n) = s_clone.parse::<i64>() {
                    *value = serde_json::Value::Number(n.into());
                }
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values_mut() {
                coerce_quirks_inner(val, quirks);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                coerce_quirks_inner(val, quirks);
            }
        }
        _ => {}
    }
}

/// Strip conversational preamble/aftermath outside structured tool-call markers.
/// Small models often emit: "I'll use the read tool...\n<function>...</function>\nThat should work."
/// This extracts only the content between the first and last recognized markers.
pub fn strip_cot_leakage(raw: &str) -> String {
    let markers = ["<function>", "<tool_call>", "<invoke>", "{"];
    let end_markers = ["</function>", "</tool_call>", "</invoke>"];

    // Find the earliest start marker
    let start_pos = markers.iter().filter_map(|m| raw.find(m)).min();
    // Find the latest end marker position (position + marker length)
    let end_pos = end_markers
        .iter()
        .filter_map(|m| {
            let pos = raw.rfind(m)?;
            Some(pos + m.len())
        })
        .max();

    match (start_pos, end_pos) {
        (Some(start), Some(end)) if end > start => raw[start..end].to_string(),
        (Some(start), None) => raw[start..].to_string(),
        _ => {
            // Fallback: try extracting a JSON object
            if let Some(brace_start) = raw.find('{') {
                if let Some(brace_end) = raw.rfind('}') {
                    if brace_end > brace_start {
                        return raw[brace_start..=brace_end].to_string();
                    }
                }
            }
            raw.to_string()
        }
    }
}

/// Strip <function> and </function> tags from input string.
pub fn strip_function_tags(input: &str) -> String {
    input.replace("<function>", "").replace("</function>", "")
}

/// Wrap the input string in <function> tags.
pub fn wrap_function_tags(input: &str) -> String {
    format!("<function>\n{}\n</function>", input)
}

/// Extract the content between <function> and </function> tags, if present.
pub fn extract_function_block(input: &str) -> Option<String> {
    let start = input.find("<function>")?;
    let after = &input[start + "<function>".len()..];
    let end = after.find("</function>")?;
    Some(after[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tool(name: &str, schema: Value) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: String::new(),
            input_schema: schema,
            category: "test".into(),
        }
    }

    #[test]
    fn test_validate_required_fields() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer"}
            },
            "required": ["name", "count"]
        });
        let def = test_tool("test", schema);

        // Missing required field
        let tc = ToolCall {
            id: "1".into(),
            name: "test".into(),
            arguments: serde_json::json!({"name": "foo"}),
        };
        assert!(validate_tool_call(&tc, &def).is_err());

        // All required present
        let tc = ToolCall {
            id: "1".into(),
            name: "test".into(),
            arguments: serde_json::json!({"name": "foo", "count": 42}),
        };
        assert!(validate_tool_call(&tc, &def).is_ok());
    }

    #[test]
    fn test_validate_wrong_type() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "value": {"type": "string"}
            },
            "required": ["value"]
        });
        let def = test_tool("test", schema);

        let tc = ToolCall {
            id: "1".into(),
            name: "test".into(),
            arguments: serde_json::json!({"value": 42}),
        };
        assert!(validate_tool_call(&tc, &def).is_err());
    }

    #[test]
    fn test_validate_valid_call() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "recursive": {"type": "boolean"}
            },
            "required": ["path"]
        });
        let def = test_tool("glob", schema);

        let tc = ToolCall {
            id: "1".into(),
            name: "glob".into(),
            arguments: serde_json::json!({"path": "src/**/*.rs", "recursive": true}),
        };
        assert!(validate_tool_call(&tc, &def).is_ok());
    }

    #[test]
    fn test_validate_nested_object() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "filter": {
                    "type": "object",
                    "properties": {
                        "min_score": {"type": "number"},
                        "max_results": {"type": "integer"}
                    },
                    "required": ["min_score"]
                }
            },
            "required": ["filter"]
        });
        let def = test_tool("search", schema);

        let tc = ToolCall {
            id: "1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"filter": {"min_score": 0.5, "max_results": 10}}),
        };
        assert!(validate_tool_call(&tc, &def).is_ok());

        let tc_bad = ToolCall {
            id: "1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"filter": {"min_score": "high"}}),
        };
        assert!(validate_tool_call(&tc_bad, &def).is_err());
    }

    #[test]
    fn test_parse_lossy_valid() {
        let val = parse_lossy_json(r#"{"name": "test", "count": 42}"#);
        assert_eq!(val["name"], "test");
        assert_eq!(val["count"], 42);
    }

    #[test]
    fn test_parse_lossy_truncated() {
        // Missing closing brace
        let val = parse_lossy_json(r#"{"name": "test", "count": 42"#);
        assert_eq!(val["name"], "test");
        assert_eq!(val["count"], 42);
    }

    #[test]
    fn test_parse_lossy_trailing_comma() {
        let val = parse_lossy_json(r#"{"name": "test", "count": 42,}"#);
        assert_eq!(val["name"], "test");
        assert_eq!(val["count"], 42);
    }

    #[test]
    fn test_parse_lossy_empty() {
        let val = parse_lossy_json("");
        assert!(val.is_object());
    }

    #[test]
    fn test_parse_lossy_garbage() {
        let val = parse_lossy_json("not json at all");
        assert!(val.is_object() || val.is_null());
    }

    #[test]
    fn test_parse_lossy_nested_truncated() {
        // Nested object with missing closing braces
        let val = parse_lossy_json(r#"{"outer": {"inner": "value""#);
        assert_eq!(val["outer"]["inner"], "value");
    }

    #[test]
    fn test_validate_enum() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["fast", "deep", "auto"]
                }
            },
            "required": ["mode"]
        });
        let def = test_tool("search", schema);

        let tc = ToolCall {
            id: "1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"mode": "fast"}),
        };
        assert!(validate_tool_call(&tc, &def).is_ok());

        let tc_bad = ToolCall {
            id: "1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"mode": "invalid_mode"}),
        };
        assert!(validate_tool_call(&tc_bad, &def).is_err());
    }

    #[test]
    fn test_strip_function_tags() {
        let input = "<function>\n{\"name\": \"web_search\"}\n</function>";
        let stripped = super::strip_function_tags(input);
        assert!(!stripped.contains("<function>"));
        assert!(!stripped.contains("</function>"));
        assert!(stripped.contains("web_search"));
    }

    #[test]
    fn test_wrap_function_tags() {
        let input = "{\"name\": \"web_search\"}";
        let wrapped = super::wrap_function_tags(input);
        assert!(wrapped.starts_with("<function>"));
        assert!(wrapped.ends_with("</function>"));
        assert!(wrapped.contains("web_search"));
    }

    #[test]
    fn test_extract_function_block() {
        let input = "prefix\n<function>\n{\"name\": \"test\"}\n</function>\nsuffix";
        let extracted = super::extract_function_block(input);
        assert_eq!(extracted, Some("{\"name\": \"test\"}".to_string()));
    }

    #[test]
    fn test_extract_function_block_no_tags() {
        let input = "no function tags here";
        assert!(super::extract_function_block(input).is_none());
    }

    #[test]
    fn test_strip_and_extract_roundtrip() {
        let inner = "{\"name\": \"search\", \"arguments\": {\"mode\": \"fast\"}}";
        let wrapped = super::wrap_function_tags(inner);
        let extracted = super::extract_function_block(&wrapped).unwrap();
        assert_eq!(extracted, inner);
        let stripped = super::strip_function_tags(&wrapped);
        assert!(!stripped.contains("<function>"));
        assert!(stripped.trim() == inner);
    }
}
