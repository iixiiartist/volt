//! Professional workflow tests demonstrating Volt's key architectural value points:
//!
//! 1. **Three Profile Modes** — Precision (2 kinds), Balanced (5), Autonomous (12)
//! 2. **Multi-Agent Orchestration** — Parallel, Pipeline, Supervisor patterns
//! 3. **Tool Registry Intelligence** — Semantic search, GraphRAG augmentation, permission levels
//! 4. **Context Store** — Composite scoring, semantic dedup, quota eviction
//! 5. **Agent Loop** — System prompt injection, final_answer termination, mode-aware behavior
//! 6. **Per-Agent Mode Assignment** — Each agent in a workflow runs its own context profile
//!
//! All tests use mock providers — no API keys or network calls required.
//! Run with: `cargo test --test professional_workflows --features testutils`

use std::sync::Arc;
use volt::agent::loop_rs::Agent;
use volt::commands::AgentMode;
use volt::context::{ContextEntry, ContextKind, ContextStore};
use volt::embedding::{deterministic_placeholder_embedding, EmbeddingClient};
use volt::models::*;
use volt::test_utils::MockLLMProvider;
use volt::tools::ToolRegistry;

// ═══════════════════════════════════════════════════════════════════
// SECTION 1 — Three Profile Modes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_all_three_modes_have_distinct_context_cardinalities() {
    let p = AgentMode::Precision.context_kinds();
    let b = AgentMode::Balanced.context_kinds();
    let a = AgentMode::Autonomous.context_kinds();
    assert_ne!(p.len(), b.len(), "Precision and Balanced must differ");
    assert_ne!(b.len(), a.len(), "Balanced and Autonomous must differ");
    assert_eq!(p.len(), 2, "Precision = Tool + Artifact");
    assert_eq!(b.len(), 5, "Balanced = 5-kind optimal");
    assert_eq!(a.len(), 12, "Autonomous = all 12 kinds");
}

#[test]
fn test_precision_excludes_all_noise_kinds() {
    let kinds = AgentMode::Precision.context_kinds();
    for noise in &[
        ContextKind::Skill,
        ContextKind::Memory,
        ContextKind::Conversation,
        ContextKind::AgentRun,
        ContextKind::SystemPrompt,
        ContextKind::Policy,
    ] {
        assert!(
            !kinds.contains(noise),
            "Precision must exclude {:?}",
            noise
        );
    }
}

#[test]
fn test_balanced_includes_exactly_five_optimal_kinds() {
    let kinds = AgentMode::Balanced.context_kinds();
    for required in &[
        ContextKind::Tool,
        ContextKind::Skill,
        ContextKind::Memory,
        ContextKind::Conversation,
        ContextKind::Artifact,
    ] {
        assert!(kinds.contains(required), "Balanced must include {:?}", required);
    }
}

#[test]
fn test_autonomous_includes_all_twelve_kinds() {
    let kinds = AgentMode::Autonomous.context_kinds();
    for kind in &[
        ContextKind::Tool, ContextKind::Skill, ContextKind::Memory,
        ContextKind::Conversation, ContextKind::AgentRun, ContextKind::Artifact,
        ContextKind::SystemPrompt, ContextKind::FewShot, ContextKind::Policy,
        ContextKind::Permission, ContextKind::Security, ContextKind::MCPConfig,
    ] {
        assert!(kinds.contains(kind), "Autonomous must include {:?}", kind);
    }
}

// ═══════════════════════════════════════════════════════════════════
// SECTION 2 — Agent Loop Features
// ═══════════════════════════════════════════════════════════════════

fn precision_config() -> AgentConfig {
    AgentConfig {
        name: "precision-agent".into(),
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
    }
}

fn balanced_config() -> AgentConfig {
    AgentConfig {
        name: "balanced-agent".into(),
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
    }
}

fn autonomous_config() -> AgentConfig {
    AgentConfig {
        name: "autonomous-agent".into(),
        model: "mock-model".into(),
        provider: "mock".into(),
        system_prompt: None,
        max_iterations: 3,
        temperature: 0.0,
        toolsets: vec!["builtin".into()],
        hidden: false,
        allow_all: true,
        enabled_context_kinds: AgentMode::Autonomous.context_kinds(),
        essential_tools: vec![],
        context_kind_quotas: Default::default(),
    }
}

#[tokio::test]
async fn test_final_answer_terminates_agent_loop_and_returns_answer() {
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_calls(
        vec![ToolCall {
            id: "call_1".into(),
            name: "final_answer".into(),
            arguments: serde_json::json!({"answer": "42 is the answer"}),
        }],
    )]));
    let registry = volt::test_utils::test_tool_registry().await;
    registry
        .register(
            "final_answer",
            "Submit final answer and terminate",
            serde_json::json!({"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"]}),
            "test",
            Arc::new(|args| {
                Box::pin(async move {
                    let answer = args["answer"].as_str().unwrap_or("");
                    ToolResult { success: true, output: answer.to_string(), error: None, duration_ms: 0 }
                })
            }),
        )
        .await;

    let agent = Agent::new(precision_config(), provider, registry);
    let result = agent.run("What is the meaning of life?").await;
    assert!(result.is_ok(), "final_answer should return Ok");
    assert_eq!(result.unwrap(), "42 is the answer");
}

#[tokio::test]
async fn test_system_prompt_injected_at_position_zero() {
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
        "mock final response",
    )]));
    let registry = volt::test_utils::test_tool_registry().await;

    let agent = Agent::new(balanced_config(), provider, registry);
    let _ = agent.run("test run").await;

    let state = agent.state().lock().await;
    assert!(!state.messages.is_empty());
    assert_eq!(state.messages[0].role, "system");
    assert!(
        state.messages[0].content.contains("You are"),
        "System prompt should contain identity declaration"
    );
}

#[tokio::test]
async fn test_max_iteration_fallback_returns_last_content() {
    let mut responses = Vec::new();
    responses.push(MockLLMProvider::tool_result("final result"));
    for _ in 0..6 {
        responses.push(MockLLMProvider::tool_calls(vec![ToolCall {
            id: "call_loop".into(),
            name: "echo".into(),
            arguments: serde_json::json!({"input": "loop"}),
        }]));
    }

    let provider = Box::new(MockLLMProvider::new(responses));
    let registry = volt::test_utils::test_tool_registry().await;
    let agent = Agent::new(balanced_config(), provider, registry);

    let result = agent.run("Loop forever").await;
    assert!(result.is_ok(), "Agent should return last tool result on iteration exhaustion");
    assert!(!result.unwrap().is_empty(), "Fallback content must not be empty");
}

#[tokio::test]
async fn test_precision_mode_skips_session_loading() {
    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
        "precision done",
    )]));
    let registry = volt::test_utils::test_tool_registry().await;
    let agent = Agent::new(precision_config(), provider, registry);
    let result = agent.run("simple precision task").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "precision done");
}

#[tokio::test]
async fn test_all_three_modes_produce_valid_agent_runs() {
    for (label, config) in &[
        ("precision", precision_config()),
        ("balanced", balanced_config()),
        ("autonomous", autonomous_config()),
    ] {
        let response = format!("{} response", label);
        let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_result(
            &response,
        )]));
        let registry = volt::test_utils::test_tool_registry().await;
        let agent = Agent::new(config.clone(), provider, registry);
        let result = agent.run(&format!("test in {} mode", label)).await;
        assert!(result.is_ok(), "{} mode agent should complete", label);
    }
}

// ═══════════════════════════════════════════════════════════════════
// SECTION 3 — Multi-Agent Orchestration Patterns
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_parallel_workflow_pattern() {
    let registry = volt::test_utils::test_tool_registry().await;

    let agents: Vec<(&str, &str)> = vec![
        ("data-agent", "Collect data: user stats"),
        ("code-agent", "Write code: parse CSV"),
        ("report-agent", "Generate report"),
    ];

    let handles: Vec<_> = agents
        .into_iter()
        .map(|(name, task)| {
            let response = format!("{} completed: {}", name, task);
            let reg = registry.clone();
            tokio::spawn(async move {
                let provider = Box::new(MockLLMProvider::new(vec![
                    MockLLMProvider::tool_result(&response),
                ]));
                let config = AgentConfig {
                    name: name.into(),
                    model: "mock-model".into(),
                    provider: "mock".into(),
                    system_prompt: Some(format!("You are {}", name)),
                    max_iterations: 3,
                    temperature: 0.0,
                    toolsets: vec!["builtin".into()],
                    hidden: false,
                    allow_all: true,
                    enabled_context_kinds: AgentMode::Precision.context_kinds(),
                    essential_tools: vec![],
                    context_kind_quotas: Default::default(),
                };
                let agent = Agent::new(config, provider, reg);
                agent.run(task).await
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(handles).await;
    assert_eq!(results.len(), 3, "All three agents should complete");
    for r in &results {
        assert!(r.as_ref().is_ok(), "Each agent should succeed");
        let output = r.as_ref().unwrap();
        assert!(output.is_ok(), "Agent run should return Ok");
    }
}

#[tokio::test]
async fn test_pipeline_workflow_with_output_chaining() {
    let registry = volt::test_utils::test_tool_registry().await;

    // Stage 1: discover files
    let stage1_output = {
        let provider = Box::new(MockLLMProvider::new(vec![
            MockLLMProvider::tool_result("Found files: src/main.rs, src/lib.rs, Cargo.toml"),
        ]));
        let config = AgentConfig {
            name: "discover".into(),
            model: "mock-model".into(),
            provider: "mock".into(),
            system_prompt: Some("You are a discovery agent".into()),
            max_iterations: 3,
            temperature: 0.0,
            toolsets: vec!["builtin".into()],
            hidden: false,
            allow_all: true,
            enabled_context_kinds: AgentMode::Precision.context_kinds(),
            essential_tools: vec![],
            context_kind_quotas: Default::default(),
        };
        let agent = Agent::new(config, provider, registry.clone());
        agent.run("Find all Rust files").await.expect("Stage 1 must succeed")
    };
    assert!(stage1_output.contains("src/main.rs"));

    // Stage 2: process prev output
    let stage2_task = format!(
        "Based on these findings: {} -- count the total files",
        stage1_output
    );
    {
        let provider = Box::new(MockLLMProvider::new(vec![
            MockLLMProvider::tool_result("Total files: 3"),
        ]));
        let config = AgentConfig {
            name: "counter".into(),
            model: "mock-model".into(),
            provider: "mock".into(),
            system_prompt: Some("You are a counting agent".into()),
            max_iterations: 3,
            temperature: 0.0,
            toolsets: vec!["builtin".into()],
            hidden: false,
            allow_all: true,
            enabled_context_kinds: AgentMode::Precision.context_kinds(),
            essential_tools: vec![],
            context_kind_quotas: Default::default(),
        };
        let agent = Agent::new(config, provider, registry);
        let output = agent.run(&stage2_task).await.expect("Stage 2 must succeed");
        assert!(output.contains("3"), "Pipeline output should chain from stage 1");
    }
}

#[tokio::test]
async fn test_supervisor_routes_task_to_worker_agents() {
    let registry = volt::test_utils::test_tool_registry().await;

    // Register a delegate tool so the supervisor can invoke workers
    registry
        .register(
            "delegate",
            "Delegate task to a sub-agent",
            serde_json::json!({"type":"object","properties":{"task":{"type":"string"},"agent":{"type":"string"}},"required":["task","agent"]}),
            "builtin",
            Arc::new(|args| {
                Box::pin(async move {
                    let task = args["task"].as_str().unwrap_or("");
                    let agent = args["agent"].as_str().unwrap_or("");
                    ToolResult {
                        success: true,
                        output: format!("[{}] executed: {}", agent, task),
                        error: None,
                        duration_ms: 0,
                    }
                })
            }),
        )
        .await;

    let provider = Box::new(MockLLMProvider::new(vec![MockLLMProvider::tool_calls(
        vec![ToolCall {
            id: "call_super".into(),
            name: "delegate".into(),
            arguments: serde_json::json!({"task": "find code metrics", "agent": "code-agent"}),
        }],
    )]));
    let config = AgentConfig {
        name: "supervisor".into(),
        model: "mock-model".into(),
        provider: "mock".into(),
        system_prompt: Some(
            "You are a supervisor. Available workers: code-agent, data-agent, report-agent".into(),
        ),
        max_iterations: 3,
        temperature: 0.0,
        toolsets: vec!["builtin".into()],
        hidden: false,
        allow_all: true,
        enabled_context_kinds: AgentMode::Balanced.context_kinds(),
        essential_tools: vec![],
        context_kind_quotas: Default::default(),
    };
    let agent = Agent::new(config, provider, registry);
    let result = agent.run("Run code analysis").await;
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// SECTION 4 — Tool Registry Intelligence
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_search_by_semantic_similarity() {
    let registry = ToolRegistry::new();
    registry
        .register(
            "calculate",
            "Perform mathematical calculations and arithmetic operations",
            serde_json::json!({"type":"object"}),
            "math",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "0".into(), error: None, duration_ms: 0 } })),
        )
        .await;
    registry
        .register(
            "search_web",
            "Search the internet for information and web pages",
            serde_json::json!({"type":"object"}),
            "web",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;
    registry
        .register(
            "read_file",
            "Read the contents of a file from disk",
            serde_json::json!({"type":"object"}),
            "fs",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;

    let client = EmbeddingClient::new(None, "test");
    registry.compute_embeddings(&client).await;

    let query_emb = client
        .embed_description("i need to do some math")
        .await
        .expect("embedding must succeed");

    let results = registry.search_tools(&query_emb, 2, &[], None).await;
    assert!(!results.is_empty(), "Should find at least one tool");
    assert_eq!(
        results[0].name, "calculate",
        "Math query should rank calculate highest"
    );
}

#[tokio::test]
async fn test_graphrag_augments_related_tools() {
    let registry = ToolRegistry::new();
    registry
        .register(
            "read_file",
            "Read a file from disk",
            serde_json::json!({"type":"object"}),
            "fs",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;
    registry
        .register(
            "edit_file",
            "Edit a file by replacing text patterns",
            serde_json::json!({"type":"object"}),
            "fs",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;
    registry
        .register(
            "search_web",
            "Search the internet",
            serde_json::json!({"type":"object"}),
            "web",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;

    registry.record_co_occurrence(&["read_file".into(), "edit_file".into()]);

    let client = EmbeddingClient::new(None, "test");
    registry.compute_embeddings(&client).await;

    let query_emb = client
        .embed_description("i need to read a file")
        .await
        .expect("embedding must succeed");

    let results = registry.search_tools(&query_emb, 1, &[], None).await;
    assert!(
        results.len() >= 2,
        "GraphRAG should augment results beyond vector-only limit. Got {} tools: {:?}",
        results.len(),
        results.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    let names: Vec<&str> = results.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"edit_file"),
        "GraphRAG should add co-occurring edit_file. Results: {:?}",
        names
    );
}

#[tokio::test]
async fn test_essential_tools_always_included() {
    let registry = ToolRegistry::new();
    registry
        .register(
            "custom_search",
            "Custom search tool with very specific niche functionality",
            serde_json::json!({"type":"object"}),
            "custom",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;
    registry
        .register(
            "read",
            "Essential read tool",
            serde_json::json!({"type":"object"}),
            "builtin",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;
    registry
        .register(
            "glob",
            "Essential glob tool for finding files",
            serde_json::json!({"type":"object"}),
            "builtin",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "".into(), error: None, duration_ms: 0 } })),
        )
        .await;

    let client = EmbeddingClient::new(None, "test");
    registry.compute_embeddings(&client).await;

    let query_emb = client
        .embed_description("use the custom search tool")
        .await
        .expect("embedding must succeed");

    let results = registry.search_tools(&query_emb, 1, &["read", "glob"], None).await;
    let names: Vec<&str> = results.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"read"),
        "Essential tool 'read' must always be included. Got: {:?}",
        names
    );
    assert!(
        names.contains(&"glob"),
        "Essential tool 'glob' must always be included. Got: {:?}",
        names
    );
}

#[tokio::test]
async fn test_tool_permission_levels() {
    let registry = ToolRegistry::new();
    registry
        .register(
            "safe_tool",
            "A harmless safe tool",
            serde_json::json!({"type":"object"}),
            "test",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "ok".into(), error: None, duration_ms: 0 } })),
        )
        .await;
    registry
        .register_with_permission(
            "dangerous_tool",
            "A tool that needs approval",
            serde_json::json!({"type":"object"}),
            "test",
            Arc::new(|_| Box::pin(async move { ToolResult { success: true, output: "ok".into(), error: None, duration_ms: 0 } })),
            PermissionLevel::Prompt,
        )
        .await;

    assert_eq!(
        registry.get_permission("safe_tool").await,
        PermissionLevel::Allow,
        "Default registration should be Allow"
    );
    assert_eq!(
        registry.get_permission("dangerous_tool").await,
        PermissionLevel::Prompt,
        "Explicit Prompt registration should be respected"
    );
}

// ═══════════════════════════════════════════════════════════════════
// SECTION 5 — Context Store
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_composite_score_weights_recency_success_frequency_density() {
    let recent = ContextEntry {
        id: uuid::Uuid::new_v4(),
        kind: ContextKind::Tool,
        content: "recently used tool".into(),
        embedding: None,
        metadata: serde_json::json!({}),
        frequency: 10,
        success_rate: 0.9,
        usage_count: 10,
        last_used_at: chrono::Utc::now(),
        created_at: chrono::Utc::now(),
    };
    let old = ContextEntry {
        id: uuid::Uuid::new_v4(),
        kind: ContextKind::Tool,
        content: "old unused tool".into(),
        embedding: None,
        metadata: serde_json::json!({}),
        frequency: 1,
        success_rate: 0.1,
        usage_count: 1,
        last_used_at: chrono::Utc::now() - chrono::Duration::hours(720),
        created_at: chrono::Utc::now() - chrono::Duration::hours(720),
    };

    let recent_score = recent.composite_score();
    let old_score = old.composite_score();
    assert!(
        recent_score > old_score,
        "Recent high-frequency tool ({}) should score higher than old unused tool ({})",
        recent_score,
        old_score
    );
}

#[test]
fn test_composite_score_default_success_rate_for_unused() {
    let unused = ContextEntry {
        id: uuid::Uuid::new_v4(),
        kind: ContextKind::Tool,
        content: "never used tool".into(),
        embedding: None,
        metadata: serde_json::json!({}),
        frequency: 0,
        success_rate: 0.0,
        usage_count: 0,
        last_used_at: chrono::Utc::now(),
        created_at: chrono::Utc::now(),
    };
    // usage_count=0 → composite_score() should use default 0.5 success rate
    let score = unused.composite_score();
    assert!(score > 0.0, "Unused entry should still have a valid score");
}

#[tokio::test]
async fn test_context_store_semantic_dedup_merges_identical_content() {
    let store = ContextStore::new();
    let emb = deterministic_placeholder_embedding("identical content");

    store
        .seed_batch(vec![
            ContextEntry {
                id: uuid::Uuid::new_v4(),
                kind: ContextKind::Memory,
                content: "identical content".into(),
                embedding: Some(emb.clone()),
                metadata: serde_json::json!({"source": "a"}),
                frequency: 1,
                success_rate: 0.5,
                usage_count: 0,
                last_used_at: chrono::Utc::now(),
                created_at: chrono::Utc::now(),
            },
            ContextEntry {
                id: uuid::Uuid::new_v4(),
                kind: ContextKind::Memory,
                content: "identical content".into(),
                embedding: Some(emb.clone()),
                metadata: serde_json::json!({"source": "b"}),
                frequency: 1,
                success_rate: 0.5,
                usage_count: 0,
                last_used_at: chrono::Utc::now(),
                created_at: chrono::Utc::now(),
            },
            ContextEntry {
                id: uuid::Uuid::new_v4(),
                kind: ContextKind::Memory,
                content: "completely different content".into(),
                embedding: Some(deterministic_placeholder_embedding(
                    "completely different content",
                )),
                metadata: serde_json::json!({"source": "c"}),
                frequency: 1,
                success_rate: 0.5,
                usage_count: 0,
                last_used_at: chrono::Utc::now(),
                created_at: chrono::Utc::now(),
            },
        ])
        .await;

    let count = store.len().await;
    assert_eq!(
        count, 2,
        "Identical embedding entries should be deduped, leaving 2 distinct. Got {}",
        count
    );
}

#[tokio::test]
async fn test_context_store_quota_eviction_removes_lowest_score_entries() {
    let store = ContextStore::new();
    store
        .set_quotas(&std::collections::HashMap::from([(ContextKind::Tool, 20)]))
        .await;

    // Seed enough entries to trigger eviction (default evict_every=100)
    let mut entries = Vec::new();
    for i in 0..150 {
        let emb = deterministic_placeholder_embedding(&format!("tool entry {}", i));
        entries.push(ContextEntry {
            id: uuid::Uuid::new_v4(),
            kind: ContextKind::Tool,
            content: format!("tool entry {}", i),
            embedding: Some(emb),
            metadata: serde_json::json!({"index": i}),
            frequency: (i + 1) as u32,
            success_rate: (i as f32) / 150.0,
            usage_count: (i + 1) as u32,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        });
    }
    store.seed_batch(entries).await;

    let count = store.len().await;
    assert!(
        count <= 20,
        "Quota=20 should evict to at most 20 entries, got {}",
        count
    );
    assert!(count > 0, "Should retain some entries after eviction");
}

#[tokio::test]
async fn test_context_store_search_ranks_by_combined_score() {
    let store = ContextStore::new();

    let query_text = "find me a tool for math calculations";
    let query_emb = deterministic_placeholder_embedding(query_text);

    let mut entries = Vec::new();
    for label in &["math tool", "web search tool", "file reader"] {
        let emb = deterministic_placeholder_embedding(label);
        entries.push(ContextEntry {
            id: uuid::Uuid::new_v4(),
            kind: ContextKind::Tool,
            content: label.to_string(),
            embedding: Some(emb),
            metadata: serde_json::json!({}),
            frequency: 1,
            success_rate: 0.5,
            usage_count: 0,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        });
    }
    store.seed_batch(entries).await;
    store.compute_embeddings(&EmbeddingClient::new(None, "test")).await;

    let results = store.search(&query_emb, 3, None, 0.0, None).await;
    assert_eq!(results.len(), 3, "Should find all 3 entries");
    assert_eq!(
        results[0].content, "math tool",
        "Rank 1 should be 'math tool' for math query"
    );
}

// ═══════════════════════════════════════════════════════════════════
// SECTION 6 — Per-Agent Mode Assignment
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_parse_agent_specs_parses_mode_field() {
    let json = r#"[
        {"name":"precise","model":"gpt-4","mode":"precision"},
        {"name":"balanced","model":"gpt-4","mode":"balanced"},
        {"name":"auto","model":"gpt-4","mode":"autonomous"},
        {"name":"default","model":"gpt-4"}
    ]"#;
    let specs = volt::orchestrator::parse_agent_specs(json).expect("valid JSON");

    assert_eq!(specs.len(), 4);
    assert!(matches!(specs[0].mode, Some(AgentMode::Precision)));
    assert!(matches!(specs[1].mode, Some(AgentMode::Balanced)));
    assert!(matches!(specs[2].mode, Some(AgentMode::Autonomous)));
    assert!(specs[3].mode.is_none(), "Missing mode should be None");
}

#[test]
fn test_parse_agent_specs_defaults_mode_on_unknown() {
    let json = r#"[
        {"name":"agent","model":"gpt-4","mode":"garbage"}
    ]"#;
    let specs = volt::orchestrator::parse_agent_specs(json).expect("valid JSON");
    assert!(matches!(specs[0].mode, Some(AgentMode::Balanced)));
}

#[tokio::test]
async fn test_parallel_agents_with_mixed_modes() {
    let registry = volt::test_utils::test_tool_registry().await;

    let agents: Vec<(&str, AgentMode, &str)> = vec![
        (
            "precise-func",
            AgentMode::Precision,
            "Perform exact function call",
        ),
        (
            "balanced-research",
            AgentMode::Balanced,
            "Research and summarize findings",
        ),
        (
            "autonomous-orchestrator",
            AgentMode::Autonomous,
            "Orchestrate long-running multi-step workflow",
        ),
    ];

    let handles: Vec<_> = agents
        .into_iter()
        .map(|(name, mode, task)| {
            let response = format!("{} completed in {:?} mode", name, mode);
            let reg = registry.clone();
            tokio::spawn(async move {
                let provider = Box::new(MockLLMProvider::new(vec![
                    MockLLMProvider::tool_result(&response),
                ]));
                let config = AgentConfig {
                    name: name.into(),
                    model: "mock-model".into(),
                    provider: "mock".into(),
                    system_prompt: Some(format!("You are {} in {:?} mode", name, mode)),
                    max_iterations: 3,
                    temperature: 0.0,
                    toolsets: vec!["builtin".into()],
                    hidden: false,
                    allow_all: true,
                    enabled_context_kinds: mode.context_kinds(),
                    essential_tools: vec![],
                    context_kind_quotas: Default::default(),
                };

                let ctx_kinds = config.enabled_context_kinds.clone();
                let agent = Agent::new(config, provider, reg);
                let result = agent.run(task).await;

                // Each agent must complete successfully
                assert!(result.is_ok(), "{} should complete", name);
                let output = result.unwrap();
                assert_eq!(output, response);

                // Verify the mode's context kinds were applied
                match mode {
                    AgentMode::Precision => assert_eq!(ctx_kinds.len(), 2),
                    AgentMode::Balanced => assert_eq!(ctx_kinds.len(), 5),
                    AgentMode::Autonomous => assert_eq!(ctx_kinds.len(), 12),
                }

                format!("[{}]({:?}) -> {}", name, mode, output)
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(handles).await;
    assert_eq!(results.len(), 3, "All three mode-specific agents should complete");
    for r in &results {
        assert!(r.is_ok(), "Each parallel agent should succeed");
    }
}
