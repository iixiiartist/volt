use crate::models::Message;
use std::sync::Arc;

pub fn compress_context(messages: &[Message], max_messages: usize) -> Vec<Message> {
    if messages.len() <= max_messages {
        return messages.to_vec();
    }

    let keep = max_messages / 2;
    let mut compressed = Vec::with_capacity(max_messages);

    compressed.push(Message {
        role: "system".into(),
        content: Arc::new("[Earlier conversation compressed]".to_string()),
        tool_calls: None,
        tool_result: None,
        tool_name: None,
        created_at: chrono::Utc::now(),
    });

    let start = messages.len() - keep;
    compressed.extend_from_slice(&messages[start..]);

    compressed
}
