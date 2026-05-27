use crate::cosine_similarity;
use crate::embedding::EmbeddingClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ── Context kinds — every context field an agent ingests or produces ────────

/// The 12 kinds of context entries stored in the unified RAG store.
/// Each kind has its own quota and is seeded from different sources.
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

/// Parse a list of context kind strings into a vector of ContextKind values.
/// Unknown strings are skipped with a warning. Returns all kinds if input is empty.
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

// ── Universal context entry ───────────────────────────────────────────────

/// A single entry in the unified context store — kind, content, embedding, metadata, and stats.
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

// ── In-memory context store ────────────────────────────────────────────────

/// The unified RAG context store. Manages 12 kinds of context entries with vector search,
/// four-pillar eviction, episodic merging, and optional pgvector persistence.
pub struct ContextStore {
    entries: RwLock<Vec<StoredEntry>>,
    insert_count: RwLock<usize>,
    db: std::sync::OnceLock<sqlx::PgPool>,
    quota_overrides: RwLock<std::collections::HashMap<ContextKind, usize>>,
    evict_every: RwLock<usize>,
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
            quota_overrides: RwLock::new(std::collections::HashMap::new()),
            evict_every: RwLock::new(100),
        })
    }

    pub fn new_with_db(pool: sqlx::PgPool) -> Arc<Self> {
        let db = std::sync::OnceLock::new();
        let _ = db.set(pool);
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
            insert_count: RwLock::new(0),
            db,
            quota_overrides: RwLock::new(std::collections::HashMap::new()),
            evict_every: RwLock::new(100),
        })
    }

    pub fn set_db(&self, pool: sqlx::PgPool) {
        let _ = self.db.set(pool);
    }

    /// Set per-kind quota overrides. Merges with hardcoded defaults.
    pub async fn set_quotas(&self, overrides: &std::collections::HashMap<ContextKind, usize>) {
        let mut q = self.quota_overrides.write().await;
        q.clear();
        for (k, v) in overrides {
            q.insert(*k, *v);
        }
    }

    /// Set the eviction frequency (number of inserts between eviction passes).
    /// Default: 100. Lower values = more frequent cleanup, higher = more memory.
    pub async fn set_evict_every(&self, n: usize) {
        *self.evict_every.write().await = n;
    }

    /// Append entries directly to the store (used by episodic merger).
    pub async fn append_entries(&self, entries: Vec<ContextEntry>) {
        let mut store = self.entries.write().await;
        for entry in entries {
            store.push(StoredEntry { entry });
        }
    }

    async fn quota_for(&self, kind: ContextKind) -> usize {
        let overrides = self.quota_overrides.read().await;
        overrides
            .get(&kind)
            .copied()
            .unwrap_or_else(|| kind.quota())
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
        query_text: Option<&str>,
    ) -> Vec<ContextEntry> {
        // 1. Prefer pgvector HNSW when a DB pool is available
        if let Some(pool) = self.db() {
            let kind_str = kind_filter.as_ref().map(|k| k.as_str());
            // Ask for 2x limit so composite re-ranking has room to improve ordering
            let db_limit = (limit as i64) * 2;
            match crate::db::search_context_entries(
                pool,
                query_embedding,
                db_limit,
                kind_str,
                min_score,
            )
            .await
            {
                Ok(db_entries) => {
                    let mut scored: Vec<(f32, ContextEntry)> = db_entries
                        .into_iter()
                        .map(|mut e| {
                            let sim = e
                                .embedding
                                .as_ref()
                                .map(|emb| cosine_similarity(emb, query_embedding))
                                .unwrap_or(0.0);
                            let score = 0.6 * sim + 0.4 * e.composite_score();
                            e.frequency += 1;
                            e.last_used_at = chrono::Utc::now();
                            (score, e)
                        })
                        .collect();
                    scored
                        .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                    return scored
                        .into_iter()
                        .filter(|(score, _)| *score >= min_score)
                        .take(limit)
                        .map(|(_, e)| e)
                        .collect();
                }
                Err(e) => {
                    tracing::warn!("pgvector search failed, falling back to in-memory: {}", e);
                }
            }
        }

        // 2. Fallback: in-memory brute-force scan with hybrid BM25 + dense fusion
        let entries = self.entries.read().await;

        // Filter candidates by kind and embedding existence
        let candidates: Vec<&StoredEntry> = entries
            .iter()
            .filter(|s| {
                s.entry.embedding.is_some()
                    && kind_filter.as_ref().is_none_or(|k| s.entry.kind == *k)
            })
            .collect();

        if candidates.is_empty() {
            return Vec::new();
        }

        // Compute cosine similarity scores
        let mut cosine_ranked: Vec<(f32, usize)> = candidates
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let emb = s.entry.embedding.as_ref()?;
                let sim = cosine_similarity(emb, query_embedding);
                Some((sim, i))
            })
            .collect();
        cosine_ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let cosine_order: Vec<usize> = cosine_ranked.iter().map(|(_, i)| *i).collect();

        // Compute BM25 scores if query text is provided
        let bm25_order: Vec<usize> = if let Some(qt) = query_text {
            let corpus: Vec<String> = candidates
                .iter()
                .map(|s| format!("{}: {}", s.entry.kind.as_str(), s.entry.content))
                .collect();
            let bm25 = crate::vector_index::Bm25Scorer::build(
                corpus.iter().enumerate().map(|(i, t)| (i, t.as_str())),
                1.2,
                0.75,
                0.5,
            );
            let bm25_results = bm25.search(qt);
            bm25_results.iter().map(|(idx, _)| *idx).collect()
        } else {
            Vec::new()
        };

        // Fuse rankings using RRF if both signals available
        let mut final_order: Vec<(f32, usize)> = if !bm25_order.is_empty() {
            let rankings: Vec<Vec<usize>> = vec![cosine_order, bm25_order];
            let rrf =
                crate::vector_index::reciprocal_rank_fusion(&rankings, 60.0, candidates.len());
            rrf.into_iter()
                .enumerate()
                .map(|(rank, (idx, _))| {
                    let entry = &candidates[idx].entry;
                    let rrf_score = 1.0 / (60.0 + rank as f32 + 1.0);
                    let composite = entry.composite_score();
                    (0.6 * rrf_score + 0.4 * composite, idx)
                })
                .collect()
        } else {
            cosine_ranked
                .into_iter()
                .map(|(_, idx)| {
                    let entry = &candidates[idx].entry;
                    let sim_score = candidates[idx]
                        .entry
                        .embedding
                        .as_ref()
                        .map(|emb| cosine_similarity(emb, query_embedding))
                        .unwrap_or(0.0);
                    let composite = entry.composite_score();
                    (0.6 * sim_score + 0.4 * composite, idx)
                })
                .collect()
        };

        final_order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        final_order
            .into_iter()
            .filter(|(score, _)| *score >= min_score)
            .take(limit)
            .map(|(_, idx)| {
                let mut entry = candidates[idx].entry.clone();
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
        if *count >= *self.evict_every.read().await {
            *count = 0;
            drop(count);

            // 3. Per-kind quota enforcement
            let mut kind_counts: HashMap<ContextKind, Vec<usize>> = HashMap::new();
            for (i, s) in store.iter().enumerate() {
                kind_counts.entry(s.entry.kind).or_default().push(i);
            }

            let mut indices_to_remove: Vec<usize> = Vec::new();
            for (kind, indices) in &kind_counts {
                let quota = self.quota_for(*kind).await;
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
        // Pre-filter to this kind with embeddings to avoid O(n²) over all entries
        let kind_entries: Vec<(usize, &StoredEntry)> = entries
            .iter()
            .enumerate()
            .filter(|(_, s)| s.entry.kind == kind && s.entry.embedding.is_some())
            .collect();
        let n = kind_entries.len();
        let mut clusters: Vec<Vec<usize>> = Vec::new();
        let mut visited = vec![false; n];

        for i in 0..n {
            if visited[i] {
                continue;
            }
            let mut cluster = vec![kind_entries[i].0];
            visited[i] = true;
            let Some(emb_i) = kind_entries[i].1.entry.embedding.as_ref() else {
                continue;
            };

            for j in (i + 1)..n {
                if visited[j] {
                    continue;
                }
                let Some(emb_j) = kind_entries[j].1.entry.embedding.as_ref() else {
                    continue;
                };
                let sim = cosine_similarity(emb_i, emb_j);
                if sim >= threshold {
                    cluster.push(kind_entries[j].0);
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

        let kind = members[0].kind;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_kind_ablation() {
        let store = ContextStore::new();

        // Seed entries of different kinds with deterministic embeddings
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

        // Search for tool-kind only — should return exactly the tool entry
        let tool_results = store
            .search(&tool_emb, 8, Some(ContextKind::Tool), 0.0, None)
            .await;
        assert_eq!(tool_results.len(), 1);
        assert_eq!(tool_results[0].id, tool_id);
        assert_eq!(tool_results[0].kind, ContextKind::Tool);

        // Search for skill-kind only
        let skill_results = store
            .search(&skill_emb, 8, Some(ContextKind::Skill), 0.0, None)
            .await;
        assert_eq!(skill_results.len(), 1);
        assert_eq!(skill_results[0].id, skill_id);

        // Search for memory-kind only
        let memory_results = store
            .search(&memory_emb, 8, Some(ContextKind::Memory), 0.0, None)
            .await;
        assert_eq!(memory_results.len(), 1);
        assert_eq!(memory_results[0].id, memory_id);

        // Search with no filter — all 3 should appear
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

        // Query aligned with skill but filter to Tool — should still return the Tool entry
        // (similarity is low but composite score keeps it above min_score=0.0).
        // The key ablation invariant: kind filter never lets excluded kinds through.
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
