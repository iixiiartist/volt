use crate::agent::loop_rs::Agent;
use crate::llm::openai::OpenAIProvider;
use crate::models::{AgentConfig, ToolResult};
use crate::tools::ToolRegistry;
use std::sync::Arc;
use std::time::Instant;

pub fn delegate_task(task: &str, context: &str, tools: Arc<ToolRegistry>) -> ToolResult {
    let started = Instant::now();

    let api_key = std::env::var("NVIDIA_API_KEY")
        .or_else(|_| std::env::var("LLM_API_KEY"))
        .unwrap_or_default();
    let base_url = std::env::var("LLM_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1".into());
    let model = std::env::var("LLM_MODEL")
        .unwrap_or_else(|_| "nvidia/llama-3.1-nemotron-70b-instruct".into());

    let provider = Box::new(OpenAIProvider::new(api_key, base_url, "delegate".into()));
    let config = AgentConfig {
        name: "sub-agent".into(),
        model,
        provider: "nvidia".into(),
        system_prompt: Some(format!(
            "You are a sub-agent delegated to complete a specific task.\n\
             Context from parent agent:\n{}\n\n\
             Focus only on the task. Report results concisely.",
            context
        )),
        max_iterations: 10,
        temperature: 0.3,
        toolsets: vec!["builtin".into()],
        hidden: true,
    };

    let sub_agent = Agent::new(config, provider, tools);

    let handle = tokio::runtime::Handle::current();
    match tokio::task::block_in_place(|| handle.block_on(sub_agent.run(task))) {
        Ok(output) => ToolResult {
            success: true,
            output,
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("delegation failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
