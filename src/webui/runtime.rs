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
use sqlx::{PgPool, SqlitePool};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex as AsyncMutex};
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
            default_model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "llama-3.1-8b-instant".into()),
            default_provider: std::env::var("LLM_DEFAULT_PROVIDER")
                .unwrap_or_else(|_| "groq".into()),
            database_url: std::env::var("DATABASE_URL").ok(),
            embedding_provider: std::env::var("EMBEDDING_PROVIDER")
                .unwrap_or_else(|_| "nvidia".into()),
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
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
fn parse_env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
fn parse_env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
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
    /// The agent is wrapped in async `Mutex` so per-turn mutations
    /// like `set_session_id` are possible from a `&self` command-loop.
    agent: AsyncMutex<Agent>,
    sqlite_pool: Option<SqlitePool>,
    /// PostgreSQL connection pool. Populated from `DATABASE_URL` (or the
    /// `database_url` config field) at startup. Used for jobs, routines,
    /// skills (with embeddings), and doctor checks. Failure to connect is
    /// non-fatal — Postgres-backed features surface an error on demand
    /// and the UI still runs SQLite-only.
    pg_pool: Option<Arc<PgPool>>,
    config: Arc<RwLock<WebuiConfig>>,
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
    pub async fn start() -> anyhow::Result<RuntimeStartResult> {
        // 1) Tracing  ~/.volt/logs/webui.log
        // `init_otel_for_tui` uses `try_init` under the hood and is
        // safe to call repeatedly; we still log file IO errors.
        let log_dir = crate::config::volt_home().join("logs");
        if let Err(e) = crate::telemetry::init_otel_for_tui("webui", &log_dir) {
            eprintln!("[webui] tracing init warning: {}", e);
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

        // 4b) Connect to PostgreSQL. Used for jobs, routines, skills, and
        //     the doctor check. Failure is non-fatal — features that
        //     require it surface a clear error on demand.
        let pg_pool = match config.database_url.as_deref() {
            Some(url) => match crate::db::build_shared_pg_pool(url).await {
                Ok(p) => {
                    tracing::info!("[webui] postgres pool ready");
                    Some(p)
                }
                Err(e) => {
                    tracing::warn!("[webui] postgres unavailable: {}", e);
                    None
                }
            },
            None => {
                tracing::info!("[webui] DATABASE_URL not set — postgres-backed features disabled");
                None
            }
        };

        // 5) Build the tool registry + embedder.
        let embedder = crate::embedding::EmbeddingClient::new_smart().await;
        let tools =
            crate::tools::setup_tools(Some(&embedder), config.database_url.as_deref()).await;

        // 6) Resolve an LLM provider. `build_provider` falls back to a
        //    generic LLM_API_KEY if the model is unknown, so this is
        //    safe even with no env vars.
        let model = config.default_model.clone();
        let (provider, _provider_kind) = crate::orchestrator::build_provider(&model, "volt-webui");

        // 6b) Detect missing API key so the UI can show the setup wizard.
        //     We still build a default provider so the runtime is fully
        //     wired (it just won't successfully chat). The
        //     `SubmitApiKey` command hot-swaps the provider once the
        //     user enters a key — no app restart needed.
        let setup_providers: Vec<crate::webui::commands::ProviderInfo> = if !crate::config::has_any_llm_key() {
            tracing::warn!("[webui] no LLM API key found — setup wizard will be shown");
            let ollama_label = if std::env::var("OLLAMA_HOST").is_ok() {
                "Ollama — running locally on this machine"
            } else {
                "Ollama — local models (no key needed if running locally) or cloud (needs OLLAMA_API_KEY)"
            };
            let ollama_env = crate::config::provider_env_var("ollama").map(String::from);
            vec![
                crate::webui::commands::ProviderInfo {
                    slug: "groq".into(),
                    label: "Groq — fast cloud inference (free tier)".into(),
                    env_var: crate::config::provider_env_var("groq").map(String::from),
                    default_model: crate::config::default_model_for_provider("groq").into(),
                },
                crate::webui::commands::ProviderInfo {
                    slug: "openai".into(),
                    label: "OpenAI — GPT-4o, GPT-4o-mini".into(),
                    env_var: crate::config::provider_env_var("openai").map(String::from),
                    default_model: crate::config::default_model_for_provider("openai").into(),
                },
                crate::webui::commands::ProviderInfo {
                    slug: "anthropic".into(),
                    label: "Anthropic — Claude Sonnet 4.5".into(),
                    env_var: crate::config::provider_env_var("anthropic").map(String::from),
                    default_model: crate::config::default_model_for_provider("anthropic").into(),
                },
                crate::webui::commands::ProviderInfo {
                    slug: "nvidia".into(),
                    label: "NVIDIA NIM — hosted open models".into(),
                    env_var: crate::config::provider_env_var("nvidia").map(String::from),
                    default_model: crate::config::default_model_for_provider("nvidia").into(),
                },
                crate::webui::commands::ProviderInfo {
                    slug: "ollama".into(),
                    label: ollama_label.into(),
                    env_var: ollama_env,
                    default_model: crate::config::default_model_for_provider("ollama").into(),
                },
            ]
        } else {
            tracing::info!("[webui] LLM key present — no setup wizard needed");
            Vec::new()
        };

        // 7) Channels
        let (cmd_tx, cmd_rx) = mpsc::channel::<UiCommand>(CMD_CHANNEL_CAPACITY);
        let (event_tx, _) = broadcast::channel::<UiEvent>(BROADCAST_CAPACITY);

        // 8) Shared state used by the approval callback
        let pending_approvals: Arc<Mutex<HashMap<Uuid, oneshot::Sender<ApprovalDecision>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let audit_log: Arc<Mutex<Vec<AuditEntry>>> = Arc::new(Mutex::new(Vec::new()));

        // 9) Streaming token callback — fires for every token the
        //    provider emits, regardless of the chat's session ID.
        let stream_tx = event_tx.clone();
        let on_token: crate::llm::provider::TokenCallback = Arc::new(move |token: &str| {
            // Broadcast send fails only if there are zero subscribers;
            // safe to ignore.
            let _ = stream_tx.send(UiEvent::ChatChunk {
                content: token.to_string(),
            });
        });

        // 10) Approval callback — sends an `ApprovalRequest` to the UI
        //     and waits up to 5 minutes for the user to decide. Defaults
        //     to Deny on timeout so an unattended UI never auto-runs a
        //     privileged tool.
        let approval_pending = pending_approvals.clone();
        let approval_event_tx = event_tx.clone();
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
                    let _ = tx.send(UiEvent::ApprovalRequest {
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
        // Bind the SQLite pool once at startup so messages can be
        // persisted on every chat. The session_id itself is bound
        // lazily per chat (via `set_session_id` in `handle_chat`),
        // because a fresh session is minted on the first turn.
        if let Some(ref pool) = sqlite_pool {
            agent = agent.with_sqlite_pool(pool.clone());
        }
        let capability_manager = agent.capability_manager.clone();

        // 12) Construct the runtime
        let runtime = Runtime {
            agent: AsyncMutex::new(agent),
            sqlite_pool,
            pg_pool,
            config: Arc::new(RwLock::new(config)),
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

        // 13) Spawn the command-processing task. The task owns the
        //     `cmd_rx` directly — no Mutex/Option dance needed.
        let runtime_for_task = runtime.clone();
        tokio::spawn(async move {
            Self::command_loop(runtime_for_task, cmd_rx).await;
        });

        // The UI gets the provider list via `RuntimeStartResult`
        // (set in `state.setup_providers` synchronously in
        // `app::Bootstrap`). No late SetupNeeded broadcast needed —
        // it would just cause a second toast.

        tracing::info!("[webui] runtime started");
        Ok(RuntimeStartResult {
            handle: RuntimeHandle::new(runtime, cmd_tx),
            setup_providers,
        })
    }

    // -------------------------------------------------------------------------
    // Command loop
    // -------------------------------------------------------------------------

    async fn command_loop(runtime: Arc<Runtime>, mut cmd_rx: mpsc::Receiver<UiCommand>) {
        while let Some(cmd) = cmd_rx.recv().await {
            tracing::info!(
                target: "webui.cmd",
                "received: {cmd:?}"
            );
            // `catch_unwind` + `AssertUnwindSafe` so a panic in one
            // handler doesn't kill the loop. Most handlers return
            // `Result`-shaped errors via `UiEvent::Error`.
            let rt = runtime.clone();
            let outcome =
                futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async move {
                    rt.process_command(cmd).await
                }))
                .await;
            if let Err(e) = outcome {
                tracing::error!("[webui] command handler panicked: {:?}", e);
                let entry = AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: AuditActor::Agent,
                    action: AuditAction::Chat,
                    target: "panic".into(),
                    result: AuditResult::Error,
                    detail: json!({ "panic": format!("{:?}", e) }),
                    session_id: None,
                };
                if let Ok(mut g) = runtime.audit_log.lock() {
                    g.push(entry);
                }
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
            UiCommand::Chat { session_id, input } => self.handle_chat(session_id, input).await,
            UiCommand::CancelChat => self.handle_cancel_chat().await,
            UiCommand::ListTools => self.handle_list_tools().await,
            UiCommand::ExecuteTool { name, args } => self.handle_execute_tool(name, args).await,
            UiCommand::ListSessions => self.handle_list_sessions().await,
            UiCommand::LoadSession { id } => self.handle_load_session(id).await,
            UiCommand::CreateSession { name } => self.handle_create_session(name).await,
            UiCommand::ForkSession { id } => self.handle_fork_session(id).await,
            UiCommand::DeleteSession { id } => self.handle_delete_session(id).await,
            UiCommand::ListModels => self.handle_list_models().await,
            UiCommand::GetConfig => self.handle_get_config().await,
            UiCommand::UpdateConfig { patch } => self.handle_update_config(patch).await,
            UiCommand::RunDoctor => self.handle_run_doctor().await,
            UiCommand::ListWorktrees => self.handle_list_worktrees().await,
            UiCommand::WorktreeStatus { branch } => self.handle_worktree_status(branch).await,
            UiCommand::WorktreeMerge { branch } => self.handle_worktree_merge(branch).await,
            UiCommand::WorktreeClean { branch } => self.handle_worktree_clean(branch).await,
            UiCommand::ListWorkflows => self.handle_list_workflows().await,
            UiCommand::RunWorkflow {
                pattern,
                agents,
                tasks,
                allow,
            } => {
                self.handle_run_workflow(pattern, agents, tasks, allow)
                    .await
            }
            UiCommand::ListJobs => self.handle_list_jobs().await,
            UiCommand::CreateJob { description } => self.handle_create_job(description).await,
            UiCommand::StartJob { id, worker_id } => self.handle_start_job(id, worker_id).await,
            UiCommand::CompleteJob { id, output } => self.handle_complete_job(id, output).await,
            UiCommand::FailJob { id, error } => self.handle_fail_job(id, error).await,
            UiCommand::ListRoutines => self.handle_list_routines().await,
            UiCommand::ToggleRoutine { id, enabled } => {
                self.handle_toggle_routine(id, enabled).await
            }
            UiCommand::CreateRoutine {
                name,
                action_prompt,
                cron,
                trigger_type,
            } => {
                self.handle_create_routine(name, action_prompt, cron, trigger_type)
                    .await
            }
            UiCommand::DeleteRoutine { id } => self.handle_delete_routine(id).await,
            UiCommand::ListSkills => self.handle_list_skills().await,
            UiCommand::SearchCatalogSkills { query } => {
                self.handle_search_catalog_skills(query).await
            }
            UiCommand::InstallSkill { name } => self.handle_install_skill(name).await,
            UiCommand::ImportSkill { path } => self.handle_import_skill(path).await,
            UiCommand::UninstallSkill { name } => self.handle_uninstall_skill(name).await,
            UiCommand::ListMcpServers => self.handle_list_mcp_servers().await,
            UiCommand::RegisterMcpServer {
                name,
                transport,
                command,
                url,
            } => {
                self.handle_register_mcp_server(name, transport, command, url)
                    .await
            }
            UiCommand::GetAuditLog { limit } => self.handle_get_audit_log(limit).await,
            UiCommand::ApprovalResponse {
                request_id,
                allow,
                allow_session,
            } => {
                self.handle_approval_response(request_id, allow, allow_session)
                    .await
            }
            UiCommand::SubmitApiKey {
                provider,
                api_key,
                model,
            } => {
                self.handle_submit_api_key(provider, api_key, model).await
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
            // writing into its state AND updating self.session_id so
            // save_session_messages_delta writes to the right row.
            let mut agent = self.agent.lock().await;
            agent.set_session_id(session_id);
            {
                let mut state = agent.state().lock().await;
                state.session_id = session_id;
            }
            if let Ok(msgs) = session::load_messages(pool, session_id).await {
                let mut state = agent.state().lock().await;
                for m in msgs {
                    let already = state.messages.iter().any(|existing| {
                        existing.id == m.id
                            || (existing.role == m.role && existing.content == m.content)
                    });
                    if !already {
                        state.messages.push(m);
                    }
                }
            } else {
                // DB load failed — proceed with empty history but
                // surface a warning so the user knows prior context
                // is missing.
                self.emit(UiEvent::Error {
                    source: "load_session".into(),
                    message: "Could not load prior chat history for this session.".into(),
                });
            }
            drop(agent);
        }
        if let Ok(mut g) = self.active_session.lock() {
            *g = Some(session_id);
        }

        // Forward tool-executed events from the agent's event bus to
        // the UI as `ToolCallEnd`. Subscribed for the lifetime of this
        // chat only.
        let mut bus_rx = self.event_bus.subscribe();
        let tool_event_tx = self.event_tx.clone();
        let tool_event_handle = tokio::spawn(async move {
            while let Ok(ev) = bus_rx.recv().await {
                if let crate::events::Event::ToolExecuted { tool_name, success } = ev {
                    let _ = tool_event_tx.send(UiEvent::ToolCallEnd {
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
            actor: AuditActor::User,
            action: AuditAction::Chat,
            target: session_id.to_string(),
            result: AuditResult::Ok,
            detail: json!({ "input_chars": input.len() }),
            session_id: Some(session_id),
        });

        // Reset cancellation flag for this turn. Both flags must
        // be cleared: the AtomicBool and the CancelToken. The token
        // is shared with the agent and `handle_cancel_chat`
        // permanently flips it; without this reset every chat
        // after the first cancel would be pre-cancelled.
        self.cancel.store(false, Ordering::SeqCst);
        self.cancel_token.reset();

        // Run the agent. The on_token callback (set in start()) already
        // streams `ChatChunk` events into the internal channel.
        let result = {
            let agent = self.agent.lock().await;
            agent.run(&input).await
        };

        // Drain task is no longer needed.
        tool_event_handle.abort();

        let duration_ms = started.elapsed().as_millis() as u64;
        let state_snapshot = self.agent.lock().await.state().lock().await.clone();
        let tokens_used =
            (state_snapshot.total_prompt_tokens + state_snapshot.total_completion_tokens) as u32;

        match result {
            Ok(final_text) => {
                let len = final_text.len();
                tracing::warn!(
                    "[webui] ChatComplete: final_text_len={} tokens={} duration_ms={}",
                    len,
                    tokens_used,
                    duration_ms
                );
                // If the model returned empty content but spent tokens
                // (i.e. it errored silently or hit context limits),
                // surface that as a user-visible error so the empty
                // bubble is at least explained.
                if len == 0 && tokens_used > 0 {
                    let detail = if tokens_used > 50_000 {
                        format!(
                            "The model returned an empty response — the conversation history is too long ({} tokens). Start a new session to continue.",
                            tokens_used
                        )
                    } else {
                        "The model returned an empty response. Try a different model or shorter message.".to_string()
                    };
                    self.emit(UiEvent::ChatError { message: detail });
                } else {
                    self.emit(UiEvent::ChatComplete {
                        final_text,
                        tokens_used,
                        duration_ms,
                    });
                }
                self.log_audit(AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: AuditActor::Agent,
                    action: AuditAction::Chat,
                    target: session_id.to_string(),
                    result: AuditResult::Ok,
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
                    self.emit(UiEvent::ChatError {
                        message: msg.clone(),
                    });
                }
                self.log_audit(AuditEntry {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    actor: AuditActor::Agent,
                    action: AuditAction::Chat,
                    target: session_id.to_string(),
                    result: AuditResult::Error,
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
                permission: permission_to_info(perm),
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
                    "error": r.error,
                }),
                r.error,
            ),
            Err(e) => (
                json!({
                    "success": false,
                    "error": e.to_string(),
                }),
                Some(e.to_string()),
            ),
        };
        self.emit(UiEvent::ToolCallEnd {
            id: Uuid::new_v4().to_string(),
            result: result_val,
            error,
        });
        self.log_audit(AuditEntry {
            id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            actor: AuditActor::User,
            action: AuditAction::ToolCall,
            target: name,
            result: AuditResult::Ok,
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
            self.emit(UiEvent::SessionLoaded {
                id,
                messages: vec![],
            });
            return;
        };
        match session::load_messages(&pool, id).await {
            Ok(msgs) => {
                let ui_msgs: Vec<ChatMessage> = msgs
                    .into_iter()
                    .map(|m| ChatMessage {
                        id: m.id,
                        role: parse_chat_role(&m.role),
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
                self.emit(UiEvent::SessionLoaded {
                    id,
                    messages: ui_msgs,
                });
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
            // No DB to delete from — still emit so the UI removes
            // the row optimistically.
            self.emit(UiEvent::SessionDeleted { id });
            return;
        };
        // Wrap in a transaction so the messages + checkpoints + row
        // either all go or none of them does. The FK constraints
        // require messages/checkpoints to be deleted before the row.
        let result: anyhow::Result<()> = async {
            let mut tx = pool.begin().await?;
            session::delete_session_messages(&mut *tx, id).await?;
            session::delete_session_checkpoints(&mut *tx, id).await?;
            sqlx::query("DELETE FROM sessions WHERE id = ?")
                .bind(id.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => self.emit(UiEvent::SessionDeleted { id }),
            Err(e) => self.emit_error("delete_session", e),
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
                    std::env::var("OLLAMA_HOST").is_ok() || std::env::var("OLLAMA_API_KEY").is_ok()
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
        let mut report = build_doctor_report();
        // Live-probe Postgres with a 2-second timeout so the doctor
        // report reflects whether the pool is actually responsive,
        // not just whether DATABASE_URL is set.
        match &self.pg_pool {
            Some(pool) => {
                let probe = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    sqlx::query("SELECT 1").fetch_one(&**pool),
                )
                .await;
                report.database = match probe {
                    Ok(Ok(_)) => "ok".into(),
                    Ok(Err(e)) => format!("unreachable: {}", e),
                    Err(_) => "unreachable: timeout".into(),
                };
            }
            None => {
                report.database = match std::env::var("DATABASE_URL").is_ok() {
                    true => "unreachable: failed to connect at startup".into(),
                    false => "not configured".into(),
                };
            }
        }
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
                    actor: AuditActor::User,
                    action: AuditAction::ToolCall,
                    target: branch,
                    result: AuditResult::Ok,
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
                        actor: AuditActor::User,
                        action: AuditAction::ToolCall,
                        target: branch.clone(),
                        result: AuditResult::Ok,
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
        let run_id = Uuid::new_v4().to_string();
        self.emit(UiEvent::WorkflowStarted {
            pattern: pattern.clone(),
            run_id: run_id.clone(),
        });
        // Run the workflow in a background task so this handler
        // returns immediately. The command_loop stays free to
        // process other commands (e.g. cancel, list jobs) while
        // the workflow runs. The run_id is the join handle's
        // return — we ignore it here; status comes through
        // WorkflowCompleted/WorkflowFailed events emitted by the
        // workflow itself.
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            match crate::commands::workflow::run(pattern.clone(), agents, tasks, None, None, allow)
                .await
            {
                Ok(()) => {
                    let _ = tx.send(UiEvent::WorkflowCompleted { pattern, run_id });
                }
                Err(e) => {
                    let _ = tx.send(UiEvent::WorkflowFailed {
                        pattern,
                        run_id,
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    async fn handle_list_jobs(&self) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit(UiEvent::JobsListed { jobs: vec![] });
            return;
        };
        let mgr = crate::jobs::JobManager::new(Some((*pool).clone()));
        match mgr.list_jobs(None).await {
            Ok(rows) => {
                let jobs: Vec<JobInfo> = rows
                    .into_iter()
                    .map(|j| JobInfo {
                        id: j.id.to_string(),
                        name: j.description.clone(),
                        schedule: String::new(),
                        last_run: j.completed_at,
                        last_status: j.state.to_string(),
                        next_run: None,
                        attempt_count: j.attempt_count,
                        worker_id: j.worker_id,
                        output: j.output,
                        created_at: j.created_at,
                        updated_at: j.updated_at,
                    })
                    .collect();
                self.emit(UiEvent::JobsListed { jobs });
            }
            Err(e) => self.emit_error("list_jobs", e),
        }
    }

    async fn handle_create_job(&self, description: String) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit(UiEvent::Error {
                source: "create_job".into(),
                message: "PostgreSQL not available — set DATABASE_URL".into(),
            });
            return;
        };
        let mgr = crate::jobs::JobManager::new(Some((*pool).clone()));
        match mgr
            .create_job(&description, serde_json::json!({ "source": "webui" }))
            .await
        {
            Ok(id) => self.emit(UiEvent::JobCreated { id: id.to_string() }),
            Err(e) => self.emit_error("create_job", e),
        }
    }

    async fn handle_start_job(&self, id: Uuid, worker_id: Option<String>) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit_error("start_job", anyhow::anyhow!("PostgreSQL not available"));
            return;
        };
        let mgr = crate::jobs::JobManager::new(Some((*pool).clone()));
        match mgr.start_job(id, worker_id.as_deref()).await {
            Ok(()) => self.emit(UiEvent::JobUpdated {
                id: id.to_string(),
                state: "InProgress".into(),
            }),
            Err(e) => self.emit_error("start_job", e),
        }
    }

    async fn handle_complete_job(&self, id: Uuid, output: String) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit_error("complete_job", anyhow::anyhow!("PostgreSQL not available"));
            return;
        };
        let mgr = crate::jobs::JobManager::new(Some((*pool).clone()));
        match mgr.complete_job(id, &output).await {
            Ok(()) => self.emit(UiEvent::JobUpdated {
                id: id.to_string(),
                state: "Completed".into(),
            }),
            Err(e) => self.emit_error("complete_job", e),
        }
    }

    async fn handle_fail_job(&self, id: Uuid, error: String) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit_error("fail_job", anyhow::anyhow!("PostgreSQL not available"));
            return;
        };
        let mgr = crate::jobs::JobManager::new(Some((*pool).clone()));
        match mgr.fail_job(id, &error).await {
            Ok(()) => self.emit(UiEvent::JobUpdated {
                id: id.to_string(),
                state: "Failed".into(),
            }),
            Err(e) => self.emit_error("fail_job", e),
        }
    }

    async fn handle_list_routines(&self) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit(UiEvent::RoutinesListed { routines: vec![] });
            return;
        };
        match crate::db::list_routines(&pool).await {
            Ok(rows) => {
                let routines: Vec<RoutineInfo> = rows
                    .into_iter()
                    .map(|r| {
                        let id = r
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let name = r
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let cron = r
                            .get("cron")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let trigger_type = r
                            .get("trigger_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let trigger = if cron.is_empty() {
                            trigger_type.clone()
                        } else {
                            format!("{} ({})", trigger_type, cron)
                        };
                        let enabled = r.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                        let last_run = r
                            .get("last_run")
                            .and_then(|v| v.as_str())
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.with_timezone(&chrono::Utc));
                        let next_run = r
                            .get("next_run")
                            .and_then(|v| v.as_str())
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.with_timezone(&chrono::Utc));
                        let action_prompt = r
                            .get("action_prompt")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        RoutineInfo {
                            id,
                            name,
                            trigger,
                            last_run,
                            enabled,
                            next_run,
                            action_prompt,
                        }
                    })
                    .collect();
                self.emit(UiEvent::RoutinesListed { routines });
            }
            Err(e) => self.emit_error("list_routines", e),
        }
    }

    async fn handle_toggle_routine(&self, id: Uuid, enabled: bool) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit_error(
                "toggle_routine",
                anyhow::anyhow!("PostgreSQL not available"),
            );
            return;
        };
        let res = sqlx::query("UPDATE routines SET enabled = $2 WHERE id = $1")
            .bind(id)
            .bind(enabled)
            .execute(&*pool)
            .await;
        match res {
            Ok(_) => self.emit(UiEvent::RoutineUpdated {
                id: id.to_string(),
                enabled,
            }),
            Err(e) => self.emit_error("toggle_routine", e),
        }
    }

    async fn handle_create_routine(
        &self,
        name: String,
        action_prompt: String,
        cron: Option<String>,
        trigger_type: Option<String>,
    ) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit_error(
                "create_routine",
                anyhow::anyhow!("PostgreSQL not available"),
            );
            return;
        };
        let new_id = Uuid::new_v4();
        let trigger = trigger_type.unwrap_or_else(|| "cron".to_string());
        let res = sqlx::query(
            "INSERT INTO routines (id, name, action_prompt, enabled, trigger_type, cron) VALUES ($1, $2, $3, true, $4, $5)",
        )
        .bind(new_id)
        .bind(&name)
        .bind(&action_prompt)
        .bind(&trigger)
        .bind(cron.as_deref())
        .execute(&*pool)
        .await;
        match res {
            Ok(_) => self.emit(UiEvent::RoutineUpdated {
                id: new_id.to_string(),
                enabled: true,
            }),
            Err(e) => self.emit_error("create_routine", e),
        }
    }

    async fn handle_delete_routine(&self, id: Uuid) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit_error(
                "delete_routine",
                anyhow::anyhow!("PostgreSQL not available"),
            );
            return;
        };
        let res = sqlx::query("DELETE FROM routines WHERE id = $1")
            .bind(id)
            .execute(&*pool)
            .await;
        match res {
            Ok(_) => self.emit(UiEvent::RoutineDeleted { id: id.to_string() }),
            Err(e) => self.emit_error("delete_routine", e),
        }
    }

    async fn handle_list_skills(&self) {
        let mut skills: Vec<SkillInfo> = Vec::new();
        // 1) Local files in ~/.volt/skills/*.md
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
                            source: SkillSource::Local,
                        });
                    }
                }
            }
        }
        // 2) Database skills (with embeddings, search-indexed)
        if let Some(pool) = self.pg_pool.clone() {
            match crate::db::list_skills(&pool).await {
                Ok(rows) => {
                    for s in rows {
                        skills.push(SkillInfo {
                            name: s.name.clone(),
                            description: s.description.clone(),
                            version: s.version.clone(),
                            installed_at: s.created_at,
                            source: if s.source_path.is_some() {
                                SkillSource::Imported
                            } else {
                                SkillSource::Catalog
                            },
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!("[webui] list_skills db: {}", e);
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
        let Some(pool) = self.pg_pool.clone() else {
            self.emit(UiEvent::Error {
                source: "install_skill".into(),
                message: "skill installation requires a Postgres connection (set DATABASE_URL)"
                    .into(),
            });
            return;
        };
        // 1) Fetch the catalog entry
        let catalog = match crate::skills::catalog::fetch_catalog(None).await {
            Ok(c) => c,
            Err(e) => {
                self.emit_error("install_skill", e);
                return;
            }
        };
        let entry = match catalog.skills.iter().find(|e| e.name == name).cloned() {
            Some(e) => e,
            None => {
                self.emit(UiEvent::Error {
                    source: "install_skill".into(),
                    message: format!("skill '{}' not found in catalog", name),
                });
                return;
            }
        };
        // 2) Embed the description so search_skills works
        let embedder = crate::embedding::EmbeddingClient::new_smart().await;
        let embedding = match embedder.embed_description(&entry.description).await {
            Ok(v) => v,
            Err(e) => {
                self.emit_error("install_skill", e);
                return;
            }
        };
        // 3) Upsert into Postgres
        let id = Uuid::new_v4();
        let mcp_servers: Vec<String> = vec![];
        let res = crate::db::upsert_skill(
            &pool,
            id,
            &entry.name,
            &entry.description,
            "1.0.0",
            &entry.description,
            &embedding,
            &mcp_servers,
            Some("catalog"),
        )
        .await;
        match res {
            Ok(()) => self.emit(UiEvent::SkillInstalled { name }),
            Err(e) => self.emit_error("install_skill", e),
        }
    }

    async fn handle_uninstall_skill(&self, name: String) {
        let Some(pool) = self.pg_pool.clone() else {
            self.emit_error(
                "uninstall_skill",
                anyhow::anyhow!("PostgreSQL not available"),
            );
            return;
        };
        let res = sqlx::query("DELETE FROM skills WHERE name = $1")
            .bind(&name)
            .execute(&*pool)
            .await;
        match res {
            Ok(_) => self.emit(UiEvent::SkillUninstalled { name }),
            Err(e) => self.emit_error("uninstall_skill", e),
        }
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
        let converted = crate::skills::importer::convert_to_volt_skill(&p, &content, &fmt, None);
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
        let path = mcp_servers_path();
        let servers = match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<Vec<serde_json::Value>>(&s)
                .unwrap_or_default()
                .into_iter()
                .map(|v| {
                    let command = v
                        .get("command")
                        .and_then(|x| x.as_str())
                        .map(str::to_string);
                    let url = v.get("url").and_then(|x| x.as_str()).map(str::to_string);
                    let endpoint = command.clone().or_else(|| url.clone()).unwrap_or_default();
                    McpServerInfo {
                        name: v
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("?")
                            .to_string(),
                        transport: v
                            .get("transport")
                            .and_then(|x| x.as_str())
                            .map(parse_mcp_transport)
                            .unwrap_or(McpTransport::Stdio),
                        status: McpStatus::Disconnected,
                        tools_count: 0,
                        endpoint,
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        self.emit(UiEvent::McpServersListed { servers });
    }

    async fn handle_register_mcp_server(
        &self,
        name: String,
        transport: String,
        command: Option<String>,
        url: Option<String>,
    ) {
        let path = mcp_servers_path();
        let mut servers: Vec<serde_json::Value> = match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        // Replace if same name exists, else append.
        if let Some(idx) = servers
            .iter()
            .position(|v| v.get("name").and_then(|x| x.as_str()) == Some(&name))
        {
            servers[idx] = json!({
                "name": name,
                "transport": transport,
                "command": command,
                "url": url,
            });
        } else {
            servers.push(json!({
                "name": name,
                "transport": transport,
                "command": command,
                "url": url,
            }));
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&servers)
            .map_err(|e| anyhow::anyhow!(e))
            .and_then(|s| std::fs::write(&path, s).map_err(|e| anyhow::anyhow!(e)))
        {
            Ok(()) => self.emit(UiEvent::McpServerRegistered { name }),
            Err(e) => self.emit_error("register_mcp_server", e),
        }
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

    async fn handle_approval_response(&self, request_id: Uuid, allow: bool, allow_session: bool) {
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
                    actor: AuditActor::User,
                    action: AuditAction::Approval,
                    target: request_id.to_string(),
                    result: if allow { AuditResult::Ok } else { AuditResult::Denied },
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

    /// Persist a new API key, rebuild the LLM provider, and announce
    /// `SetupReady` so the UI can close its wizard. Returns an error
    /// event if persistence or provider construction fails — the UI
    /// surfaces the error and keeps the wizard open.
    async fn handle_submit_api_key(
        &self,
        provider_slug: String,
        api_key: String,
        model: String,
    ) {
        tracing::info!(
            "[webui] submit_api_key: provider={} model={}",
            provider_slug,
            model
        );

        // 1) Persist to volt_home()/.env and set the process env.
        if let Err(e) = crate::config::save_api_key(&provider_slug, &api_key) {
            tracing::error!("[webui] save_api_key failed: {}", e);
            self.emit(UiEvent::Error {
                source: "setup".into(),
                message: format!("Failed to save API key: {}", e),
            });
            return;
        }

        // 2) Build the new provider and swap it into the agent.
        let (new_provider, _kind) =
            crate::orchestrator::build_provider(&model, "volt-webui");
        {
            let mut agent = self.agent.lock().await;
            agent.replace_provider(new_provider);
        }

        // 3) Update the UI's notion of the model so the header shows
        //    the right thing.
        {
            if let Ok(mut cfg) = self.config.write() {
                cfg.default_model = model.clone();
            }
        }

        self.emit(UiEvent::SetupReady {
            provider: provider_slug,
            model,
        });
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Push a `UiEvent` into the internal channel — the background
    /// emitter forwards it to all broadcast subscribers.
    fn emit(&self, event: UiEvent) {
        tracing::info!(
            target: "webui.event",
            "emit: {event:?}"
        );
        // Broadcast send only fails if there are zero subscribers.
        // That's fine — the UI is allowed to be temporarily disconnected.
        let _ = self.event_tx.send(event);
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
// RuntimeStartResult
// =============================================================================

/// Returned by [`Runtime::start`]. Bundles the clonable handle with
/// any setup state the UI should display immediately — currently the
/// provider list for the first-run wizard. We return this directly
/// instead of relying on the broadcast channel so the UI can render
/// without a race against the `subscribe()` call.
pub struct RuntimeStartResult {
    pub handle: RuntimeHandle,
    /// Empty when an LLM key is already configured; populated when
    /// the runtime started without one and the user needs to be
    /// prompted via the setup wizard.
    pub setup_providers: Vec<crate::webui::commands::ProviderInfo>,
}

// =============================================================================
// RuntimeHandle
// =============================================================================

/// A clonable, `Send + Sync` handle to the runtime. UI components hold a
/// handle and use it to send commands and subscribe to events.
#[derive(Clone)]
pub struct RuntimeHandle {
    /// Keeps the `Runtime` (and its `event_tx` broadcast) alive even
    /// after the command-loop task exits. Cloning is cheap — just an
    /// `Arc::clone` and an `mpsc::Sender::clone`.
    _runtime: Arc<Runtime>,
    cmd_tx: mpsc::Sender<UiCommand>,
    event_tx: broadcast::Sender<UiEvent>,
}

impl std::fmt::Debug for RuntimeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHandle").finish_non_exhaustive()
    }
}

impl RuntimeHandle {
    /// Wrap a started `Arc<Runtime>` in a handle. The runtime must
    /// have been started via `Runtime::start()` (otherwise no command
    /// receiver is listening). Does not spawn any new tasks.
    pub fn new(runtime: Arc<Runtime>, cmd_tx: mpsc::Sender<UiCommand>) -> Self {
        let event_tx = runtime.event_tx.clone();
        Self {
            _runtime: runtime,
            cmd_tx,
            event_tx,
        }
    }

    /// Send a command to the runtime. The command is enqueued on the
    /// mpsc channel and processed asynchronously by the command-loop task.
    pub async fn send(&self, cmd: UiCommand) -> Result<(), String> {
        self.cmd_tx.send(cmd).await.map_err(|e| e.to_string())
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

/// Map a `PermissionLevel` to the wire-level permission enum.
fn permission_to_info(perm: crate::models::PermissionLevel) -> ToolPermission {
    match perm {
        crate::models::PermissionLevel::Allow => ToolPermission::Allow,
        crate::models::PermissionLevel::Prompt => ToolPermission::Prompt,
        crate::models::PermissionLevel::ReadOnly => ToolPermission::Allow,
        crate::models::PermissionLevel::Blocked => ToolPermission::Deny,
    }
}

/// Map a runtime `role` string (as stored in SQLite) to a `ChatRole`.
/// Unknown roles default to `User` so the UI never breaks on legacy data.
fn parse_chat_role(s: &str) -> ChatRole {
    match s {
        "assistant" => ChatRole::Assistant,
        "tool" => ChatRole::Tool,
        "system" => ChatRole::System,
        _ => ChatRole::User,
    }
}

/// Map an MCP transport string (as stored in JSON config) to an enum.
fn parse_mcp_transport(s: &str) -> McpTransport {
    match s {
        "http" => McpTransport::Http,
        "websocket" => McpTransport::Websocket,
        "grpc" => McpTransport::Grpc,
        _ => McpTransport::Stdio,
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
                let bytes = p.metadata().map(|m| m.len()).unwrap_or(0);
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
        rust_channel: std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".into()),
        api_keys,
        database: if std::env::var("DATABASE_URL").is_ok() {
            "ok".into()
        } else {
            "not configured".into()
        },
        embedder_provider: std::env::var("EMBEDDING_PROVIDER").unwrap_or_else(|_| "nvidia".into()),
        embedder_model: std::env::var("EMBEDDING_MODEL")
            .unwrap_or_else(|_| "nvidia/llama-nemotron-embed-1b-v2".into()),
        disk_free_gb: 0.0,
        permissions_default: "Prompt".into(),
        recent_failures: 0,
        workspace_files,
    }
}

/// Build a `WorktreeManager` rooted at the current directory. Returns `None` if
/// the current directory is not inside a git repository, so callers can return a
/// safe default without having to repeat the canonicalize dance.
async fn worktree_manager_or_none() -> Option<crate::commands::worktree::WorktreeManager> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let repo_root = crate::commands::worktree::WorktreeManager::detect_repo_root(&cwd)
        .await
        .ok()
        .flatten()?;
    Some(crate::commands::worktree::WorktreeManager::new(repo_root))
}

/// Worktree helpers — wrap `WorktreeManager` so handlers stay terse.
async fn worktree_list() -> Vec<WorktreeInfo> {
    let Some(mgr) = worktree_manager_or_none().await else {
        return Vec::new();
    };
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
    let mgr = worktree_manager_or_none()
        .await
        .ok_or_else(|| anyhow::anyhow!("not in a git repository"))?;
    mgr.diff_summary(branch).await.map_err(anyhow::Error::from)
}

async fn worktree_merge_back(branch: &str) -> anyhow::Result<String> {
    let mgr = worktree_manager_or_none()
        .await
        .ok_or_else(|| anyhow::anyhow!("not in a git repository"))?;
    mgr.merge_back(branch).await.map_err(anyhow::Error::from)
}

async fn worktree_remove(branch: &str) -> anyhow::Result<()> {
    let mgr = worktree_manager_or_none()
        .await
        .ok_or_else(|| anyhow::anyhow!("not in a git repository"))?;
    let infos = mgr.list().await.map_err(anyhow::Error::from)?;
    let path = infos
        .iter()
        .find(|w| w.branch == branch)
        .map(|w| w.path.clone())
        .ok_or_else(|| anyhow::anyhow!("worktree branch {} not found", branch))?;
    mgr.remove(&path, true).await.map_err(anyhow::Error::from)
}

/// Path to the user's MCP-server config JSON file (`~/.volt/mcp_servers.json`).
fn mcp_servers_path() -> std::path::PathBuf {
    crate::config::volt_home().join("mcp_servers.json")
}

