//! v0.3.0 Profile mode integration tests.
//! Validates the exact context kind sets, tool call loop termination,
//! and the "Agent Tax" tradeoff documented in paper §4.5.

use volt::agent::Agent;
use volt::commands::AgentMode;
use volt::context::ContextKind;
use volt::models::*;
use volt::test_utils::MockLLMProvider;

// ── Context cardinality guarantees ──────────────────────────────────

#[test]
fn test_precision_mode_exactly_two_kinds() {
    let kinds = AgentMode::Precision.context_kinds();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&ContextKind::Tool));
    assert!(kinds.contains(&ContextKind::Artifact));
    assert!(!kinds.contains(&ContextKind::Skill));
    assert!(!kinds.contains(&ContextKind::Memory));
    assert!(!kinds.contains(&ContextKind::Conversation));
    assert!(!kinds.contains(&ContextKind::Policy));
}

#[test]
fn test_balanced_mode_exactly_five_kinds() {
    let kinds = AgentMode::Balanced.context_kinds();
    assert_eq!(kinds.len(), 5);
    assert!(kinds.contains(&ContextKind::Tool));
    assert!(kinds.contains(&ContextKind::Skill));
    assert!(kinds.contains(&ContextKind::Memory));
    assert!(kinds.contains(&ContextKind::Conversation));
    assert!(kinds.contains(&ContextKind::Artifact));
    assert!(!kinds.contains(&ContextKind::Policy));
    assert!(!kinds.contains(&ContextKind::Security));
}

#[test]
fn test_autonomous_mode_all_twelve_kinds() {
    let kinds = AgentMode::Autonomous.context_kinds();
    assert_eq!(kinds.len(), 12);
    assert!(kinds.contains(&ContextKind::Tool));
    assert!(kinds.contains(&ContextKind::Skill));
    assert!(kinds.contains(&ContextKind::Memory));
    assert!(kinds.contains(&ContextKind::Conversation));
    assert!(kinds.contains(&ContextKind::AgentRun));
    assert!(kinds.contains(&ContextKind::Artifact));
    assert!(kinds.contains(&ContextKind::SystemPrompt));
    assert!(kinds.contains(&ContextKind::FewShot));
    assert!(kinds.contains(&ContextKind::Policy));
    assert!(kinds.contains(&ContextKind::Permission));
    assert!(kinds.contains(&ContextKind::Security));
    assert!(kinds.contains(&ContextKind::MCPConfig));
}

#[test]
fn test_mode_from_str_defaults_to_balanced() {
    assert!(matches!(
        "balanced".parse::<AgentMode>().unwrap(),
        AgentMode::Balanced
    ));
    assert!(matches!(
        "precision".parse::<AgentMode>().unwrap(),
        AgentMode::Precision
    ));
    assert!(matches!(
        "autonomous".parse::<AgentMode>().unwrap(),
        AgentMode::Autonomous
    ));
    assert!(matches!(
        "garbage".parse::<AgentMode>().unwrap(),
        AgentMode::Balanced
    ));
    assert!(matches!(
        "".parse::<AgentMode>().unwrap(),
        AgentMode::Balanced
    ));
}

// ── Agent Tax: precision mode structural guarantees ─────────────────

fn precision_config() -> AgentConfig {
    AgentConfig {
        name: "test-precision".into(),
        model: "mock-model".into(),
        provider: "mock".into(),
        system_prompt: None,
        max_iterations: 3,
        temperature: 0.0,
        toolsets: vec!["builtin".into()],
        hidden: false,
        allow_all: true,
        enabled_context_kinds: AgentMode::Precision.context_kinds(),
        essential_tools: vec![],
        context_kind_quotas: Default::default(),
        use_mtp: false,
        use_cot: false,
        allow_write: false,
        framework: None,
        model_variant: None,
        quantization: None,
        format_dialect: Default::default(),
        quirks: vec![],
        strict_mode: false,
        max_tools_per_turn: None,
        blueprint_path: None,
    }
}

fn balanced_config() -> AgentConfig {
    AgentConfig {
        name: "test-balanced".into(),
        model: "mock-model".into(),
        provider: "mock".into(),
        system_prompt: None,
        max_iterations: 3,
        temperature: 0.0,
        toolsets: vec!["builtin".into()],
        hidden: false,
        allow_all: true,
        enabled_context_kinds: AgentMode::Balanced.context_kinds(),
        essential_tools: vec![],
        context_kind_quotas: Default::default(),
        use_mtp: false,
        use_cot: false,
        allow_write: false,
        framework: None,
        model_variant: None,
        quantization: None,
        format_dialect: Default::default(),
        quirks: vec![],
        strict_mode: false,
        max_tools_per_turn: None,
        blueprint_path: None,
    }
}

// ── text-only termination test ──────────────────────────────────────

#[tokio::test]
async fn test_text_only_response_terminates_agent_loop() {
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
        "42 is the answer",
    )]));
    let registry = volt::test_utils::test_tool_registry().await;

    let agent = Agent::new(precision_config(), provider, registry).await;
    let result = agent.run("What is the meaning of life?").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "42 is the answer");
}

#[tokio::test]
async fn test_precision_mode_agent_runs_without_context_noise() {
    // Verify that a precision-mode agent works with mock LLM
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
        "done",
    )]));
    let registry = volt::tools::ToolRegistry::new();
    let agent = Agent::new(precision_config(), provider, registry).await;
    let result = agent.run("simple task").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_balanced_mode_agent_runs_without_context_noise() {
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
        "done",
    )]));
    let registry = volt::tools::ToolRegistry::new();
    let agent = Agent::new(balanced_config(), provider, registry).await;
    let result = agent.run("simple task").await;
    assert!(result.is_ok());
}

// ── Context kind correctness in config ──────────────────────────────

#[test]
fn test_precision_config_has_only_tool_and_artifact() {
    let kinds = precision_config().enabled_context_kinds;
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&ContextKind::Tool));
    assert!(kinds.contains(&ContextKind::Artifact));
}

#[test]
fn test_balanced_config_has_five_kinds() {
    let kinds = balanced_config().enabled_context_kinds;
    assert_eq!(kinds.len(), 5);
}

#[test]
fn test_precision_and_balanced_are_distinct() {
    let p = AgentMode::Precision.context_kinds();
    let b = AgentMode::Balanced.context_kinds();
    let a = AgentMode::Autonomous.context_kinds();
    assert_ne!(p.len(), b.len());
    assert_ne!(b.len(), a.len());
    assert!(a.len() > b.len());
    assert!(b.len() > p.len());
}

// ── Agent Tax documentation: 16.3pp gap explained ───────────────────

#[test]
fn test_agent_tax_cardinality() {
    // The 16.3pp gap between raw LLM (98.8%) and agent pipeline (82.5%)
    // is the cost of wrapping a text-prediction engine in an autonomous loop.
    //
    // Precision mode removes three noise sources:
    // 1. Session message loading (conversation history)
    // 2. Skill registry lookups
    // 3. DB memory searches
    //
    // This test verifies the structural guarantee: precision mode
    // excludes all non-essential context kinds.
    let precision = AgentMode::Precision.context_kinds();
    let noise_kinds: Vec<ContextKind> = vec![
        ContextKind::Skill,
        ContextKind::Memory,
        ContextKind::Conversation,
        ContextKind::AgentRun,
        ContextKind::SystemPrompt,
        ContextKind::FewShot,
        ContextKind::Policy,
        ContextKind::Permission,
        ContextKind::Security,
        ContextKind::MCPConfig,
    ];
    for noise in noise_kinds {
        assert!(
            !precision.contains(&noise),
            "Precision mode must exclude {:?} — this is an Agent Tax source",
            noise
        );
    }
}

#[test]
fn test_balanced_mode_retains_optimal_five() {
    // The 5-kind optimal from context kind ablation (§4.8)
    // must be the exact balanced mode set — changing it regresses accuracy.
    let balanced = AgentMode::Balanced.context_kinds();
    let required: Vec<ContextKind> = vec![
        ContextKind::Tool,
        ContextKind::Skill,
        ContextKind::Memory,
        ContextKind::Conversation,
        ContextKind::Artifact,
    ];
    for kind in required {
        assert!(
            balanced.contains(&kind),
            "Balanced mode must include {:?} — 5-kind optimal from ablation",
            kind
        );
    }
}
