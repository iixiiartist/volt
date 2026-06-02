mod clustering;
mod eviction;
mod persistence;
mod search;

#[cfg(feature = "tools-turbovec")]
use crate::turbovec_index::TurbovecIndex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
#[cfg(feature = "tools-turbovec")]
use std::sync::RwLock as SyncRwLock;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContextKind {
    Skill,
    Tool,
    Memory,
    Conversation,
    AgentRun,
    Artifact,
    SystemPrompt,
    FewShot,
    Policy,
    Permission,
    Security,
    MCPConfig,
}

impl ContextKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextKind::Skill => "skill",
            ContextKind::Tool => "tool",
            ContextKind::Memory => "memory",
            ContextKind::Conversation => "conversation",
            ContextKind::AgentRun => "agent_run",
            ContextKind::Artifact => "artifact",
            ContextKind::SystemPrompt => "system_prompt",
            ContextKind::FewShot => "few_shot",
            ContextKind::Policy => "policy",
            ContextKind::Permission => "permission",
            ContextKind::Security => "security",
            ContextKind::MCPConfig => "mcp_config",
        }
    }

    pub fn quota(&self) -> usize {
        match self {
            ContextKind::Skill => 200,
            ContextKind::Tool => 500,
            ContextKind::Memory => 500,
            ContextKind::Conversation => 300,
            ContextKind::AgentRun => 200,
            ContextKind::Artifact => 300,
            ContextKind::SystemPrompt => 20,
            ContextKind::FewShot => 50,
            ContextKind::Policy => 50,
            ContextKind::Permission => 50,
            ContextKind::Security => 30,
            ContextKind::MCPConfig => 100,
        }
    }
}

pub fn parse_context_kinds(input: &[String]) -> Vec<ContextKind> {
    if input.is_empty() {
        return crate::models::default_context_kinds();
    }
    input
        .iter()
        .filter_map(|s| match s.to_lowercase().as_str() {
            "tool" => Some(ContextKind::Tool),
            "skill" => Some(ContextKind::Skill),
            "memory" => Some(ContextKind::Memory),
            "conversation" => Some(ContextKind::Conversation),
            "agent_run" | "agentrun" => Some(ContextKind::AgentRun),
            "artifact" => Some(ContextKind::Artifact),
            "system_prompt" | "systemprompt" => Some(ContextKind::SystemPrompt),
            "few_shot" | "fewshot" => Some(ContextKind::FewShot),
            "policy" => Some(ContextKind::Policy),
            "permission" => Some(ContextKind::Permission),
            "security" => Some(ContextKind::Security),
            "mcp_config" | "mcpconfig" => Some(ContextKind::MCPConfig),
            _ => {
                eprintln!("[warn] unknown context kind '{}', skipping", s);
                None
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    pub id: Uuid,
    pub kind: ContextKind,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub metadata: Value,
    pub frequency: u32,
    pub success_rate: f32,
    pub usage_count: u32,
    pub last_used_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ContextEntry {
    pub fn composite_score(&self) -> f32 {
        let recency_days = (chrono::Utc::now() - self.last_used_at).num_hours() as f32 / 24.0;
        let recency = (-recency_days / 30.0).exp();
        let freq = (1.0 + self.frequency as f32).ln();
        let success = if self.usage_count > 0 {
            self.success_rate
        } else {
            0.5
        };
        0.4 * recency
            + 0.3 * success
            + 0.2 * freq
            + 0.1 * (self.content.len() as f32 / 1000.0).min(1.0)
    }
}

pub struct ContextStore {
    pub(crate) entries: RwLock<Vec<StoredEntry>>,
    pub(crate) insert_count: AtomicUsize,
    pub(crate) db: std::sync::OnceLock<sqlx::PgPool>,
    pub(crate) quota_overrides: RwLock<HashMap<ContextKind, usize>>,
    pub(crate) evict_every: RwLock<usize>,
    #[cfg(feature = "tools-turbovec")]
    pub(crate) turbovec: SyncRwLock<Option<TurbovecIndex>>,
}

pub struct StoredEntry {
    pub entry: ContextEntry,
}

impl ContextStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
            insert_count: AtomicUsize::new(0),
            db: std::sync::OnceLock::new(),
            quota_overrides: RwLock::new(HashMap::new()),
            evict_every: RwLock::new(100),
            #[cfg(feature = "tools-turbovec")]
            turbovec: SyncRwLock::new(None),
        })
    }

    pub fn new_with_db(pool: sqlx::PgPool) -> Arc<Self> {
        let db = std::sync::OnceLock::new();
        let _ = db.set(pool);
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
            insert_count: AtomicUsize::new(0),
            db,
            quota_overrides: RwLock::new(HashMap::new()),
            evict_every: RwLock::new(100),
            #[cfg(feature = "tools-turbovec")]
            turbovec: SyncRwLock::new(None),
        })
    }

    #[cfg(feature = "tools-turbovec")]
    pub fn with_turbovec(self: Arc<Self>) -> Arc<Self> {
        match TurbovecIndex::new() {
            Ok(idx) => {
                *self.turbovec.write().unwrap_or_else(|e| e.into_inner()) = Some(idx);
                tracing::info!("turbovec accelerated index enabled");
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to create turbovec index, falling back to brute-force");
            }
        }
        self
    }

    pub(crate) fn db(&self) -> Option<&sqlx::PgPool> {
        self.db.get()
    }

    pub async fn append_entries(&self, entries: Vec<ContextEntry>) {
        if let Some(db) = self.db() {
            if let Err(e) = crate::db::bulk_insert_context_entries(db, &entries).await {
                tracing::warn!("[context] append_entries DB bulk insert failed: {}", e);
            }
        }
        let mut store = self.entries.write().await;
        for entry in entries {
            store.push(StoredEntry { entry });
        }
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    pub async fn add(&self, kind: ContextKind, content: &str, metadata: Value) -> Uuid {
        let id = Uuid::new_v4();
        let entry = ContextEntry {
            id,
            kind,
            content: content.to_string(),
            embedding: None,
            metadata,
            frequency: 0,
            success_rate: 0.0,
            usage_count: 0,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        };
        self.entries.write().await.push(StoredEntry { entry });
        id
    }

    pub async fn learn(&self, entry_id: Uuid, success: bool) {
        let mut entries = self.entries.write().await;
        if let Some(s) = entries.iter_mut().find(|s| s.entry.id == entry_id) {
            s.entry.usage_count += 1;
            let count = s.entry.usage_count as f32;
            let rate = s.entry.success_rate;
            s.entry.success_rate = (rate * (count - 1.0) + if success { 1.0 } else { 0.0 }) / count;
        }
    }

    pub async fn record_run(
        &self,
        query: &str,
        tool_name: &str,
        success: bool,
        metadata: Value,
    ) -> Uuid {
        let content = format!(
            "Query: {}\nTool used: {}\nSuccess: {}",
            query, tool_name, success
        );
        let id = self.add(ContextKind::AgentRun, &content, metadata).await;
        self.learn(id, success).await;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_kind_ablation() {
        let store = ContextStore::new();

        let tool_emb = vec![1.0, 0.0, 0.0, 0.0];
        let skill_emb = vec![0.0, 1.0, 0.0, 0.0];
        let memory_emb = vec![0.0, 0.0, 1.0, 0.0];

        let tool_id = store
            .add(ContextKind::Tool, "read file", serde_json::json!({}))
            .await;
        let skill_id = store
            .add(ContextKind::Skill, "parse markdown", serde_json::json!({}))
            .await;
        let memory_id = store
            .add(
                ContextKind::Memory,
                "user likes rust",
                serde_json::json!({}),
            )
            .await;

        {
            let mut entries = store.entries.write().await;
            for e in entries.iter_mut() {
                match e.entry.kind {
                    ContextKind::Tool => e.entry.embedding = Some(tool_emb.clone()),
                    ContextKind::Skill => e.entry.embedding = Some(skill_emb.clone()),
                    ContextKind::Memory => e.entry.embedding = Some(memory_emb.clone()),
                    _ => {}
                }
            }
        }

        let tool_results = store
            .search(&tool_emb, 8, Some(ContextKind::Tool), 0.0, None)
            .await;
        assert_eq!(tool_results.len(), 1);
        assert_eq!(tool_results[0].id, tool_id);
        assert_eq!(tool_results[0].kind, ContextKind::Tool);

        let skill_results = store
            .search(&skill_emb, 8, Some(ContextKind::Skill), 0.0, None)
            .await;
        assert_eq!(skill_results.len(), 1);
        assert_eq!(skill_results[0].id, skill_id);

        let memory_results = store
            .search(&memory_emb, 8, Some(ContextKind::Memory), 0.0, None)
            .await;
        assert_eq!(memory_results.len(), 1);
        assert_eq!(memory_results[0].id, memory_id);

        let all_results = store.search(&tool_emb, 8, None, 0.0, None).await;
        assert_eq!(all_results.len(), 3);
    }

    #[tokio::test]
    async fn test_context_kind_ablation_excludes_disabled() {
        let store = ContextStore::new();

        let tool_emb = vec![1.0, 0.0, 0.0, 0.0];
        let skill_emb = vec![0.0, 1.0, 0.0, 0.0];

        store
            .add(ContextKind::Tool, "read file", serde_json::json!({}))
            .await;
        store
            .add(ContextKind::Skill, "parse markdown", serde_json::json!({}))
            .await;

        {
            let mut entries = store.entries.write().await;
            for e in entries.iter_mut() {
                match e.entry.kind {
                    ContextKind::Tool => e.entry.embedding = Some(tool_emb.clone()),
                    ContextKind::Skill => e.entry.embedding = Some(skill_emb.clone()),
                    _ => {}
                }
            }
        }

        let filtered = store
            .search(&skill_emb, 8, Some(ContextKind::Tool), 0.0, None)
            .await;
        assert!(!filtered.is_empty());
        for entry in &filtered {
            assert_eq!(entry.kind, ContextKind::Tool);
        }
        assert!(!filtered.iter().any(|e| e.kind == ContextKind::Skill));
    }
}
