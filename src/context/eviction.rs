use super::{ContextEntry, ContextKind, ContextStore, StoredEntry};
use crate::cosine_similarity;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

impl ContextStore {
    const DEDUP_THRESHOLD: f32 = 0.92;

    pub async fn set_quotas(&self, overrides: &HashMap<ContextKind, usize>) {
        let mut q = self.quota_overrides.write().await;
        q.clear();
        for (k, v) in overrides {
            q.insert(*k, *v);
        }
    }

    pub async fn set_evict_every(&self, n: usize) {
        *self.evict_every.write().await = n;
    }

    async fn quota_for(&self, kind: ContextKind) -> usize {
        let overrides = self.quota_overrides.read().await;
        overrides
            .get(&kind)
            .copied()
            .unwrap_or_else(|| kind.quota())
    }

    pub async fn seed_batch(&self, entries: Vec<ContextEntry>) {
        let staged: Vec<ContextEntry> = if let Some(db) = self.db() {
            futures::future::join_all(entries.into_iter().map(|entry| {
                let db = db.clone();
                async move {
                    if let Err(e) = crate::db::insert_context_entry(&db, &entry).await {
                        tracing::warn!("[context] seed_batch DB insert failed: {}", e);
                    }
                    entry
                }
            }))
            .await
        } else {
            entries
        };

        let mut quota_snapshot: HashMap<ContextKind, usize> = HashMap::new();
        let kinds: std::collections::HashSet<ContextKind> = staged.iter().map(|e| e.kind).collect();
        for kind in kinds {
            quota_snapshot.insert(kind, self.quota_for(kind).await);
        }
        let evict_every = *self.evict_every.read().await;

        #[cfg_attr(not(feature = "tools-turbovec"), allow(unused_variables))]
        let evicted = {
            let mut store = self.entries.write().await;
            let mut inserted = 0usize;

            for entry in staged {
                let mut merged = false;
                if let Some(ref emb) = entry.embedding {
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
                } else {
                    for existing in store.iter_mut() {
                        if existing.entry.kind != entry.kind {
                            continue;
                        }
                        if existing.entry.content == entry.content {
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

                store.push(StoredEntry { entry });
                inserted += 1;
            }

            let total = self.insert_count.fetch_add(inserted, Ordering::SeqCst) + inserted;
            if total >= evict_every {
                self.insert_count.store(0, Ordering::SeqCst);

                let mut kind_counts: HashMap<ContextKind, Vec<usize>> = HashMap::new();
                for (i, s) in store.iter().enumerate() {
                    kind_counts.entry(s.entry.kind).or_default().push(i);
                }

                let mut indices_to_remove: Vec<usize> = Vec::new();
                for (kind, indices) in &kind_counts {
                    let quota = quota_snapshot
                        .get(kind)
                        .copied()
                        .unwrap_or_else(|| kind.quota());
                    if indices.len() <= quota {
                        continue;
                    }
                    let excess = indices.len() - quota;
                    let mut scored: Vec<(f32, usize)> = indices
                        .iter()
                        .map(|&idx| (store[idx].entry.composite_score(), idx))
                        .collect();
                    scored
                        .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
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
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        #[cfg(feature = "tools-turbovec")]
        if evicted {
            let count = self.entries.read().await.len();
            if let Some(ref tv) = *self.turbovec.read().unwrap_or_else(|e| e.into_inner()) {
                let positions: Vec<usize> = (0..count).collect();
                tv.reindex(&positions);
                tv.prepare();
            }
        }
    }
}
