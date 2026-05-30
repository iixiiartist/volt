// Chain-of-Thought (CoT) orchestration for Gemma-4 models.
// Pre-turn planning prompt and plan parsing.

use crate::models::ToolCall;

/// Generate a planning prompt that asks the model to break down the task into steps.
pub fn planning_prompt(user_request: &str, available_tools: &[String]) -> String {
    let tools_str = available_tools.join(", ");
    format!(
        r#"You are a planning assistant. Break down the following user request into a numbered list of concrete steps.
Each step should describe a single action, possibly using one of these tools: {}.

User request: {}

Plan:"#,
        tools_str, user_request
    )
}

/// Parse the model's planning output into a list of steps.
/// Each step is a tuple of (step_number, description, optional_tool_name).
pub fn parse_plan(plan_text: &str) -> Vec<(usize, String, Option<String>)> {
    let mut steps = Vec::new();
    for line in plan_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Match patterns like "1. Step description" or "1) Step description"
        if let Some(pos) = line.find('.') {
            let num_part = &line[..pos].trim();
            if let Ok(step_num) = num_part.parse::<usize>() {
                let desc = line[pos + 1..].trim().to_string();
                // Try to extract tool name from the description
                let tool_name = extract_tool_name(&desc);
                steps.push((step_num, desc, tool_name));
            }
        } else if let Some(pos) = line.find(')') {
            let num_part = &line[..pos].trim();
            if let Ok(step_num) = num_part.parse::<usize>() {
                let desc = line[pos + 1..].trim().to_string();
                let tool_name = extract_tool_name(&desc);
                steps.push((step_num, desc, tool_name));
            }
        }
    }
    steps
}

/// Extract a tool name from a step description.
fn extract_tool_name(description: &str) -> Option<String> {
    let lower = description.to_lowercase();
    let trigger_words = ["use", "using", "call"];
    for trigger in &trigger_words {
        if lower.contains(trigger) {
            let words: Vec<&str> = lower.split_whitespace().collect();
            for (i, word) in words.iter().enumerate() {
                if word == trigger && i + 1 < words.len() {
                    return Some(words[i + 1].to_string());
                }
            }
        }
    }
    None
}

/// Convert a parsed plan into a list of tool calls.
pub fn plan_to_tool_calls(plan: &[(usize, String, Option<String>)]) -> Vec<ToolCall> {
    plan.iter()
        .filter_map(|(step, desc, tool_name)| {
            tool_name.as_ref().map(|name| {
                ToolCall {
                    id: format!("plan-{}", step),
                    name: name.clone(),
                    arguments: serde_json::json!({"description": desc.clone()}),
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planning_prompt() {
        let tools = vec!["web_search".to_string(), "read_file".to_string()];
        let prompt = planning_prompt("Find information about Rust", &tools);
        assert!(prompt.contains("Find information about Rust"));
        assert!(prompt.contains("web_search"));
    }

    #[test]
    fn test_parse_plan() {
        let plan_text = r#"1. Search the web for Rust tutorials using web_search
        2. Read the first result using read_file
        3. Summarize the findings"#;
        
        let steps = parse_plan(plan_text);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].0, 1);
        assert_eq!(steps[0].2, Some("web_search".to_string()));
        assert_eq!(steps[1].0, 2);
        assert_eq!(steps[1].2, Some("read_file".to_string()));
        assert_eq!(steps[2].0, 3);
        assert_eq!(steps[2].2, None);
    }

    #[test]
    fn test_plan_to_tool_calls() {
        let plan = vec![
            (1, "Search the web".to_string(), Some("web_search".to_string())),
            (2, "Read file".to_string(), Some("read_file".to_string())),
        ];
        let calls = plan_to_tool_calls(&plan);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(calls[1].name, "read_file");
    }

    #[test]
    fn test_parse_plan_empty() {
        let steps = parse_plan("");
        assert!(steps.is_empty());
    }

    #[test]
    fn test_parse_plan_no_tool_patterns() {
        let plan_text = "1. First step\n2. Second step\n3. Third step";
        let steps = parse_plan(plan_text);
        assert_eq!(steps.len(), 3);
        assert!(steps.iter().all(|s| s.2.is_none()));
    }

    #[test]
    fn test_parse_plan_parenthesis_format() {
        let plan_text = "1) Search using web_search\n2) Read using read_file";
        let steps = parse_plan(plan_text);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].2, Some("web_search".to_string()));
        assert_eq!(steps[1].2, Some("read_file".to_string()));
    }

    #[test]
    fn test_parse_plan_call_verb() {
        let plan_text = "1. Call web_search to find results";
        let steps = parse_plan(plan_text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].2, Some("web_search".to_string()));
    }

    #[test]
    fn test_plan_to_tool_calls_empty() {
        let calls = plan_to_tool_calls(&[]);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_plan_to_tool_calls_skips_missing_tools() {
        let plan = vec![
            (1, "Think".to_string(), None),
            (2, "Search".to_string(), Some("web_search".to_string())),
        ];
        let calls = plan_to_tool_calls(&plan);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_search");
    }
}
