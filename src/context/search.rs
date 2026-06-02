use super::{ContextEntry, ContextKind, ContextStore, StoredEntry};
use crate::cosine_similarity;
use crate::embedding::EmbeddingClient;
use futures::future::join_all;
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

impl ContextStore {
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

        let sem = Arc::new(Semaphore::new(5));
        let results: Vec<(Uuid, Option<Vec<f32>>)> =
            join_all(items.into_iter().map(|(id, text)| {
                let sem = sem.clone();
                async move {
                    let _permit = sem.acquire().await.ok();
                    let emb = embedder.embed_description(&text).await.ok();
                    (id, emb)
                }
            }))
            .await;

        let mut newly_embedded: Vec<(usize, Vec<f32>)> = Vec::new();
        let mut updates: Vec<(Uuid, Vec<f32>)> = Vec::new();
        let mut entries = self.entries.write().await;
        for (id, emb) in results {
            if let Some(emb_vec) = emb {
                if let Some(pos) = entries.iter().position(|e| e.entry.id == id) {
                    entries[pos].entry.embedding = Some(emb_vec.clone());
                    newly_embedded.push((pos, emb_vec.clone()));
                    updates.push((id, emb_vec));
                }
            }
        }

        if let Some(db) = self.db() {
            if !updates.is_empty() {
                if let Err(e) = crate::db::bulk_update_embeddings(db, &updates).await {
                    tracing::warn!("[context] bulk_update_embeddings failed: {}", e);
                }
            }
        }

        #[cfg(feature = "tools-turbovec")]
        if let Some(ref tv) = *self.turbovec.read().unwrap_or_else(|e| e.into_inner()) {
            if !newly_embedded.is_empty() {
                let flat_vecs: Vec<f32> =
                    newly_embedded.iter().flat_map(|(_, v)| v.clone()).collect();
                let entry_positions: Vec<usize> =
                    newly_embedded.iter().map(|(idx, _)| *idx).collect();
                tv.add(&flat_vecs, &entry_positions);
                tv.prepare();
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
        if let Some(pool) = self.db() {
            let kind_str = kind_filter.as_ref().map(|k| k.as_str());
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

        let entries = self.entries.read().await;

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

        #[cfg(feature = "tools-turbovec")]
        if let Some(ref tv) = *self.turbovec.read().unwrap_or_else(|e| e.into_inner()) {
            if tv.len() > 0 {
                let mask: Vec<bool> = entries
                    .iter()
                    .map(|s| {
                        s.entry.embedding.is_some()
                            && kind_filter.as_ref().is_none_or(|k| s.entry.kind == *k)
                    })
                    .collect();
                let tv_results = tv.search_with_mask(query_embedding, limit * 2, &mask);
                if !tv_results.is_empty() {
                    let mut scored: Vec<(f32, ContextEntry)> = tv_results
                        .into_iter()
                        .filter_map(|(l2_dist, entry_idx)| {
                            let entry = entries.get(entry_idx)?.entry.clone();
                            let composite = entry.composite_score();
                            let sim = 1.0 / (1.0 + l2_dist.sqrt());
                            Some((0.6 * sim + 0.4 * composite, entry))
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
            }
        }

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
}
