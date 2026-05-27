use crate::cosine_similarity;
use crate::embedding::EmbeddingClient;
use crate::models::{PermissionLevel, ToolDefinition, ToolResult};
use dashmap::DashMap;
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
        )
        .await;
    }

    pub async fn register_with_permission(
        &self,
        name: &str,
        description: &str,
        input_schema: Value,
        category: &str,
        exec: ToolFn,
        permission: PermissionLevel,
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
            .map(|r| r.permission)
            .unwrap_or(PermissionLevel::Allow)
    }

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

        let sem = Arc::new(tokio::sync::Semaphore::new(5));
        let results: Vec<(String, Option<Vec<f32>>)> =
            futures::future::join_all(items.into_iter().map(|(name, text)| {
                let sem = sem.clone();
                async move {
                    let _permit = sem.acquire().await.ok();
                    let emb = embedder.embed_description(&text).await.ok();
                    (name, emb)
                }
            }))
            .await;

        for (name, emb) in results {
            if let Some(mut tool) = self.tools.get_mut(&name) {
                tool.embedding = emb;
            }
        }
    }

    pub async fn search_tools(
        &self,
        query_embedding: &[f32],
        limit: usize,
        essential: &[&str],
        query_text: Option<&str>,
    ) -> Vec<ToolDefinition> {
        // 1. Compute both dense (cosine) and sparse (BM25) rankings, then fuse with RRF
        let all_tools: Vec<(String, ToolDefinition)> = self
            .tools
            .iter()
            .map(|r| (r.key().clone(), r.value().def.clone()))
            .collect();

        // Dense: cosine similarity ranking
        let mut dense_scored: Vec<(f32, usize)> = all_tools
            .iter()
            .enumerate()
            .filter_map(|(i, (name, _))| {
                let t = self.tools.get(name)?;
                let emb = t.embedding.as_ref()?;
                let sim = cosine_similarity(emb, query_embedding);
                Some((sim, i))
            })
            .collect();
        dense_scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let dense_order: Vec<usize> = dense_scored.iter().map(|(_, i)| *i).collect();

        // Sparse: BM25 ranking (if query text provided)
        let bm25_order: Vec<usize> = if let Some(qt) = query_text {
            let corpus: Vec<String> = all_tools
                .iter()
                .map(|(name, def)| format!("{}: {}", name, def.description))
                .collect();
            let bm25 = crate::vector_index::Bm25Scorer::build(
                corpus.iter().enumerate().map(|(i, t)| (i, t.as_str())),
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

        let mut result: Vec<ToolDefinition> = fused_order
            .into_iter()
            .filter_map(|i| all_tools.get(i))
            .map(|(_, def)| def.clone())
            .collect();

        // 2. GraphRAG augmentation: append related tools up to 2 hops away
        let mut names_in_result: std::collections::HashSet<String> =
            result.iter().map(|d| d.name.clone()).collect();
        let seed_names: Vec<String> = result.iter().map(|d| d.name.clone()).collect();
        for tool_name in &seed_names {
            for related in self.graph.find_related(tool_name, 2) {
                if names_in_result.contains(&related) {
                    continue;
                }
                if let Some(t) = self.tools.get(&related) {
                    result.push(t.def.clone());
                    names_in_result.insert(related);
                }
            }
        }

        // 3. Essential tools always included
        for name in essential {
            if !names_in_result.contains(*name) {
                if let Some(t) = self.tools.get(*name) {
                    result.push(t.def.clone());
                    names_in_result.insert(name.to_string());
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
