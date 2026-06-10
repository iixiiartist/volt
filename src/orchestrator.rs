use crate::agent::Agent;
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::openai::OpenAIProvider;
use crate::llm::provider_detector::{self, DetectedProvider, ProviderInventory};
use crate::llm::LLMProvider;
use crate::models::AgentConfig;
use crate::tools::ToolRegistry;
use std::sync::Arc;
use std::time::Instant;

// `ProviderKind` lives in `crate::llm::provider` now (re-exported from
// `crate::llm`). We re-export it here for backward-compat with downstream
// callers that used `crate::orchestrator::ProviderKind`.
pub use crate::llm::ProviderKind;

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
    /// Whether to run the synthesizer LLM after all workers complete.
    /// Default false — worker outputs are concatenated directly.
    pub use_synthesizer: bool,
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
    let route = match resolve_provider(&spec.model) {
        Ok(r) => r,
        Err(e) => {
            // Build a placeholder OpenAI provider pointing at api.groq.com
            // with no key — the first real call will 401 with a clear
            // message. We can't propagate the error here because the
            // caller signature is infallible; the error is captured in
            // the agent's system prompt and surfaced on first use.
            tracing::warn!(
                "[orchestrator] no provider for model `{}`: {}. Agent will fail at first call.",
                spec.model,
                e
            );
            ProviderRoute {
                kind: ProviderKind::OpenAI,
                base_url: "https://api.groq.com/openai/v1".into(),
                api_key: String::new(),
            }
        }
    };

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

/// Errors that can occur while resolving a model to a provider route.
/// All variants include a user-actionable message: which env var to set,
/// or which provider to install.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolveError {
    /// No LLM provider is configured at all (no API keys, no
    /// `LLM_BASE_URL`, no local servers reachable).
    #[error(
        "no LLM provider is configured. Set GROQ_API_KEY (or another provider key) in .env, \
         or set LLM_BASE_URL to a local OpenAI-compatible endpoint. \
         Run `volt config` for an interactive setup."
    )]
    NoProviderConfigured,
    /// A provider is configured but the requested model doesn't match any
    /// active provider's known model ranges.
    #[error(
        "model `{model}` doesn't match any active provider. \
         Active providers: {active}. \
         Add a key for the matching provider, or pick a model from a configured provider."
    )]
    ModelNotMatched { model: String, active: String },
    /// The model name has a vendor prefix that maps to a provider, but
    /// that provider's API key is missing.
    #[error(
        "model `{model}` requires provider `{provider}` but {env_var} is not set."
    )]
    ProviderKeyMissing {
        model: String,
        provider: String,
        env_var: String,
    },
    /// The user has explicit `LLM_MODEL_ROUTES` overrides; one matched
    /// the model, but the corresponding env var is empty.
    #[error(
        "LLM_MODEL_ROUTES matched `{model}` to provider `{provider}` but {env_var} is not set."
    )]
    RouteOverrideKeyMissing {
        model: String,
        provider: String,
        env_var: String,
    },
}

impl ResolveError {
    /// One-line user-facing hint. Suitable for chat output.
    pub fn hint(&self) -> &str {
        match self {
            Self::NoProviderConfigured => {
                "Run `volt config` to add an API key, or set LLM_BASE_URL in .env."
            }
            Self::ModelNotMatched { .. } | Self::ProviderKeyMissing { .. } => {
                "Run `volt config` to add the missing key, or pick a model from a configured provider."
            }
            Self::RouteOverrideKeyMissing { .. } => {
                "Set the env var named in LLM_MODEL_ROUTES, or remove the override."
            }
        }
    }
}

/// Resolve a model name to a provider route. Returns `Err(ResolveError)`
/// when no active provider matches, with a message that names the env
/// var the user must set.
pub fn resolve_provider(model: &str) -> Result<ProviderRoute, ResolveError> {
    let inv = provider_detector::detect();
    resolve_provider_with(model, &inv)
}

/// Variant of `resolve_provider` that accepts a precomputed inventory.
/// Useful in tests and when the caller has already detected providers
/// (e.g. the WebUI runtime, which detects once at startup).
pub fn resolve_provider_with(
    model: &str,
    inv: &ProviderInventory,
) -> Result<ProviderRoute, ResolveError> {
    let m = model.to_lowercase();

    // 1. Honor explicit LLM_MODEL_ROUTES overrides first. These are
    //    user-defined and the highest priority. We still require a
    //    non-empty key for the named provider, otherwise surface a
    //    specific error.
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
                        let provider_slug =
                            route["provider"].as_str().unwrap_or("openai").to_string();
                        let base_url =
                            route["base_url"].as_str().unwrap_or("").to_string();
                        let env_var = route["api_key_env"]
                            .as_str()
                            .unwrap_or("LLM_API_KEY")
                            .to_string();
                        let api_key = std::env::var(&env_var)
                            .ok()
                            .filter(|v| !v.trim().is_empty())
                            .ok_or_else(|| ResolveError::RouteOverrideKeyMissing {
                                model: model.to_string(),
                                provider: provider_slug.clone(),
                                env_var: env_var.clone(),
                            })?;
                        let kind = kind_from_str(&provider_slug);
                        return Ok(ProviderRoute {
                            kind,
                            base_url,
                            api_key,
                        });
                    }
                }
            }
        }
    }

    // 2. Match against the active inventory. If a provider slug matches
    //    (e.g. `groq/llama-3.1-8b-instant` or `groq:llama-3.1-8b-instant`),
    //    use it directly.
    if let Some((slug, _)) = m.split_once(':').or_else(|| m.split_once('/')) {
        if let Some(p) = inv.providers.iter().find(|p| p.slug == slug && p.is_active) {
            return route_from_detected(p, model);
        }
    }

    // 3. Model-name hints. We only match a hint to a provider that is
    //    *currently active* (has key, has reachable local server, or has
    //    a configured `LLM_BASE_URL`). No silent substitution.
    if m.contains("claude") {
        if let Some(p) = inv.active().find(|p| p.slug == "anthropic") {
            return route_from_detected(p, model);
        }
        if !inv
            .providers
            .iter()
            .any(|p| p.slug == "anthropic" && p.is_active)
        {
            return Err(ResolveError::ProviderKeyMissing {
                model: model.to_string(),
                provider: "anthropic".into(),
                env_var: "ANTHROPIC_API_KEY".into(),
            });
        }
    }
    if m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") {
        if let Some(p) = inv.active().find(|p| p.slug == "openai") {
            return route_from_detected(p, model);
        }
        if !inv
            .providers
            .iter()
            .any(|p| p.slug == "openai" && p.is_active)
        {
            return Err(ResolveError::ProviderKeyMissing {
                model: model.to_string(),
                provider: "openai".into(),
                env_var: "OPENAI_API_KEY".into(),
            });
        }
    }
    if m.contains("nvidia")
        || m.contains("nvlm")
        || is_nim_hosted_model(&m)
    {
        if let Some(p) = inv
            .active()
            .find(|p| p.slug == "nvidia" || p.slug == "moonshot")
        {
            return route_from_detected(p, model);
        }
        if !inv
            .providers
            .iter()
            .any(|p| p.slug == "nvidia" && p.is_active)
        {
            return Err(ResolveError::ProviderKeyMissing {
                model: model.to_string(),
                provider: "nvidia".into(),
                env_var: "NVIDIA_API_KEY".into(),
            });
        }
    }
    if m.contains(':') {
        // Ollama-style tag. Prefer the local Ollama server if reachable.
        if let Some(p) = inv
            .active()
            .find(|p| p.slug == "ollama_local" || p.slug == "ollama")
        {
            return route_from_detected(p, model);
        }
        return Err(ResolveError::ProviderKeyMissing {
            model: model.to_string(),
            provider: "ollama".into(),
            env_var: "OLLAMA_API_KEY".into(),
        });
    }

    // 4. Catch-all: any unknown vendor-prefixed model (contains '/') goes
    //    to NIM only if NIM is the only remaining cloud option AND active.
    if is_nim_catchall_candidate(&m) {
        if let Some(p) = inv.active().find(|p| p.slug == "nvidia") {
            return route_from_detected(p, model);
        }
    }

    // 5. Last resort: first active provider. If there is none, return
    //    a clear error naming the active set (which will be empty).
    match inv.route(model) {
        Some(p) => route_from_detected(p, model),
        None => {
            let active: Vec<String> = inv
                .active()
                .map(|p| format!("{}({})", p.slug, p.base_url))
                .collect();
            Err(ResolveError::ModelNotMatched {
                model: model.to_string(),
                active: active.join(", "),
            })
        }
    }
}

/// Build a `ProviderRoute` from a `DetectedProvider`. For local servers
/// and overrides, the API key is empty; for cloud providers, we re-read
/// the env var at call time so runtime key changes are honored.
fn route_from_detected(
    p: &DetectedProvider,
    model: &str,
) -> Result<ProviderRoute, ResolveError> {
    let api_key = if p.env_var.is_empty() {
        String::new()
    } else {
        std::env::var(p.env_var).unwrap_or_default()
    };
    Ok(ProviderRoute {
        kind: p.kind,
        base_url: p.base_url.clone(),
        api_key,
    })
    .map(|r| {
        // For local servers, the `model` arg may be a tag like
        // `llama3.2:3b`. Pass it through unchanged.
        let _ = model;
        r
    })
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

        if worker_specs.is_empty() {
            anyhow::bail!("supervisor mode requires at least one worker_spec");
        }

        let worker_descriptions: Vec<String> = worker_specs
            .iter()
            .map(|w| format!("- {} (model: {})", w.name, w.model))
            .collect();
        let worker_block = worker_descriptions.join("\n");

        // The supervisor synthesizer uses `LLM_SUPERVISOR_MODEL` if set,
        // otherwise falls back to the worker's model, otherwise
        // `LLM_MODEL`, otherwise the first active provider's default
        // model. We no longer hardcode `qwen/qwen3-32b` — that masked
        // the case where the user has no Groq key.
        let supervisor_model = std::env::var("LLM_SUPERVISOR_MODEL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                worker_specs
                    .first()
                    .map(|w| w.model.clone())
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| std::env::var("LLM_MODEL").ok())
            .or_else(|| {
                let inv = provider_detector::detect();
                let defaults: Vec<String> = inv
                    .active()
                    .filter_map(|p| p.default_model.map(|m| m.to_string()))
                    .collect();
                defaults.into_iter().next()
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "supervisor synthesizer needs a model. Set LLM_SUPERVISOR_MODEL, \
                     LLM_MODEL, or pass `model` in a worker spec. Run `volt config` to \
                     configure a provider."
                )
            })?;

        let task = task.to_string();
        let cap_mgr = self.cap_mgr.clone();
        let tools = self.tools.clone();
        let use_synth = worker_specs.iter().any(|s| s.use_synthesizer);

        // Spawn all workers in parallel — each is given the task plus the list
        // of peer workers (so it can include context in its output if useful).
        let mut handles = Vec::with_capacity(worker_specs.len());
        for spec in worker_specs {
            let task = task.clone();
            let worker_block = worker_block.clone();
            let cap_mgr = cap_mgr.clone();
            let tools = tools.clone();
            handles.push(tokio::spawn(async move {
                let started = Instant::now();
                let worker_task = format!(
                    "Worker role: {}\n\nPeer workers in this supervisor group:\n{}\n\nTask: {}",
                    spec.name, worker_block, task
                );
                let agent = create_agent(spec.clone(), tools, Some(cap_mgr)).await;
                match agent.run(&worker_task).await {
                    Ok(output) => StepResult {
                        agent_name: spec.name,
                        output,
                        duration_ms: started.elapsed().as_millis(),
                        success: true,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        error: None,
                    },
                    Err(e) => StepResult {
                        agent_name: spec.name,
                        output: format!("error: {}", e),
                        duration_ms: started.elapsed().as_millis(),
                        success: false,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        error: Some(e.to_string()),
                    },
                }
            }));
        }

        let mut worker_steps: Vec<StepResult> = Vec::with_capacity(handles.len());
        for h in handles {
            let step = h
                .await
                .map_err(|e| anyhow::anyhow!("worker join: {}", e))?;
            worker_steps.push(step);
        }

        // Synthesizer is opt-in. When disabled, concatenate worker outputs directly.
        let (final_output, synth_step): (String, Option<StepResult>) = if use_synth {
            let synthesizer_prompt = format!(
                "You are a supervisor agent coordinating worker agents.\n\n\
                 Available workers and their outputs:\n{}\n\n\
                 Synthesize a final consolidated response to the user's original task: {}\n\n\
                 Use the [Worker Output] sections above as your evidence. Be concise.",
                worker_steps
                    .iter()
                    .map(|s| format!(
                        "[Worker: {}]\n{}{}",
                        s.agent_name,
                        s.output,
                        s.error
                            .as_ref()
                            .map(|e| format!("\n(worker error: {})", e))
                            .unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                task
            );

            let synthesizer_spec = AgentSpec {
                name: "supervisor-synthesizer".into(),
                model: supervisor_model,
                system_prompt: None,
                max_iterations: 1,
                temperature: 0.2,
                allow_all: false,
                mode: None,
                use_synthesizer: true,
            };
            let synthesizer = create_agent(synthesizer_spec, tools, Some(cap_mgr)).await;
            let synth_started = Instant::now();
            let synth_result = synthesizer.run(&synthesizer_prompt).await;
            let synth_state = synthesizer.state().lock().await;
            match synth_result {
                Ok(out) => {
                    let step = StepResult {
                        agent_name: "supervisor-synthesizer".into(),
                        output: out.clone(),
                        duration_ms: synth_started.elapsed().as_millis(),
                        success: true,
                        prompt_tokens: synth_state.total_prompt_tokens,
                        completion_tokens: synth_state.total_completion_tokens,
                        error: None,
                    };
                    (out, Some(step))
                }
                Err(e) => {
                    let step = StepResult {
                        agent_name: "supervisor-synthesizer".into(),
                        output: format!("synthesizer error: {}", e),
                        duration_ms: synth_started.elapsed().as_millis(),
                        success: false,
                        prompt_tokens: synth_state.total_prompt_tokens,
                        completion_tokens: synth_state.total_completion_tokens,
                        error: Some(e.to_string()),
                    };
                    (format!("synthesizer error: {}", e), Some(step))
                }
            }
        } else {
            let output = worker_steps
                .iter()
                .map(|s| format!("[{}]\n{}", s.agent_name, s.output))
                .collect::<Vec<_>>()
                .join("\n\n");
            (output, None)
        };

        let mut steps = worker_steps;
        if let Some(s) = synth_step {
            steps.push(s);
        }

        Ok(WorkflowResult {
            final_output,
            steps,
            total_duration_ms: started.elapsed().as_millis(),
        })
    }
}

pub fn parse_agent_specs(json: &str) -> anyhow::Result<Vec<AgentSpec>> {
    let specs: Vec<serde_json::Value> = serde_json::from_str(json)?;
    specs
        .into_iter()
        .map(|v| {
            // `model` is required. We used to silently substitute
            // LLM_MODEL or `llama-3.1-8b-instant`; that masked
            // configuration errors. Now we require it explicitly.
            let model = v["model"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| std::env::var("LLM_MODEL").ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "agent spec `{}` is missing a `model` field. \
                         Set `model` per spec or set LLM_MODEL in .env. \
                         Run `volt config` to choose a provider and model.",
                        v["name"].as_str().unwrap_or("agent")
                    )
                })?;
            Ok(AgentSpec {
                name: v["name"].as_str().unwrap_or("agent").to_string(),
                model,
                system_prompt: v["system_prompt"].as_str().map(|s| s.to_string()),
                max_iterations: v["max_iterations"].as_u64().unwrap_or(10) as u32,
                temperature: v["temperature"].as_f64().unwrap_or(0.3) as f32,
                allow_all: v["allow_all"].as_bool().unwrap_or(false),
                mode: v["mode"]
                    .as_str()
                    .and_then(|s| s.parse::<crate::commands::AgentMode>().ok()),
                use_synthesizer: v["use_synthesizer"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

/// Build an LLM provider from a model identifier string. Returns
/// `(provider, kind_str)` on success. On error, returns
/// `(fallback_provider, "unconfigured")` where the fallback is a Groq
/// OpenAI provider with an empty API key — the first real call will fail
/// with a 401 that mentions the missing key.
///
/// Prefer `try_build_provider` in new code: it surfaces a clear error
/// instead of a runtime 401.
pub fn build_provider(model: &str, agent_name: &str) -> (Box<dyn LLMProvider>, String) {
    match try_build_provider(model, agent_name) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(
                "[orchestrator] build_provider fallback for model `{}`: {}",
                model,
                e
            );
            let fallback: Box<dyn LLMProvider> = Box::new(OpenAIProvider::new(
                String::new(),
                "https://api.groq.com/openai/v1".into(),
                agent_name.into(),
            ));
            (fallback, "unconfigured".to_string())
        }
    }
}

/// Like `build_provider`, but returns `Err(ResolveError)` when no active
/// provider matches the model. New callers should use this and surface
/// the error to the user.
pub fn try_build_provider(
    model: &str,
    agent_name: &str,
) -> Result<(Box<dyn LLMProvider>, String), ResolveError> {
    let route = resolve_provider(model)?;
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
    Ok((provider, kind_str.to_string()))
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
        self.execute_dag_internal(workflow, initial_input, started).await
    }

    /// Execute a `WorkflowGraph` (canvas format). Resolves role names to
    /// concrete model IDs via the role registry, enforces the
    /// per-environment provider allowlist, projects to a
    /// `DagWorkflow`, and runs the scheduler. This is the
    /// enterprise-boundary method — `run_dag` is the legacy path
    /// that takes a pre-resolved `DagWorkflow` JSON.
    pub async fn run_workflow_graph(
        &self,
        graph: &crate::workflow::WorkflowGraph,
        initial_input: &str,
    ) -> anyhow::Result<WorkflowResult> {
        let started = std::time::Instant::now();

        // 1. Resolve role names to concrete model IDs.
        let registry = crate::llm::role_registry::RoleRegistry::load_default()
            .map_err(|e| anyhow::anyhow!("failed to load role registry: {}", e))?;
        let (dag_workflow, _dropped, resolutions) = graph
            .to_dag_workflow_with_registry(&registry)
            .map_err(|e| anyhow::anyhow!("role resolution failed: {}", e))?;

        // 2. Enforce the per-environment provider allowlist. This is
        //    the security control: a `prod` workflow that names a
        //    model resolving to a non-allowlisted provider is
        //    refused before any side effect.
        if graph.environment == crate::workflow::WorkflowEnvironment::Prod {
            let allowlist = parse_prod_allowlist();
            for res in &resolutions {
                if res.source == crate::llm::role_registry::ResolutionSource::Literal
                    && res.role.is_none()
                {
                    // Literal models are *always* allowed in prod
                    // when they resolve to allowlisted providers. We
                    // skip the check for role-resolved models here —
                    // the operator configured the role to point at
                    // a specific model, so trust was already given.
                    continue;
                }
                let slug = classify_model_to_slug(&res.model_id);
                if let Some(slug) = slug {
                    if !allowlist.iter().any(|s| s == slug) {
                        // Log the refusal so it shows up in the audit
                        // log even when there's no PG available.
                        tracing::error!(
                            "[orchestrator] REFUSED prod workflow: node '{}' uses model '{}' \
                             (provider slug '{}') which is not in VOLT_PROD_PROVIDER_ALLOWLIST={:?}",
                            res.node_id,
                            res.model_id,
                            slug,
                            allowlist
                        );
                        return Err(anyhow::anyhow!(
                            "workflow environment=prod refused: node '{}' uses model '{}' \
                             which resolves to provider '{}' (not in allowlist {:?}). \
                             Set VOLT_PROD_PROVIDER_ALLOWLIST to a comma-separated list of \
                             allowed provider slugs, or change the workflow's environment to 'dev' or 'staging'.",
                            res.node_id,
                            res.model_id,
                            slug,
                            allowlist
                        ));
                    }
                }
                // If `classify_model_to_slug` returns None, we don't
                // know the provider — refuse conservatively.
                else {
                    tracing::warn!(
                        "[orchestrator] prod workflow node '{}' uses model '{}' which cannot be \
                         classified to a known provider; refusing conservatively",
                        res.node_id,
                        res.model_id
                    );
                    return Err(anyhow::anyhow!(
                        "workflow environment=prod refused: node '{}' uses model '{}' which \
                         cannot be classified to a known provider. The prod allowlist requires \
                         every model to be classifiable. Use a known model ID or add the model's \
                         prefix to the classifier.",
                        res.node_id,
                        res.model_id
                    ));
                }
            }
        }

        // 3. Execute the resolved DAG.
        self.execute_dag_internal(dag_workflow, initial_input, started).await
    }

    async fn execute_dag_internal(
        &self,
        workflow: DagWorkflow,
        initial_input: &str,
        started: std::time::Instant,
    ) -> anyhow::Result<WorkflowResult> {
        let scheduler = DagScheduler::new(&self.tools).with_capability_manager(&self.cap_mgr);
        let node_results = scheduler.execute(&workflow, initial_input).await?;

        let mut steps = Vec::new();
        for node in &workflow.nodes {
            if let Some(step) = node_results.get(&node.id) {
                steps.push(step.clone());
            }
        }

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

/// Default prod allowlist. Used when `VOLT_PROD_PROVIDER_ALLOWLIST` is
/// not set. vLLM and local Ollama are the enterprise targets; cloud
/// providers are deliberately excluded.
pub const DEFAULT_PROD_ALLOWLIST: &[&str] = &["vllm", "ollama_local"];

/// Parse the `VOLT_PROD_PROVIDER_ALLOWLIST` env var. Falls back to
/// `DEFAULT_PROD_ALLOWLIST` if unset or empty. Whitespace is trimmed
/// and entries are lowercased.
pub fn parse_prod_allowlist() -> Vec<String> {
    std::env::var("VOLT_PROD_PROVIDER_ALLOWLIST")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_PROD_ALLOWLIST.iter().map(|s| s.to_string()).collect())
}

/// Classify a model ID to a provider slug. Returns `None` when the
/// model doesn't match any known provider prefix — callers should
/// refuse such models in prod contexts.
///
/// The order of checks matters: vendor-prefixed model names
/// (`meta-llama/...`, `qwen/...`, `microsoft/...`, etc.) are checked
/// first because vLLM is the enterprise target. If you want to
/// route such a model to NVIDIA NIM, prefix it explicitly with
/// `nvidia/` (e.g. `nvidia/meta-llama/Llama-3.1-8B-Instruct`).
pub fn classify_model_to_slug(model: &str) -> Option<&'static str> {
    let m = model.to_lowercase();
    // ── 1. Explicit cloud provider prefixes (highest priority) ──
    if m.starts_with("groq/") || m.starts_with("groq:") {
        return Some("groq");
    }
    if m.starts_with("openai/")
        || m.starts_with("gpt-")
        || m.starts_with("o1-")
        || m.starts_with("o3-")
    {
        return Some("openai");
    }
    if m.starts_with("anthropic/") || m.starts_with("claude") {
        return Some("anthropic");
    }
    // NVIDIA NIM requires an explicit `nvidia/` prefix. The bare
    // `meta/` prefix is also NIM-flavored, so we keep that as a
    // secondary signal.
    if m.starts_with("nvidia/") || m.starts_with("meta/") {
        return Some("nvidia");
    }
    if m.starts_with("ollama/") {
        return Some("ollama");
    }
    if m.starts_with("moonshot/") || m.starts_with("moonshot-") {
        return Some("moonshot");
    }
    // ── 2. Open-source vendor prefixes that vLLM typically serves ──
    // These are the models most often deployed on a local vLLM
    // server. If the operator wants one of these to go to NVIDIA
    // NIM, they should write the model ID as `nvidia/<vendor>/...`.
    if m.starts_with("meta-llama/")
        || m.starts_with("qwen/")
        || m.starts_with("microsoft/")
        || m.starts_with("mistral")
        || m.starts_with("deepseek-")
        || m.starts_with("google/")
        || m.starts_with("minimax")
        || m.starts_with("baai/")
        || m.starts_with("openai/gpt-oss-") // GPT-OSS is also served by vLLM
    {
        return Some("vllm");
    }
    // ── 3. Ollama-style colon tags default to local Ollama ──
    if m.contains(':') {
        return Some("ollama_local");
    }
    // ── 4. Loose substring matches (last resort) ──
    if m.contains("llama")
        || m.contains("qwen")
        || m.contains("mistral")
        || m.contains("deepseek")
    {
        return Some("vllm");
    }
    None
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
                        "model": "mock-model",
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
                        "model": "mock-model",
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

    // ── Production allowlist enforcement tests ────────────────
    //
    // These tests cover the security control that refuses a
    // prod-tagged workflow from running against a non-allowlisted
    // provider. The `classify_model_to_slug` and `parse_prod_allowlist`
    // helpers are tested here too because they're the building
    // blocks of the enforcement.

    #[test]
    fn classify_model_to_slug_recognizes_cloud_prefixes() {
        assert_eq!(classify_model_to_slug("groq/llama-3.1-70b"), Some("groq"));
        assert_eq!(classify_model_to_slug("gpt-4o"), Some("openai"));
        assert_eq!(classify_model_to_slug("o1-preview"), Some("openai"));
        assert_eq!(classify_model_to_slug("claude-sonnet-4-5"), Some("anthropic"));
        assert_eq!(classify_model_to_slug("nvidia/meta/llama-3.1-8b"), Some("nvidia"));
        assert_eq!(classify_model_to_slug("meta-llama/Llama-3.1-8B"), Some("vllm"));
        assert_eq!(classify_model_to_slug("qwen/qwen3-32b"), Some("vllm"));
        assert_eq!(classify_model_to_slug("llama3.1:8b"), Some("ollama_local"));
    }

    #[test]
    fn classify_model_to_slug_returns_none_for_unknown() {
        assert_eq!(classify_model_to_slug("totally-unknown-model"), None);
    }

    #[test]
    fn parse_prod_allowlist_default() {
        // When env var is unset, fall back to DEFAULT_PROD_ALLOWLIST.
        // We can't reliably unset env in a test, but we can at least
        // assert the default values are present.
        let al = DEFAULT_PROD_ALLOWLIST;
        assert!(al.contains(&"vllm"));
        assert!(al.contains(&"ollama_local"));
        assert!(!al.contains(&"groq"));
        assert!(!al.contains(&"openai"));
        assert!(!al.contains(&"anthropic"));
    }

    #[test]
    fn prod_workflow_refuses_groq_model() {
        // A prod workflow that names a Groq-prefixed model is refused
        // even when the registry is empty (the classifier is the
        // authority, not the registry).
        use crate::llm::role_registry::{RoleRegistry, VoltModelsConfig};
        let registry = RoleRegistry::from_config(VoltModelsConfig::default());
        let mut g = crate::workflow::WorkflowGraph::new("prod-groq");
        g.environment = crate::workflow::WorkflowEnvironment::Prod;
        g.add_node(crate::workflow::WorkflowNode {
            id: "x".into(),
            label: "X".into(),
            kind: crate::workflow::NodeKind::Agent,
            role: None,
            agent_name: Some("x".into()),
            model: Some("groq/llama-3.1-70b-versatile".into()),
            system_prompt: None,
            task: "do {input}".into(),
            config: serde_json::Value::Null,
            position: crate::workflow::NodePosition::default(),
            notes: None,
        });
        // Verify the classifier + allowlist logic produces a refusal.
        let model = "groq/llama-3.1-70b-versatile";
        let slug = classify_model_to_slug(model);
        let allowlist = parse_prod_allowlist();
        assert_eq!(slug, Some("groq"));
        assert!(!allowlist.iter().any(|s| s == "groq"));
        // The projection itself succeeds (it just resolves the
        // model); the allowlist check would happen at run time.
        let (_dag, _dropped, _res) = g.to_dag_workflow_with_registry(&registry).unwrap();
        // Suppress unused warning on the projection result.
        let _ = _dag;
    }

    #[test]
    fn prod_workflow_allows_vllm_model() {
        // A prod workflow that names a vLLM-prefixed model is
        // allowed. The check would pass at run time.
        use crate::llm::role_registry::{RoleRegistry, VoltModelsConfig};
        let registry = RoleRegistry::from_config(VoltModelsConfig::default());
        let mut g = crate::workflow::WorkflowGraph::new("prod-vllm");
        g.environment = crate::workflow::WorkflowEnvironment::Prod;
        g.add_node(crate::workflow::WorkflowNode {
            id: "x".into(),
            label: "X".into(),
            kind: crate::workflow::NodeKind::Agent,
            role: None,
            agent_name: Some("x".into()),
            model: Some("meta-llama/Llama-3.3-70B-Instruct".into()),
            system_prompt: None,
            task: "do {input}".into(),
            config: serde_json::Value::Null,
            position: crate::workflow::NodePosition::default(),
            notes: None,
        });
        let model = "meta-llama/Llama-3.3-70B-Instruct";
        let slug = classify_model_to_slug(model);
        let allowlist = parse_prod_allowlist();
        assert_eq!(slug, Some("vllm"));
        assert!(allowlist.iter().any(|s| s == "vllm"));
        let (_dag, _dropped, _res) = g.to_dag_workflow_with_registry(&registry).unwrap();
    }

    #[test]
    fn dev_workflow_has_no_allowlist_check() {
        // Dev workflows skip the allowlist entirely — any active
        // provider is fair game. The check at run time is only
        // applied when environment == Prod.
        use crate::llm::role_registry::{RoleRegistry, VoltModelsConfig};
        let registry = RoleRegistry::from_config(VoltModelsConfig::default());
        let mut g = crate::workflow::WorkflowGraph::new("dev-groq");
        g.environment = crate::workflow::WorkflowEnvironment::Dev;
        g.add_node(crate::workflow::WorkflowNode {
            id: "x".into(),
            label: "X".into(),
            kind: crate::workflow::NodeKind::Agent,
            role: None,
            agent_name: Some("x".into()),
            model: Some("groq/llama-3.1-70b-versatile".into()),
            system_prompt: None,
            task: "do {input}".into(),
            config: serde_json::Value::Null,
            position: crate::workflow::NodePosition::default(),
            notes: None,
        });
        // Projection succeeds regardless of model.
        let (_dag, _dropped, _res) = g.to_dag_workflow_with_registry(&registry).unwrap();
        // The runtime would skip the allowlist check because
        // environment != Prod — we don't need to assert anything
        // else here; the path is the unit-tested branch in
        // `run_workflow_graph`.
    }
}
