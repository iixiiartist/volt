use std::path::Path;
use std::sync::Mutex;

#[cfg(feature = "tools-turbovec")]
use turbovec::TurboQuantIndex;

const EMBED_DIM: usize = 1024;
const BIT_WIDTH: usize = 4;

/// Wrapper around `turboquant::TurboQuantIndex` that maintains a
/// position mapping (turbovec index position → entry position in
/// `ContextStore.entries`) so search results can be mapped back to
/// the correct `ContextEntry` after eviction or insertion shifts.
///
/// ## Locking
/// Uses `std::sync::Mutex` — never held across an `.await` point.
/// Callers must invoke `prepare()` after all batch additions complete
/// and before calling `search()` / `search_with_mask()`.
pub struct TurbovecIndex {
    #[cfg(feature = "tools-turbovec")]
    inner: Mutex<TurboQuantIndex>,
    /// Maps turbovec index position → entry position in ContextStore.entries
    #[cfg(feature = "tools-turbovec")]
    pos_to_entry: Mutex<Vec<usize>>,
}

impl TurbovecIndex {
    pub fn new() -> anyhow::Result<Self> {
        #[cfg(feature = "tools-turbovec")]
        {
            let idx = TurboQuantIndex::new(EMBED_DIM, BIT_WIDTH)?;
            Ok(Self {
                inner: Mutex::new(idx),
                pos_to_entry: Mutex::new(Vec::new()),
            })
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            let _ = (EMBED_DIM, BIT_WIDTH);
            Ok(Self {})
        }
    }

    /// Add vectors to the index.
    ///
    /// `entry_positions` maps each vector to its position in the
    /// `ContextStore` entries vec.  Must have the same length as
    /// the number of vectors (`vectors.len() / EMBED_DIM`).
    pub fn add(&self, vectors: &[f32], entry_positions: &[usize]) {
        #[cfg(feature = "tools-turbovec")]
        {
            let n_vecs = entry_positions.len();
            if n_vecs == 0 {
                tracing::debug!("turbovec add: empty batch, skipping");
                return;
            }
            if n_vecs * EMBED_DIM != vectors.len() {
                tracing::error!(
                    expected = n_vecs * EMBED_DIM,
                    actual = vectors.len(),
                    n_vecs = n_vecs,
                    "turbovec add: dimension mismatch, skipping"
                );
                return;
            }
            let mut idx = self.inner.lock().unwrap();
            idx.add(vectors);
            let mut map = self.pos_to_entry.lock().unwrap();
            map.extend_from_slice(entry_positions);
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            let _ = (vectors, entry_positions);
        }
    }

    /// Build IVF centroids.  Must be called after every batch of
    /// `add()` calls and before the first `search()` or
    /// `search_with_mask()`.
    ///
    /// **Do not call this inside the search hot path** — centroid
    /// training is CPU-intensive (millions of FLOPs).  Call it once
    /// after seeding / embedding computation settles.
    pub fn prepare(&self) {
        #[cfg(feature = "tools-turbovec")]
        {
            let idx = self.inner.lock().unwrap();
            if idx.len() > 0 {
                idx.prepare();
                tracing::debug!("turbovec: IVF centroids trained on {} vectors", idx.len());
            }
        }
    }

    pub fn search(&self, query: &[f32], k: usize) -> Vec<(f32, usize)> {
        #[cfg(feature = "tools-turbovec")]
        {
            let idx = self.inner.lock().unwrap();
            if idx.len() == 0 {
                return Vec::new();
            }
            let results = idx.search(query, k);
            if results.nq == 0 || results.k == 0 || results.indices.len() < results.k {
                return Vec::new();
            }
            let scores = results.scores_for_query(0);
            let indices = results.indices_for_query(0);
            let map = self.pos_to_entry.lock().unwrap();
            scores
                .iter()
                .zip(indices.iter())
                .filter_map(|(&s, &i)| {
                    let pos = i as usize;
                    if pos < map.len() {
                        Some((s, map[pos]))
                    } else {
                        None
                    }
                })
                .collect()
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            let _ = (query, k);
            Vec::new()
        }
    }

    pub fn search_with_mask(&self, query: &[f32], k: usize, mask: &[bool]) -> Vec<(f32, usize)> {
        #[cfg(feature = "tools-turbovec")]
        {
            let idx = self.inner.lock().unwrap();
            if idx.len() == 0 {
                return Vec::new();
            }
            let results = idx.search_with_mask(query, k, Some(mask));
            if results.nq == 0 || results.k == 0 {
                return Vec::new();
            }
            let scores = results.scores_for_query(0);
            let indices = results.indices_for_query(0);
            let map = self.pos_to_entry.lock().unwrap();
            scores
                .iter()
                .zip(indices.iter())
                .filter_map(|(&s, &i)| {
                    let pos = i as usize;
                    if pos < map.len() {
                        Some((s, map[pos]))
                    } else {
                        None
                    }
                })
                .collect()
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            let _ = (query, k, mask);
            Vec::new()
        }
    }

    pub fn len(&self) -> usize {
        #[cfg(feature = "tools-turbovec")]
        {
            self.inner.lock().unwrap().len()
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            0
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Update the position mapping after entries have shifted (e.g. eviction).
    /// `new_positions[i]` = the entry's current index in the store.
    /// The slice must have length equal to the current number of indexed vectors.
    pub fn reindex(&self, new_positions: &[usize]) {
        #[cfg(feature = "tools-turbovec")]
        {
            *self.pos_to_entry.lock().unwrap() = new_positions.to_vec();
            tracing::debug!("turbovec: reindexed {} positions", new_positions.len());
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            let _ = new_positions;
        }
    }

    /// Persist index to disk (index data only — position mapping is ephemeral).
    /// On `load()` the caller must call `reindex()` to restore positions.
    pub fn write(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        #[cfg(feature = "tools-turbovec")]
        {
            self.inner.lock().unwrap().write(path)?;
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            let _ = path;
        }
        Ok(())
    }

    /// Load index from disk.  Position mapping starts empty — you **must**
    /// call `reindex()` before `search()` or results will be silently empty.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        #[cfg(feature = "tools-turbovec")]
        {
            let idx = TurboQuantIndex::load(path)?;
            tracing::warn!("turbovec: loaded index from disk with empty position mapping — call reindex() before use");
            Ok(Self {
                inner: Mutex::new(idx),
                pos_to_entry: Mutex::new(Vec::new()),
            })
        }
        #[cfg(not(feature = "tools-turbovec"))]
        {
            let _ = path;
            Ok(Self {})
        }
    }
}

impl Default for TurbovecIndex {
    fn default() -> Self {
        Self::new().expect("TurbovecIndex::default() failed — check BIT_WIDTH compatibility")
    }
}

#[cfg(feature = "tools-turbovec")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_index() {
        let idx = TurbovecIndex::new();
        assert!(idx.is_ok());
    }

    #[test]
    fn test_add_and_len() {
        let idx = TurbovecIndex::new().unwrap();
        assert_eq!(idx.len(), 0);
        assert!(idx.is_empty());
        let mut vectors = vec![0.0f32; EMBED_DIM * 5];
        for i in 0..5 {
            vectors[i * EMBED_DIM + i % EMBED_DIM] = 1.0;
        }
        idx.add(&vectors, &[0, 1, 2, 3, 4]);
        assert_eq!(idx.len(), 5);
    }

    #[test]
    fn test_add_dimension_mismatch_skipped() {
        let idx = TurbovecIndex::new().unwrap();
        // Wrong vector count: 3 entries but only 2*1024 elements
        let vectors = vec![0.0f32; EMBED_DIM * 2];
        idx.add(&vectors, &[0, 1, 2]); // 3 entry positions, 2 vectors → mismatch
        assert_eq!(idx.len(), 0); // should have been skipped
    }

    #[test]
    fn test_search_returns_results() {
        let idx = TurbovecIndex::new().unwrap();
        let mut vectors = vec![0.0f32; EMBED_DIM * 10];
        for i in 0..10 {
            vectors[i * EMBED_DIM + 0] = (i as f32) / 10.0;
        }
        idx.add(&vectors, &(0..10).collect::<Vec<_>>());
        idx.prepare();
        let query = vec![1.0f32; EMBED_DIM];
        let results = idx.search(&query, 3);
        assert!(!results.is_empty());
        assert!(results.len() <= 3);
    }

    #[test]
    fn test_search_empty_returns_empty() {
        let idx = TurbovecIndex::new().unwrap();
        let query = vec![0.0f32; EMBED_DIM];
        let results = idx.search(&query, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_returns_mapped_positions() {
        let idx = TurbovecIndex::new().unwrap();
        let mut vectors = vec![0.0f32; EMBED_DIM * 3];
        for i in 0..3 {
            vectors[i * EMBED_DIM + 0] = (i as f32) / 10.0;
        }
        // Map: turbovec pos 0 → entry pos 10, pos 1 → entry pos 20, pos 2 → entry pos 30
        idx.add(&vectors, &[10, 20, 30]);
        idx.prepare();
        let query = vec![1.0f32; EMBED_DIM];
        let results = idx.search(&query, 3);
        assert!(!results.is_empty());
        for (_, entry_pos) in &results {
            assert!([10usize, 20, 30].contains(entry_pos));
        }
    }

    #[test]
    fn test_search_after_reindex() {
        let idx = TurbovecIndex::new().unwrap();
        let mut vectors = vec![0.0f32; EMBED_DIM * 3];
        for i in 0..3 {
            vectors[i * EMBED_DIM + 0] = (i as f32) / 10.0;
        }
        idx.add(&vectors, &[10, 20, 30]);
        // Simulate eviction: entries shifted to positions 5, 6, 7
        idx.reindex(&[5, 6, 7]);
        idx.prepare();
        let query = vec![1.0f32; EMBED_DIM];
        let results = idx.search(&query, 3);
        assert!(!results.is_empty());
        for (_, entry_pos) in &results {
            assert!([5usize, 6, 7].contains(entry_pos));
        }
    }

    #[test]
    fn test_write_and_load() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_turbovec.tv");
        let idx = TurbovecIndex::new().unwrap();
        let vectors = vec![0.0f32; EMBED_DIM * 3];
        idx.add(&vectors, &[0, 1, 2]);
        idx.write(&path).unwrap();
        let loaded = TurbovecIndex::load(&path).unwrap();
        assert_eq!(loaded.len(), 3);
        // After load, reindex is required
        loaded.reindex(&[0, 1, 2]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_search_fails_without_reindex_after_load() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_turbovec_empty_pos.tv");
        let idx = TurbovecIndex::new().unwrap();
        let vectors = vec![0.0f32; EMBED_DIM * 3];
        idx.add(&vectors, &[0, 1, 2]);
        idx.write(&path).unwrap();
        let loaded = TurbovecIndex::load(&path).unwrap();
        // Without reindex, search returns empty because pos_to_entry is empty
        loaded.prepare();
        let results = loaded.search(&vec![0.0f32; EMBED_DIM], 3);
        assert!(results.is_empty()); // correctly fails — doc warns about this
        let _ = std::fs::remove_file(&path);
    }
}
