use crate::models::{ToolCall, ToolDefinition};
use serde_json::Value;

/// Validate a single tool call's arguments against its tool definition schema.
/// Returns Ok(()) or a descriptive error message that can be fed back to the LLM.
pub fn validate_tool_call(tool_call: &ToolCall, tool_def: &ToolDefinition) -> Result<(), String> {
    let schema = &tool_def.input_schema;
    let args = &tool_call.arguments;

    // Top-level must be an object
    if let Some(obj_type) = schema.get("type").and_then(|v| v.as_str()) {
        if obj_type == "object" {
            if !args.is_object() {
                return Err(format!(
                    "argument must be a JSON object, got {}",
                    json_type_name(args)
                ));
            }
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
        "integer" => value.is_i64() || value.is_u64() || (value.is_f64() && value.as_f64().map_or(false, |f| f.fract() == 0.0)),
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
}
