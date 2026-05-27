use crate::agent::loop_rs::Agent;
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::openai::OpenAIProvider;
use crate::llm::LLMProvider;
use crate::models::AgentConfig;
use crate::tools::ToolRegistry;
use std::sync::Arc;
use std::time::Instant;

/// The cloud provider type for LLM API routing.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
}

#[derive(Clone)]
/// Resolved route for an LLM provider — kind, base URL, and API key.
// NOTE: Debug is manual to redact api_key.
pub struct ProviderRoute {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
}

impl std::fmt::Debug for ProviderRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRoute")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key", &"***")
            .finish()
    }
}

/// Specification for an agent in a multi-agent workflow — name, model, prompt, limits.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub max_iterations: u32,
    pub temperature: f32,
    pub allow_all: bool,
    /// Context profile mode. None = default (all 12 kinds).
    pub mode: Option<crate::commands::AgentMode>,
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub agent_name: String,
    pub output: String,
    pub duration_ms: u128,
    pub success: bool,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct WorkflowResult {
    pub steps: Vec<StepResult>,
    pub final_output: String,
    pub total_duration_ms: u128,
}

pub struct Orchestrator {
    tools: Arc<ToolRegistry>,
}

fn create_agent(spec: AgentSpec, tools: Arc<ToolRegistry>) -> Agent {
    let route = resolve_provider(&spec.model);

    let llm_provider: Box<dyn LLMProvider> = match route.kind {
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(
            route.api_key,
            Some(route.base_url),
            spec.name.clone(),
        )),
        ProviderKind::OpenAI => Box::new(OpenAIProvider::new(
            route.api_key,
            route.base_url,
            spec.name.clone(),
        )),
    };

    let provider_label = match route.kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAI => "openai",
    };

    let enabled_context_kinds = match spec.mode {
        Some(ref mode) => mode.context_kinds(),
        None => crate::models::default_context_kinds(),
    };

    Agent::new(
        AgentConfig {
            name: spec.name,
            model: spec.model,
            provider: provider_label.into(),
            system_prompt: spec.system_prompt,
            max_iterations: spec.max_iterations,
            temperature: spec.temperature,
            toolsets: vec!["builtin".into()],
            hidden: true,
            allow_all: spec.allow_all,
            enabled_context_kinds,
            essential_tools: crate::models::default_essential_tools(),
            context_kind_quotas: Default::default(),
        },
        llm_provider,
        tools,
    )
}

fn kind_from_str(s: &str) -> ProviderKind {
    if s.eq_ignore_ascii_case("anthropic") {
        ProviderKind::Anthropic
    } else {
        ProviderKind::OpenAI
    }
}

/// Resolve model name to a provider route.
///
/// Routing order:
///   1. User-defined routes from `LLM_MODEL_ROUTES` env var (JSON array)
///   2. Built-in model family detection (Anthropic, OpenAI, Nvidia)
///   3. Default: Groq (set `LLM_DEFAULT_PROVIDER` env var to override)
///
/// NOTE: Ollama is NOT routed here. Ollama is used exclusively for
/// embeddings via `src/embedding.rs` and is separate from text generation.
pub fn resolve_provider(model: &str) -> ProviderRoute {
    let m = model.to_lowercase();

    // ── 1. User-defined overrides ─────────────────────────────────
    if let Ok(routes_json) = std::env::var("LLM_MODEL_ROUTES") {
        if let Ok(routes) = serde_json::from_str::<Vec<serde_json::Value>>(&routes_json) {
            for route in &routes {
                let prefixes = route["models"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for prefix in &prefixes {
                    if m.contains(prefix) {
                        let kind = kind_from_str(route["provider"].as_str().unwrap_or("openai"));
                        let base_url = route["base_url"].as_str().unwrap_or("").to_string();
                        let api_key_env = route["api_key_env"].as_str().unwrap_or("LLM_API_KEY");
                        let api_key = std::env::var(api_key_env).unwrap_or_default();
                        return ProviderRoute {
                            kind,
                            base_url,
                            api_key,
                        };
                    }
                }
            }
        }
    }

    // ── 2. Built-in smart routing ─────────────────────────────────
    // Claude → Anthropic
    if m.contains("claude") {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default();
        return ProviderRoute {
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key,
        };
    }

    // GPT / O-series → OpenAI (vendor-prefixed models like openai/gpt-oss-20b
    // are hosted on Groq, not OpenAI; only native GPT/O names here)
    if m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        return ProviderRoute {
            kind: ProviderKind::OpenAI,
            base_url: "https://api.openai.com/v1".into(),
            api_key,
        };
    }

    // Nvidia NIM
    if m.starts_with("nvlm") || m.contains("nvidia") {
        let api_key = std::env::var("NVIDIA_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default();
        return ProviderRoute {
            kind: ProviderKind::OpenAI,
            base_url: "https://integrate.api.nvidia.com/v1".into(),
            api_key,
        };
    }

    // ── 3. Default: Groq (or `LLM_DEFAULT_PROVIDER`) ──────────────
    let default_provider = std::env::var("LLM_DEFAULT_PROVIDER")
        .unwrap_or_else(|_| "groq".into())
        .to_lowercase();

    match default_provider.as_str() {
        "anthropic" => {
            let api_key = std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            ProviderRoute {
                kind: ProviderKind::Anthropic,
                base_url: "https://api.anthropic.com".into(),
                api_key,
            }
        }
        _ => {
            let (base_url, key_env) = match default_provider.as_str() {
                "openai" => ("https://api.openai.com/v1", "OPENAI_API_KEY"),
                "nvidia" => ("https://integrate.api.nvidia.com/v1", "NVIDIA_API_KEY"),
                _ => ("https://api.groq.com/openai/v1", "GROQ_API_KEY"),
            };
            let api_key = std::env::var(key_env)
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            ProviderRoute {
                kind: ProviderKind::OpenAI,
                base_url: base_url.into(),
                api_key,
            }
        }
    }
}

impl Orchestrator {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }

    pub async fn run_parallel(
        &self,
        tasks: Vec<(AgentSpec, String)>,
    ) -> anyhow::Result<Vec<StepResult>> {
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(5));
        let mut handles = Vec::new();

        for (spec, task) in tasks {
            let tools = self.tools.clone();
            let permit = semaphore.clone().acquire_owned().await;
            let agent_name = spec.name.clone();
            handles.push(tokio::spawn(async move {
                let _permit = match permit {
                    Ok(p) => p,
                    Err(_) => {
                        return StepResult {
                            agent_name,
                            output: String::new(),
                            duration_ms: 0,
                            success: false,
                            prompt_tokens: 0,
                            completion_tokens: 0,
                        }
                    }
                };
                let step_started = Instant::now();
                let agent = create_agent(spec, tools);
                let result = match agent.run(&task).await {
                    Ok(output) => {
                        let state = agent.state().lock().await;
                        StepResult {
                            agent_name,
                            output,
                            duration_ms: step_started.elapsed().as_millis(),
                            success: true,
                            prompt_tokens: state.total_prompt_tokens,
                            completion_tokens: state.total_completion_tokens,
                        }
                    }
                    Err(e) => {
                        let state = agent.state().lock().await;
                        StepResult {
                            agent_name,
                            output: format!("error: {}", e),
                            duration_ms: step_started.elapsed().as_millis(),
                            success: false,
                            prompt_tokens: state.total_prompt_tokens,
                            completion_tokens: state.total_completion_tokens,
                        }
                    }
                };
                result
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(r) => results.push(r),
                Err(_) => results.push(StepResult {
                    agent_name: "unknown".into(),
                    output: String::new(),
                    duration_ms: 0,
                    success: false,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                }),
            }
        }
        Ok(results)
    }

    pub async fn run_pipeline(
        &self,
        stages: Vec<(AgentSpec, String)>,
    ) -> anyhow::Result<WorkflowResult> {
        let started = Instant::now();
        let mut step_results = Vec::new();
        let mut prev_output = String::new();

        for (spec, task_template) in stages {
            let step_started = Instant::now();
            let task = task_template.replace("{prev}", &prev_output);
            let agent_name = spec.name.clone();

            let agent = create_agent(spec, self.tools.clone());
            match agent.run(&task).await {
                Ok(output) => {
                    let state = agent.state().lock().await;
                    prev_output = output.clone();
                    step_results.push(StepResult {
                        agent_name,
                        output,
                        duration_ms: step_started.elapsed().as_millis(),
                        success: true,
                        prompt_tokens: state.total_prompt_tokens,
                        completion_tokens: state.total_completion_tokens,
                    });
                }
                Err(e) => {
                    let state = agent.state().lock().await;
                    step_results.push(StepResult {
                        agent_name,
                        output: format!("error: {}", e),
                        duration_ms: step_started.elapsed().as_millis(),
                        success: false,
                        prompt_tokens: state.total_prompt_tokens,
                        completion_tokens: state.total_completion_tokens,
                    });
                    break;
                }
            }
        }

        let final_output = step_results
            .last()
            .map(|r| r.output.clone())
            .unwrap_or_default();

        Ok(WorkflowResult {
            steps: step_results,
            final_output,
            total_duration_ms: started.elapsed().as_millis(),
        })
    }

    pub async fn run_workflow(
        &self,
        pattern: &str,
        specs: Vec<AgentSpec>,
        tasks: Vec<String>,
    ) -> anyhow::Result<WorkflowResult> {
        match pattern {
            "parallel" => {
                let task_pairs: Vec<(AgentSpec, String)> = specs.into_iter().zip(tasks).collect();
                let steps = self.run_parallel(task_pairs).await?;
                let final_output = steps
                    .iter()
                    .map(|s| format!("[{}]\n{}", s.agent_name, s.output))
                    .collect::<Vec<_>>()
                    .join("\n---\n");
                let total_duration_ms: u128 = steps.iter().map(|s| s.duration_ms).sum();
                Ok(WorkflowResult {
                    steps,
                    final_output,
                    total_duration_ms,
                })
            }
            "pipeline" => {
                let stages: Vec<(AgentSpec, String)> = specs.into_iter().zip(tasks).collect();
                self.run_pipeline(stages).await
            }
            "supervisor" => {
                let task = tasks.first().cloned().unwrap_or_default();
                self.run_supervisor(&task, specs).await
            }
            _ => anyhow::bail!(
                "unknown workflow pattern: {}. use 'parallel', 'pipeline', or 'supervisor'",
                pattern
            ),
        }
    }

    pub async fn run_supervisor(
        &self,
        task: &str,
        worker_specs: Vec<AgentSpec>,
    ) -> anyhow::Result<WorkflowResult> {
        let started = Instant::now();

        let worker_descriptions: Vec<String> = worker_specs
            .iter()
            .map(|w| format!("- {} (model: {})", w.name, w.model))
            .collect();
        let worker_block = worker_descriptions.join("\n");

        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "qwen/qwen3-32b".into());
        let supervisor_spec = AgentSpec {
            name: "supervisor".into(),
            model,
            system_prompt: Some(format!(
                "You are a supervisor agent coordinating worker agents.\n\n\
                 Available workers:\n{}\n\n\
                 Route the user's task to the appropriate worker(s) and synthesize their results.",
                worker_block
            )),
            max_iterations: 15,
            temperature: 0.3,
            allow_all: false,
            mode: None,
        };

        let supervisor = create_agent(supervisor_spec, self.tools.clone());
        let output = supervisor.run(task).await?;
        let state = supervisor.state().lock().await;

        Ok(WorkflowResult {
            steps: vec![StepResult {
                agent_name: "supervisor".into(),
                output: output.clone(),
                duration_ms: started.elapsed().as_millis(),
                success: true,
                prompt_tokens: state.total_prompt_tokens,
                completion_tokens: state.total_completion_tokens,
            }],
            final_output: output,
            total_duration_ms: started.elapsed().as_millis(),
        })
    }
}

pub fn parse_agent_specs(json: &str) -> anyhow::Result<Vec<AgentSpec>> {
    let specs: Vec<serde_json::Value> = serde_json::from_str(json)?;
    specs
        .into_iter()
        .map(|v| {
            Ok(AgentSpec {
                name: v["name"].as_str().unwrap_or("agent").to_string(),
                model: v["model"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into())
                    }),
                system_prompt: v["system_prompt"].as_str().map(|s| s.to_string()),
                max_iterations: v["max_iterations"].as_u64().unwrap_or(10) as u32,
                temperature: v["temperature"].as_f64().unwrap_or(0.3) as f32,
                allow_all: v["allow_all"].as_bool().unwrap_or(false),
                mode: v["mode"]
                    .as_str()
                    .and_then(|s| s.parse::<crate::commands::AgentMode>().ok()),
            })
        })
        .collect()
}

/// Build an LLM provider from a model identifier string.
pub fn build_provider(model: &str, agent_name: &str) -> (Box<dyn LLMProvider>, String) {
    let route = resolve_provider(model);
    let kind_str = match route.kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAI => "openai",
    };
    let provider: Box<dyn LLMProvider> = match route.kind {
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(
            route.api_key,
            Some(route.base_url),
            agent_name.into(),
        )),
        ProviderKind::OpenAI => Box::new(OpenAIProvider::new(
            route.api_key,
            route.base_url,
            agent_name.into(),
        )),
    };
    (provider, kind_str.to_string())
}

// ── DAG Multi-Agent Orchestration ──────────────────────────────

/// A node in the DAG — represents a single agent task with input/output bindings.
#[derive(Debug, Clone)]
pub struct DagNode {
    /// Unique identifier for this node (used in edge references).
    pub id: String,
    /// Agent specification (name, model, prompt, etc.).
    pub agent: AgentSpec,
    /// Task template with {input} / {node_id} placeholders for substitution.
    pub task_template: String,
}

/// A directed edge between two DAG nodes — represents data flow.
#[derive(Debug, Clone)]
pub struct DagEdge {
    /// Source node ID.
    pub from: String,
    /// Target node ID.
    pub to: String,
}

/// A complete DAG workflow definition.
#[derive(Debug, Clone)]
pub struct DagWorkflow {
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
}

impl DagWorkflow {
    /// Parse a DAG workflow from JSON.
    /// Expected format:
    /// ```json
    /// {
    ///   "nodes": [
    ///     {"id": "a", "task": "do {input}", "agent": {"name": "agent-a", ...}},
    ///     {"id": "b", "task": "process {a}", "agent": {"name": "agent-b", ...}}
    ///   ],
    ///   "edges": [{"from": "a", "to": "b"}]
    /// }
    /// ```
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        let nodes_arr = v["nodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("DAG must have a 'nodes' array"))?;

        let nodes: Vec<DagNode> = nodes_arr
            .iter()
            .map(|n| {
                let agent_val = &n["agent"];
                let agent_specs =
                    parse_agent_specs(&serde_json::to_string(std::slice::from_ref(agent_val))?)?;
                let agent = agent_specs
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("each DAG node must have an 'agent' object"))?;
                Ok(DagNode {
                    id: n["id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("each DAG node must have an 'id' string"))?
                        .to_string(),
                    agent,
                    task_template: n["task"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("each DAG node must have a 'task' string"))?
                        .to_string(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let edges: Vec<DagEdge> = if let Some(edges_arr) = v["edges"].as_array() {
            edges_arr
                .iter()
                .map(|e| {
                    Ok(DagEdge {
                        from: e["from"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("each edge must have a 'from' string"))?
                            .to_string(),
                        to: e["to"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("each edge must have a 'to' string"))?
                            .to_string(),
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?
        } else {
            Vec::new()
        };

        Ok(Self { nodes, edges })
    }

    /// Build a lookup of node ID -> node.
    fn node_map(&self) -> std::collections::HashMap<String, &DagNode> {
        self.nodes.iter().map(|n| (n.id.clone(), n)).collect()
    }

    /// Build adjacency: for each node ID, list of successor node IDs.
    fn adjacency(&self) -> std::collections::HashMap<String, Vec<String>> {
        let mut adj: std::collections::HashMap<String, Vec<String>> = self
            .nodes
            .iter()
            .map(|n| (n.id.clone(), Vec::new()))
            .collect();
        for edge in &self.edges {
            adj.entry(edge.from.clone())
                .or_default()
                .push(edge.to.clone());
        }
        adj
    }

    /// Build reverse adjacency: for each node ID, list of predecessor node IDs.
    fn reverse_adjacency(&self) -> std::collections::HashMap<String, Vec<String>> {
        let mut radj: std::collections::HashMap<String, Vec<String>> = self
            .nodes
            .iter()
            .map(|n| (n.id.clone(), Vec::new()))
            .collect();
        for edge in &self.edges {
            radj.entry(edge.to.clone())
                .or_default()
                .push(edge.from.clone());
        }
        radj
    }

    /// Topological sort using Kahn's algorithm.
    /// Returns node IDs in execution order (all predecessors before successors).
    pub fn topological_sort(&self) -> anyhow::Result<Vec<String>> {
        let adj = self.adjacency();
        let mut in_degree: std::collections::HashMap<String, usize> =
            self.nodes.iter().map(|n| (n.id.clone(), 0usize)).collect();
        for edge in &self.edges {
            *in_degree.entry(edge.to.clone()).or_insert(0) += 1;
        }

        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| id.clone())
            .collect();

        let mut sorted = Vec::new();
        while let Some(node_id) = queue.pop() {
            sorted.push(node_id.clone());
            if let Some(successors) = adj.get(&node_id) {
                for succ in successors {
                    if let Some(deg) = in_degree.get_mut(succ) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(succ.clone());
                        }
                    }
                }
            }
        }

        if sorted.len() != self.nodes.len() {
            anyhow::bail!(
                "DAG contains a cycle: sorted {} of {} nodes",
                sorted.len(),
                self.nodes.len()
            );
        }

        Ok(sorted)
    }

    /// Group topologically sorted nodes into parallel execution levels.
    /// Nodes within a level have no path between them and can run concurrently.
    pub fn execution_levels(&self) -> anyhow::Result<Vec<Vec<String>>> {
        let sorted = self.topological_sort()?;
        let radj = self.reverse_adjacency();

        // Level 0: nodes with no predecessors
        let mut levels: Vec<Vec<String>> = Vec::new();
        let mut assigned: std::collections::HashSet<String> = std::collections::HashSet::new();

        for node_id in &sorted {
            let preds = radj.get(node_id).map(|v| v.as_slice()).unwrap_or(&[]);
            let all_preds_assigned = preds.iter().all(|p| assigned.contains(p));
            if preds.is_empty() || all_preds_assigned {
                // Find the level to assign: max level of predecessors + 1
                let pred_level = preds
                    .iter()
                    .filter_map(|p| {
                        levels.iter().enumerate().find_map(|(li, level)| {
                            if level.contains(p) {
                                Some(li)
                            } else {
                                None
                            }
                        })
                    })
                    .max()
                    .map(|l| l + 1)
                    .unwrap_or(0);

                // Ensure levels vector has room
                while levels.len() <= pred_level {
                    levels.push(Vec::new());
                }
                levels[pred_level].push(node_id.clone());
                assigned.insert(node_id.clone());
            }
        }

        Ok(levels)
    }
}

/// A DAG scheduler that executes multi-agent workflows as directed acyclic graphs.
pub struct DagScheduler<'a> {
    tools: &'a Arc<ToolRegistry>,
}

impl<'a> DagScheduler<'a> {
    pub fn new(tools: &'a Arc<ToolRegistry>) -> Self {
        Self { tools }
    }

    /// Execute a DAG workflow with the given initial input.
    /// Returns a map of node ID -> agent output for all executed nodes.
    pub async fn execute(
        &self,
        workflow: &DagWorkflow,
        initial_input: &str,
    ) -> anyhow::Result<std::collections::HashMap<String, String>> {
        let levels = workflow.execution_levels()?;
        let node_map = workflow.node_map();
        let radj = workflow.reverse_adjacency();

        // Accumulates outputs: node_id -> output text
        let mut outputs: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        outputs.insert("input".to_string(), initial_input.to_string());

        for level_nodes in &levels {
            // Build all task payloads before spawning (avoids lifetime issues)
            let mut payloads: Vec<(String, AgentSpec, String)> = Vec::new();

            for node_id in level_nodes {
                let node = node_map
                    .get(node_id)
                    .ok_or_else(|| anyhow::anyhow!("node '{}' not found", node_id))?;

                let predecesors = radj.get(node_id).cloned().unwrap_or_default();
                let mut task = node.task_template.clone();

                if predecesors.is_empty() {
                    task = task.replace("{input}", initial_input);
                }

                for pred_id in &predecesors {
                    if let Some(pred_output) = outputs.get(pred_id) {
                        task = task.replace(&format!("{{{}}}", pred_id), pred_output);
                    }
                }

                for (oid, ooutput) in &outputs {
                    task = task.replace(&format!("{{{}}}", oid), ooutput);
                }

                payloads.push((node_id.clone(), node.agent.clone(), task));
            }

            // Execute all payloads in this level concurrently
            let mut handles = Vec::new();
            for (node_id, agent_spec, task) in payloads {
                let tools = self.tools.clone();
                handles.push(tokio::spawn(async move {
                    let agent = create_agent(agent_spec, tools);
                    let result = agent.run(&task).await;
                    (node_id, result)
                }));
            }

            // Collect results for this level
            for handle in handles {
                let (node_id, result) = handle
                    .await
                    .map_err(|e| anyhow::anyhow!("task join: {}", e))?;
                let output = result?;
                outputs.insert(node_id, output);
            }
        }

        Ok(outputs)
    }
}

impl Orchestrator {
    /// Execute a DAG workflow. The JSON should contain the workflow definition
    /// plus a "task" field for the initial input.
    pub async fn run_dag(
        &self,
        dag_json: &str,
        initial_input: &str,
    ) -> anyhow::Result<WorkflowResult> {
        let started = std::time::Instant::now();
        let workflow = DagWorkflow::from_json(dag_json)?;
        let scheduler = DagScheduler::new(&self.tools);
        let outputs = scheduler.execute(&workflow, initial_input).await?;

        // Build the workflow result
        let mut steps = Vec::new();
        for node in &workflow.nodes {
            if let Some(output) = outputs.get(&node.id) {
                steps.push(StepResult {
                    agent_name: node.agent.name.clone(),
                    output: output.clone(),
                    duration_ms: 0,
                    success: true,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                });
            }
        }

        // Find a "final_output" marker or use the last executed node's output
        let final_output = outputs
            .get("final_output")
            .or_else(|| workflow.nodes.last().and_then(|n| outputs.get(&n.id)))
            .cloned()
            .unwrap_or_default();

        Ok(WorkflowResult {
            steps,
            final_output,
            total_duration_ms: started.elapsed().as_millis(),
        })
    }
}
