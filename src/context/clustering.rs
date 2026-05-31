use super::{ContextEntry, ContextKind, ContextStore};
use crate::cosine_similarity;
use uuid::Uuid;

impl ContextStore {
    pub async fn find_clusters(
        &self,
        kind: ContextKind,
        threshold: f32,
        min_cluster: usize,
    ) -> Vec<Vec<usize>> {
        let entries = self.entries.read().await;
        let kind_entries: Vec<(usize, &super::StoredEntry)> = entries
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
