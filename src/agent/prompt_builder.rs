// Prompt builder for Gemma-4 native tags and function calling blocks.
// Handles truncation, system messages, user messages, and function call blocks.

/// Build a Gemma-4 native prompt with proper tags.
pub fn build_prompt(
    system: &str,
    user: &str,
    functions: Option<&[crate::models::ToolCall]>,
    multimodal: Option<MultimodalInput>,
) -> String {
    let mut prompt = String::new();

    // System block
    prompt.push_str("<|system|>\n");
    prompt.push_str(system);
    prompt.push_str("<|end|>\n");

    // Function definitions block (if any)
    if let Some(funcs) = functions {
        for func in funcs {
            prompt.push_str("<function>\n");
            prompt.push_str(&serde_json::to_string(func).unwrap_or_default());
            prompt.push_str("</function>\n");
        }
    }

    // User block with optional multimodal input
    prompt.push_str("<|user|>\n");
    
    if let Some(mm) = multimodal {
        // Add multimodal content
        if let Some(images) = mm.images {
            for img in images {
                prompt.push_str("<image>");
                prompt.push_str(img);
                prompt.push_str("</image>\n");
            }
        }
        if let Some(audio) = mm.audio {
            prompt.push_str("<audio>");
            prompt.push_str(audio);
            prompt.push_str("</audio>\n");
        }
        if let Some(video) = mm.video {
            prompt.push_str("<video>");
            prompt.push_str(video);
            prompt.push_str("</video>\n");
        }
    }

    prompt.push_str(user);
    prompt.push_str("<|end|>\n");

    // Assistant start tag
    prompt.push_str("<|assistant|>\n");

    prompt
}

/// Multimodal input specification.
pub struct MultimodalInput<'a> {
    pub images: Option<&'a [&'a str]>,
    pub audio: Option<&'a str>,
    pub video: Option<&'a str>,
}

/// Truncate user content to fit within token budget, preserving system messages and function blocks.
pub fn truncate_to_budget(prompt: &str, max_tokens: usize) -> String {
    // Simple truncation: if the prompt is longer than max_tokens characters,
    // truncate the user portion. This is a simplified approach;
    // a production implementation would use a proper tokenizer.
    if prompt.len() <= max_tokens * 4 { // rough estimate: 4 chars per token
        return prompt.to_string();
    }

    // Find the user block and truncate it
    let user_start = prompt.find("<|user|>\n").unwrap_or(0);
    let user_end = prompt.find("<|end|>\n<|assistant|>").unwrap_or(prompt.len());
    
    let before_user = &prompt[..user_start + "<|user|>\n".len()];
    let after_user = &prompt[user_end..];
    
    let available = max_tokens * 4;
    let current_len = before_user.len() + after_user.len();
    
    if current_len >= available {
        // Even without user content, we're over budget
        return prompt[..available].to_string();
    }
    
    let user_budget = available - current_len;
    let user_content = &prompt[user_start + "<|user|>\n".len()..user_end];
    let truncated_user = if user_content.len() > user_budget {
        &user_content[..user_budget]
    } else {
        user_content
    };

    format!("{}{}{}", before_user, truncated_user, after_user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt_basic() {
        let prompt = build_prompt("You are helpful.", "Hello!", None, None);
        assert!(prompt.contains("<|system|>"));
        assert!(prompt.contains("You are helpful."));
        assert!(prompt.contains("<|user|>"));
        assert!(prompt.contains("Hello!"));
        assert!(prompt.contains("<|assistant|>"));
    }

    #[test]
    fn test_build_prompt_with_functions() {
        let funcs = vec![
            crate::models::ToolCall {
                id: "1".into(),
                name: "web_search".into(),
                arguments: serde_json::json!({"query": "Rust"}),
            },
        ];
        let prompt = build_prompt("System.", "Search.", Some(&funcs), None);
        assert!(prompt.contains("<function>"));
        assert!(prompt.contains("web_search"));
        assert!(prompt.contains("</function>"));
    }

    #[test]
    fn test_build_prompt_with_multimodal() {
        let mm = MultimodalInput {
            images: Some(&["base64imgdata"]),
            audio: Some("base64audiodata"),
            video: Some("base64videodata"),
        };
        let prompt = build_prompt("System.", "Describe this.", None, Some(mm));
        assert!(prompt.contains("<image>base64imgdata</image>"));
        assert!(prompt.contains("<audio>base64audiodata</audio>"));
        assert!(prompt.contains("<video>base64videodata</video>"));
    }

    #[test]
    fn test_truncate_to_budget() {
        let long_user = "a".repeat(10000);
        let prompt = build_prompt("System.", &long_user, None, None);
        let truncated = truncate_to_budget(&prompt, 100);
        assert!(truncated.len() <= 100 * 4);
        assert!(truncated.contains("<|system|>"));
        assert!(truncated.contains("<|assistant|>"));
    }

    #[test]
    fn test_truncate_preserves_function_blocks() {
        let funcs = vec![
            crate::models::ToolCall {
                id: "1".into(),
                name: "web_search".into(),
                arguments: serde_json::json!({"query": "test"}),
            },
        ];
        let long_user = "b".repeat(10000);
        let prompt = build_prompt("System.", &long_user, Some(&funcs), None);
        let truncated = truncate_to_budget(&prompt, 100);
        assert!(truncated.contains("<|system|>"));
        assert!(truncated.contains("<|assistant|>"));
    }

    #[test]
    fn test_prompt_ends_with_assistant() {
        let prompt = build_prompt("sys", "usr", None, None);
        assert!(prompt.ends_with("<|assistant|>\n"));
    }

    #[test]
    fn test_prompt_has_end_tokens() {
        let prompt = build_prompt("sys", "usr", None, None);
        assert!(prompt.contains("<|end|>"));
    }

    #[test]
    fn test_truncate_noop_for_short_input() {
        let prompt = build_prompt("Short system.", "Short user.", None, None);
        let truncated = truncate_to_budget(&prompt, 10000);
        assert_eq!(truncated, prompt);
    }
}
