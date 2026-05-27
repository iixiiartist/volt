use crate::context::ContextKind;

/// Task-aware context profiles backed by the context kind ablation data.
#[derive(Debug, Clone)]
pub enum AgentMode {
    /// Tool + Artifact only. Recovers +6pp over all-12-kinds.
    /// For function calling, code tasks, structured output.
    Precision,
    /// Tool + Skill + Memory + Conversation + Artifact (5-kind optimal).
    /// Default mode. Best general-purpose accuracy from ablation.
    Balanced,
    /// All 12 context kinds. For long-running, multi-step, autonomous agents.
    Autonomous,
}

impl std::str::FromStr for AgentMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "precision" => AgentMode::Precision,
            "balanced" => AgentMode::Balanced,
            "autonomous" => AgentMode::Autonomous,
            _ => AgentMode::Balanced,
        })
    }
}

impl AgentMode {
    pub fn context_kinds(&self) -> Vec<ContextKind> {
        match self {
            AgentMode::Precision => vec![ContextKind::Tool, ContextKind::Artifact],
            AgentMode::Balanced => vec![
                ContextKind::Tool,
                ContextKind::Skill,
                ContextKind::Memory,
                ContextKind::Conversation,
                ContextKind::Artifact,
            ],
            AgentMode::Autonomous => vec![
                ContextKind::Tool,
                ContextKind::Skill,
                ContextKind::Conversation,
                ContextKind::Memory,
                ContextKind::AgentRun,
                ContextKind::Artifact,
                ContextKind::SystemPrompt,
                ContextKind::FewShot,
                ContextKind::Policy,
                ContextKind::Permission,
                ContextKind::Security,
                ContextKind::MCPConfig,
            ],
        }
    }
}

pub mod agent_run;
pub mod agent_tui;
pub mod eval;
pub mod mcp;
pub mod provision;
pub mod skills;
pub mod tools;
pub mod workflow;
