use crate::context::{ContextEntry, ContextKind, ContextStore};
use crate::embedding::EmbeddingClient;
use crate::models::{CancelToken, PermissionLevel, SandboxPolicy};
use crate::tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

// ── Seed event types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Events emitted by the agent loop and consumed by the background auto-seeding worker.
/// Three variants: EpisodeComplete (after agent run), ArtifactCreated (file/code side effects),
/// and MCPRegistered (MCP server tool schemas).
pub enum SeedEvent {
    EpisodeComplete {
        session_id: uuid::Uuid,
        task: String,
        resolution: String,
        tools_used: Vec<String>,
        success: bool,
        iteration_count: u32,
    },
    ArtifactCreated {
        file_path: String,
        description: String,
        language: String,
        tool_used: String,
    },
    MCPRegistered {
        server_name: String,
        tools: Vec<String>,
        intent_descriptors: Vec<String>,
    },
}

impl SeedEvent {
    fn to_context_entry(&self) -> ContextEntry {
        use chrono::Utc;
        let now = Utc::now();
        let (kind, content, metadata) = match self {
            SeedEvent::EpisodeComplete {
                session_id,
                task,
                resolution,
                tools_used,
                success,
                iteration_count,
            } => {
                let content = format!(
                    "[History: Episodic Memory]\nUser Problem: {}\nAction Taken: {}\nResolution: {}",
                    task,
                    tools_used.join(", "),
                    resolution
                );
                let metadata = serde_json::json!({
                    "session_id": session_id.to_string(),
                    "success": success,
                    "tools_used": tools_used,
                    "iteration_count": iteration_count,
                });
                (ContextKind::Conversation, content, metadata)
            }
            SeedEvent::ArtifactCreated {
                file_path,
                description,
                language,
                tool_used,
            } => {
                let content = format!(
                    "[Artifact: Codebase Manifest]\nFile Path: {}\nDescription: {}\nLanguage: {}",
                    file_path, description, language
                );
                let metadata = serde_json::json!({
                    "file_path": file_path,
                    "language": language,
                    "tool_used": tool_used,
                });
                (ContextKind::Artifact, content, metadata)
            }
            SeedEvent::MCPRegistered {
                server_name,
                tools,
                intent_descriptors,
            } => {
                let content = format!(
                    "[MCP Server: {}]\nHost: internal\nIntent Mapping: {}\nActive Tool Hooks: {}",
                    server_name,
                    intent_descriptors.join(", "),
                    tools.join(", ")
                );
                let metadata = serde_json::json!({
                    "server_name": server_name,
                    "tools": tools,
                    "intents": intent_descriptors,
                });
                (ContextKind::MCPConfig, content, metadata)
            }
        };

        ContextEntry {
            id: uuid::Uuid::new_v4(),
            kind,
            content,
            embedding: None,
            metadata,
            frequency: 0,
            success_rate: 1.0,
            usage_count: 0,
            last_used_at: now,
            created_at: now,
        }
    }
}

// ── MPSC channel wrapper ───────────────────────────────────────────────────

/// Bounded channel capacity for the seed-event bus. Adversarial agent loops
/// (or pathological retries) could otherwise OOM by emitting unlimited events.
const SEED_CHANNEL_CAPACITY: usize = 256;

/// Clone-able sender for the MPSC channel from agent loop to background worker.
/// Non-blocking — drops events with a warning if the buffer is full or the
/// worker has stopped.
#[derive(Clone)]
pub struct SeedChannel {
    tx: mpsc::Sender<SeedEvent>,
}

impl SeedChannel {
    /// `try_send` is non-blocking. If the buffer is full we drop the event
    /// (with a warning) rather than block the agent loop. The seed channel
    /// is best-effort telemetry; losing events is preferable to backpressuring
    /// tool execution.
    pub fn send(&self, event: SeedEvent) {
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("[volt worker] seed channel full ({}), event dropped", SEED_CHANNEL_CAPACITY);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!("[volt worker] seed channel closed, event dropped");
            }
        }
    }

    pub fn episode_complete(
        &self,
        session_id: uuid::Uuid,
        task: &str,
        resolution: &str,
        tools_used: Vec<String>,
        success: bool,
        iteration_count: u32,
    ) {
        self.send(SeedEvent::EpisodeComplete {
            session_id,
            task: task.to_string(),
            resolution: resolution.to_string(),
            tools_used,
            success,
            iteration_count,
        });
    }

    pub fn artifact_created(
        &self,
        file_path: &str,
        description: &str,
        language: &str,
        tool_used: &str,
    ) {
        self.send(SeedEvent::ArtifactCreated {
            file_path: file_path.to_string(),
            description: description.to_string(),
            language: language.to_string(),
            tool_used: tool_used.to_string(),
        });
    }

    pub fn mcp_registered(
        &self,
        server_name: &str,
        tools: Vec<String>,
        intent_descriptors: Vec<String>,
    ) {
        self.send(SeedEvent::MCPRegistered {
            server_name: server_name.to_string(),
            tools,
            intent_descriptors,
        });
    }
}

pub fn create_seed_channel() -> (SeedChannel, mpsc::Receiver<SeedEvent>) {
    let (tx, rx) = mpsc::channel(SEED_CHANNEL_CAPACITY);
    (SeedChannel { tx }, rx)
}

// ── Background auto-seeding daemon ─────────────────────────────────────────

const MERGE_CLUSTER_THRESHOLD: f32 = 0.85;
const MERGE_MIN_CLUSTER: usize = 3;
const MERGE_EVERY_N_BATCHES: u32 = 10;

/// Background daemon that drains seed events from the MPSC channel, computes embeddings,
/// and seeds entries into the unified context store with dedup and eviction.
/// Runs episodic merging every 10 batches.
pub struct AutoSeedWorker {
    context_store: Arc<ContextStore>,
    embedder: EmbeddingClient,
    cancel: CancelToken,
}

impl AutoSeedWorker {
    pub fn new(
        context_store: Arc<ContextStore>,
        embedder: EmbeddingClient,
        cancel: CancelToken,
    ) -> Self {
        Self {
            context_store,
            embedder,
            cancel,
        }
    }

    pub fn spawn(self, mut rx: mpsc::Receiver<SeedEvent>) {
        tokio::spawn(async move {
            info!("[volt worker] auto-seed daemon started");

            let mut batch_count: u32 = 0;

            loop {
                if self.cancel.is_cancelled() {
                    info!("[volt worker] received cancel signal, shutting down");
                    break;
                }

                let mut batch: Vec<SeedEvent> = Vec::with_capacity(32);
                match rx.recv().await {
                    Some(first) => {
                        batch.push(first);
                        while batch.len() < 32 {
                            match rx.try_recv() {
                                Ok(event) => batch.push(event),
                                Err(_) => break,
                            }
                        }
                    }
                    None => {
                        info!("[volt worker] seed channel closed, shutting down");
                        break;
                    }
                }

                if batch.is_empty() {
                    continue;
                }

                info!("[volt worker] processing {} seed events", batch.len());

                let entries: Vec<ContextEntry> =
                    batch.iter().map(|e| e.to_context_entry()).collect();

                let sem = Arc::new(tokio::sync::Semaphore::new(5));
                let embed_futures: Vec<_> = entries
                    .iter()
                    .map(|entry| {
                        let sem = sem.clone();
                        let embedder = self.embedder.clone();
                        let text = format!("{}: {}", entry.kind.as_str(), entry.content);
                        async move {
                            let _permit = sem.acquire().await.ok();
                            let emb = match embedder.embed_description(&text).await {
                                Ok(e) => Some(e),
                                Err(e) => {
                                    tracing::warn!("[worker] embedder failed for entry: {}", e);
                                    None
                                }
                            };
                            (entry.clone(), emb)
                        }
                    })
                    .collect();

                let results = futures::future::join_all(embed_futures).await;
                let mut embedded_entries: Vec<ContextEntry> = Vec::with_capacity(results.len());
                for (mut entry, embedding) in results {
                    entry.embedding = embedding;
                    embedded_entries.push(entry);
                }

                self.context_store.seed_batch(embedded_entries).await;

                batch_count += 1;
                if batch_count.is_multiple_of(MERGE_EVERY_N_BATCHES) {
                    self.run_episodic_merge().await;
                }
            }

            info!("[volt worker] auto-seed daemon stopped");
        });
    }

    async fn run_episodic_merge(&self) {
        let clusters = self
            .context_store
            .find_clusters(
                ContextKind::Conversation,
                MERGE_CLUSTER_THRESHOLD,
                MERGE_MIN_CLUSTER,
            )
            .await;

        if clusters.is_empty() {
            return;
        }

        let mut all_indices: Vec<usize> = Vec::new();
        let mut replacements: Vec<ContextEntry> = Vec::new();

        for cluster in &clusters {
            if let Some(merged) = self.context_store.merge_episodic_cluster(cluster).await {
                all_indices.extend_from_slice(cluster);
                replacements.push(merged);
            }
        }

        if all_indices.is_empty() {
            return;
        }

        // Collect IDs to delete from DB before removing from memory.
        let ids_to_remove = {
            let entries = self.context_store.entries.read().await;
            all_indices
                .iter()
                .filter_map(|&i| entries.get(i).map(|s| s.entry.id))
                .collect::<Vec<_>>()
        };

        self.context_store.remove_indices(&all_indices).await;

        if let Some(db) = self.context_store.db() {
            if let Err(e) = crate::db::delete_context_entries_by_ids(db, &ids_to_remove).await {
                tracing::warn!(
                    "[volt worker] failed to delete merged entries from DB: {}",
                    e
                );
            }
        }

        let sem = Arc::new(tokio::sync::Semaphore::new(5));
        let embed_futures: Vec<_> = replacements
            .iter()
            .map(|entry| {
                let sem = sem.clone();
                let embedder = self.embedder.clone();
                let text = format!("{}: {}", entry.kind.as_str(), entry.content);
                async move {
                    let _permit = sem.acquire().await.ok();
                    let emb = match embedder.embed_description(&text).await {
                        Ok(e) => Some(e),
                        Err(e) => {
                            tracing::warn!("[worker] embedder failed: {}", e);
                            None
                        }
                    };
                    (entry.clone(), emb)
                }
            })
            .collect();

        let results = futures::future::join_all(embed_futures).await;
        let mut merged_entries: Vec<ContextEntry> = Vec::with_capacity(results.len());
        for (mut entry, embedding) in results {
            entry.embedding = embedding;
            merged_entries.push(entry);
        }

        self.context_store.append_entries(merged_entries).await;

        info!(
            "[volt worker] episodic merge: {} clusters merged into {} high-density entries",
            clusters.len(),
            replacements.len()
        );
    }
}

// ── Auto-seed pre-warm: all context fields ─────────────────────────────────

pub async fn seed_from_workspace(store: &Arc<ContextStore>, embedder: &EmbeddingClient) {
    seed_from_workspace_at(store, embedder, None).await
}

/// Like `seed_from_workspace` but with an explicit workspace directory.
/// When `workspace` is `None`, the current working directory is used.
/// Seeding is silently skipped when the workspace directory does not exist.
pub async fn seed_from_workspace_at(
    store: &Arc<ContextStore>,
    embedder: &EmbeddingClient,
    workspace: Option<&std::path::Path>,
) {
    let cwd = workspace
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));

    if workspace.map_or(false, |p| !p.exists()) {
        tracing::info!("[worker] workspace path {:?} does not exist — skipping workspace seed", workspace.unwrap());
        return;
    }

    let seed_files = [
        ("SOUL.md", ContextKind::SystemPrompt),
        ("MEMORY.md", ContextKind::Memory),
        ("AGENTS.md", ContextKind::Policy),
    ];

    let leak_detector = crate::leak_detector::LeakDetector::new();
    let mut entries: Vec<ContextEntry> = Vec::new();
    let mut leaks_found = 0usize;

    for (filename, kind) in &seed_files {
        let path = cwd.join(filename);
        if path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                let scan = leak_detector.scan(&content);
                if scan.found.is_empty() {
                    entries.push(ContextEntry {
                        id: uuid::Uuid::new_v4(),
                        kind: *kind,
                        content,
                        embedding: None,
                        metadata: serde_json::json!({"source": filename}),
                        frequency: 0,
                        success_rate: 1.0,
                        usage_count: 0,
                        last_used_at: chrono::Utc::now(),
                        created_at: chrono::Utc::now(),
                    });
                } else {
                    leaks_found += scan.found.len();
                    tracing::warn!(
                        "[worker] skipped seeding {} due to {} leaks (categories: {:?})",
                        filename,
                        scan.found.len(),
                        scan.found.iter().map(|m| &m.category).collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    if leaks_found > 0 {
        tracing::warn!(
            "[worker] workspace seed skipped {} files containing {} potential secret leaks",
            leaks_found,
            leaks_found
        );
    }

    if !entries.is_empty() {
        store.seed_batch(entries).await;
        store.compute_embeddings(embedder).await;
        info!(
            "[volt worker] seeded {} workspace files into ContextStore",
            seed_files.len()
        );
    }
}

/// Spawn background seeding: workspace, tool intents, permissions, security.
/// `workspace` is an optional path to the workspace directory. When `None`,
/// the current directory is used.
pub async fn seed_background(
    store: Arc<ContextStore>,
    embedder: EmbeddingClient,
    tools: Arc<ToolRegistry>,
    sandbox: SandboxPolicy,
    workspace: Option<std::path::PathBuf>,
) {
    seed_from_workspace_at(&store, &embedder, workspace.as_deref()).await;
    seed_tool_intents(&store, &tools, &embedder).await;
    seed_permissions(&store, &tools, &embedder).await;
    seed_security_policy(&store, &sandbox, &embedder).await;
}

pub async fn seed_tool_intents(
    store: &Arc<ContextStore>,
    tools: &Arc<ToolRegistry>,
    embedder: &EmbeddingClient,
) {
    let defs = tools.get_definitions().await;
    let total = defs.len();
    let mut entries: Vec<ContextEntry> = Vec::new();

    for def in &defs {
        let content = format!(
            "[Tool Intent: {}]\nCategory: {}\nCapability: {}\nSchema: {}",
            def.name,
            def.category,
            def.description,
            serde_json::to_string(&def.input_schema).unwrap_or_default()
        );
        let perm = tools.get_permission(&def.name).await;
        let perm_label = match perm {
            PermissionLevel::Allow => "allow",
            PermissionLevel::Prompt => "prompt",
            PermissionLevel::ReadOnly => "readonly",
            PermissionLevel::Blocked => "blocked",
        };
        entries.push(ContextEntry {
            id: uuid::Uuid::new_v4(),
            kind: ContextKind::Tool,
            content,
            embedding: None,
            metadata: serde_json::json!({
                "tool_name": def.name,
                "category": def.category,
                "permission": perm_label,
            }),
            frequency: 0,
            success_rate: 1.0,
            usage_count: 0,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        });
    }

    if !entries.is_empty() {
        store.seed_batch(entries).await;
        store.compute_embeddings(embedder).await;
        info!(
            "[volt worker] seeded {} tool intents into ContextStore",
            total
        );
    }
}

pub async fn seed_permissions(
    store: &Arc<ContextStore>,
    tools: &Arc<ToolRegistry>,
    embedder: &EmbeddingClient,
) {
    let defs = tools.get_definitions().await;
    let mut entries: Vec<ContextEntry> = Vec::new();

    for def in &defs {
        let perm = tools.get_permission(&def.name).await;
        let content = format!(
            "[Permission Rule: {}]\nLevel: {}\nTool: {}\nDescription: {}",
            def.name,
            match perm {
                PermissionLevel::Allow => "Allow (auto-execute)",
                PermissionLevel::Prompt => "Prompt (requires human approval)",
                PermissionLevel::ReadOnly => "Read-only (no execution)",
                PermissionLevel::Blocked => "Blocked (denied)",
            },
            def.name,
            def.description,
        );
        let embedding = match embedder.embed_description(&content).await {
            Ok(e) => Some(e),
            Err(e) => {
                tracing::warn!("[worker] embedder failed for skill content: {}", e);
                None
            }
        };
        entries.push(ContextEntry {
            id: uuid::Uuid::new_v4(),
            kind: ContextKind::Permission,
            content,
            embedding,
            metadata: serde_json::json!({
                "tool_name": def.name,
                "permission": match perm {
                    PermissionLevel::Allow => "allow",
                    PermissionLevel::Prompt => "prompt",
                    PermissionLevel::ReadOnly => "readonly",
                    PermissionLevel::Blocked => "blocked",
                },
            }),
            frequency: 0,
            success_rate: 1.0,
            usage_count: 0,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        });
    }

    if !entries.is_empty() {
        let count = entries.len();
        store.seed_batch(entries).await;
        info!(
            "[volt worker] seeded {} permission rules into ContextStore",
            count
        );
    }
}

pub async fn seed_security_policy(
    store: &Arc<ContextStore>,
    sandbox: &SandboxPolicy,
    embedder: &EmbeddingClient,
) {
    let content = format!(
        "[Security Policy]\nSandbox Execution Limits:\n  Timeout: {}ms\n  Max stdout: {} bytes\n  Working directory: {}\n\nConstraints:\n  All sandboxed commands run in isolated Python runner\n  Network access is restricted to configured allowlists\n  Filesystem writes are contained to working directory\n  Tools marked 'Prompt' require explicit human approval per EU AI Act Art. 14",
        sandbox.timeout_ms,
        sandbox.max_stdout_bytes,
        sandbox.working_dir.as_deref().unwrap_or("temp directory"),
    );

    let entries = vec![ContextEntry {
        id: uuid::Uuid::new_v4(),
        kind: ContextKind::Security,
        content,
        embedding: None,
        metadata: serde_json::json!({
            "timeout_ms": sandbox.timeout_ms,
            "max_stdout_bytes": sandbox.max_stdout_bytes,
            "working_dir": sandbox.working_dir,
        }),
        frequency: 0,
        success_rate: 1.0,
        usage_count: 0,
        last_used_at: chrono::Utc::now(),
        created_at: chrono::Utc::now(),
    }];

    store.seed_batch(entries).await;
    store.compute_embeddings(embedder).await;
    info!("[volt worker] seeded security policy into ContextStore");
}

pub async fn seed_skills_from_db(
    store: &Arc<ContextStore>,
    pool: &sqlx::PgPool,
    embedder: &EmbeddingClient,
) {
    if let Ok(skills) = crate::db::list_skills(pool).await {
        let mut entries: Vec<ContextEntry> = Vec::new();
        for skill in skills {
            let content = format!(
                "[Skill: {} v{}]\n{}\nMCP Servers: {}",
                skill.name,
                skill.version,
                skill.description,
                skill.mcp_servers.join(", ")
            );
            entries.push(ContextEntry {
                id: skill.id,
                kind: ContextKind::Skill,
                content,
                embedding: None,
                metadata: serde_json::json!({
                    "skill_name": skill.name,
                    "version": skill.version,
                    "mcp_servers": skill.mcp_servers,
                }),
                frequency: 0,
                success_rate: 1.0,
                usage_count: 0,
                last_used_at: chrono::Utc::now(),
                created_at: chrono::Utc::now(),
            });
        }
        if !entries.is_empty() {
            let count = entries.len();
            store.seed_batch(entries).await;
            store.compute_embeddings(embedder).await;
            info!(
                "[volt worker] seeded {} skills from DB into ContextStore",
                count
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextStore;
    use uuid::Uuid;

    #[test]
    fn test_seed_event_episode_complete_to_context_entry() {
        let event = SeedEvent::EpisodeComplete {
            session_id: Uuid::new_v4(),
            task: "write docs".into(),
            resolution: "wrote README".into(),
            tools_used: vec!["write".into()],
            success: true,
            iteration_count: 3,
        };
        let entry = event.to_context_entry();
        assert_eq!(entry.kind, ContextKind::Conversation);
        assert!(entry.content.contains("Episodic Memory"));
        assert!(entry.content.contains("write docs"));
    }

    #[test]
    fn test_seed_event_artifact_to_context_entry() {
        let event = SeedEvent::ArtifactCreated {
            file_path: "src/main.rs".into(),
            description: "entry point".into(),
            language: "Rust".into(),
            tool_used: "write".into(),
        };
        let entry = event.to_context_entry();
        assert_eq!(entry.kind, ContextKind::Artifact);
        assert!(entry.content.contains("src/main.rs"));
    }

    #[test]
    fn test_seed_event_mcp_to_context_entry() {
        let event = SeedEvent::MCPRegistered {
            server_name: "filesystem".into(),
            tools: vec!["read".into(), "write".into()],
            intent_descriptors: vec!["file ops".into()],
        };
        let entry = event.to_context_entry();
        assert_eq!(entry.kind, ContextKind::MCPConfig);
        assert!(entry.content.contains("filesystem"));
    }

    #[test]
    fn test_seed_channel_send_receive() {
        let (tx, mut rx) = create_seed_channel();
        tx.send(SeedEvent::EpisodeComplete {
            session_id: Uuid::new_v4(),
            task: "test".into(),
            resolution: "done".into(),
            tools_used: vec![],
            success: true,
            iteration_count: 1,
        });
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_seed_channel_dropped_receiver_no_panic() {
        let (tx, _rx) = create_seed_channel();
        tx.send(SeedEvent::EpisodeComplete {
            session_id: Uuid::new_v4(),
            task: "test".into(),
            resolution: "done".into(),
            tools_used: vec![],
            success: true,
            iteration_count: 1,
        });
    }

    #[tokio::test]
    async fn test_seed_batch_dedup() {
        let store = ContextStore::new();
        let entry = ContextEntry {
            id: Uuid::new_v4(),
            kind: ContextKind::Tool,
            content: "echo tool".into(),
            embedding: Some(vec![0.5, 0.5, 0.5, 0.5]),
            metadata: serde_json::json!({}),
            frequency: 1,
            success_rate: 1.0,
            usage_count: 1,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        };
        let dup = ContextEntry {
            id: Uuid::new_v4(),
            kind: ContextKind::Tool,
            content: "echo tool".into(),
            embedding: Some(vec![0.5, 0.5, 0.5, 0.5]),
            metadata: serde_json::json!({}),
            frequency: 1,
            success_rate: 1.0,
            usage_count: 1,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        };
        store.seed_batch(vec![entry, dup]).await;
        assert_eq!(store.len().await, 1, "identical entries should be deduped");
    }
}
