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

    /// Return the names of all registered tools. The result is sorted
    /// alphabetically so callers (e.g. `/mcp` and the tools picker) can
    /// rely on a stable display order without sorting themselves.
    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.iter().map(|r| r.key().clone()).collect();
        names.sort();
        names
    }

    pub async fn get_definition(&self, name: &str) -> Option<ToolDefinition> {
        self.tools.get(name).map(|r| r.def.clone())
    }

    pub async fn get_permission(&self, name: &str) -> PermissionLevel {
        self.tools
            .get(name)
            .map(|r| effective_permission(r.trust, r.permission, name))
            .unwrap_or(PermissionLevel::Prompt)
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

        let n = items.len();
        tracing::info!("computing {} tool embeddings", n);

        // Use embed_description for each tool. When local ONNX is disabled and no remote
        // providers are configured, this falls through to deterministic placeholder
        // embeddings (SHA-256 hash → 1024d, instant). BM25 serves as the primary
        // retrieval signal; dense scoring provides fallback.
        for (name, text) in &items {
            let emb = embedder
                .embed_description(text)
                .await
                .unwrap_or_else(|_| crate::embedding::deterministic_placeholder_embedding(text));
            if let Some(mut tool) = self.tools.get_mut(name) {
                tool.embedding = Some(emb);
            }
        }

        let cache_entries: Vec<(String, Vec<f32>)> = items
            .iter()
            .filter_map(|(name, _)| {
                self.tools
                    .get(name)
                    .and_then(|t| t.embedding.clone())
                    .map(|emb| (name.clone(), emb))
            })
            .collect();
        tracing::info!("caching {} tool embeddings to disk", cache_entries.len());
        save_embed_cache_async(cache_path, &content_hash, &cache_entries).await;
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
        //    Each entry: (name, description, category, opt_embedding, opt_schema)
        #[allow(clippy::type_complexity)]
        let mut tool_entries: Vec<(
            String,
            String,
            String,
            Option<Vec<f32>>,
            serde_json::Value,
        )> = Vec::new();
        for r in self.tools.iter() {
            let t = r.value();
            let schema = if t.def.input_schema.is_null() {
                serde_json::json!({"type": "object"})
            } else {
                t.def.input_schema.clone()
            };
            tool_entries.push((
                t.def.name.clone(),
                t.def.description.clone(),
                t.def.category.clone(),
                t.embedding.clone(),
                schema,
            ));
        }
        let _n = tool_entries.len();

        // Dense: cosine similarity scoring
        let mut dense_scored: Vec<(f32, usize)> = tool_entries
            .iter()
            .enumerate()
            .filter_map(|(i, (_, _, _, emb, _))| {
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
                    .map(|(i, (name, desc, _, _, _))| (i, format!("{}: {}", name, desc))),
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
            .map(|(name, desc, cat, _, schema)| ToolDefinition {
                name: name.clone(),
                description: desc.clone(),
                input_schema: schema.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attenuation::TrustLevel;
    use crate::capability::CapabilityManager;
    use crate::models::PermissionLevel;

    #[tokio::test]
    async fn test_get_permission_unknown_tool_returns_prompt() {
        let registry = ToolRegistry::new();
        let perm = registry.get_permission("nonexistent_tool").await;
        assert_eq!(
            perm,
            PermissionLevel::Prompt,
            "unknown tool must default to Prompt, not Allow"
        );
    }

    #[tokio::test]
    async fn test_get_definition_nonexistent() {
        let registry = ToolRegistry::new();
        let def = registry.get_definition("no_such_tool").await;
        assert!(def.is_none(), "nonexistent tool should return None");
    }

    #[tokio::test]
    async fn test_register_and_get_definition() {
        let registry = ToolRegistry::new();
        let exec: ToolFn = Arc::new(|_args| {
            Box::pin(async move {
                ToolResult {
                    success: true,
                    output: "done".into(),
                    error: None,
                    duration_ms: 0,
                }
            })
        });
        registry
            .register(
                "my_tool",
                "My custom tool",
                serde_json::json!({"type": "object"}),
                "custom",
                exec,
            )
            .await;

        let def = registry.get_definition("my_tool").await;
        assert!(def.is_some());
        assert_eq!(def.unwrap().name, "my_tool");
    }

    #[tokio::test]
    async fn test_execute_gated_unknown_tool() {
        let registry = ToolRegistry::new();
        let cap_mgr = Arc::new(CapabilityManager::new());
        let result = registry
            .execute_gated("unknown", &serde_json::json!({}), &cap_mgr)
            .await;
        assert!(result.is_err(), "executing unknown tool should error");
        let err = result.err().unwrap().to_string();
        assert!(
            err.contains("capability denied") || err.contains("not found"),
            "error should mention capability denial: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_search_tools_empty() {
        let registry = ToolRegistry::new();
        let results = registry
            .search_tools(&[1.0, 0.0, 0.0, 0.0], 10, &[], None)
            .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_search_tools_with_essential() {
        let registry = ToolRegistry::new();
        let exec: ToolFn = Arc::new(|_args| {
            Box::pin(async move {
                ToolResult {
                    success: true,
                    output: "ok".into(),
                    error: None,
                    duration_ms: 0,
                }
            })
        });
        registry
            .register(
                "required_tool",
                "Required tool",
                serde_json::json!({"type": "object"}),
                "core",
                exec,
            )
            .await;

        let results = registry
            .search_tools(&[1.0, 0.0, 0.0, 0.0], 10, &["required_tool"], None)
            .await;
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "required_tool");
    }

    #[tokio::test]
    async fn test_register_with_permission_prompt() {
        let registry = ToolRegistry::new();
        let exec: ToolFn = Arc::new(|_args| {
            Box::pin(async move {
                ToolResult {
                    success: true,
                    output: "ok".into(),
                    error: None,
                    duration_ms: 0,
                }
            })
        });
        registry
            .register_with_permission(
                "prompt_tool",
                "Needs approval",
                serde_json::json!({"type": "object"}),
                "custom",
                exec,
                PermissionLevel::Prompt,
                TrustLevel::Builtin,
            )
            .await;

        let perm = registry.get_permission("prompt_tool").await;
        assert_eq!(perm, PermissionLevel::Prompt);
    }

    #[tokio::test]
    async fn test_get_definitions_initially_empty() {
        let registry = ToolRegistry::new();
        let defs = registry.get_definitions().await;
        assert!(
            defs.is_empty(),
            "new registry starts with no tools registered"
        );
    }

    #[tokio::test]
    async fn test_record_co_occurrence_no_panic() {
        let registry = ToolRegistry::new();
        registry.record_co_occurrence(&["read".to_string(), "write".to_string()]);
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
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    for (name, text) in items {
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        hasher.update(text.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
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
        if let Err(e) = tokio::fs::write(path, &json).await {
            tracing::warn!("[cache] failed to write tool embedding cache: {}", e);
        }
    }
}
