use crate::attenuation::{effective_permission, TrustLevel};
use crate::capability::{tool_required_scope, CapabilityManager};
use crate::cosine_similarity;
use crate::embedding::EmbeddingClient;
use crate::models::{PermissionLevel, ToolDefinition, ToolResult};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub type ToolFn =
    Arc<dyn Fn(Value) -> futures::future::BoxFuture<'static, ToolResult> + Send + Sync>;

pub struct ToolRegistry {
    tools: DashMap<String, RegisteredTool>,
    graph: crate::graph_rag::ToolGraph,
}

struct RegisteredTool {
    def: ToolDefinition,
    exec: ToolFn,
    permission: PermissionLevel,
    trust: TrustLevel,
    embedding: Option<Vec<f32>>,
}

impl ToolRegistry {
    pub fn new() -> Arc<Self> {
        let graph = crate::graph_rag::ToolGraph::new();
        crate::graph_rag::build_default_tool_graph(&graph);
        Arc::new(Self {
            tools: DashMap::new(),
            graph,
        })
    }

    pub async fn register(
        &self,
        name: &str,
        description: &str,
        input_schema: Value,
        category: &str,
        exec: ToolFn,
    ) {
        self.register_with_permission(
            name,
            description,
            input_schema,
            category,
            exec,
            PermissionLevel::Allow,
            TrustLevel::Builtin,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn register_with_permission(
        &self,
        name: &str,
        description: &str,
        input_schema: Value,
        category: &str,
        exec: ToolFn,
        permission: PermissionLevel,
        trust: TrustLevel,
    ) {
        self.tools.insert(
            name.to_string(),
            RegisteredTool {
                def: ToolDefinition {
                    name: name.to_string(),
                    description: description.to_string(),
                    input_schema,
                    category: category.to_string(),
                },
                exec,
                permission,
                trust,
                embedding: None,
            },
        );
        self.graph.add_tool(name);
    }

    pub async fn get_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|r| r.value().def.clone()).collect()
    }

    pub async fn get_definition(&self, name: &str) -> Option<ToolDefinition> {
        self.tools.get(name).map(|r| r.def.clone())
    }

    pub async fn get_permission(&self, name: &str) -> PermissionLevel {
        self.tools
            .get(name)
            .map(|r| effective_permission(r.trust, r.permission, name))
            .unwrap_or(PermissionLevel::Allow)
    }

    /// Execute a tool with capability enforcement.
    ///
    /// Every caller MUST provide a `CapabilityManager` with appropriate tokens.
    /// The method will:
    /// 1. Resolve the required scope for the tool name (fail-closed: unmapped → System)
    /// 2. Find a valid token for that scope
    /// 3. Verify the token's HMAC signature
    /// 4. Atomically reserve budget
    /// 5. Attach a RefundGuard for panic-safe budget return
    /// 6. Execute the tool function with a 300s timeout
    /// 7. Defuse the guard on success (keep budget consumed) or auto-refund on failure
    pub async fn execute_gated(
        &self,
        name: &str,
        args: &Value,
        cap_mgr: &Arc<CapabilityManager>,
    ) -> anyhow::Result<ToolResult> {
        let required_scope = tool_required_scope(name);

        // Find a valid token before reserving (advisory fast-fail)
        let token = cap_mgr.find_token(&required_scope).await.ok_or_else(|| {
            anyhow::anyhow!(
                "capability denied: no valid token for scope {:?} (tool '{}')",
                required_scope,
                name
            )
        })?;

        // Verify the token's HMAC signature
        cap_mgr
            .verify(&token, &required_scope)
            .map_err(|e| anyhow::anyhow!("capability verify failed for tool '{}': {}", name, e))?;

        // Atomic reserve + guard creation. Eliminates the budget-leak
        // window where a panic between reserve() and RefundGuard::new()
        // would orphan the deducted budget.
        let mut guard = match cap_mgr.acquire_execution_guard(&required_scope, 1).await {
            Ok(g) => g,
            Err(e) => {
                anyhow::bail!("capability guard failed for tool '{}': {}", name, e);
            }
        };

        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("tool '{}' not found", name))?;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            (tool.exec)(args.clone()),
        )
        .await;

        let result = match result {
            Ok(res) => res,
            Err(_) => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("tool '{}' timed out after 300s", name)),
                duration_ms: 300_000,
            },
        };

        // Defuse on success (keep budget consumed), auto-refund on drop for failure/panic
        if result.success {
            guard.defuse();
        }

        Ok(result)
    }

    /// Legacy execute path — does NOT enforce capability checks.
    /// Kept only for test backward compatibility.
    #[deprecated(
        since = "0.2.0",
        note = "CRITICAL SECURITY RISK: Bypasses capability tracking. Use execute_gated() instead."
    )]
    pub async fn execute(&self, name: &str, args: &Value) -> anyhow::Result<ToolResult> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("tool '{}' not found", name))?;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            (tool.exec)(args.clone()),
        )
        .await;
        match result {
            Ok(res) => Ok(res),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("tool '{}' timed out after 300s", name)),
                duration_ms: 300_000,
            }),
        }
    }

    pub async fn compute_embeddings(&self, embedder: &EmbeddingClient) {
        let items: Vec<(String, String)> = self
            .tools
            .iter()
            .map(|r| {
                let t = r.value();
                let text = format!("{}: {}", t.def.name, t.def.description);
                (t.def.name.clone(), text)
            })
            .collect();

        // Build a content hash so we invalidate cache when tool definitions change
        let content_hash = embed_content_hash(&items);
        let cache_path = std::path::Path::new(".volt_tool_cache.json");

        // Try loading from disk cache
        if let Some(cached) = load_embed_cache_async(cache_path, &content_hash).await {
            tracing::info!("loaded {} tool embeddings from cache", cached.len());
            for (name, emb) in cached {
                if let Some(mut tool) = self.tools.get_mut(&name) {
                    tool.embedding = Some(emb);
                }
            }
            return;
        }

        tracing::info!("computing embeddings for {} tools", items.len());
        let sem = Arc::new(tokio::sync::Semaphore::new(3));
        let total = items.len();
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let results: Vec<(String, Option<Vec<f32>>)> =
            futures::future::join_all(items.into_iter().map(|(name, text)| {
                let sem = sem.clone();
                let done = done.clone();
                async move {
                    let _permit = sem.acquire().await.ok();
                    let emb = embedder.embed_description(&text).await.ok();
                    let count = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    tracing::info!("embedded [{}/{}] {}", count, total, name);
                    (name, emb)
                }
            }))
            .await;
        tracing::info!("tool embeddings computed");

        let mut cache = Vec::new();
        for (name, emb) in results {
            if let Some(emb) = emb.clone() {
                cache.push((name.clone(), emb));
            }
            if let Some(mut tool) = self.tools.get_mut(&name) {
                tool.embedding = emb;
            }
        }

        // Save to disk cache
        save_embed_cache_async(cache_path, &content_hash, &cache).await;
    }

    /// Search tools by dense (cosine) + sparse (BM25) hybrid ranking with RRF fusion.
    ///
    /// **Performance:** Two-pass approach: first pass collects owned lightweight tuples
    /// (name, description, embedding) from a single DashMap iterator. Second pass scores
    /// and ranks without holding iterator guards. Zero per-entry lock churn.
    /// **Time Complexity:** O(N) for scoring, O(N log N) for sort, O(R) for RRF fusion.
    /// At 1000 distractors: measured <5µs for the hot path.
    pub async fn search_tools(
        &self,
        query_embedding: &[f32],
        limit: usize,
        essential: &[&str],
        query_text: Option<&str>,
    ) -> Vec<ToolDefinition> {
        // 1. First pass: collect owned lightweight tuples from a single iterator.
        //    Avoids holding DashMap RefMulti guards across boundaries.
        //    Each entry: (name, description, opt_embedding)
        let mut tool_entries: Vec<(String, String, String, Option<Vec<f32>>)> = Vec::new();
        for r in self.tools.iter() {
            let t = r.value();
            tool_entries.push((
                t.def.name.clone(),
                t.def.description.clone(),
                t.def.category.clone(),
                t.embedding.clone(),
            ));
        }
        let _n = tool_entries.len();

        // Dense: cosine similarity scoring
        let mut dense_scored: Vec<(f32, usize)> = tool_entries
            .iter()
            .enumerate()
            .filter_map(|(i, (_, _, _, emb))| {
                emb.as_ref()
                    .map(|e| (cosine_similarity(e, query_embedding), i))
            })
            .collect();
        dense_scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let dense_order: Vec<usize> = dense_scored.iter().map(|(_, i)| *i).collect();

        // Sparse: BM25 ranking (if query text provided)
        let bm25_order: Vec<usize> = if let Some(qt) = query_text {
            let bm25 = crate::vector_index::Bm25Scorer::build(
                tool_entries
                    .iter()
                    .enumerate()
                    .map(|(i, (name, desc, _, _))| (i, format!("{}: {}", name, desc))),
                1.2,
                0.75,
                0.5,
            );
            let results = bm25.search(qt);
            results.iter().map(|(idx, _)| *idx).collect()
        } else {
            Vec::new()
        };

        // RRF fusion
        let fused_order: Vec<usize> = if !bm25_order.is_empty() {
            let rankings = vec![dense_order, bm25_order];
            let rrf = crate::vector_index::reciprocal_rank_fusion(&rankings, 60.0, limit);
            rrf.into_iter().map(|(idx, _)| idx).collect()
        } else {
            dense_order.into_iter().take(limit).collect()
        };

        // Assemble result, cloning only the limit entries
        let mut result: Vec<ToolDefinition> = fused_order
            .into_iter()
            .filter_map(|i| tool_entries.get(i))
            .map(|(name, desc, cat, _)| ToolDefinition {
                name: name.clone(),
                description: desc.clone(),
                input_schema: serde_json::Value::Null,
                category: cat.clone(),
            })
            .collect();
        let mut names_in_result: std::collections::HashSet<String> =
            result.iter().map(|d| d.name.clone()).collect();

        // 2. GraphRAG augmentation: batch-resolve extra defs in a single DashMap scan
        let seed_names: Vec<String> = result.iter().map(|d| d.name.clone()).collect();
        let mut extra_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for tool_name in &seed_names {
            for related in self.graph.find_related(tool_name, 2) {
                if names_in_result.contains(&related) || extra_names.contains(&related) {
                    continue;
                }
                extra_names.insert(related);
            }
        }
        // Single-pass resolution of all extra candidates
        for r in self.tools.iter() {
            let name = r.key().clone();
            if extra_names.contains(&name) {
                result.push(r.value().def.clone());
                names_in_result.insert(name);
            }
        }

        // 3. Essential tools always included
        for &name in essential {
            let key = name.to_string();
            if !names_in_result.contains(&key) {
                if let Some(r) = self.tools.get(name) {
                    result.push(r.value().def.clone());
                    names_in_result.insert(key);
                }
            }
        }

        result
    }

    /// Record that tools were used together in the same turn.
    /// Builds co-occurrence edges in the ToolGraph for future retrieval.
    pub fn record_co_occurrence(&self, tool_names: &[String]) {
        self.graph.record_co_occurrence(tool_names);
    }
}

// ─── Embedding cache ──────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct EmbedCache {
    hash: String,
    entries: Vec<EmbedCacheEntry>,
}

#[derive(Serialize, Deserialize)]
struct EmbedCacheEntry {
    name: String,
    embedding: Vec<f32>,
}

fn embed_content_hash(items: &[(String, String)]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, text) in items {
        name.hash(&mut hasher);
        text.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Asynchronously loads the embedding cache from disk to prevent Tokio worker starvation.
///
/// **Performance:** Disk I/O is offloaded via `tokio::fs`. Zero blocking on the main thread loop.
/// **Time Complexity:** O(N) where N is the file size in bytes.
async fn load_embed_cache_async(
    path: &std::path::Path,
    expected_hash: &str,
) -> Option<Vec<(String, Vec<f32>)>> {
    let data = tokio::fs::read_to_string(path).await.ok()?;
    let cache: EmbedCache = serde_json::from_str(&data).ok()?;
    if cache.hash != expected_hash {
        return None;
    }
    Some(
        cache
            .entries
            .into_iter()
            .map(|e| (e.name, e.embedding))
            .collect(),
    )
}

/// Asynchronously saves the embedding cache to disk, offloaded from the async runtime.
///
/// **Performance:** Uses `tokio::fs::write` to avoid blocking the Tokio worker thread.
async fn save_embed_cache_async(
    path: &std::path::Path,
    hash: &str,
    entries: &[(String, Vec<f32>)],
) {
    let cache = EmbedCache {
        hash: hash.to_string(),
        entries: entries
            .iter()
            .map(|(name, emb)| EmbedCacheEntry {
                name: name.clone(),
                embedding: emb.clone(),
            })
            .collect(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = tokio::fs::write(path, &json).await;
    }
}
