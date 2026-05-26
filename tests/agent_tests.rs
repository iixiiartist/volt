use volt::agent::loop_rs::Agent;
use volt::models::*;
use volt::test_utils::MockLLMProvider;

fn agent_config() -> AgentConfig {
    AgentConfig {
        name: "test-agent".into(),
        model: "mock-model".into(),
        provider: "mock".into(),
        system_prompt: None,
        max_iterations: 5,
        temperature: 0.0,
        toolsets: vec!["builtin".into()],
        hidden: false,
        allow_all: false,
        enabled_context_kinds: volt::models::default_context_kinds(),
        essential_tools: volt::models::default_essential_tools(),
        context_kind_quotas: Default::default(),
    }
}

#[tokio::test]
async fn test_agent_returns_text_response() {
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
        "Hello from the mock LLM!",
    )]));
    let tools = volt::tools::ToolRegistry::new();
    let agent = Agent::new(agent_config(), provider, tools);

    let result = agent.run("Say hello").await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("mock") || output.contains("Hello"));
}

#[tokio::test]
async fn test_agent_runs_tool_and_uses_result() {
    let provider = Box::new(MockLLMProvider::new(vec![
        // First: return a tool call
        MockLLMProvider::tool_calls(vec![ToolCall {
            id: "call_1".into(),
            name: "echo".into(),
            arguments: serde_json::json!({"input": "hello"}),
        }]),
        // Second: return final response using tool result
        MockLLMProvider::tool_result("Tool result was received."),
    ]));
    let tools = volt::test_utils::test_tool_registry().await;
    let agent = Agent::new(agent_config(), provider, tools);

    let result = agent.run("Use the echo tool").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_agent_respects_max_iterations() {
    // Mock uses pop() (LIFO), so push final result first, then tool calls
    let mut responses = Vec::new();
    // Push final result first (popped last)
    responses.push(MockLLMProvider::tool_result("done"));
    // Push tool calls (popped first, keeps agent looping)
    for _ in 0..6 {
        responses.push(MockLLMProvider::tool_calls(vec![ToolCall {
            id: "call_loop".into(),
            name: "echo".into(),
            arguments: serde_json::json!({"input": "loop"}),
        }]));
    }

    let provider = Box::new(MockLLMProvider::new(responses));
    let tools = volt::test_utils::test_tool_registry().await;
    let agent = Agent::new(agent_config(), provider, tools);

    let result = agent.run("Loop forever").await;
    // With max_iterations=5 and tool calls causing loops, should exceed iterations
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("iteration") || err.contains("max") || err.contains("iterations"));
}

#[tokio::test]
async fn test_agent_push_user_message() {
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
        "response",
    )]));
    let tools = volt::tools::ToolRegistry::new();
    let agent = Agent::new(agent_config(), provider, tools);

    // Check initial state
    {
        let state = agent.state.lock().await;
        assert_eq!(state.messages.len(), 0);
    }

    agent.run("test input").await.ok();

    // After run, there should be at least the user message + assistant response
    let state = agent.state.lock().await;
    assert!(state.messages.len() >= 2);
    assert_eq!(state.messages[0].role, "user");
    assert_eq!(state.messages[0].content.as_str(), "test input");
    assert_eq!(state.messages[1].role, "assistant");
}
