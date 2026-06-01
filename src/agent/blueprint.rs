use serde::{Deserialize, Serialize};

/// Full AgentBlueprint TOML schema — a model-specific execution profile
/// that overrides AgentConfig fields and injects scaffolding constraints
/// and quirk-compensation interceptors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBlueprint {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "model_card")]
    pub model_card: ModelCard,
    pub scaffolding: ScaffoldingConfig,
    pub tools: ToolSelection,
    pub prompts: PromptOverrides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCard {
    pub model_name: String,
    pub provider: String,
    #[serde(rename = "format_dialect")]
    pub format_dialect: FormatDialect,
    #[serde(default)]
    pub quirks: Vec<ModelQuirk>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum FormatDialect {
    /// `<function>` / `</function>` XML tags around JSON tool calls (default)
    StandardXml,
    /// `<|system|>` / `<|user|>` — Gemma-4 native (current default)
    #[default]
    GemmaNative,
    /// `<|begin_of_text|><|start_header_id|>system<|end_header_id|>` — Llama chat template
    LlamaChat,
    /// Tools as JSON in message body — OpenAI-style
    OpenAiJson,
    /// `<function_calls>` / `<invoke>` XML — Claude-style
    ClaudeXml,
    /// ChatML-style tool format with `<|im_start|>` / `<|im_end|>` delimiters (Gemma 3 / GPT-4)
    #[serde(rename = "ChatMlTools")]
    ChatMlTools,
}

impl FormatDialect {
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatDialect::StandardXml => "StandardXml",
            FormatDialect::GemmaNative => "GemmaNative",
            FormatDialect::LlamaChat => "LlamaChat",
            FormatDialect::OpenAiJson => "OpenAiJson",
            FormatDialect::ClaudeXml => "ClaudeXml",
            FormatDialect::ChatMlTools => "ChatMlTools",
        }
    }
}

/// Model quirks that require interceptors in the agent loop or tool parser.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelQuirk {
    /// Model emits `"true"` / `"false"` strings instead of JSON booleans
    #[serde(rename = "StringifiedBooleans")]
    StringifiedBooleans,
    /// Model emits conversational text outside tool-call XML/JSON tags
    #[serde(rename = "ChainOfThoughtLeak")]
    ChainOfThoughtLeak,
    /// Model struggles to emit multiple tool calls per turn
    #[serde(rename = "MultiToolParalysis")]
    MultiToolParalysis,
    /// Model wraps integer values in quotes (e.g. `"42"` instead of `42`)
    #[serde(rename = "StringifiedIntegers")]
    StringifiedIntegers,
    /// Limit tool retrieval to 10 max (models with small context windows)
    #[serde(rename = "SchemaLimitTen")]
    SchemaLimitTen,
    /// Model tends to skip the final_answer call; inject a forced-final system message
    #[serde(rename = "MissingFinalAnswer")]
    MissingFinalAnswer,
    /// Model does NOT support tool calling on this provider
    #[serde(rename = "NoToolCalling")]
    NoToolCalling,
    /// Model uses reasoning_effort parameter (DeepSeek-style)
    #[serde(rename = "ReasoningEffort")]
    ReasoningEffort,
    /// API returns 202 Accepted; caller must poll for completion
    #[serde(rename = "AsyncPolling")]
    AsyncPolling,
    /// Model accepts multimodal input (images, video, audio)
    #[serde(rename = "MultimodalInput")]
    MultimodalInput,
    /// Max output tokens capped below model's native limit (e.g. 4096 on NIM)
    #[serde(rename = "MaxOutput4096")]
    MaxOutput4096,
    /// Max context window capped at 4096
    #[serde(rename = "MaxContext4096")]
    MaxContext4096,
    /// Model is deprecated but still accessible on this provider
    #[serde(rename = "Deprecated")]
    Deprecated,
    /// Model supports configurable thinking/reasoning mode
    #[serde(rename = "ThinkingEnabled")]
    ThinkingEnabled,
    /// Model runs in non-thinking mode only (fast path)
    #[serde(rename = "NonThinking")]
    NonThinking,
    /// Model is a Compound System orchestrator, not a raw LLM
    #[serde(rename = "CompoundSystem")]
    CompoundSystem,
    /// Compound system returns per-model usage breakdown
    #[serde(rename = "UsageBreakdown")]
    UsageBreakdown,
    /// Supports up to 10 tool calls per turn
    #[serde(rename = "UpTo10Tools")]
    UpTo10Tools,
    /// Has built-in web search capability
    #[serde(rename = "BuiltinSearch")]
    BuiltinSearch,
    /// Has built-in code interpreter
    #[serde(rename = "BuiltinCodeInterpreter")]
    BuiltinCodeInterpreter,
    /// Tends to wrap code blocks in markdown triple backticks
    #[serde(rename = "MarkdownCodeBlocks")]
    MarkdownCodeBlocks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaffoldingConfig {
    pub max_tools_per_turn: Option<usize>,
    pub strict_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSelection {
    pub core_tools: Vec<String>,
    #[serde(default)]
    pub builtin_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOverrides {
    pub system_prompt_override: Option<String>,
}

/// Load a blueprint from a TOML file path.
pub fn load_blueprint(path: &std::path::Path) -> Option<AgentBlueprint> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Search standard directories for a blueprint by name (id).
pub fn find_blueprint(name: &str) -> Option<AgentBlueprint> {
    let dirs = [
        std::env::current_dir().ok().map(|d| d.join("blueprints")),
        std::env::var("HOME")
            .ok()
            .map(|h| std::path::PathBuf::from(h).join(".volt").join("blueprints")),
    ];

    for dir in dirs.iter().flatten() {
        let path = dir.join(format!("{}.toml", name));
        if path.exists() {
            if let Some(bp) = load_blueprint(&path) {
                return Some(bp);
            }
        }
    }
    None
}
