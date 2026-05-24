use std::sync::RwLock;

/// In-memory vector index for ContextStore.
/// Simple brute-force search, suitable for up to ~50k entries.
/// For larger scale, enable pgvector HNSW via ContextStore::hydrate_from_db.
pub struct HnswIndex {
    vectors: RwLock<Vec<(uuid::Uuid, Vec<f32>)>>,
}

impl HnswIndex {
    pub fn new(_dim: usize, _max_elements: usize) -> Self {
        Self {
            vectors: RwLock::new(Vec::new()),
        }
    }

    pub fn insert(&self, id: uuid::Uuid, vector: &[f32]) {
        let mut vecs = self.vectors.write().unwrap();
        vecs.push((id, vector.to_vec()));
    }

    pub fn search(&self, query: &[f32], k: usize) -> Vec<(uuid::Uuid, f32)> {
        let vecs = self.vectors.read().unwrap();
        let mut scored: Vec<(f32, uuid::Uuid)> = vecs
            .iter()
            .map(|(id, v)| (cosine_similarity(v, query), *id))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(k).map(|(s, id)| (id, s)).collect()
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b).max(f32::EPSILON)
}
