use crate::embedding::EmbeddingClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ── Context kinds ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContextKind {
    Skill,
    Memory,
    AgentRun,
    Artifact,
    SystemPrompt,
    FewShot,
    Policy,
}

impl ContextKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextKind::Skill => "skill",
            ContextKind::Memory => "memory",
            ContextKind::AgentRun => "agent_run",
            ContextKind::Artifact => "artifact",
            ContextKind::SystemPrompt => "system_prompt",
            ContextKind::FewShot => "few_shot",
            ContextKind::Policy => "policy",
        }
    }
}

// ── Universal context entry ───────────────────────────────────────────────

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

// ── In-memory context store ────────────────────────────────────────────────

pub struct ContextStore {
    entries: RwLock<Vec<StoredEntry>>,
}

struct StoredEntry {
    entry: ContextEntry,
}

impl ContextStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
        })
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

    pub async fn compute_embeddings(&self, embedder: &EmbeddingClient) {
        let items: Vec<(Uuid, String)> = {
            let entries = self.entries.read().await;
            entries.iter().map(|s| (s.entry.id, format!("{}: {}", s.entry.kind.as_str(), s.entry.content))).collect()
        };

        let sem = Arc::new(tokio::sync::Semaphore::new(5));
        let results: Vec<(Uuid, Option<Vec<f32>>)> = futures::future::join_all(items.into_iter().map(|(id, text)| {
            let sem = sem.clone();
            async move {
                let _permit = sem.acquire().await.ok();
                let emb = embedder.embed_description(&text).await.ok();
                (id, emb)
            }
        }))
        .await;

        let mut entries = self.entries.write().await;
        for (id, emb) in results {
            if let Some(s) = entries.iter_mut().find(|s| s.entry.id == id) {
                s.entry.embedding = emb;
            }
        }
    }

    pub async fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        kind_filter: Option<ContextKind>,
        min_score: f32,
    ) -> Vec<ContextEntry> {
        let entries = self.entries.read().await;
        let mut scored: Vec<(f32, &StoredEntry)> = entries
            .iter()
            .filter(|s| {
                s.entry.embedding.is_some()
                    && kind_filter.as_ref().map_or(true, |k| s.entry.kind == *k)
            })
            .map(|s| {
                let emb = s.entry.embedding.as_ref().unwrap();
                let sim = cosine_similarity(emb, query_embedding);

                let recency_days = (chrono::Utc::now() - s.entry.last_used_at).num_hours() as f32 / 24.0;
                let recency = (-recency_days / 30.0).exp();

                let freq = (1.0 + s.entry.frequency as f32).ln();

                let success = if s.entry.usage_count > 0 {
                    s.entry.success_rate
                } else {
                    0.5
                };

                let score = 0.6 * sim + 0.2 * success + 0.1 * recency + 0.1 * freq;
                (score, s)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        scored.into_iter()
            .filter(|(score, _)| *score >= min_score)
            .take(limit)
            .map(|(_, s)| {
                let mut entry = s.entry.clone();
                entry.frequency += 1;
                entry.last_used_at = chrono::Utc::now();
                entry
            })
            .collect()
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

    pub async fn record_run(&self, query: &str, tool_name: &str, success: bool, metadata: Value) -> Uuid {
        let content = format!("Query: {}\nTool used: {}\nSuccess: {}", query, tool_name, success);
        let id = self.add(ContextKind::AgentRun, &content, metadata).await;
        self.learn(id, success).await;
        id
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b).max(f32::EPSILON)
}
