use crate::embedding::EmbeddingClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ── Context kinds — every context field an agent ingests or produces ────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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

impl ContextEntry {
    fn composite_score(&self) -> f32 {
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

// ── In-memory context store ────────────────────────────────────────────────

pub struct ContextStore {
    pub entries: RwLock<Vec<StoredEntry>>,
    insert_count: RwLock<usize>,
    db: std::sync::OnceLock<sqlx::PgPool>,
}

pub struct StoredEntry {
    pub entry: ContextEntry,
}

impl ContextStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
            insert_count: RwLock::new(0),
            db: std::sync::OnceLock::new(),
        })
    }

    pub fn new_with_db(pool: sqlx::PgPool) -> Arc<Self> {
        let db = std::sync::OnceLock::new();
        let _ = db.set(pool);
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
            insert_count: RwLock::new(0),
            db,
        })
    }

    pub fn set_db(&self, pool: sqlx::PgPool) {
        let _ = self.db.set(pool);
    }

    fn db(&self) -> Option<&sqlx::PgPool> {
        self.db.get()
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
            entries
                .iter()
                .filter(|s| s.entry.embedding.is_none())
                .map(|s| {
                    (
                        s.entry.id,
                        format!("{}: {}", s.entry.kind.as_str(), s.entry.content),
                    )
                })
                .collect()
        };

        if items.is_empty() {
            return;
        }

        let sem = Arc::new(tokio::sync::Semaphore::new(5));
        let results: Vec<(Uuid, Option<Vec<f32>>)> =
            futures::future::join_all(items.into_iter().map(|(id, text)| {
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
                    && kind_filter.as_ref().is_none_or(|k| s.entry.kind == *k)
            })
            .map(|s| {
                let emb = s.entry.embedding.as_ref().unwrap();
                let sim = cosine_similarity(emb, query_embedding);
                let score = 0.6 * sim + 0.4 * s.entry.composite_score();
                (score, s)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        scored
            .into_iter()
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

    /// Load existing context entries from PostgreSQL into the in-memory store.
    pub async fn hydrate_from_db(&self, limit: i64) -> anyhow::Result<usize> {
        let db = self
            .db()
            .ok_or_else(|| anyhow::anyhow!("no database connection configured"))?;
        let entries = crate::db::load_context_entries(db, limit).await?;
        let count = entries.len();
        let mut store = self.entries.write().await;
        for entry in entries {
            store.push(StoredEntry { entry });
        }
        Ok(count)
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    // ── Seed batch with dedup and quota eviction ─────────────────────────

    const DEDUP_THRESHOLD: f32 = 0.92;
    const EVICT_EVERY_N_INSERTS: usize = 100;

    pub async fn seed_batch(&self, entries: Vec<ContextEntry>) {
        let mut store = self.entries.write().await;
        let mut inserted = 0usize;

        for entry in entries {
            // 1. Semantic dedup: if an existing entry with same kind has
            //    cosine ≥ DEDUP_THRESHOLD, merge frequencies instead of inserting.
            if let Some(ref emb) = entry.embedding {
                let mut merged = false;
                for existing in store.iter_mut() {
                    if existing.entry.kind != entry.kind {
                        continue;
                    }
                    if let Some(ref existing_emb) = existing.entry.embedding {
                        let sim = cosine_similarity(emb, existing_emb);
                        if sim >= Self::DEDUP_THRESHOLD {
                            existing.entry.frequency += 1;
                            existing.entry.last_used_at = chrono::Utc::now();
                            merged = true;
                            break;
                        }
                    }
                }
                if merged {
                    continue;
                }
            }

            store.push(StoredEntry {
                entry: entry.clone(),
            });
            inserted += 1;

            // Persist to PostgreSQL if available
            if let Some(db) = self.db() {
                if entry.embedding.is_some() {
                    let _ = crate::db::insert_context_entry(db, &entry).await;
                }
            }
        }

        if inserted > 0 && self.db().is_some() {
            // Persistence complete — background write handled above
        }

        // 2. Track insert count; evict periodically
        let mut count = self.insert_count.write().await;
        *count += inserted;
        if *count >= Self::EVICT_EVERY_N_INSERTS {
            *count = 0;
            drop(count);

            // 3. Per-kind quota enforcement
            let mut kind_counts: HashMap<ContextKind, Vec<usize>> = HashMap::new();
            for (i, s) in store.iter().enumerate() {
                kind_counts.entry(s.entry.kind.clone()).or_default().push(i);
            }

            let mut indices_to_remove: Vec<usize> = Vec::new();
            for (kind, indices) in &kind_counts {
                let quota = kind.quota();
                if indices.len() <= quota {
                    continue;
                }
                let excess = indices.len() - quota;
                let mut scored: Vec<(f32, usize)> = indices
                    .iter()
                    .map(|&idx| (store[idx].entry.composite_score(), idx))
                    .collect();
                scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                for (_score, idx) in scored.iter().take(excess) {
                    indices_to_remove.push(*idx);
                }
            }

            if !indices_to_remove.is_empty() {
                indices_to_remove.sort_unstable();
                indices_to_remove.reverse();
                for idx in indices_to_remove {
                    store.remove(idx);
                }
            }
        }
    }

    // ── Episodic cluster detection & merging ─────────────────────────────

    /// Find clusters of the same kind with cosine ≥ threshold.
    /// Returns groups of entry indices that should be merged.
    pub async fn find_clusters(
        &self,
        kind: ContextKind,
        threshold: f32,
        min_cluster: usize,
    ) -> Vec<Vec<usize>> {
        let entries = self.entries.read().await;
        let mut clusters: Vec<Vec<usize>> = Vec::new();
        let mut visited = vec![false; entries.len()];

        for i in 0..entries.len() {
            if visited[i] || entries[i].entry.kind != kind || entries[i].entry.embedding.is_none() {
                continue;
            }
            let mut cluster = vec![i];
            visited[i] = true;
            let emb_i = entries[i].entry.embedding.as_ref().unwrap();

            for j in (i + 1)..entries.len() {
                if visited[j]
                    || entries[j].entry.kind != kind
                    || entries[j].entry.embedding.is_none()
                {
                    continue;
                }
                let emb_j = entries[j].entry.embedding.as_ref().unwrap();
                let sim = cosine_similarity(emb_i, emb_j);
                if sim >= threshold {
                    cluster.push(j);
                    visited[j] = true;
                }
            }

            if cluster.len() >= min_cluster {
                clusters.push(cluster);
            }
        }

        clusters
    }

    /// Merge a cluster of entries into a single higher-density entry.
    /// Returns the replacement entry (caller should remove originals and insert this).
    pub async fn merge_episodic_cluster(&self, cluster_indices: &[usize]) -> Option<ContextEntry> {
        let entries = self.entries.read().await;
        if cluster_indices.len() < 2 {
            return None;
        }

        let members: Vec<&ContextEntry> = cluster_indices
            .iter()
            .filter_map(|&i| entries.get(i))
            .map(|s| &s.entry)
            .collect();

        if members.is_empty() {
            return None;
        }

        let kind = members[0].kind.clone();
        let total_freq: u32 = members.iter().map(|e| e.frequency).sum();
        let total_usage: u32 = members.iter().map(|e| e.usage_count).sum();
        let avg_success: f32 = if total_usage > 0 {
            members
                .iter()
                .map(|e| e.success_rate * e.usage_count as f32)
                .sum::<f32>()
                / total_usage as f32
        } else {
            0.5
        };

        let merged_content = format!(
            "[Merged Episodic Memory — {} related runs]\nCore Problem: {}\nResolution Pattern: {}\nTotal occurrences: {}",
            members.len(),
            members.iter().filter_map(|e| {
                let c = &e.content;
                if c.contains("User Problem:") {
                    c.lines()
                        .find(|l| l.contains("User Problem:"))
                        .map(|l| l.trim_start_matches("User Problem:").trim().to_string())
                } else {
                    Some(c.chars().take(80).collect())
                }
            }).next().unwrap_or_default(),
            members.iter().filter_map(|e| {
                let c = &e.content;
                if c.contains("Resolution:") {
                    c.lines()
                        .find(|l| l.contains("Resolution:"))
                        .map(|l| l.trim_start_matches("Resolution:").trim().to_string())
                } else {
                    None
                }
            }).next().unwrap_or_else(|| "see merged runs".into()),
            total_freq,
        );

        Some(ContextEntry {
            id: Uuid::new_v4(),
            kind,
            content: merged_content,
            embedding: None,
            metadata: serde_json::json!({
                "merged_from": cluster_indices.len(),
                "total_frequency": total_freq,
                "total_usage": total_usage,
                "avg_success_rate": avg_success,
            }),
            frequency: total_freq,
            success_rate: avg_success,
            usage_count: total_usage,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        })
    }

    /// Remove entries at the given indices (must be sorted descending).
    pub async fn remove_indices(&self, indices: &[usize]) {
        let mut entries = self.entries.write().await;
        let mut sorted: Vec<usize> = indices.to_vec();
        sorted.sort_unstable();
        sorted.reverse();
        for idx in sorted {
            if idx < entries.len() {
                entries.remove(idx);
            }
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b).max(f32::EPSILON)
}
