use crate::agent::loop_rs::Agent;
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::openai::OpenAIProvider;
use crate::llm::LLMProvider;
use crate::models::*;
use crate::tools::ToolRegistry;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub max_iterations: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub agent_name: String,
    pub output: String,
    pub duration_ms: u128,
    pub success: bool,
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

fn create_agent(spec: &AgentSpec, tools: Arc<ToolRegistry>) -> Agent {
    let (provider_kind, base_url, api_key) = resolve_provider(&spec.model);

    let llm_provider: Box<dyn LLMProvider> = if provider_kind == "anthropic" {
        Box::new(AnthropicProvider::new(api_key, Some(base_url.clone()), spec.name.clone()))
    } else {
        Box::new(OpenAIProvider::new(api_key, base_url.clone(), spec.name.clone()))
    };

    let config = AgentConfig {
        name: spec.name.clone(),
        model: spec.model.clone(),
        provider: provider_kind,
        system_prompt: spec.system_prompt.clone(),
        max_iterations: spec.max_iterations,
        temperature: spec.temperature,
        toolsets: vec!["builtin".into()],
        hidden: true,
    };
    Agent::new(config, llm_provider, tools)
}

/// Resolve model name to (provider_kind, base_url, api_key).
///
/// Routing order:
///   1. User-defined routes from `LLM_MODEL_ROUTES` env var (JSON array)
///   2. Built-in model family detection (Anthropic, OpenAI, Nvidia)
///   3. Default: Groq (set `LLM_DEFAULT_PROVIDER` env var to override)
///
/// NOTE: Ollama is NOT routed here. Ollama is used exclusively for
/// embeddings via `src/embedding.rs` and is separate from text generation.
pub fn resolve_provider(model: &str) -> (String, String, String) {
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
                        let provider = route["provider"].as_str().unwrap_or("openai").to_string();
                        let base_url = route["base_url"].as_str().unwrap_or("").to_string();
                        let api_key_env = route["api_key_env"].as_str().unwrap_or("LLM_API_KEY");
                        let api_key = std::env::var(api_key_env).unwrap_or_default();
                        return (provider, base_url, api_key);
                    }
                }
            }
        }
    }

    // ── 2. Built-in smart routing ─────────────────────────────────
    // Claude → Anthropic
    if m.contains("claude") {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default();
        return ("anthropic".into(), "https://api.anthropic.com".into(), key);
    }

    // GPT / O-series → OpenAI
    if m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") || m.contains("openai") {
        let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        return ("openai".into(), "https://api.openai.com/v1".into(), key);
    }

    // Nvidia NIM
    if m.starts_with("nvlm") || m.contains("nvidia") {
        let key = std::env::var("NVIDIA_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default();
        return ("openai".into(), "https://integrate.api.nvidia.com/v1".into(), key);
    }

    // ── 3. Default: Groq (or `LLM_DEFAULT_PROVIDER`) ──────────────
    let default_provider = std::env::var("LLM_DEFAULT_PROVIDER")
        .unwrap_or_else(|_| "groq".into())
        .to_lowercase();

    match default_provider.as_str() {
        "openai" => {
            let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
            ("openai".into(), "https://api.openai.com/v1".into(), key)
        }
        "anthropic" => {
            let key = std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            ("anthropic".into(), "https://api.anthropic.com".into(), key)
        }
        "nvidia" => {
            let key = std::env::var("NVIDIA_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            ("openai".into(), "https://integrate.api.nvidia.com/v1".into(), key)
        }
        // Default to Groq
        _ => {
            let key = std::env::var("GROQ_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default();
            ("openai".into(), "https://api.groq.com/openai/v1".into(), key)
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
        let mut handles = Vec::new();

        for (spec, task) in tasks {
            let tools = self.tools.clone();
            handles.push(tokio::spawn(async move {
                let step_started = Instant::now();
                let agent = create_agent(&spec, tools);
                match agent.run(&task).await {
                    Ok(output) => StepResult {
                        agent_name: spec.name,
                        output,
                        duration_ms: step_started.elapsed().as_millis(),
                        success: true,
                    },
                    Err(e) => StepResult {
                        agent_name: spec.name,
                        output: format!("error: {}", e),
                        duration_ms: step_started.elapsed().as_millis(),
                        success: false,
                    },
                }
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

            let agent = create_agent(&spec, self.tools.clone());
            match agent.run(&task).await {
                Ok(output) => {
                    prev_output = output.clone();
                    step_results.push(StepResult {
                        agent_name: spec.name,
                        output,
                        duration_ms: step_started.elapsed().as_millis(),
                        success: true,
                    });
                }
                Err(e) => {
                    step_results.push(StepResult {
                        agent_name: spec.name,
                        output: format!("error: {}", e),
                        duration_ms: step_started.elapsed().as_millis(),
                        success: false,
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
                let task_pairs: Vec<(AgentSpec, String)> =
                    specs.into_iter().zip(tasks.into_iter()).collect();
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
                let stages: Vec<(AgentSpec, String)> =
                    specs.into_iter().zip(tasks.into_iter()).collect();
                self.run_pipeline(stages).await
            }
            "supervisor" => {
                let task = tasks.first().cloned().unwrap_or_default();
                self.run_supervisor(&task, specs).await
            }
            _ => anyhow::bail!("unknown workflow pattern: {}. use 'parallel', 'pipeline', or 'supervisor'", pattern),
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

        let supervisor_spec = AgentSpec {
            name: "supervisor".into(),
            model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "llama-3.1-8b-instant".into()),
            system_prompt: Some(format!(
                "You are a supervisor agent coordinating multiple workers.\n\n\
                Available workers:\n{}\n\n\
                Break down the user's task into sub-tasks and delegate each one to the appropriate worker \
                using the `delegate` tool. Pass the sub-task description in the `task` parameter and relevant \
                context in the `context` parameter. After all workers complete, synthesize their outputs into \
                a final answer.",
                worker_block
            )),
            max_iterations: 50,
            temperature: 0.3,
        };

        let supervisor = create_agent(&supervisor_spec, self.tools.clone());
        let output = supervisor.run(task).await?;

        Ok(WorkflowResult {
            steps: vec![StepResult {
                agent_name: "supervisor".into(),
                output: output.clone(),
                duration_ms: started.elapsed().as_millis(),
                success: true,
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
                        std::env::var("LLM_MODEL")
                            .unwrap_or_else(|_| "llama-3.1-8b-instant".into())
                    }),
                system_prompt: v["system_prompt"].as_str().map(|s| s.to_string()),
                max_iterations: v["max_iterations"].as_u64().unwrap_or(10) as u32,
                temperature: v["temperature"].as_f64().unwrap_or(0.3) as f32,
            })
        })
        .collect()
}