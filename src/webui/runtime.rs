//! Dioxus 0.7 web UI runtime bridge.
//!
//! Owns the `Agent`, the SQLite session pool, and an audit log. The
//! runtime exposes two surfaces:
//!
//! * `RuntimeHandle` — clonable, holds an `mpsc::Sender<UiCommand>` (for the
//!   UI to send commands) and a `broadcast::Receiver<UiEvent>` (for the UI
//!   to read events). The handle is `Send + Sync` and can live in any
//!   Dioxus signal/coroutine.
//! * `Runtime` — the actual owner. Built once at startup via
//!   `Runtime::start()`. Spawns a command-processing task that drains the
//!   command channel, dispatches each command to a handler, and emits a
//!   stream of `UiEvent`s back through a broadcast channel.
//!
//! All command handlers are isolated: an error in one command is logged
//! but never kills the task. Every command received and every event
//! emitted is recorded to `tracing` and (where relevant) the in-memory
//! audit log.

use crate::agent::{Agent, ApprovalCallback, ApprovalDecision};
use crate::events::EventBus;
use crate::models::CancelToken;
use crate::session;
use crate::tools::ToolRegistry;
use crate::webui::commands::*;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

// =============================================================================
// Constants
// =============================================================================

/// Broadcast channel buffer (events sent to UI subscribers).
const BROADCAST_CAPACITY: usize = 256;
/// mpsc channel buffer for in-flight commands queued by the UI.
const CMD_CHANNEL_CAPACITY: usize = 256;
/// Cap on the in-memory audit log before old entries are evicted FIFO.
const AUDIT_LOG_CAPACITY: usize = 2_000;
/// Timeout for waiting on a user approval response (5 minutes).
const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

// =============================================================================
// WebuiConfig
// =============================================================================

/// Runtime-side configuration the web UI cares about.
///
/// Distinct from the legacy `Settings` type in `crate::config` — that one
/// is a flat env-loader for the CLI; this one is structured for
/// round-tripping through JSON (UI sends a patch, runtime merges).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebuiConfig {
    pub default_model: String,
    pub default_provider: String,
    pub database_url: Option<String>,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub max_iterations: u32,
    pub temperature: f32,
    pub allow_write: bool,
}

impl Default for WebuiConfig {
    fn default() -> Self {
        Self {
            default_model: "llama-3.1-8b-instant".into(),
            default_provider: "groq".into(),
            database_url: None,
            embedding_provider: "nvidia".into(),
            embedding_model: "nvidia/llama-nemotron-embed-1b-v2".into(),
            max_iterations: 8,
            temperature: 0.3,
            allow_write: false,
        }
    }
}

impl WebuiConfig {
    /// Build a config from the process environment with sensible fallbacks.
    pub fn load() -> Self {
        Self {
            default_model: std::env::var("LLM_MODEL").unwrap_or_else(|_| "llama-3.1-8b-instant".into()),
            default_provider: std::env::var("LLM_DEFAULT_PROVIDER").unwrap_or_else(|_| "groq".into()),
            database_url: std::env::var("DATABASE_URL").ok(),
            embedding_provider: std::env::var("EMBEDDING_PROVIDER").unwrap_or_else(|_| "nvidia".into()),
            embedding_model: std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "nvidia/llama-nemotron-embed-1b-v2".into()),
            max_iterations: parse_env_u32("VOLT_MAX_ITERATIONS", 8),
            temperature: parse_env_f32("VOLT_TEMPERATURE", 0.3),
            allow_write: parse_env_bool("VOLT_ALLOW_WRITE", false),
        }
    }

    /// Apply a JSON Merge Patch to this config in place. Unknown fields
    /// are silently ignored (forward-compatible).
    pub fn merge_patch(&mut self, patch: Value) {
        if let Some(v) = patch.get("default_model").and_then(Value::as_str) {
            self.default_model = v.to_string();
        }
        if let Some(v) = patch.get("default_provider").and_then(Value::as_str) {
            self.default_provider = v.to_string();
        }
        if let Some(v) = patch.get("database_url") {
            self.database_url = v.as_str().map(str::to_string);
        }
        if let Some(v) = patch.get("embedding_provider").and_then(Value::as_str) {
            self.embedding_provider = v.to_string();
        }
        if let Some(v) = patch.get("embedding_model").and_then(Value::as_str) {
            self.embedding_model = v.to_string();
        }
        if let Some(v) = patch.get("max_iterations").and_then(Value::as_u64) {
            self.max_iterations = v as u32;
        }
        if let Some(v) = patch.get("temperature").and_then(Value::as_f64) {
            self.temperature = v as f32;
        }
        if let Some(v) = patch.get("allow_write").and_then(Value::as_bool) {
            self.allow_write = v;
        }
    }
}

fn parse_env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn parse_env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn parse_env_bool(key: &str, default: bool) -> bool {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// =============================================================================
// Runtime
// =============================================================================

/// The actual runtime — owns the Agent, the SQLite pool, and the audit log.
///
/// Constructed once at startup via [`Runtime::start`]. The struct is held
/// inside an `Arc` by the command-loop task; once the task exits, the
/// runtime is dropped.
pub struct Runtime {
    agent: Arc<Agent>,
    sqlite_pool: Option<SqlitePool>,
    config: Arc<RwLock<WebuiConfig>>,
    /// Command receiver. Taken once at startup and moved into the
    /// command-processing task; left as `None` thereafter.
    cmd_rx: Mutex<Option<mpsc::Receiver<UiCommand>>>,
    /// Internal single-producer event channel; the background emitter
    /// task drains it into the broadcast channel.
    internal_event_tx: mpsc::Sender<UiEvent>,
    /// Public broadcast channel that all handles subscribe to.
    event_tx: broadcast::Sender<UiEvent>,
    /// Atomic flag the cancel-chat command flips. Mirrored to the
    /// agent's `CancelToken` so the agent loop sees the signal too.
    cancel: Arc<AtomicBool>,
    /// Pending approval requests, keyed by `request_id`. The agent loop
    /// awaits the matching `oneshot` when the user decides.
    pending_approvals: Arc<Mutex<HashMap<Uuid, oneshot::Sender<ApprovalDecision>>>>,
    /// Append-only audit log of all commands and major events.
    audit_log: Arc<Mutex<Vec<AuditEntry>>>,
    /// Tool registry — kept on the runtime so the UI can list tools and
    /// the doctor handler can report on it.
    tools: Arc<ToolRegistry>,
    /// Cooperative cancellation token shared with the agent.
    cancel_token: CancelToken,
    /// Event bus the agent publishes `ToolExecuted` events into. The
    /// runtime subscribes a receiver for chat tool-call streaming.
    event_bus: EventBus,
    /// Currently active session ID (the one the next Chat command will
    /// use if none is specified). Lets the UI send follow-up turns
    /// without re-sending the UUID.
    active_session: Arc<Mutex<Option<Uuid>>>,
    /// Capability manager from the agent. Used by `execute_gated`.
    capability_manager: Arc<crate::capability::CapabilityManager>,
}

impl Runtime {
    // -------------------------------------------------------------------------
    // Construction
    // -------------------------------------------------------------------------

    /// Boot the runtime. Loads config, opens the SQLite session DB,
    /// builds the tool registry + embedder, instantiates the agent with
    /// streaming and approval callbacks wired in, and spawns the
    /// command-processing task. Returns a clonable handle for the UI.
    pub async fn start() -> anyhow::Result<RuntimeHandle> {
        // 1) Tracing  ~/.volt/logs/webui.log
        // Try to init; if a global subscriber is already set, that's fine.
        let log_dir = crate::config::volt_home().join("logs");
        match crate::telemetry::init_otel_for_tui("webui", &log_dir) {
            Ok(_) => {}
            Err(e) => {
                // Already-initialized is not fatal; only log other errors.
                let msg = e.to_string();
                if !msg.contains("already been set") {
                    eprintln!("[webui] failed to init tracing: {}", e);
                }
            }
        }

        // 2) Pick up .env so DATABASE_URL / GROQ_API_KEY etc. are visible.
        let _ = dotenvy::dotenv();

        // 3) Load the UI-facing config.
        let config = WebuiConfig::load();

        // 4) Open the SQLite sessions DB. Failure is non-fatal — the UI
        //    can still list tools, run doctor, etc. without a DB.
        let home = crate::config::volt_home();
        let _ = std::fs::create_dir_all(&home);
        let db_path = home.join("volt_sessions.db");
        let sqlite_pool = match session::open_sessions(&db_path).await {
            Ok(pool) => Some(pool),
            Err(e) => {
                tracing::warn!(
                    "[webui] failed to open sessions DB at {}: {}",
                    db_path.display(),
                    e
                );
                None
            }
        };

        // 5) Build the tool registry + embedder.
        let embedder = crate::embedding::EmbeddingClient::new_smart().await;
        let tools = crate::tools::setup_tools(
            Some(&embedder),
            config.database_url.as_deref(),
        )
        .await;

        // 6) Resolve an LLM provider. `build_provider` falls back to a
        //    generic LLM_API_KEY if the model is unknown, so this is
        //    safe even with no env vars.
        let model = config.default_model.clone();
        let (provider, _provider_kind) =
            crate::orchestrator::build_provider(&model, "volt-webui");

        // 7) Channels
        let (cmd_tx, cmd_rx) = mpsc::channel::<UiCommand>(CMD_CHANNEL_CAPACITY);
        let (internal_event_tx, mut internal_event_rx) =
            mpsc::channel::<UiEvent>(CMD_CHANNEL_CAPACITY);
        let (event_tx, _) = broadcast::channel::<UiEvent>(BROADCAST_CAPACITY);

        // 8) Shared state used by the approval callback
        let pending_approvals: Arc<Mutex<HashMap<Uuid, oneshot::Sender<ApprovalDecision>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let audit_log: Arc<Mutex<Vec<AuditEntry>>> = Arc::new(Mutex::new(Vec::new()));

        // 9) Streaming token callback — fires for every token the
        //    provider emits, regardless of the chat's session ID.
        let stream_tx = internal_event_tx.clone();
        let on_token: crate::llm::provider::TokenCallback = Arc::new(move |token: &str| {
            // Best-effort send; drop on full channel.
            let _ = stream_tx.try_send(UiEvent::ChatChunk {
                content: token.to_string(),
            });
        });

        // 10) Approval callback — sends an `ApprovalRequest` to the UI
        //     and waits up to 5 minutes for the user to decide. Defaults
        //     to Deny on timeout so an unattended UI never auto-runs a
        //     privileged tool.
        let approval_pending = pending_approvals.clone();
        let approval_event_tx = internal_event_tx.clone();
        let approval_fn: ApprovalCallback = Arc::new(
            move |tool: &str, args: &Value| -> BoxFuture<'static, ApprovalDecision> {
                let pending = approval_pending.clone();
                let tx = approval_event_tx.clone();
                let tool = tool.to_string();
                let args = args.clone();
                Box::pin(async move {
                    let request_id = Uuid::new_v4();
                    let (decision_tx, decision_rx) = oneshot::channel::<ApprovalDecision>();
                    if let Ok(mut g) = pending.lock() {
                        g.insert(request_id, decision_tx);
                    } else {
                        tracing::error!("[webui] pending_approvals poisoned");
                    }
                    let _ = tx.try_send(UiEvent::ApprovalRequest {
                        request_id,
                        tool_name: tool,
                        args,
                    });
                    match tokio::time::timeout(APPROVAL_TIMEOUT, decision_rx).await {
                        Ok(Ok(decision)) => decision,
                        _ => {
                            // UI never responded (timeout, dropped oneshot).
                            // Best to deny than to silently run an unapproved
                            // tool. Also clean up the entry so the map
                            // doesn't grow unbounded.
                            if let Ok(mut g) = pending.lock() {
                                g.remove(&request_id);
                            }
                            ApprovalDecision::Deny
                        }
                    }
                })
            },
        );

        // 11) Build the agent
        let cancel_token = CancelToken::new();
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let event_bus = EventBus::new();
        let agent_config = crate::models::AgentConfig {
            name: "volt-webui".into(),
            model: model.clone(),
            provider: config.default_provider.clone(),
            system_prompt: None,
            max_iterations: config.max_iterations,
            temperature: config.temperature,
            toolsets: vec!["builtin".into()],
            hidden: true,
            allow_all: false,
            enabled_context_kinds: crate::models::default_context_kinds(),
            essential_tools: crate::models::default_essential_tools(),
            context_kind_quotas: Default::default(),
            use_mtp: false,
            use_cot: false,
            allow_write: config.allow_write,
            framework: None,
            model_variant: None,
            quantization: None,
            format_dialect: Default::default(),
            quirks: vec![],
            strict_mode: false,
            max_tools_per_turn: None,
            blueprint_path: None,
        };
        let mut agent = Agent::new(agent_config, provider, tools.clone()).await;
        agent = agent
            .with_workspace(workspace)
            .with_cancel(cancel_token.clone())
            .with_event_bus(event_bus.clone())
            .with_approval(approval_fn)
            .with_stream(on_token);
        if let Some(ref pool) = sqlite_pool {
            // Bind a fresh session id so the agent has somewhere to save
            // messages. The actual session row is created lazily by
            // `handle_chat` (the agent upserts via the session_id
            // column on the messages table).
            agent = agent.with_session(Uuid::new_v4(), pool.clone());
        }
        let capability_manager = agent_capability_manager(&agent);

        // 12) Construct the runtime
        let runtime = Runtime {
            agent: Arc::new(agent),
            sqlite_pool,
            config: Arc::new(RwLock::new(config)),
            cmd_rx: Mutex::new(Some(cmd_rx)),
            internal_event_tx,
            event_tx: event_tx.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
            pending_approvals,
            audit_log,
            tools,
            cancel_token,
            event_bus,
            active_session: Arc::new(Mutex::new(None)),
            capability_manager,
        };
        let runtime = Arc::new(runtime);

        // 13) Spawn the background event-emitter task — drains the
        //     internal mpsc and fans out to broadcast. The handle's
        //     `subscribe()` is the public entry point for new consumers.
        let event_tx_for_emitter = event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = internal_event_rx.recv().await {
                let _ = event_tx_for_emitter.send(event);
            }
        });

        // 14) Spawn the command-processing task. Holds the only strong
        //     `Arc<Runtime>` reference (alongside the implicit strong
        //     ref on `event_tx` clones — but those don't keep `Runtime`
        //     alive because the broadcast channel is held via `event_tx`
        //     on the runtime itself, not on the struct).
        let cmd_rx = runtime
            .cmd_rx
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .ok_or_else(|| anyhow::anyhow!("command receiver already taken"))?;
        let runtime_for_task = runtime.clone();
        tokio::spawn(async move {
            Self::command_loop(runtime_for_task, cmd_rx).await;
        });

        tracing::info!("[webui] runtime started");
        Ok(RuntimeHandle::new(runtime, cmd_tx))
    }

    // -------------------------------------------------------------------------
    // Command loop
    // -------------------------------------------------------------------------

    async fn command_loop(
        runtime: Arc<Runtime>,
        mut cmd_rx: mpsc::Receiver<UiCommand>,
    ) {
        while let Some(cmd) = cmd_rx.recv().await {
            tracing::info!(
                target: "webui.cmd",
                "received command: {}",
                command_short(&cmd)
            );
            // `catch_unwind` + `AssertUnwindSafe` so a panic in one
            // handler doesn't kill the loop. Most handlers return
            // `Result`-shaped errors via `UiEvent::Error`.
            let rt = runtime.clone();
            let outcome = futures::FutureExt::catch_unwind(
                std::panic::AssertUnwindSafe(async move {
                    rt.process_command(cmd).await
                }),
            )
            .await;
            if let Err(e) = outcome {
                tracing::error!("[webui] command handler panicked: {:?}", e);
                let entry = AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: "agent".into(),
                    action: "chat".into(),
                    target: "panic".into(),
                    result: "error".into(),
                    detail: json!({ "panic": format!("{:?}", e) }),
                    session_id: None,
                };
                runtime.audit_log.lock().ok().map(|mut g| g.push(entry));
            }
        }
        tracing::info!("[webui] command loop exited");
    }

    // -------------------------------------------------------------------------
    // Command dispatch
    // -------------------------------------------------------------------------

    async fn process_command(&self, cmd: UiCommand) {
        match cmd {
            UiCommand::Ping => self.handle_ping().await,
            UiCommand::Chat { session_id, input } => {
                self.handle_chat(session_id, input).await
            }
            UiCommand::CancelChat => self.handle_cancel_chat().await,
            UiCommand::ListTools => self.handle_list_tools().await,
            UiCommand::ExecuteTool { name, args } => {
                self.handle_execute_tool(name, args).await
            }
            UiCommand::ListSessions => self.handle_list_sessions().await,
            UiCommand::LoadSession { id } => self.handle_load_session(id).await,
            UiCommand::CreateSession { name } => {
                self.handle_create_session(name).await
            }
            UiCommand::ForkSession { id } => self.handle_fork_session(id).await,
            UiCommand::DeleteSession { id } => self.handle_delete_session(id).await,
            UiCommand::ListModels => self.handle_list_models().await,
            UiCommand::GetConfig => self.handle_get_config().await,
            UiCommand::UpdateConfig { patch } => {
                self.handle_update_config(patch).await
            }
            UiCommand::RunDoctor => self.handle_run_doctor().await,
            UiCommand::ListWorktrees => self.handle_list_worktrees().await,
            UiCommand::WorktreeStatus { branch } => {
                self.handle_worktree_status(branch).await
            }
            UiCommand::WorktreeMerge { branch } => {
                self.handle_worktree_merge(branch).await
            }
            UiCommand::WorktreeClean { branch } => {
                self.handle_worktree_clean(branch).await
            }
            UiCommand::ListWorkflows => self.handle_list_workflows().await,
            UiCommand::RunWorkflow {
                pattern,
                agents,
                tasks,
                allow,
            } => {
                self.handle_run_workflow(pattern, agents, tasks, allow).await
            }
            UiCommand::ListJobs => self.handle_list_jobs().await,
            UiCommand::ListRoutines => self.handle_list_routines().await,
            UiCommand::ListSkills => self.handle_list_skills().await,
            UiCommand::SearchCatalogSkills { query } => {
                self.handle_search_catalog_skills(query).await
            }
            UiCommand::InstallSkill { name } => self.handle_install_skill(name).await,
            UiCommand::ImportSkill { path } => self.handle_import_skill(path).await,
            UiCommand::ListMcpServers => self.handle_list_mcp_servers().await,
            UiCommand::GetAuditLog { limit } => {
                self.handle_get_audit_log(limit).await
            }
            UiCommand::ApprovalResponse {
                request_id,
                allow,
                allow_session,
            } => {
                self.handle_approval_response(request_id, allow, allow_session)
                    .await
            }
        }
    }

    // -------------------------------------------------------------------------
    // Handlers
    // -------------------------------------------------------------------------

    async fn handle_ping(&self) {
        self.emit(UiEvent::Pong);
    }

    async fn handle_chat(&self, session_id: Option<Uuid>, input: String) {
        let started = std::time::Instant::now();
        let session_id = match session_id {
            Some(id) => id,
            None => {
                // No session supplied — mint one and persist it so the
                // agent's save logic has a row to write into.
                let new_id = Uuid::new_v4();
                if let Some(ref pool) = self.sqlite_pool {
                    let s = crate::models::Session {
                        id: new_id,
                        agent_name: "volt-webui".into(),
                        title: "untitled".into(),
                        message_count: 0,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    };
                    if let Err(e) = session::create_session(pool, &s).await {
                        tracing::warn!("[webui] failed to create session: {}", e);
                    }
                }
                if let Ok(mut g) = self.active_session.lock() {
                    *g = Some(new_id);
                }
                new_id
            }
        };
        if let Some(ref pool) = self.sqlite_pool {
            // Bind the session to the agent for this turn. The agent
            // already has a session_id from start(); we re-bind by
            // writing into its state.
            {
                let mut state = self.agent.state().lock().await;
                state.session_id = session_id;
            }
            if let Ok(msgs) = session::load_messages(pool, session_id).await {
                let mut state = self.agent.state().lock().await;
                for m in msgs {
                    let already = state.messages.iter().any(|existing| {
                        existing.id == m.id
                            || (existing.role == m.role
                                && existing.content == m.content)
                    });
                    if !already {
                        state.messages.push(m);
                    }
                }
            }
        }
        if let Ok(mut g) = self.active_session.lock() {
            *g = Some(session_id);
        }

        // Forward tool-executed events from the agent's event bus to
        // the UI as `ToolCallEnd`. Subscribed for the lifetime of this
        // chat only.
        let mut bus_rx = self.event_bus.subscribe();
        let tool_event_tx = self.internal_event_tx.clone();
        let tool_event_handle = tokio::spawn(async move {
            while let Ok(ev) = bus_rx.recv().await {
                if let crate::events::Event::ToolExecuted { tool_name, success } = ev {
                    let _ = tool_event_tx.try_send(UiEvent::ToolCallEnd {
                        id: Uuid::new_v4().to_string(),
                        result: json!({ "tool_name": tool_name, "success": success }),
                        error: if success {
                            None
                        } else {
                            Some("tool reported failure".into())
                        },
                    });
                }
            }
        });

        // Emit ChatStarted
        self.emit(UiEvent::ChatStarted { session_id });
        self.log_audit(AuditEntry {
            id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            actor: "user".into(),
            action: "chat".into(),
            target: session_id.to_string(),
            result: "ok".into(),
            detail: json!({ "input_chars": input.len() }),
            session_id: Some(session_id),
        });

        // Reset cancellation flag for this turn.
        self.cancel.store(false, Ordering::SeqCst);

        // Run the agent. The on_token callback (set in start()) already
        // streams `ChatChunk` events into the internal channel.
        let result = self.agent.run(&input).await;

        // Drain task is no longer needed.
        tool_event_handle.abort();

        let duration_ms = started.elapsed().as_millis() as u64;
        let state_snapshot = self.agent.state().lock().await.clone();
        let tokens_used = (state_snapshot.total_prompt_tokens
            + state_snapshot.total_completion_tokens) as u32;

        match result {
            Ok(final_text) => {
                self.emit(UiEvent::ChatComplete {
                    final_text,
                    tokens_used,
                    duration_ms,
                });
                self.log_audit(AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: "agent".into(),
                    action: "chat".into(),
                    target: session_id.to_string(),
                    result: "ok".into(),
                    detail: json!({
                        "tokens_used": tokens_used,
                        "duration_ms": duration_ms,
                    }),
                    session_id: Some(session_id),
                });
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("cancelled") {
                    self.emit(UiEvent::ChatCancelled);
                } else {
                    self.emit(UiEvent::ChatError { message: msg.clone() });
                }
                self.log_audit(AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: "agent".into(),
                    action: "chat".into(),
                    target: session_id.to_string(),
                    result: "error".into(),
                    detail: json!({ "error": msg }),
                    session_id: Some(session_id),
                });
            }
        }
    }

    async fn handle_cancel_chat(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.cancel_token.cancel();
        self.emit(UiEvent::ChatCancelled);
    }

    async fn handle_list_tools(&self) {
        let defs = self.tools.get_definitions().await;
        let mut infos: Vec<ToolInfo> = Vec::with_capacity(defs.len());
        for d in defs.iter() {
            let perm = self.tools.get_permission(&d.name).await;
            infos.push(ToolInfo {
                name: d.name.clone(),
                description: d.description.clone(),
                category: d.category.clone(),
                permission: permission_label(perm),
                schema: d.input_schema.clone(),
                enabled: !matches!(perm, crate::models::PermissionLevel::Blocked),
            });
        }
        self.emit(UiEvent::ToolsListed { tools: infos });
    }

    async fn handle_execute_tool(&self, name: String, args: Value) {
        let result = self
            .tools
            .execute_gated(&name, &args, &self.capability_manager)
            .await;
        let (result_val, error) = match result {
            Ok(r) => (
                json!({
                    "output": r.output,
                    "success": r.success,
                    "duration_ms": r.duration_ms,
                }),
                r.error,
            ),
            Err(e) => (json!({}), Some(e.to_string())),
        };
        self.emit(UiEvent::ToolCallEnd {
            id: Uuid::new_v4().to_string(),
            result: result_val,
            error,
        });
        self.log_audit(AuditEntry {
            id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            actor: "user".into(),
            action: "tool_call".into(),
            target: name,
            result: "ok".into(),
            detail: json!({ "args": args }),
            session_id: None,
        });
    }

    async fn handle_list_sessions(&self) {
        let Some(pool) = self.sqlite_pool.clone() else {
            self.emit(UiEvent::SessionsListed { sessions: vec![] });
            return;
        };
        match session::list_sessions(&pool, 200).await {
            Ok(rows) => {
                let infos: Vec<SessionInfo> = rows
                    .into_iter()
                    .map(|s| SessionInfo {
                        id: s.id,
                        name: s.title,
                        created_at: s.created_at,
                        updated_at: s.updated_at,
                        message_count: s.message_count,
                        tokens_used: 0,
                    })
                    .collect();
                self.emit(UiEvent::SessionsListed { sessions: infos });
            }
            Err(e) => self.emit_error("list_sessions", e),
        }
    }

    async fn handle_load_session(&self, id: Uuid) {
        let Some(pool) = self.sqlite_pool.clone() else {
            self.emit(UiEvent::SessionLoaded { id, messages: vec![] });
            return;
        };
        match session::load_messages(&pool, id).await {
            Ok(msgs) => {
                let ui_msgs: Vec<ChatMessage> = msgs
                    .into_iter()
                    .map(|m| ChatMessage {
                        id: m.id,
                        role: m.role,
                        content: m.content.as_str().to_string(),
                        tool_calls: m
                            .tool_calls
                            .as_ref()
                            .map(|tcs| {
                                tcs.iter()
                                    .map(|tc| ToolCallInfo {
                                        id: tc.id.clone(),
                                        name: tc.name.clone(),
                                        args: tc.arguments.clone(),
                                        result: None,
                                        error: None,
                                        duration_ms: None,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        timestamp: m.created_at,
                    })
                    .collect();
                self.emit(UiEvent::SessionLoaded { id, messages: ui_msgs });
            }
            Err(e) => self.emit_error("load_session", e),
        }
    }

    async fn handle_create_session(&self, name: String) {
        let Some(pool) = self.sqlite_pool.clone() else {
            self.emit(UiEvent::Error {
                source: "create_session".into(),
                message: "no SQLite database available".into(),
            });
            return;
        };
        let id = Uuid::new_v4();
        let s = crate::models::Session {
            id,
            agent_name: "volt-webui".into(),
            title: name,
            message_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        match session::create_session(&pool, &s).await {
            Ok(()) => self.emit(UiEvent::SessionCreated { id }),
            Err(e) => self.emit_error("create_session", e),
        }
    }

    async fn handle_fork_session(&self, id: Uuid) {
        let Some(pool) = self.sqlite_pool.clone() else {
            self.emit(UiEvent::Error {
                source: "fork_session".into(),
                message: "no SQLite database available".into(),
            });
            return;
        };
        match session::fork_session(&pool, id, usize::MAX, None).await {
            Ok(new_id) => self.emit(UiEvent::SessionCreated { id: new_id }),
            Err(e) => self.emit_error("fork_session", e),
        }
    }

    async fn handle_delete_session(&self, id: Uuid) {
        let Some(pool) = self.sqlite_pool.clone() else {
            self.emit(UiEvent::SessionDeleted { id });
            return;
        };
        // Drop messages first (foreign key), then the row.
        let r1 = session::delete_session_messages(&pool, id).await;
        let r2 = sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id.to_string())
            .execute(&pool)
            .await;
        match (r1, r2) {
            (Ok(_), Ok(_)) => self.emit(UiEvent::SessionDeleted { id }),
            (Err(e), _) => self.emit_error("delete_session", e),
            (_, Err(e)) => self.emit_error("delete_session", e),
        }
    }

    async fn handle_list_models(&self) {
        let mut models: Vec<ModelInfo> = Vec::new();
        for provider in crate::agent::router::get_active_providers() {
            let has_key = match provider.as_str() {
                "groq" => std::env::var("GROQ_API_KEY").is_ok(),
                "nvidia" => std::env::var("NVIDIA_API_KEY").is_ok(),
                "openai" => std::env::var("OPENAI_API_KEY").is_ok(),
                "anthropic" => std::env::var("ANTHROPIC_API_KEY").is_ok(),
                "ollama" => {
                    std::env::var("OLLAMA_HOST").is_ok()
                        || std::env::var("OLLAMA_API_KEY").is_ok()
                }
                _ => true,
            };
            let (display_name, supports_tools, supports_vision) = match provider.as_str() {
                "groq" => ("llama-3.1-8b-instant", true, false),
                "nvidia" => ("meta/llama-3.1-70b-instruct", true, false),
                "openai" => ("gpt-4o", true, true),
                "anthropic" => ("claude-sonnet-4-5", true, true),
                "ollama" => ("phi4-mini:3.8b", false, false),
                "llamacpp" => ("llama-3-8b-local", false, false),
                "litertlm" => ("gemma-4-e2b", false, false),
                _ => (provider.as_str(), true, false),
            };
            models.push(ModelInfo {
                id: display_name.to_string(),
                provider: provider.clone(),
                display_name: display_name.to_string(),
                context_window: 8192,
                supports_tools,
                supports_vision,
                available: has_key,
            });
        }
        for (_, spec) in crate::agent::model_registry::MODEL_REGISTRY.iter() {
            models.push(ModelInfo {
                id: spec.framework.to_string(),
                provider: "local".into(),
                display_name: spec.framework.to_string(),
                context_window: 4096,
                supports_tools: false,
                supports_vision: false,
                available: spec.binary_path.exists(),
            });
        }
        self.emit(UiEvent::ModelsListed { models });
    }

    async fn handle_get_config(&self) {
        let val = self
            .config
            .read()
            .ok()
            .and_then(|g| serde_json::to_value(g.clone()).ok())
            .unwrap_or(Value::Null);
        self.emit(UiEvent::ConfigLoaded { config: val });
    }

    async fn handle_update_config(&self, patch: Value) {
        if let Ok(mut g) = self.config.write() {
            g.merge_patch(patch);
        }
        self.emit(UiEvent::ConfigUpdated);
    }

    async fn handle_run_doctor(&self) {
        let report = build_doctor_report();
        self.emit(UiEvent::DoctorCompleted { report });
    }

    async fn handle_list_worktrees(&self) {
        let infos = worktree_list().await;
        self.emit(UiEvent::WorktreesListed { worktrees: infos });
    }

    async fn handle_worktree_status(&self, branch: String) {
        match worktree_diff_summary(&branch).await {
            Ok(summary) => self.emit(UiEvent::AuditLog {
                entries: vec![AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: "user".into(),
                    action: "tool_call".into(),
                    target: branch,
                    result: "ok".into(),
                    detail: json!({ "diff": summary }),
                    session_id: None,
                }],
            }),
            Err(e) => self.emit_error("worktree_status", e),
        }
    }

    async fn handle_worktree_merge(&self, branch: String) {
        match worktree_merge_back(&branch).await {
            Ok(msg) => {
                tracing::info!("[webui] merged {}: {}", branch, msg);
                self.emit(UiEvent::AuditLog {
                    entries: vec![AuditEntry {
                        id: Uuid::new_v4(),
                        timestamp: chrono::Utc::now(),
                        actor: "user".into(),
                        action: "tool_call".into(),
                        target: branch.clone(),
                        result: "ok".into(),
                        detail: json!({ "merge_output": msg }),
                        session_id: None,
                    }],
                });
            }
            Err(e) => self.emit_error("worktree_merge", e),
        }
    }

    async fn handle_worktree_clean(&self, branch: String) {
        match worktree_remove(&branch).await {
            Ok(()) => {
                tracing::info!("[webui] cleaned worktree branch {}", branch);
            }
            Err(e) => self.emit_error("worktree_clean", e),
        }
    }

    async fn handle_list_workflows(&self) {
        let workflows = vec![
            WorkflowInfo {
                name: "Parallel".into(),
                description: "Run N agents in parallel on the same input.".into(),
                pattern: "parallel".into(),
                agents: vec!["agent-a".into(), "agent-b".into()],
            },
            WorkflowInfo {
                name: "Pipeline".into(),
                description: "Chain agents: output of one feeds the next.".into(),
                pattern: "pipeline".into(),
                agents: vec!["agent-a".into(), "agent-b".into()],
            },
            WorkflowInfo {
                name: "Supervisor".into(),
                description: "One supervisor agent fans out tasks to workers.".into(),
                pattern: "supervisor".into(),
                agents: vec!["supervisor".into(), "worker".into()],
            },
            WorkflowInfo {
                name: "DAG".into(),
                description: "Arbitrary DAG with {input}/{node_id} templating.".into(),
                pattern: "dag".into(),
                agents: vec![],
            },
        ];
        self.emit(UiEvent::WorkflowsListed { workflows });
    }

    async fn handle_run_workflow(
        &self,
        pattern: String,
        agents: Option<String>,
        tasks: Option<String>,
        allow: bool,
    ) {
        if let Err(e) = crate::commands::workflow::run(
            pattern.clone(),
            agents,
            tasks,
            None,
            None,
            allow,
        )
        .await
        {
            self.emit_error("run_workflow", e);
            return;
        }
        self.emit(UiEvent::WorkflowStarted {
            pattern,
            run_id: Uuid::new_v4().to_string(),
        });
    }

    async fn handle_list_jobs(&self) {
        // Jobs live in Postgres; the webui is sqlite-only.
        self.emit(UiEvent::JobsListed { jobs: vec![] });
    }

    async fn handle_list_routines(&self) {
        // Routines live in Postgres; the webui is sqlite-only.
        self.emit(UiEvent::RoutinesListed { routines: vec![] });
    }

    async fn handle_list_skills(&self) {
        let mut skills: Vec<SkillInfo> = Vec::new();
        if let Some(home) = dirs_home() {
            let skills_dir = home.join("skills");
            if let Ok(read) = std::fs::read_dir(&skills_dir) {
                for entry in read.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        continue;
                    }
                    if let Ok(manifest) = crate::skills::parse_skill_manifest(&path) {
                        skills.push(SkillInfo {
                            name: manifest.name,
                            description: manifest.description,
                            version: manifest.version,
                            installed_at: chrono::Utc::now(),
                            source: "local".into(),
                        });
                    }
                }
            }
        }
        self.emit(UiEvent::SkillsListed { skills });
    }

    async fn handle_search_catalog_skills(&self, query: String) {
        let skills = match crate::skills::catalog::fetch_catalog(None).await {
            Ok(catalog) => {
                let matches = crate::skills::catalog::search_catalog(&catalog, &query);
                matches
                    .into_iter()
                    .map(|e| CatalogSkillInfo {
                        name: e.name.clone(),
                        description: e.description.clone(),
                        author: e.author.clone(),
                        downloads: 0,
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("[webui] catalog fetch failed: {}", e);
                Vec::new()
            }
        };
        self.emit(UiEvent::CatalogResults { query, skills });
    }

    async fn handle_install_skill(&self, name: String) {
        // Without a pg pool we cannot compile a skill into the
        // database. Emit a hint error so the UI can surface it.
        tracing::info!("[webui] install_skill({}) — pg pool required", name);
        self.emit(UiEvent::Error {
            source: "install_skill".into(),
            message: "skill installation requires a Postgres connection; the webui is sqlite-only"
                .into(),
        });
    }

    async fn handle_import_skill(&self, path: String) {
        let p = std::path::PathBuf::from(&path);
        let content = match std::fs::read_to_string(&p) {
            Ok(c) => c,
            Err(e) => {
                self.emit_error("import_skill", e);
                return;
            }
        };
        let fmt = crate::skills::importer::detect_format(&p, &content);
        let converted =
            crate::skills::importer::convert_to_volt_skill(&p, &content, &fmt, None);
        let Some(home) = dirs_home() else { return };
        let skills_dir = home.join("skills");
        if let Err(e) = std::fs::create_dir_all(&skills_dir) {
            self.emit_error("import_skill", e);
            return;
        }
        let target = skills_dir.join(p.file_name().unwrap_or_default());
        match std::fs::write(&target, converted) {
            Ok(()) => {
                if let Some(name) = target.file_stem().and_then(|n| n.to_str()) {
                    self.emit(UiEvent::SkillInstalled {
                        name: name.to_string(),
                    });
                }
            }
            Err(e) => self.emit_error("import_skill", e),
        }
    }

    async fn handle_list_mcp_servers(&self) {
        // The webui has no MCP server registry of its own; the agent's
        // tools may have been populated by an MCP client at startup.
        self.emit(UiEvent::McpServersListed { servers: vec![] });
    }

    async fn handle_get_audit_log(&self, limit: u32) {
        let entries = self.audit_log.lock().ok().map(|g| {
            let n = (limit as usize).min(g.len());
            let start = g.len().saturating_sub(n);
            // Most-recent-first.
            g[start..].iter().rev().cloned().collect::<Vec<_>>()
        });
        let entries = entries.unwrap_or_default();
        self.emit(UiEvent::AuditLog { entries });
    }

    async fn handle_approval_response(
        &self,
        request_id: Uuid,
        allow: bool,
        allow_session: bool,
    ) {
        let decision = if !allow {
            ApprovalDecision::Deny
        } else if allow_session {
            ApprovalDecision::AllowSession
        } else {
            ApprovalDecision::AllowOnce
        };
        let sender = self
            .pending_approvals
            .lock()
            .ok()
            .and_then(|mut g| g.remove(&request_id));
        match sender {
            Some(tx) => {
                let _ = tx.send(decision);
                self.log_audit(AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: "user".into(),
                    action: "approval".into(),
                    target: request_id.to_string(),
                    result: if allow { "ok" } else { "denied" }.into(),
                    detail: json!({ "allow_session": allow_session }),
                    session_id: None,
                });
            }
            None => {
                tracing::warn!(
                    "[webui] approval response for unknown request_id {}",
                    request_id
                );
            }
        }
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Push a `UiEvent` into the internal channel — the background
    /// emitter forwards it to all broadcast subscribers.
    fn emit(&self, event: UiEvent) {
        tracing::info!(
            target: "webui.event",
            "emit: {}",
            event_short(&event)
        );
        if let Err(e) = self.internal_event_tx.try_send(event) {
            tracing::warn!("[webui] internal event channel full: {}", e);
        }
    }

    /// Emit a generic `UiEvent::Error` from a `source: &'static str`
    /// label and any `Display`-able error.
    fn emit_error<E: std::fmt::Display>(&self, source: &'static str, e: E) {
        self.emit(UiEvent::Error {
            source: source.to_string(),
            message: e.to_string(),
        });
    }

    /// Append an entry to the audit log and emit it to the tracing
    /// subscriber. The log is bounded — oldest entries are evicted
    /// FIFO.
    fn log_audit(&self, entry: AuditEntry) {
        tracing::info!(
            target: "webui.audit",
            "actor={} action={} target={} result={}",
            entry.actor,
            entry.action,
            entry.target,
            entry.result
        );
        if let Ok(mut log) = self.audit_log.lock() {
            log.push(entry);
            if log.len() > AUDIT_LOG_CAPACITY {
                let excess = log.len() - AUDIT_LOG_CAPACITY;
                log.drain(0..excess);
            }
        }
    }
}

// =============================================================================
// RuntimeHandle
// =============================================================================

/// A clonable, `Send + Sync` handle to the runtime. UI components hold a
/// handle and use it to send commands and subscribe to events.
#[derive(Clone)]
pub struct RuntimeHandle {
    cmd_tx: mpsc::Sender<UiCommand>,
    /// Wrapped in a `Mutex` so multiple `try_recv` callers don't fight
    /// over the same `Receiver`. Subscribers who want a private stream
    /// should call [`subscribe`] instead.
    event_rx: Arc<Mutex<broadcast::Receiver<UiEvent>>>,
    /// Public broadcast sender — `subscribe` calls `event_tx.subscribe()`
    /// to mint a fresh receiver.
    event_tx: broadcast::Sender<UiEvent>,
}

impl std::fmt::Debug for RuntimeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHandle").finish_non_exhaustive()
    }
}

impl RuntimeHandle {
    /// Wrap an existing `Arc<Runtime>` in a handle. The runtime must
    /// have been started via `Runtime::start()` (otherwise no command
    /// receiver is listening). Does not spawn any new tasks.
    pub fn new(runtime: Arc<Runtime>, cmd_tx: mpsc::Sender<UiCommand>) -> Self {
        // Cheap clones of the broadcast sender; subscribe a fresh
        // receiver for the try_recv path.
        let event_tx = runtime.event_tx.clone();
        let event_rx = Arc::new(Mutex::new(event_tx.subscribe()));
        Self {
            cmd_tx,
            event_rx,
            event_tx,
        }
    }

    /// Send a command to the runtime. The command is enqueued on the
    /// internal mpsc channel and processed asynchronously by the
    /// command-loop task.
    pub async fn send(&self, cmd: UiCommand) -> Result<(), String> {
        self.cmd_tx.send(cmd).await.map_err(|e| e.to_string())
    }

    /// Try to receive the next event without blocking. Returns `None`
    /// if no event is available.
    pub fn try_recv(&self) -> Option<UiEvent> {
        let mut guard = self.event_rx.lock().ok()?;
        match guard.try_recv() {
            Ok(ev) => Some(ev),
            Err(_) => None,
        }
    }

    /// Mint a fresh broadcast receiver. Each subscriber gets its own
    /// independent stream of events.
    pub fn subscribe(&self) -> broadcast::Receiver<UiEvent> {
        self.event_tx.subscribe()
    }
}

// =============================================================================
// Free helpers
// =============================================================================

/// Return the volt home directory (`~/.volt/` on Unix,
/// `%APPDATA%\volt\` on Windows). Thin wrapper around
/// `crate::config::volt_home()` that gracefully returns `None` if the
/// path can't be determined.
fn dirs_home() -> Option<PathBuf> {
    Some(crate::config::volt_home())
}

/// Pull the agent's capability manager. Done in a free function so we
/// don't need to add a public accessor on `Agent` itself.
fn agent_capability_manager(
    agent: &Agent,
) -> Arc<crate::capability::CapabilityManager> {
    // The `capability_manager` field on `Agent` is private (pub(crate)).
    // We can reach it through the crate-internal `Arc::clone` since
    // we're in the same crate. Use a small helper.
    agent.capability_manager_inner()
}

/// Map a `PermissionLevel` to the string label the UI displays.
fn permission_label(perm: crate::models::PermissionLevel) -> String {
    match perm {
        crate::models::PermissionLevel::Allow => "allow".into(),
        crate::models::PermissionLevel::Prompt => "prompt".into(),
        crate::models::PermissionLevel::ReadOnly => "allow".into(),
        crate::models::PermissionLevel::Blocked => "deny".into(),
    }
}

/// Build a `DoctorReport` synchronously from the current environment.
/// Mirrors `volt doctor` but in DTO form so the UI can render it.
fn build_doctor_report() -> DoctorReport {
    let keys = [
        "GROQ_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "NVIDIA_API_KEY",
        "OLLAMA_API_KEY",
        "HF_TOKEN",
        "YOUCOM_API_KEY",
        "EMBEDDING_API_KEY",
        "LLM_API_KEY",
    ];
    let api_keys: Vec<ApiKeyStatus> = keys
        .iter()
        .map(|k| {
            let v = std::env::var(k).unwrap_or_default();
            let present = !v.is_empty();
            let masked = if v.len() > 4 {
                format!("…{}", &v[v.len() - 4..])
            } else {
                "(not set)".into()
            };
            ApiKeyStatus {
                name: (*k).to_string(),
                present,
                masked,
            }
        })
        .collect();

    let workspace_files: Vec<WorkspaceFileStatus> =
        ["AGENTS.md", "SOUL.md", "MEMORY.md", "USER.md"]
            .iter()
            .map(|n| {
                let p = std::path::Path::new(n);
                let present = p.exists();
                let bytes = p
                    .metadata()
                    .map(|m| m.len())
                    .unwrap_or(0);
                WorkspaceFileStatus {
                    name: (*n).to_string(),
                    present,
                    bytes,
                }
            })
            .collect();

    DoctorReport {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        rust_channel: std::env::var("RUSTUP_TOOLCHAIN")
            .unwrap_or_else(|_| "stable".into()),
        api_keys,
        database: if std::env::var("DATABASE_URL").is_ok() {
            "ok".into()
        } else {
            "not configured".into()
        },
        embedder_provider: std::env::var("EMBEDDING_PROVIDER")
            .unwrap_or_else(|_| "nvidia".into()),
        embedder_model: std::env::var("EMBEDDING_MODEL")
            .unwrap_or_else(|_| "nvidia/llama-nemotron-embed-1b-v2".into()),
        disk_free_gb: 0.0,
        permissions_default: "Prompt".into(),
        recent_failures: 0,
        workspace_files,
    }
}

/// Worktree helpers — wrap `WorktreeManager` so handlers stay terse.
async fn worktree_list() -> Vec<WorktreeInfo> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let repo_root = match crate::commands::worktree::WorktreeManager::detect_repo_root(&cwd).await {
        Ok(Some(r)) => r,
        _ => return Vec::new(),
    };
    let mgr = crate::commands::worktree::WorktreeManager::new(repo_root);
    match mgr.list().await {
        Ok(items) => items
            .into_iter()
            .map(|w| WorktreeInfo {
                branch: w.branch,
                path: w.path.to_string_lossy().to_string(),
                session_id: w.session_id.to_string(),
                created_at: chrono::Utc::now(),
                commits_ahead: 0,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

async fn worktree_diff_summary(branch: &str) -> anyhow::Result<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let repo_root = crate::commands::worktree::WorktreeManager::detect_repo_root(&cwd)
        .await?
        .ok_or_else(|| anyhow::anyhow!("not in a git repository"))?;
    let mgr = crate::commands::worktree::WorktreeManager::new(repo_root);
    mgr.diff_summary(branch).await.map_err(|e| anyhow::anyhow!(e.to_string()))
}

async fn worktree_merge_back(branch: &str) -> anyhow::Result<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let repo_root = crate::commands::worktree::WorktreeManager::detect_repo_root(&cwd)
        .await?
        .ok_or_else(|| anyhow::anyhow!("not in a git repository"))?;
    let mgr = crate::commands::worktree::WorktreeManager::new(repo_root);
    mgr.merge_back(branch).await.map_err(|e| anyhow::anyhow!(e.to_string()))
}

async fn worktree_remove(branch: &str) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let repo_root = crate::commands::worktree::WorktreeManager::detect_repo_root(&cwd)
        .await?
        .ok_or_else(|| anyhow::anyhow!("not in a git repository"))?;
    let mgr = crate::commands::worktree::WorktreeManager::new(repo_root);
    let infos = mgr.list().await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let path = infos
        .iter()
        .find(|w| w.branch == branch)
        .map(|w| w.path.clone())
        .ok_or_else(|| anyhow::anyhow!("worktree branch {} not found", branch))?;
    mgr.remove(&path, true)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Compact, log-friendly summary of a `UiCommand`.
fn command_short(cmd: &UiCommand) -> String {
    match cmd {
        UiCommand::Ping => "Ping".into(),
        UiCommand::Chat { session_id, .. } => match session_id {
            Some(id) => format!("Chat(session={})", id),
            None => "Chat(new)".into(),
        },
        UiCommand::CancelChat => "CancelChat".into(),
        UiCommand::ListTools => "ListTools".into(),
        UiCommand::ExecuteTool { name, .. } => format!("ExecuteTool({})", name),
        UiCommand::ListSessions => "ListSessions".into(),
        UiCommand::LoadSession { id } => format!("LoadSession({})", id),
        UiCommand::CreateSession { name } => format!("CreateSession({})", name),
        UiCommand::ForkSession { id } => format!("ForkSession({})", id),
        UiCommand::DeleteSession { id } => format!("DeleteSession({})", id),
        UiCommand::ListModels => "ListModels".into(),
        UiCommand::GetConfig => "GetConfig".into(),
        UiCommand::UpdateConfig { .. } => "UpdateConfig".into(),
        UiCommand::RunDoctor => "RunDoctor".into(),
        UiCommand::ListWorktrees => "ListWorktrees".into(),
        UiCommand::WorktreeStatus { branch } => format!("WorktreeStatus({})", branch),
        UiCommand::WorktreeMerge { branch } => format!("WorktreeMerge({})", branch),
        UiCommand::WorktreeClean { branch } => format!("WorktreeClean({})", branch),
        UiCommand::ListWorkflows => "ListWorkflows".into(),
        UiCommand::RunWorkflow { pattern, .. } => format!("RunWorkflow({})", pattern),
        UiCommand::ListJobs => "ListJobs".into(),
        UiCommand::ListRoutines => "ListRoutines".into(),
        UiCommand::ListSkills => "ListSkills".into(),
        UiCommand::SearchCatalogSkills { query } => format!("SearchCatalogSkills({})", query),
        UiCommand::InstallSkill { name } => format!("InstallSkill({})", name),
        UiCommand::ImportSkill { path } => format!("ImportSkill({})", path),
        UiCommand::ListMcpServers => "ListMcpServers".into(),
        UiCommand::GetAuditLog { limit } => format!("GetAuditLog({})", limit),
        UiCommand::ApprovalResponse { request_id, allow, .. } => {
            format!("ApprovalResponse({} allow={})", request_id, allow)
        }
    }
}

/// Compact, log-friendly summary of a `UiEvent`.
fn event_short(event: &UiEvent) -> String {
    match event {
        UiEvent::ChatStarted { session_id } => format!("ChatStarted({})", session_id),
        UiEvent::ChatChunk { content } => format!("ChatChunk({} chars)", content.len()),
        UiEvent::ToolCallStart { id, name, .. } => format!("ToolCallStart({}={})", id, name),
        UiEvent::ToolCallEnd { id, error, .. } => {
            if let Some(e) = error {
                format!("ToolCallEnd({} err={})", id, e)
            } else {
                format!("ToolCallEnd({})", id)
            }
        }
        UiEvent::ChatComplete { final_text, .. } => {
            format!("ChatComplete({} chars)", final_text.len())
        }
        UiEvent::ChatError { message } => format!("ChatError({})", message),
        UiEvent::ChatCancelled => "ChatCancelled".into(),
        UiEvent::ToolsListed { tools } => format!("ToolsListed({})", tools.len()),
        UiEvent::SessionsListed { sessions } => format!("SessionsListed({})", sessions.len()),
        UiEvent::SessionLoaded { id, messages } => {
            format!("SessionLoaded({} msg={})", id, messages.len())
        }
        UiEvent::SessionCreated { id } => format!("SessionCreated({})", id),
        UiEvent::SessionDeleted { id } => format!("SessionDeleted({})", id),
        UiEvent::ModelsListed { models } => format!("ModelsListed({})", models.len()),
        UiEvent::ConfigLoaded { .. } => "ConfigLoaded".into(),
        UiEvent::ConfigUpdated => "ConfigUpdated".into(),
        UiEvent::DoctorCompleted { .. } => "DoctorCompleted".into(),
        UiEvent::WorktreesListed { worktrees } => {
            format!("WorktreesListed({})", worktrees.len())
        }
        UiEvent::WorkflowsListed { workflows } => {
            format!("WorkflowsListed({})", workflows.len())
        }
        UiEvent::WorkflowStarted { pattern, .. } => format!("WorkflowStarted({})", pattern),
        UiEvent::JobsListed { jobs } => format!("JobsListed({})", jobs.len()),
        UiEvent::RoutinesListed { routines } => format!("RoutinesListed({})", routines.len()),
        UiEvent::SkillsListed { skills } => format!("SkillsListed({})", skills.len()),
        UiEvent::CatalogResults { query, skills } => {
            format!("CatalogResults({}={})", query, skills.len())
        }
        UiEvent::SkillInstalled { name } => format!("SkillInstalled({})", name),
        UiEvent::McpServersListed { servers } => {
            format!("McpServersListed({})", servers.len())
        }
        UiEvent::AuditLog { entries } => format!("AuditLog({})", entries.len()),
        UiEvent::ApprovalRequest { request_id, tool_name, .. } => {
            format!("ApprovalRequest({} for {})", request_id, tool_name)
        }
        UiEvent::Pong => "Pong".into(),
        UiEvent::Error { source, message } => format!("Error({}: {})", source, message),
    }
}

impl Agent {
    /// Crate-internal accessor for the agent's `capability_manager`
    /// field. Wraps the `Arc::clone` so callers can pass it to
    /// `ToolRegistry::execute_gated` without needing to make the
    /// field public.
    pub(crate) fn capability_manager_inner(&self) -> Arc<crate::capability::CapabilityManager> {
        self.capability_manager.clone()
    }
}
