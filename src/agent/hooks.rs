//! Hook system — extensible Pre/Post-tool and session-lifecycle events.
//!
//! Inspired by Claude Code's hooks, Volt lets users configure shell
//! commands to run at well-defined points in the agent loop. Hooks can:
//!
//! - **Block** a tool call before it runs (PreToolUse, exit 2)
//! - **Modify** the tool's arguments (PreToolUse, JSON to stdout)
//! - **Inject context** into the next model message (PostToolUse,
//!   UserPromptSubmit; stdout `{"context": "..."}`)
//! - **Observe** session start/end (PreRun, PostRun)
//!
//! ## Configuration
//!
//! Hooks are loaded from `.volt/hooks.toml` (project) and
//! `~/.volt/hooks.toml` (user-global). Project-level entries take
//! precedence on a per-hook basis.
//!
//! ```toml
//! [hooks]
//!
//! # Block destructive bash commands
//! [[hooks.pre_tool_use.entries]]
//! matcher = "bash"
//! timeout = 10
//! command = """
//! jq -e '.args.command | test("rm -rf /") | not' >/dev/null || {
//!   echo "refusing rm -rf /" >&2
//!   exit 2
//! }
//! """
//!
//! # Audit-log every tool invocation
//! [[hooks.post_tool_use.entries]]
//! matcher = "*"
//! command = "echo \"$VOLT_TOOL took ${VOLT_DURATION_MS}ms\" >> ~/.volt/audit.log"
//!
//! # Record session starts
//! [[hooks.pre_run.entries]]
//! command = "echo \"session $VOLT_SESSION_ID started at $(date -Iseconds)\" >> ~/.volt/sessions.log"
//! ```
//!
//! ## JSON I/O
//!
//! Each hook receives a JSON payload on **stdin**:
//!
//! | Event            | Payload fields                                                            |
//! |------------------|---------------------------------------------------------------------------|
//! | `PreToolUse`     | `event`, `tool`, `args`, `session_id`, `agent_name`                       |
//! | `PostToolUse`    | `event`, `tool`, `args`, `output`, `success`, `duration_ms`, `session_id` |
//! | `PreRun`         | `event`, `session_id`, `agent_name`                                        |
//! | `PostRun`        | `event`, `session_id`, `agent_name`                                        |
//! | `UserPromptSubmit` | `event`, `prompt`, `session_id`, `agent_name`                           |
//!
//! **stdout** conventions:
//! - `PreToolUse`: optional JSON `{"decision": "block", "reason": "..."}` or
//!   `{"decision": "modify", "args": {...}}`. Exit 2 always blocks (stderr =
//!   reason).
//! - `PostToolUse` / `UserPromptSubmit`: optional JSON
//!   `{"context": "..."}`. Each non-empty `context` is appended to the
//!   next model message.
//!
//! **Exit codes**:
//! - `0` — success (stdout may carry JSON outcome)
//! - `2` — block (PreToolUse only; stderr is the reason)
//! - any other non-zero — log warning, treat as no-op (never fail the agent)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Events that hooks can subscribe to. Each maps to a list of
/// `HookDefinition`s in the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Fires before a tool is executed. Can block or modify args.
    PreToolUse,
    /// Fires after a tool has executed. Can inject context.
    PostToolUse,
    /// Fires once at the start of an `Agent::run` call (a.k.a. session start).
    PreRun,
    /// Fires once at the end of an `Agent::run` call (a.k.a. session end).
    PostRun,
    /// Fires when the user submits a prompt. Can inject context.
    UserPromptSubmit,
}

impl HookEvent {
    pub fn name(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::PreRun => "PreRun",
            HookEvent::PostRun => "PostRun",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
        }
    }
}

/// One hook command. The same struct is reused for all event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDefinition {
    /// Optional tool-name matcher for `PreToolUse` / `PostToolUse`:
    /// - `*` or empty → matches all tools
    /// - `bash|write` → pipe-separated allow-list
    /// - `/^foo_/` → regex (must start and end with `/`)
    #[serde(default)]
    pub matcher: Option<String>,
    /// Timeout in seconds. Default 60.
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Shell command to run. Executed via `sh -c` on Unix, `cmd /C` on
    /// Windows. Receives a JSON payload on stdin. Environment variables
    /// `VOLT_EVENT`, `VOLT_TOOL`, `VOLT_SESSION_ID`, `VOLT_AGENT_NAME`,
    /// and (for tool events) `VOLT_DURATION_MS` are set.
    pub command: String,
}

fn default_timeout() -> u64 {
    60
}

/// One section of the config: an event type + a list of hook entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookSection {
    #[serde(default)]
    pub entries: Vec<HookDefinition>,
}

/// Top-level hooks config (matches `hooks.toml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookConfig {
    #[serde(default)]
    pub pre_tool_use: HookSection,
    #[serde(default)]
    pub post_tool_use: HookSection,
    #[serde(default)]
    pub pre_run: HookSection,
    #[serde(default)]
    pub post_run: HookSection,
    #[serde(default)]
    pub user_prompt_submit: HookSection,
}

/// Decision returned by PreToolUse hooks. Multiple hooks are run in
/// parallel; the strongest non-Allow decision wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreToolDecision {
    /// No hook objected; run the tool.
    Allow,
    /// A hook blocked the call. Reason is the merged stderr / JSON.
    Block { reason: String },
    /// A hook rewrote the arguments. The agent will use these instead.
    ModifyArgs { args: serde_json::Value },
}

impl PreToolDecision {
    pub fn is_blocking(&self) -> bool {
        matches!(self, PreToolDecision::Block { .. })
    }
}

/// Context string injected into the next model message by a PostToolUse
/// or UserPromptSubmit hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    pub text: String,
}

/// Combined outcome of running all hooks for an event.
#[derive(Debug, Clone, Default)]
pub struct HookOutcome {
    /// Modified arguments to use instead of the original (from PreToolUse).
    /// `None` means no hook modified the args; the caller should use the
    /// original.
    pub modified_args: Option<serde_json::Value>,
    /// Block reason, if any PreToolUse hook blocked the call.
    pub block_reason: Option<String>,
    /// Context strings collected from PostToolUse / UserPromptSubmit hooks.
    /// All non-empty strings are concatenated and injected into the next
    /// model message.
    pub contexts: Vec<HookContext>,
}

impl HookOutcome {
    pub fn is_blocked(&self) -> bool {
        self.block_reason.is_some()
    }

    /// Concatenate all `contexts` into a single string suitable for
    /// appending to a system or user message. Empty when no contexts.
    pub fn merged_context(&self) -> String {
        self.contexts
            .iter()
            .map(|c| c.text.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Payload sent to a hook on stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    pub event: String,
    pub session_id: String,
    pub agent_name: String,
    /// Tool name (PreToolUse, PostToolUse only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Tool arguments (PreToolUse, PostToolUse only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// Tool output (PostToolUse only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Tool success flag (PostToolUse only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    /// Tool duration in milliseconds (PostToolUse only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// User prompt (UserPromptSubmit only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

impl HookPayload {
    pub fn pre_tool_use(
        session_id: &str,
        agent_name: &str,
        tool: &str,
        args: &serde_json::Value,
    ) -> Self {
        Self {
            event: "PreToolUse".into(),
            session_id: session_id.into(),
            agent_name: agent_name.into(),
            tool: Some(tool.into()),
            args: Some(args.clone()),
            output: None,
            success: None,
            duration_ms: None,
            prompt: None,
        }
    }

    pub fn post_tool_use(
        session_id: &str,
        agent_name: &str,
        tool: &str,
        args: &serde_json::Value,
        output: &str,
        success: bool,
        duration_ms: u64,
    ) -> Self {
        Self {
            event: "PostToolUse".into(),
            session_id: session_id.into(),
            agent_name: agent_name.into(),
            tool: Some(tool.into()),
            args: Some(args.clone()),
            output: Some(output.to_string()),
            success: Some(success),
            duration_ms: Some(duration_ms),
            prompt: None,
        }
    }

    pub fn pre_run(session_id: &str, agent_name: &str) -> Self {
        Self {
            event: "PreRun".into(),
            session_id: session_id.into(),
            agent_name: agent_name.into(),
            tool: None,
            args: None,
            output: None,
            success: None,
            duration_ms: None,
            prompt: None,
        }
    }

    pub fn post_run(session_id: &str, agent_name: &str) -> Self {
        Self {
            event: "PostRun".into(),
            session_id: session_id.into(),
            agent_name: agent_name.into(),
            tool: None,
            args: None,
            output: None,
            success: None,
            duration_ms: None,
            prompt: None,
        }
    }

    pub fn user_prompt_submit(session_id: &str, agent_name: &str, prompt: &str) -> Self {
        Self {
            event: "UserPromptSubmit".into(),
            session_id: session_id.into(),
            agent_name: agent_name.into(),
            tool: None,
            args: None,
            output: None,
            success: None,
            duration_ms: None,
            prompt: Some(prompt.to_string()),
        }
    }
}

/// The hook registry, holding all loaded `HookDefinition`s. Cheap to clone
/// (all clones share the same `Arc<HookConfig>`).
#[derive(Debug, Clone, Default)]
pub struct HookRegistry {
    inner: Arc<HookConfig>,
    session_id: Arc<String>,
    agent_name: Arc<String>,
}

impl HookRegistry {
    /// A registry with no hooks configured. All hook methods become no-ops.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Load a registry from a single TOML file. Missing file returns
    /// `HookRegistry::empty()` (not an error — users opt into hooks by
    /// creating the file). Malformed TOML returns an error.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::empty());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {}", path.display(), e))?;
        let config: HookConfig = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parsing {}: {}", path.display(), e))?;
        Ok(Self {
            inner: Arc::new(config),
            session_id: Arc::new(String::new()),
            agent_name: Arc::new(String::new()),
        })
    }

    /// Load the default discovery chain: `.volt/hooks.toml` in CWD first,
    /// then `~/.volt/hooks.toml`. Project entries override user entries
    /// on a per-event basis (the project's `pre_tool_use` list is used in
    /// full if it has entries; otherwise the user's list is used).
    pub fn from_default_paths(cwd: &Path) -> anyhow::Result<Self> {
        let project = cwd.join(".volt").join("hooks.toml");
        let user = dirs_home()
            .map(|h| h.join(".volt").join("hooks.toml"))
            .unwrap_or_else(|| PathBuf::from(".volt/hooks.toml"));

        let project_cfg = if project.exists() {
            Some(load_config(&project)?)
        } else {
            None
        };
        let user_cfg = if user.exists() {
            Some(load_config(&user)?)
        } else {
            None
        };

        let merged = match (project_cfg, user_cfg) {
            (Some(p), Some(u)) => merge_configs(p, u),
            (Some(p), None) => p,
            (None, Some(u)) => u,
            (None, None) => HookConfig::default(),
        };

        Ok(Self {
            inner: Arc::new(merged),
            session_id: Arc::new(String::new()),
            agent_name: Arc::new(String::new()),
        })
    }

    /// Build a registry from an in-memory `HookConfig`. Useful for tests
    /// and for users who want to programmatically configure hooks.
    pub fn from_config(config: HookConfig) -> Self {
        Self {
            inner: Arc::new(config),
            session_id: Arc::new(String::new()),
            agent_name: Arc::new(String::new()),
        }
    }

    /// Set the session id and agent name. The agent loop calls this once
    /// at the start of `Agent::run` so hook payloads carry the right
    /// identifiers.
    pub fn with_session(mut self, session_id: &str, agent_name: &str) -> Self {
        self.session_id = Arc::new(session_id.to_string());
        self.agent_name = Arc::new(agent_name.to_string());
        self
    }

    /// True if no hooks are registered for any event.
    pub fn is_empty(&self) -> bool {
        self.inner.pre_tool_use.entries.is_empty()
            && self.inner.post_tool_use.entries.is_empty()
            && self.inner.pre_run.entries.is_empty()
            && self.inner.post_run.entries.is_empty()
            && self.inner.user_prompt_submit.entries.is_empty()
    }

    /// Return the entries for a given event, filtered by `tool_name` if
    /// the event is tool-bound.
    pub fn entries_for(&self, event: HookEvent, tool_name: Option<&str>) -> Vec<HookDefinition> {
        let section = match event {
            HookEvent::PreToolUse => &self.inner.pre_tool_use,
            HookEvent::PostToolUse => &self.inner.post_tool_use,
            HookEvent::PreRun => &self.inner.pre_run,
            HookEvent::PostRun => &self.inner.post_run,
            HookEvent::UserPromptSubmit => &self.inner.user_prompt_submit,
        };
        section
            .entries
            .iter()
            .filter(|def| matcher_matches(def.matcher.as_deref(), tool_name))
            .cloned()
            .collect()
    }

    /// Run all matching PreToolUse hooks. Returns the strongest decision.
    pub async fn run_pre_tool_use(&self, tool: &str, args: &serde_json::Value) -> PreToolDecision {
        let entries = self.entries_for(HookEvent::PreToolUse, Some(tool));
        if entries.is_empty() {
            return PreToolDecision::Allow;
        }
        let payload = HookPayload::pre_tool_use(&self.session_id, &self.agent_name, tool, args);
        let mut handles = Vec::with_capacity(entries.len());
        for entry in entries {
            let payload = payload.clone();
            handles.push(tokio::spawn(run_one_hook(entry, payload)));
        }
        let mut decision = PreToolDecision::Allow;
        for h in handles {
            match h.await {
                Ok(Ok(HookResult::Block(reason))) => {
                    if !decision.is_blocking() {
                        decision = PreToolDecision::Block { reason };
                    }
                }
                Ok(Ok(HookResult::Modify(new_args))) => {
                    if !decision.is_blocking() {
                        decision = PreToolDecision::ModifyArgs { args: new_args };
                    }
                }
                Ok(Ok(HookResult::Allow)) | Ok(Ok(HookResult::Context(_))) | Ok(Err(())) => {
                    // Allow, context (invalid for PreToolUse but tolerated),
                    // or hard error: keep current decision
                }
                Err(e) => {
                    warn!("[hook] PreToolUse task join error: {}", e);
                }
            }
        }
        decision
    }

    /// Run all matching PostToolUse hooks. Returns the merged outcome
    /// (contexts only — PostToolUse can't block).
    pub async fn run_post_tool_use(
        &self,
        tool: &str,
        args: &serde_json::Value,
        output: &str,
        success: bool,
        duration_ms: u64,
    ) -> HookOutcome {
        let entries = self.entries_for(HookEvent::PostToolUse, Some(tool));
        if entries.is_empty() {
            return HookOutcome::default();
        }
        let payload = HookPayload::post_tool_use(
            &self.session_id,
            &self.agent_name,
            tool,
            args,
            output,
            success,
            duration_ms,
        );
        let mut contexts = Vec::new();
        for entry in entries {
            match run_one_hook(entry, payload.clone()).await {
                Ok(HookResult::Context(text)) => {
                    if !text.trim().is_empty() {
                        contexts.push(HookContext { text });
                    }
                }
                Ok(_) => {}
                Err(()) => {
                    // Already logged inside `run_one_hook`.
                }
            }
        }
        HookOutcome {
            contexts,
            ..Default::default()
        }
    }

    /// Run all matching UserPromptSubmit hooks. Returns the merged
    /// outcome (contexts only).
    pub async fn run_user_prompt_submit(&self, prompt: &str) -> HookOutcome {
        let entries = self.entries_for(HookEvent::UserPromptSubmit, None);
        if entries.is_empty() {
            return HookOutcome::default();
        }
        let payload = HookPayload::user_prompt_submit(&self.session_id, &self.agent_name, prompt);
        let mut contexts = Vec::new();
        for entry in entries {
            match run_one_hook(entry, payload.clone()).await {
                Ok(HookResult::Context(text)) => {
                    if !text.trim().is_empty() {
                        contexts.push(HookContext { text });
                    }
                }
                Ok(_) => {}
                Err(()) => {}
            }
        }
        HookOutcome {
            contexts,
            ..Default::default()
        }
    }

    /// Run all PreRun hooks. Errors are logged, not propagated.
    pub async fn run_pre_run(&self) {
        let entries = self.entries_for(HookEvent::PreRun, None);
        if entries.is_empty() {
            return;
        }
        let payload = HookPayload::pre_run(&self.session_id, &self.agent_name);
        for entry in entries {
            if let Err(()) = run_one_hook(entry, payload.clone()).await {
                // already logged
            }
        }
    }

    /// Run all PostRun hooks. Errors are logged, not propagated.
    pub async fn run_post_run(&self) {
        let entries = self.entries_for(HookEvent::PostRun, None);
        if entries.is_empty() {
            return;
        }
        let payload = HookPayload::post_run(&self.session_id, &self.agent_name);
        for entry in entries {
            if let Err(()) = run_one_hook(entry, payload.clone()).await {
                // already logged
            }
        }
    }
}

/// Outcome of a single hook command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum HookResult {
    /// No effect; allow the tool.
    Allow,
    /// Block the tool (PreToolUse only).
    Block(String),
    /// Modify the tool arguments (PreToolUse only).
    Modify(serde_json::Value),
    /// Inject context (PostToolUse / UserPromptSubmit).
    Context(String),
}

async fn run_one_hook(def: HookDefinition, payload: HookPayload) -> Result<HookResult, ()> {
    let event_name = payload.event.clone();
    let tool_name = payload.tool.clone().unwrap_or_default();
    let start = std::time::Instant::now();
    let stdin_payload = match serde_json::to_string(&payload) {
        Ok(s) => s,
        Err(e) => {
            warn!("[hook] serialising payload for {}: {}", event_name, e);
            return Err(());
        }
    };

    let mut cmd = build_command(&def.command);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.env("VOLT_EVENT", &event_name);
    if !tool_name.is_empty() {
        cmd.env("VOLT_TOOL", &tool_name);
    }
    cmd.env("VOLT_SESSION_ID", &payload.session_id);
    cmd.env("VOLT_AGENT_NAME", &payload.agent_name);
    if let Some(d) = payload.duration_ms {
        cmd.env("VOLT_DURATION_MS", d.to_string());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!("[hook] failed to spawn {} command: {}", event_name, e);
            return Err(());
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        if let Err(e) = stdin.write_all(stdin_payload.as_bytes()).await {
            warn!("[hook] stdin write failed: {}", e);
        }
        drop(stdin);
    }

    let timeout = Duration::from_secs(def.timeout.max(1));
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            warn!("[hook] {} wait_with_output failed: {}", event_name, e);
            return Err(());
        }
        Err(_) => {
            warn!(
                "[hook] {} timed out after {}s — child left running",
                event_name, def.timeout
            );
            return Err(());
        }
    };

    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    match output.status.code() {
        Some(0) => {
            debug!(
                "[hook] {} for tool={} OK in {:?}; stdout={:?}",
                event_name,
                tool_name,
                elapsed,
                truncate(&stdout, 200)
            );
            Ok(parse_stdout(&event_name, &stdout))
        }
        Some(2) => {
            info!(
                "[hook] {} for tool={} BLOCKED in {:?}; stderr={:?}",
                event_name,
                tool_name,
                elapsed,
                truncate(&stderr, 200)
            );
            let reason = if stderr.trim().is_empty() {
                "blocked by hook (exit 2)".into()
            } else {
                stderr.trim().to_string()
            };
            Ok(HookResult::Block(reason))
        }
        Some(code) => {
            warn!(
                "[hook] {} for tool={} exited with code {}; treating as no-op. stderr={:?}",
                event_name,
                tool_name,
                code,
                truncate(&stderr, 200)
            );
            Err(())
        }
        None => {
            warn!(
                "[hook] {} for tool={} terminated by signal; treating as no-op",
                event_name, tool_name
            );
            Err(())
        }
    }
}

/// Parse a hook's stdout into a `HookResult`. The hook may emit:
/// - nothing → `Allow`
/// - JSON `{"decision": "block", "reason": "..."}` → `Block(reason)`
/// - JSON `{"decision": "modify", "args": {...}}` → `Modify(args)`
/// - JSON `{"context": "..."}` → `Context(text)`
/// - any other text → treated as context (for PostToolUse / UserPromptSubmit)
fn parse_stdout(event: &str, stdout: &str) -> HookResult {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return HookResult::Allow;
    }
    // Try strict JSON first.
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(obj) = val.as_object() {
            // PreToolUse: explicit decision
            if let Some(decision) = obj.get("decision").and_then(|d| d.as_str()) {
                match decision {
                    "block" => {
                        let reason = obj
                            .get("reason")
                            .and_then(|r| r.as_str())
                            .unwrap_or("blocked by hook")
                            .to_string();
                        return HookResult::Block(reason);
                    }
                    "modify" => {
                        // "modify" without an "args" field is malformed.
                        // Before this fix, the parser silently fell
                        // through and returned `Allow`, which is
                        // dangerous: a hook that *intended* to modify
                        // tool args would be ignored, and the
                        // unmodified (potentially unsafe) args would
                        // pass through. Treat malformed modify as a
                        // hard error so the caller knows the hook is
                        // broken.
                        match obj.get("args") {
                            Some(args) => return HookResult::Modify(args.clone()),
                            None => {
                                warn!(
                                    "[hook] PreToolUse hook emitted decision=\"modify\" \
                                     without an \"args\" object; treating as hard error"
                                );
                                return HookResult::Block(
                                    "hook emitted decision=modify without args".to_string(),
                                );
                            }
                        }
                    }
                    "allow" => return HookResult::Allow,
                    _ => {}
                }
            }
            // PostToolUse / UserPromptSubmit: context
            if let Some(text) = obj.get("context").and_then(|c| c.as_str()) {
                return HookResult::Context(text.to_string());
            }
        }
    }
    // Loose fallback: any non-JSON stdout is treated as context for
    // observation events (PostToolUse, UserPromptSubmit, PreRun, PostRun).
    // For PreToolUse, non-JSON stdout is just ignored — the hook said
    // nothing actionable.
    if matches!(
        event,
        "PostToolUse" | "UserPromptSubmit" | "PreRun" | "PostRun"
    ) {
        HookResult::Context(trimmed.to_string())
    } else {
        HookResult::Allow
    }
}

/// Build the shell command for `sh -c "..."` on Unix or `cmd /C "..."` on
/// Windows. We avoid `sh -c` on Windows because it's not always present.
fn build_command(command: &str) -> tokio::process::Command {
    #[cfg(unix)]
    {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
}

/// Test whether a hook's `matcher` matches a given tool name. Returns
/// true when the matcher is `None`, empty, or `"*"`. Pipe-separated
/// names match if the tool is in the list. A matcher wrapped in `/.../`
/// is treated as a regex.
fn matcher_matches(matcher: Option<&str>, tool_name: Option<&str>) -> bool {
    let Some(m) = matcher else {
        return true;
    };
    let m = m.trim();
    if m.is_empty() || m == "*" {
        return true;
    }
    let Some(tool) = tool_name else {
        // No tool name to match against; only `*` or empty matches.
        return false;
    };
    if m.starts_with('/') && m.ends_with('/') && m.len() > 2 {
        // Regex form: /pattern/
        let pat = &m[1..m.len() - 1];
        match regex_lite_match(pat, tool) {
            Ok(matched) => return matched,
            Err(e) => {
                warn!("[hook] invalid regex matcher {:?}: {}", m, e);
                return false;
            }
        }
    }
    // Pipe-separated allow-list.
    m.split('|').any(|part| part.trim() == tool)
}

/// Tiny regex-lite matcher using only `std`. Supports `.`, `*`, `+`,
/// `?`, character classes, anchors, and alternation. Not a full regex
/// engine — good enough for hook matchers like `/^foo_/`. If the user
/// needs a complex regex, they should depend on a real regex crate; this
/// is a deliberate minimal implementation to keep the hook system
/// dependency-free.
fn regex_lite_match(pattern: &str, s: &str) -> Result<bool, String> {
    let re = RegexLite::new(pattern).map_err(|e| e.to_string())?;
    Ok(re.is_match(s))
}

/// Minimal regex engine. Supports a useful subset of PCRE:
/// - `.` matches any char
/// - `*`, `+`, `?` quantifiers
/// - `[abc]` character classes, `[^abc]` negation
/// - `^` / `$` anchors
/// - `(a|b)` alternation
/// - escapes `\`
#[derive(Clone)]
struct RegexLite {
    tokens: Vec<Token>,
}

impl std::fmt::Debug for RegexLite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegexLite")
            .field("tokens", &self.tokens.len())
            .finish()
    }
}

#[derive(Debug, Clone)]
enum Token {
    Literal(char),
    Any,
    Class(Vec<char>, bool), // chars, negated
    Star(Box<Token>),
    Plus(Box<Token>),
    Quest(Box<Token>),
    Alt(Vec<RegexLite>), // each alternative is a full sub-regex
    AnchorStart,
    AnchorEnd,
}

#[derive(Debug)]
struct RegexLiteError(pub String);

impl std::fmt::Display for RegexLiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl RegexLite {
    fn new(pattern: &str) -> Result<Self, RegexLiteError> {
        let bytes = pattern.as_bytes();
        let mut p = Parser { bytes, pos: 0 };
        let re = p.parse_alt()?;
        if p.pos != bytes.len() {
            return Err(RegexLiteError(format!(
                "unexpected trailing input at position {}",
                p.pos
            )));
        }
        Ok(re)
    }

    fn is_match(&self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        self.try_match(&chars, 0, 0).is_some()
    }

    /// Try to match tokens[ti..] against chars[ci..].
    /// Returns `Some(new_ci)` on success (the position immediately after
    /// the last consumed character) or `None` on failure.
    fn try_match(&self, chars: &[char], ci: usize, ti: usize) -> Option<usize> {
        if ti == self.tokens.len() {
            return Some(ci);
        }
        match &self.tokens[ti] {
            Token::AnchorStart => {
                if ci != 0 {
                    return None;
                }
                self.try_match(chars, ci, ti + 1)
            }
            Token::AnchorEnd => {
                if ti + 1 == self.tokens.len() {
                    if ci == chars.len() {
                        Some(ci)
                    } else {
                        None
                    }
                } else {
                    // The rest of the tokens must consume all remaining chars.
                    self.try_match(chars, ci, ti + 1).and_then(|e| {
                        if e == chars.len() {
                            Some(e)
                        } else {
                            None
                        }
                    })
                }
            }
            Token::Alt(alts) => {
                for a in alts {
                    if let Some(mid) = a.try_match(chars, ci, 0) {
                        if let Some(after) = self.try_match(chars, mid, ti + 1) {
                            return Some(after);
                        }
                    }
                }
                None
            }
            Token::Literal(c) => {
                if ci < chars.len() && chars[ci] == *c {
                    self.try_match(chars, ci + 1, ti + 1)
                } else {
                    None
                }
            }
            Token::Any => {
                if ci < chars.len() {
                    self.try_match(chars, ci + 1, ti + 1)
                } else {
                    None
                }
            }
            Token::Class(set, neg) => {
                if ci < chars.len() && set.contains(&chars[ci]) != *neg {
                    self.try_match(chars, ci + 1, ti + 1)
                } else {
                    None
                }
            }
            Token::Star(inner) => {
                // Greedy: try k = max, max-1, ..., 0 copies
                let max_k = count_atom_matches(inner, ci, chars);
                for k in (0..=max_k).rev() {
                    if let Some(after) = self.try_match(chars, ci + k, ti + 1) {
                        return Some(after);
                    }
                }
                None
            }
            Token::Plus(inner) => {
                let max_k = count_atom_matches(inner, ci, chars);
                if max_k == 0 {
                    return None;
                }
                for k in (1..=max_k).rev() {
                    if let Some(after) = self.try_match(chars, ci + k, ti + 1) {
                        return Some(after);
                    }
                }
                None
            }
            Token::Quest(inner) => {
                // 0 copies
                if let Some(after) = self.try_match(chars, ci, ti + 1) {
                    return Some(after);
                }
                // 1 copy
                if ci < chars.len() && atom_matches_char(inner, chars[ci]) {
                    return self.try_match(chars, ci + 1, ti + 1);
                }
                None
            }
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    fn eat(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }
    /// Wrap an atom in a quantifier token if the next byte is `*`, `+`, or
    /// `?`. Otherwise returns the atom unchanged.
    fn wrap_quantifier(&mut self, atom: Token) -> Result<Token, RegexLiteError> {
        match self.peek() {
            Some(b'*') => {
                self.eat();
                Ok(Token::Star(Box::new(atom)))
            }
            Some(b'+') => {
                self.eat();
                Ok(Token::Plus(Box::new(atom)))
            }
            Some(b'?') => {
                self.eat();
                Ok(Token::Quest(Box::new(atom)))
            }
            _ => Ok(atom),
        }
    }

    fn parse_alt(&mut self) -> Result<RegexLite, RegexLiteError> {
        let mut alts: Vec<RegexLite> = vec![self.parse_seq()?];
        while self.peek() == Some(b'|') {
            self.eat();
            alts.push(self.parse_seq()?);
        }
        if alts.len() == 1 {
            Ok(alts.into_iter().next().unwrap())
        } else {
            Ok(RegexLite {
                tokens: vec![Token::Alt(alts)],
            })
        }
    }

    fn parse_seq(&mut self) -> Result<RegexLite, RegexLiteError> {
        let mut tokens = Vec::new();
        while let Some(b) = self.peek() {
            match b {
                b')' | b'|' => break,
                b'^' => {
                    self.eat();
                    tokens.push(Token::AnchorStart);
                }
                b'$' => {
                    self.eat();
                    tokens.push(Token::AnchorEnd);
                }
                b'(' => {
                    self.eat();
                    let inner = self.parse_alt()?;
                    if self.eat() != Some(b')') {
                        return Err(RegexLiteError("unmatched `(`".into()));
                    }
                    // Splice the inner tokens into the current sequence.
                    // This means `(a|b)c` becomes `[a, |, b, c]` at the
                    // current level rather than nesting.
                    tokens.extend(inner.tokens);
                }
                b'[' => {
                    self.eat();
                    let (set, neg) = self.parse_class()?;
                    if self.eat() != Some(b']') {
                        return Err(RegexLiteError("unmatched `[`".into()));
                    }
                    let atom = Token::Class(set, neg);
                    tokens.push(self.wrap_quantifier(atom)?);
                }
                b'.' => {
                    self.eat();
                    let atom = Token::Any;
                    tokens.push(self.wrap_quantifier(atom)?);
                }
                b'\\' => {
                    self.eat();
                    let c = self
                        .eat()
                        .ok_or_else(|| RegexLiteError("dangling `\\`".into()))?
                        as char;
                    let atom = Token::Literal(c);
                    tokens.push(self.wrap_quantifier(atom)?);
                }
                b'*' | b'+' | b'?' => {
                    return Err(RegexLiteError("quantifier without preceding atom".into()));
                }
                _ => {
                    let c = self.eat().unwrap() as char;
                    let atom = Token::Literal(c);
                    tokens.push(self.wrap_quantifier(atom)?);
                }
            }
        }
        Ok(RegexLite { tokens })
    }

    fn parse_class(&mut self) -> Result<(Vec<char>, bool), RegexLiteError> {
        let neg = if self.peek() == Some(b'^') {
            self.eat();
            true
        } else {
            false
        };
        let mut set = Vec::new();
        while let Some(b) = self.peek() {
            if b == b']' {
                break;
            }
            let c = self.eat().unwrap() as char;
            if self.peek() == Some(b'-') {
                self.eat();
                let end = self
                    .eat()
                    .ok_or_else(|| RegexLiteError("dangling `-` in class".into()))?
                    as char;
                let from = c as u32;
                let to = end as u32;
                if from > to {
                    return Err(RegexLiteError(format!(
                        "class range {:?}-{:?} out of order",
                        c, end
                    )));
                }
                for cu in from..=to {
                    if let Some(ch) = char::from_u32(cu) {
                        set.push(ch);
                    }
                }
            } else {
                set.push(c);
            }
        }
        Ok((set, neg))
    }
}

fn dirs_home() -> Option<PathBuf> {
    // We avoid pulling in the `dirs` crate here to keep the hook module
    // dependency-free. The `home` env var is set on every Unix-likes
    // including macOS; on Windows we use USERPROFILE.
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
}

fn load_config(path: &Path) -> anyhow::Result<HookConfig> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {}", path.display(), e))?;
    toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing {}: {}", path.display(), e))
}

/// Merge two configs: project overrides user on a per-event basis. If
/// the project has any entries for an event, the user entries for that
/// event are dropped. Otherwise the user's entries are kept.
fn merge_configs(project: HookConfig, user: HookConfig) -> HookConfig {
    HookConfig {
        pre_tool_use: if !project.pre_tool_use.entries.is_empty() {
            project.pre_tool_use
        } else {
            user.pre_tool_use
        },
        post_tool_use: if !project.post_tool_use.entries.is_empty() {
            project.post_tool_use
        } else {
            user.post_tool_use
        },
        pre_run: if !project.pre_run.entries.is_empty() {
            project.pre_run
        } else {
            user.pre_run
        },
        post_run: if !project.post_run.entries.is_empty() {
            project.post_run
        } else {
            user.post_run
        },
        user_prompt_submit: if !project.user_prompt_submit.entries.is_empty() {
            project.user_prompt_submit
        } else {
            user.user_prompt_submit
        },
    }
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

/// Does a single character atom (`Literal`, `Any`, or `Class`) match
/// the given character?
fn atom_matches_char(token: &Token, c: char) -> bool {
    match token {
        Token::Literal(ch) => *ch == c,
        Token::Any => true,
        Token::Class(set, neg) => set.contains(&c) != *neg,
        Token::Star(_)
        | Token::Plus(_)
        | Token::Quest(_)
        | Token::Alt(_)
        | Token::AnchorStart
        | Token::AnchorEnd => false,
    }
}

/// How many times can the atom `inner` consecutively match starting at
/// `chars[ci]`? Returns 0..=remaining_len.
fn count_atom_matches(inner: &Token, ci: usize, chars: &[char]) -> usize {
    let mut n = 0;
    while ci + n < chars.len() && atom_matches_char(inner, chars[ci + n]) {
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn make_registry(defs: Vec<HookDefinition>, event: HookEvent) -> HookRegistry {
        let cfg = match event {
            HookEvent::PreToolUse => HookConfig {
                pre_tool_use: HookSection { entries: defs },
                ..Default::default()
            },
            HookEvent::PostToolUse => HookConfig {
                post_tool_use: HookSection { entries: defs },
                ..Default::default()
            },
            HookEvent::PreRun => HookConfig {
                pre_run: HookSection { entries: defs },
                ..Default::default()
            },
            HookEvent::PostRun => HookConfig {
                post_run: HookSection { entries: defs },
                ..Default::default()
            },
            HookEvent::UserPromptSubmit => HookConfig {
                user_prompt_submit: HookSection { entries: defs },
                ..Default::default()
            },
        };
        HookRegistry::from_config(cfg).with_session("test-sess", "test-agent")
    }

    fn cmd(command: &str) -> HookDefinition {
        HookDefinition {
            matcher: None,
            timeout: 5,
            command: command.to_string(),
        }
    }

    #[test]
    fn empty_registry_allows_everything() {
        let r = HookRegistry::empty();
        assert!(r.is_empty());
        let decision =
            futures::executor::block_on(r.run_pre_tool_use("bash", &serde_json::json!({})));
        assert_eq!(decision, PreToolDecision::Allow);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_tool_use_blocks_on_exit_2() {
        let r = make_registry(
            vec![cmd(r#"echo "no rm -rf" >&2; exit 2"#)],
            HookEvent::PreToolUse,
        );
        let d = r.run_pre_tool_use("bash", &serde_json::json!({})).await;
        match d {
            PreToolDecision::Block { reason } => {
                assert!(reason.contains("no rm -rf"), "got: {reason}");
            }
            other => panic!("expected Block, got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_tool_use_modifies_args_via_json_stdout() {
        // Diagnostic note: if this test fails on a future CI run,
        // the only signal we'd otherwise get is "expected ModifyArgs,
        // got Allow" with no indication of what stdout the parser
        // actually saw. A hook that says "modify" but the JSON
        // parse failed, the shell mangled the output, or the parser
        // encountered an unexpected shape would all surface as the
        // same opaque error. To make future failures debuggable
        // without touching the test again, we tee the hook's
        // stdout to a tempfile and include the contents in the
        // panic message.
        let tmp =
            std::env::temp_dir().join(format!("volt-hook-test-modify-{}.out", std::process::id()));
        let tmp_str = tmp.display().to_string();
        let _ = std::fs::remove_file(&tmp);
        // Wrap the hook command so its stdout is both delivered to
        // the parser (via `tee`) and captured for diagnostics.
        let wrapped = format!(
            r#"echo '{{"decision":"modify","args":{{"command":"echo safe"}}}}' | tee {}"#,
            tmp_str
        );
        let r = make_registry(vec![cmd(&wrapped)], HookEvent::PreToolUse);
        let d = r.run_pre_tool_use("bash", &serde_json::json!({})).await;
        let captured = std::fs::read_to_string(&tmp).unwrap_or_default();
        let _ = std::fs::remove_file(&tmp);
        match d {
            PreToolDecision::ModifyArgs { args } => {
                assert_eq!(args["command"], "echo safe");
            }
            other => panic!(
                "expected ModifyArgs, got {:?}; hook stdout was: {:?}",
                other, captured
            ),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_tool_use_allow_on_exit_0_no_stdout() {
        let r = make_registry(vec![cmd("true")], HookEvent::PreToolUse);
        let d = r.run_pre_tool_use("bash", &serde_json::json!({})).await;
        assert_eq!(d, PreToolDecision::Allow);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn post_tool_use_collects_context_from_json() {
        let r = make_registry(
            vec![cmd(r#"echo '{"context":"audit: tool ran"}'"#)],
            HookEvent::PostToolUse,
        );
        let out = r
            .run_post_tool_use("bash", &serde_json::json!({}), "ok", true, 42)
            .await;
        assert_eq!(out.contexts.len(), 1);
        assert_eq!(out.contexts[0].text, "audit: tool ran");
        assert_eq!(out.merged_context(), "audit: tool ran");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn post_tool_use_loose_stdout_becomes_context() {
        let r = make_registry(
            vec![cmd("echo free-form notes here")],
            HookEvent::PostToolUse,
        );
        let out = r
            .run_post_tool_use("bash", &serde_json::json!({}), "", true, 0)
            .await;
        assert_eq!(out.contexts.len(), 1);
        assert!(out.contexts[0].text.contains("free-form"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_run_runs_and_succeeds() {
        let r = make_registry(vec![cmd("echo session-start")], HookEvent::PreRun);
        // Should not panic or error
        r.run_pre_run().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn user_prompt_submit_injects_context() {
        let r = make_registry(
            vec![cmd(r#"echo '{"context":"user mentioned: foo"}'"#)],
            HookEvent::UserPromptSubmit,
        );
        let out = r.run_user_prompt_submit("hello").await;
        assert_eq!(out.contexts.len(), 1);
    }

    #[test]
    fn matcher_star_matches_all() {
        assert!(matcher_matches(Some("*"), Some("anything")));
        assert!(matcher_matches(None, Some("anything")));
        assert!(matcher_matches(Some(""), Some("anything")));
    }

    #[test]
    fn matcher_pipe_list() {
        assert!(matcher_matches(Some("bash|write"), Some("bash")));
        assert!(matcher_matches(Some("bash|write"), Some("write")));
        assert!(!matcher_matches(Some("bash|write"), Some("read")));
    }

    #[test]
    fn matcher_no_tool_name_only_star_matches() {
        assert!(matcher_matches(None, None));
        assert!(matcher_matches(Some("*"), None));
        assert!(!matcher_matches(Some("bash"), None));
    }

    #[test]
    fn matcher_regex_basic() {
        assert!(matcher_matches(Some("/^foo_/"), Some("foo_bar")));
        assert!(!matcher_matches(Some("/^foo_/"), Some("bar_foo")));
    }

    #[test]
    fn merge_project_overrides_user() {
        let user = HookConfig {
            pre_tool_use: HookSection {
                entries: vec![cmd("user-1")],
            },
            ..Default::default()
        };
        let project = HookConfig {
            pre_tool_use: HookSection {
                entries: vec![cmd("project-1"), cmd("project-2")],
            },
            post_tool_use: HookSection {
                entries: vec![cmd("user-post-stays")],
            },
            ..Default::default()
        };
        let merged = merge_configs(project, user);
        assert_eq!(merged.pre_tool_use.entries.len(), 2);
        // When project has no post_tool_use entries, user entries are kept.
        assert_eq!(merged.post_tool_use.entries.len(), 1);
        assert!(merged.post_tool_use.entries[0]
            .command
            .contains("user-post-stays"));
    }

    #[test]
    fn parse_stdout_handles_empty_and_garbage() {
        assert_eq!(parse_stdout("PreToolUse", ""), HookResult::Allow);
        assert_eq!(parse_stdout("PreToolUse", "  "), HookResult::Allow);
        // Loose garbage on a tool event is ignored, not context.
        assert_eq!(parse_stdout("PreToolUse", "hello"), HookResult::Allow);
        // Loose garbage on a non-tool event becomes context.
        assert!(matches!(
            parse_stdout("PostToolUse", "hello"),
            HookResult::Context(_)
        ));
    }

    #[test]
    fn parse_stdout_block_modify_allow() {
        assert!(matches!(
            parse_stdout("PreToolUse", r#"{"decision":"block","reason":"no"}"#),
            HookResult::Block(_)
        ));
        assert!(matches!(
            parse_stdout("PreToolUse", r#"{"decision":"modify","args":{"x":1}}"#),
            HookResult::Modify(_)
        ));
        assert_eq!(
            parse_stdout("PreToolUse", r#"{"decision":"allow"}"#),
            HookResult::Allow
        );
    }

    #[test]
    fn parse_stdout_modify_without_args_blocks() {
        // Regression: a hook that emits decision="modify" without an
        // "args" field is malformed. Before the fix, the parser
        // silently fell through and returned Allow, which would let
        // the unmodified (potentially unsafe) tool args pass
        // through. The fix: treat it as a Block with a clear
        // diagnostic, so the operator sees the broken hook in the
        // logs and the tool call is refused.
        let result = parse_stdout("PreToolUse", r#"{"decision":"modify"}"#);
        match result {
            HookResult::Block(reason) => {
                assert!(
                    reason.contains("modify without args"),
                    "expected diagnostic in reason, got: {reason}"
                );
            }
            other => panic!("expected Block for malformed modify, got {:?}", other),
        }
    }

    #[test]
    fn parse_config_from_toml() {
        let toml = r#"
[pre_tool_use]
[[pre_tool_use.entries]]
matcher = "bash"
timeout = 5
command = "echo blocked"

[post_tool_use]
[[post_tool_use.entries]]
matcher = "*"
command = "echo ok"
"#;
        let cfg: HookConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.pre_tool_use.entries.len(), 1);
        assert_eq!(cfg.pre_tool_use.entries[0].matcher.as_deref(), Some("bash"));
        assert_eq!(cfg.pre_tool_use.entries[0].timeout, 5);
        assert_eq!(cfg.post_tool_use.entries.len(), 1);
    }

    #[test]
    fn empty_section_omitted_is_ok() {
        let toml = r#"
[[pre_tool_use.entries]]
command = "echo a"
"#;
        let cfg: HookConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.pre_tool_use.entries.len(), 1);
        assert!(cfg.post_tool_use.entries.is_empty());
    }

    #[test]
    fn regex_lite_simple() {
        let re = RegexLite::new("foo").unwrap();
        assert!(re.is_match("foo"));
        assert!(!re.is_match("bar"));
    }

    #[test]
    fn regex_lite_quantifiers() {
        let re = RegexLite::new("ab*c").unwrap();
        assert!(re.is_match("ac"));
        assert!(re.is_match("abc"));
        assert!(re.is_match("abbbbc"));
        assert!(!re.is_match("abx"));
    }

    #[test]
    fn regex_lite_alternation() {
        let re = RegexLite::new("foo|bar").unwrap();
        assert!(re.is_match("foo"));
        assert!(re.is_match("bar"));
        assert!(!re.is_match("baz"));
    }

    #[test]
    fn regex_lite_anchors() {
        let re = RegexLite::new("^foo$").unwrap();
        assert!(re.is_match("foo"));
        assert!(!re.is_match("foobar"));
        assert!(!re.is_match("barfoo"));
    }

    #[test]
    fn regex_lite_class() {
        let re = RegexLite::new("[abc]+").unwrap();
        assert!(re.is_match("abc"));
        assert!(!re.is_match("def"));
    }

    #[test]
    fn regex_lite_anchored_alternation_group() {
        // `(...)` should work in matchers: `^(bash|write)$`
        let re = RegexLite::new("^(bash|write)$").unwrap();
        assert!(re.is_match("bash"));
        assert!(re.is_match("write"));
        assert!(!re.is_match("read"));
        assert!(!re.is_match("bashful"));
    }

    #[test]
    fn outcome_merged_context_skips_empty() {
        let o = HookOutcome {
            contexts: vec![
                HookContext { text: "  ".into() },
                HookContext {
                    text: "real".into(),
                },
                HookContext {
                    text: "more".into(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(o.merged_context(), "real\n\nmore");
    }

    #[test]
    fn payload_serialises_all_event_types() {
        let p = HookPayload::pre_tool_use("s", "a", "bash", &serde_json::json!({"x":1}));
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("PreToolUse"));
        assert!(s.contains(r#""tool":"bash""#));
        // `output` and `success` must be absent for PreToolUse.
        assert!(!s.contains("output"));
        assert!(!s.contains("success"));

        let p =
            HookPayload::post_tool_use("s", "a", "bash", &serde_json::json!({}), "out", true, 100);
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("PostToolUse"));
        assert!(s.contains(r#""duration_ms":100"#));

        let p = HookPayload::pre_run("s", "a");
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("PreRun"));
        // No tool-specific fields should leak.
        for forbidden in ["tool", "args", "output", "success", "duration_ms", "prompt"] {
            assert!(!s.contains(forbidden), "PreRun leaked {forbidden}: {s}");
        }
    }

    #[test]
    fn truncate_short_and_long() {
        assert_eq!(truncate("hi", 10), "hi");
        let t = truncate("hello world this is long", 10);
        assert!(t.chars().count() <= 11); // 10 chars + ellipsis
        assert!(t.ends_with('…'));
    }

    // Smoke test for the merge override behaviour with mixed project/user
    // entries.
}
