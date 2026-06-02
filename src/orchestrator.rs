use crate::agent::Agent;
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
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkflowResult {
    pub steps: Vec<StepResult>,
    pub final_output: String,
    pub total_duration_ms: u128,
}

pub struct Orchestrator {
    tools: Arc<ToolRegistry>,
    cap_mgr: Arc<crate::capability::CapabilityManager>,
}

impl Orchestrator {
    pub async fn new(tools: Arc<ToolRegistry>) -> Self {
        let cap_mgr = {
            let mgr = Arc::new(crate::capability::CapabilityManager::new());
            mgr.issue(
                crate::capability::CapabilityScope::FsRead,
                100,
                chrono::Duration::hours(24),
            )
            .await;
            mgr.issue(
                crate::capability::CapabilityScope::FsWrite,
                50,
                chrono::Duration::hours(24),
            )
            .await;
            mgr.issue(
                crate::capability::CapabilityScope::System,
                20,
                chrono::Duration::hours(24),
            )
            .await;
            mgr.issue(
                crate::capability::CapabilityScope::Network,
                200,
                chrono::Duration::hours(24),
            )
            .await;
            mgr.issue(
                crate::capability::CapabilityScope::Database,
                30,
                chrono::Duration::hours(24),
            )
            .await;
            mgr.issue(
                crate::capability::CapabilityScope::Memory,
                50,
                chrono::Duration::hours(24),
            )
            .await;
            mgr
        };
        Self { tools, cap_mgr }
    }
}

async fn create_agent_with_provider(
    spec: AgentSpec,
    tools: Arc<ToolRegistry>,
    cap_mgr: Option<Arc<crate::capability::CapabilityManager>>,
    llm_provider: Box<dyn LLMProvider>,
) -> Agent {
    let enabled_context_kinds = match spec.mode {
        Some(ref mode) => mode.context_kinds(),
        None => crate::models::default_context_kinds(),
    };

    let mut agent = Agent::new(
        AgentConfig {
            name: spec.name,
            model: spec.model,
            provider: llm_provider.name().into(),
            system_prompt: spec.system_prompt,
            max_iterations: spec.max_iterations,
            temperature: spec.temperature,
            toolsets: vec!["builtin".into()],
            hidden: true,
            allow_all: spec.allow_all,
            enabled_context_kinds,
            essential_tools: crate::models::default_essential_tools(),
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
        },
        llm_provider,
        tools,
    )
    .await;
    if let Some(mgr) = cap_mgr {
        agent = agent.with_capability_manager(mgr);
    }
    agent
}

async fn create_agent(
    spec: AgentSpec,
    tools: Arc<ToolRegistry>,
    cap_mgr: Option<Arc<crate::capability::CapabilityManager>>,
) -> Agent {
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

    create_agent_with_provider(spec, tools, cap_mgr, llm_provider).await
}

fn kind_from_str(s: &str) -> ProviderKind {
    match s.to_lowercase().as_str() {
        "openai" => ProviderKind::OpenAI,
        "anthropic" => ProviderKind::Anthropic,
        _ => ProviderKind::OpenAI,
    }
}

/// Escape curly braces in user-provided values to prevent template injection
/// into subsequent placeholder substitutions.
fn escape_braces(s: &str) -> String {
    s.replace('{', "{{").replace('}', "}}")
}

/// Vendor prefixes for models available on NVIDIA NIM (integrate.api.nvidia.com).
/// Models from these vendors are served through NIM when NVIDIA_API_KEY is set.
/// This list covers the full catalog from docs.api.nvidia.com.
const NIM_VENDOR_PREFIXES: &[&str] = &[
    "abacusai/",
    "arc/",
    "bytedance/",
    "baai/",
    "black-forest-labs/",
    "colabfold/",
    "deepmind/",
    "deepseek-ai/",
    "google/",
    "hive/",
    "ipd/",
    "meta/",
    "microsoft/",
    "minimaxai/",
    "mistralai/",
    "mit/",
    "moonshotai/",
    "nvidia/",
    "openfold/",
    "qwen/",
    "sarvamai/",
    "snowflake/",
    "stabilityai/",
    "stepfun-ai/",
    "stockmark/",
    "upstage/",
    "z-ai/",
    "zhipuai/",
];

/// Vendor prefixes served through Groq's API (these should NOT route to NVIDIA
/// even when NVIDIA_API_KEY is set, since Groq also hosts them).
const GROQ_VENDOR_PREFIXES: &[&str] = &["openai/gpt-oss-", "meta-llama/", "canopylabs/"];

/// Check if a model name matches a known NIM-hosted vendor prefix.
fn is_nim_hosted_model(model: &str) -> bool {
    let m = model.to_lowercase();
    NIM_VENDOR_PREFIXES
        .iter()
        .any(|prefix| m.starts_with(prefix))
}

/// Check if model has a vendor prefix (contains '/') and is not a known
/// Groq-hosted model. Used as a catch-all to route unknown vendor-prefixed
/// models to NVIDIA NIM when NVIDIA_API_KEY is available.
fn is_nim_catchall_candidate(model: &str) -> bool {
    let m = model.to_lowercase();
    if !m.contains('/') {
        return false;
    }
    // Exclude known Groq-hosted prefixes
    !GROQ_VENDOR_PREFIXES
        .iter()
        .any(|prefix| m.starts_with(prefix))
}

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

    // Nvidia NIM — catches native nvidia models + explicitly known partner prefixes
    if m.starts_with("nvlm") || m.contains("nvidia") || is_nim_hosted_model(&m) {
        let api_key = std::env::var("NVIDIA_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default();
        return ProviderRoute {
            kind: ProviderKind::OpenAI,
            base_url: "https://integrate.api.nvidia.com/v1".into(),
            api_key,
        };
    }

    // ── 3. LLM_BASE_URL override (Ollama, vLLM, LM Studio, etc.) ──
    if let Ok(base_url) = std::env::var("LLM_BASE_URL") {
        let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
        return ProviderRoute {
            kind: ProviderKind::OpenAI,
            base_url,
            api_key,
        };
    }

    // ── 4. Ollama: models with Ollama-style naming (colon-separated tags like `gpt-oss:120b`)
    //     route to Ollama when OLLAMA_API_KEY is set.
    if m.contains(':') {
        let api_key = std::env::var("OLLAMA_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default();
        if !api_key.is_empty() {
            return ProviderRoute {
                kind: ProviderKind::OpenAI,
                base_url: "https://ollama.com/v1".into(),
                api_key,
            };
        }
        // If no Ollama key but base URL is set, use that
        if let Ok(base_url) = std::env::var("LLM_BASE_URL") {
            let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
            return ProviderRoute {
                kind: ProviderKind::OpenAI,
                base_url,
                api_key,
            };
        }
    }

    // ── 5. NIM catch-all: any unknown vendor-prefixed model routes to NVIDIA
    //     only when LLM_BASE_URL is not set (Ollama/self-hosted takes priority).
    if is_nim_catchall_candidate(&m) {
        let api_key = std::env::var("NVIDIA_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default();
        return ProviderRoute {
            kind: ProviderKind::OpenAI,
            base_url: "https://integrate.api.nvidia.com/v1".into(),
            api_key,
        };
    }

    // ── 6. Default: Groq (or `LLM_DEFAULT_PROVIDER`) ──────────────
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
                "ollama" => ("https://ollama.com/v1", "OLLAMA_API_KEY"),
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
    pub async fn run_parallel(
        &self,
        tasks: Vec<(AgentSpec, String)>,
    ) -> anyhow::Result<Vec<StepResult>> {
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(5));
        let mut handles = Vec::new();

        for (spec, task) in tasks {
            let tools = self.tools.clone();
            let cap_mgr = self.cap_mgr.clone();
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
                            error: None,
                        }
                    }
                };
                let step_started = Instant::now();
                let agent = create_agent(spec, tools, Some(cap_mgr.clone())).await;
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
                            error: None,
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
                            error: Some(e.to_string()),
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
                    error: Some("task join failed".into()),
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
            let task = task_template.replace("{prev}", &escape_braces(&prev_output));
            let agent_name = spec.name.clone();

            let agent = create_agent(spec, self.tools.clone(), Some(self.cap_mgr.clone())).await;
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
                        error: None,
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
                        error: Some(e.to_string()),
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

        let supervisor = create_agent(
            supervisor_spec,
            self.tools.clone(),
            Some(self.cap_mgr.clone()),
        )
        .await;
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
                error: None,
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
    cap_mgr: Option<&'a Arc<crate::capability::CapabilityManager>>,
    #[doc(hidden)]
    #[allow(clippy::type_complexity)]
    provider_factory:
        Option<std::sync::Arc<dyn Fn(AgentSpec) -> Box<dyn LLMProvider> + Send + Sync>>,
}

impl<'a> DagScheduler<'a> {
    pub fn new(tools: &'a Arc<ToolRegistry>) -> Self {
        Self {
            tools,
            cap_mgr: None,
            provider_factory: None,
        }
    }

    pub fn with_capability_manager(
        mut self,
        cap_mgr: &'a Arc<crate::capability::CapabilityManager>,
    ) -> Self {
        self.cap_mgr = Some(cap_mgr);
        self
    }

    #[doc(hidden)]
    pub fn with_provider_factory(
        mut self,
        factory: std::sync::Arc<dyn Fn(AgentSpec) -> Box<dyn LLMProvider> + Send + Sync>,
    ) -> Self {
        self.provider_factory = Some(factory);
        self
    }

    /// Execute a DAG workflow with the given initial input.
    /// Returns a map of node ID -> full execution telemetry for all executed nodes.
    pub async fn execute(
        &self,
        workflow: &DagWorkflow,
        initial_input: &str,
    ) -> anyhow::Result<std::collections::HashMap<String, StepResult>> {
        let levels = workflow.execution_levels()?;
        let node_map = workflow.node_map();
        let radj = workflow.reverse_adjacency();

        // Accumulates outputs and telemetry: node_id -> StepResult
        let mut results: std::collections::HashMap<String, StepResult> =
            std::collections::HashMap::new();
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
                    task = task.replace("{input}", &escape_braces(initial_input));
                }

                for pred_id in &predecesors {
                    if let Some(pred_output) = outputs.get(pred_id) {
                        task =
                            task.replace(&format!("{{{}}}", pred_id), &escape_braces(pred_output));
                    }
                }

                for (oid, ooutput) in &outputs {
                    task = task.replace(&format!("{{{}}}", oid), &escape_braces(ooutput));
                }

                payloads.push((node_id.clone(), node.agent.clone(), task));
            }

            // Execute all payloads in this level concurrently
            let mut handles = Vec::new();
            let factory = self.provider_factory.clone();
            for (node_id, agent_spec, task) in payloads {
                let tools = self.tools.clone();
                let cap_mgr = self.cap_mgr.cloned();
                let factory = factory.clone();
                handles.push(tokio::spawn(async move {
                    let step_started = std::time::Instant::now();
                    let agent = if let Some(ref f) = factory {
                        create_agent_with_provider(
                            agent_spec.clone(),
                            tools,
                            cap_mgr,
                            f(agent_spec.clone()),
                        )
                        .await
                    } else {
                        create_agent(agent_spec.clone(), tools, cap_mgr).await
                    };
                    let run_result = agent.run(&task).await;
                    let state = agent.state().lock().await;
                    let step = match run_result {
                        Ok(output) => StepResult {
                            agent_name: agent_spec.name,
                            output: output.clone(),
                            duration_ms: step_started.elapsed().as_millis(),
                            success: true,
                            prompt_tokens: state.total_prompt_tokens,
                            completion_tokens: state.total_completion_tokens,
                            error: None,
                        },
                        Err(e) => StepResult {
                            agent_name: agent_spec.name,
                            output: format!("error: {}", e),
                            duration_ms: step_started.elapsed().as_millis(),
                            success: false,
                            prompt_tokens: state.total_prompt_tokens,
                            completion_tokens: state.total_completion_tokens,
                            error: Some(e.to_string()),
                        },
                    };
                    (node_id, step)
                }));
            }

            // Collect results for this level
            for handle in handles {
                let (node_id, step) = handle
                    .await
                    .map_err(|e| anyhow::anyhow!("task join: {}", e))?;
                outputs.insert(node_id.clone(), step.output.clone());
                results.insert(node_id, step);
            }
        }

        Ok(results)
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
        let scheduler = DagScheduler::new(&self.tools).with_capability_manager(&self.cap_mgr);
        let node_results = scheduler.execute(&workflow, initial_input).await?;

        // Build the workflow result from real telemetry
        let mut steps = Vec::new();
        for node in &workflow.nodes {
            if let Some(step) = node_results.get(&node.id) {
                steps.push(step.clone());
            }
        }

        // Find a "final_output" marker or use the last executed node's output
        let final_output = node_results
            .get("final_output")
            .map(|s| s.output.clone())
            .or_else(|| {
                workflow
                    .nodes
                    .last()
                    .and_then(|n| node_results.get(&n.id))
                    .map(|s| s.output.clone())
            })
            .unwrap_or_default();

        Ok(WorkflowResult {
            steps,
            final_output,
            total_duration_ms: started.elapsed().as_millis(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LLMProvider;
    use crate::models::{LLMRequest, LLMResponse};
    use async_trait::async_trait;

    /// Mock provider that always fails — used to verify DAG telemetry capture.
    struct FailingProvider;

    #[async_trait]
    impl LLMProvider for FailingProvider {
        fn name(&self) -> &str {
            "mock-failing"
        }
        fn supported_models(&self) -> Vec<String> {
            vec![]
        }
        async fn complete(&self, _request: &LLMRequest) -> anyhow::Result<LLMResponse> {
            anyhow::bail!("mock hardcoded failure")
        }
        async fn complete_stream(
            &self,
            _request: &LLMRequest,
            _on_token: crate::llm::provider::TokenCallback,
        ) -> anyhow::Result<LLMResponse> {
            anyhow::bail!("mock hardcoded failure")
        }
    }

    #[tokio::test]
    async fn test_dag_scheduler_captures_failure_telemetry() {
        let registry = ToolRegistry::new();
        let dag_json = r#"{
            "nodes": [
                {
                    "id": "fail_node",
                    "task": "This will fail: {input}",
                    "agent": {
                        "name": "failing-agent",
                        "max_iterations": 2
                    }
                }
            ],
            "edges": []
        }"#;
        let workflow = DagWorkflow::from_json(dag_json).expect("valid DAG JSON");
        let scheduler = DagScheduler::new(&registry).with_provider_factory(std::sync::Arc::new(
            |_spec: AgentSpec| -> Box<dyn LLMProvider> { Box::new(FailingProvider) },
        ));
        let results = scheduler
            .execute(&workflow, "test input")
            .await
            .expect("execution completes");

        let step = results.get("fail_node").expect("result for fail_node");
        assert!(!step.success, "step should report failure");
        assert!(
            step.error
                .as_ref()
                .expect("error present")
                .contains("mock hardcoded failure"),
            "error should contain the mock failure message"
        );
        assert!(
            step.duration_ms > 0,
            "duration_ms should be recorded (got {})",
            step.duration_ms
        );
        assert!(
            step.output.contains("error:"),
            "output should contain the error text"
        );
    }

    #[tokio::test]
    async fn test_dag_scheduler_captures_success_telemetry() {
        let registry = ToolRegistry::new();
        let dag_json = r#"{
            "nodes": [
                {
                    "id": "ok_node",
                    "task": "Echo: {input}",
                    "agent": {
                        "name": "ok-agent",
                        "max_iterations": 2
                    }
                }
            ],
            "edges": []
        }"#;
        let workflow = DagWorkflow::from_json(dag_json).expect("valid DAG JSON");
        let scheduler = DagScheduler::new(&registry).with_provider_factory(std::sync::Arc::new(
            |_spec: AgentSpec| -> Box<dyn LLMProvider> {
                Box::new(crate::test_utils::MockLLMProvider::new(vec![
                    crate::test_utils::MockLLMProvider::tool_result("success output"),
                ]))
            },
        ));
        let results = scheduler
            .execute(&workflow, "hello")
            .await
            .expect("execution completes");

        let step = results.get("ok_node").expect("result for ok_node");
        assert!(step.success, "step should report success");
        assert_eq!(step.error, None, "error should be None on success");
        assert!(
            step.duration_ms > 0,
            "duration_ms should be recorded on success (got {})",
            step.duration_ms
        );
        assert_eq!(
            step.output, "success output",
            "output should match mock response"
        );
    }
}
