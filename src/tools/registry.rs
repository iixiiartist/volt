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
    ) -> Vec<ToolDefinition> {
        // 1. Vector search (current primary signal)
        let mut scored: Vec<(f32, ToolDefinition)> = self
            .tools
            .iter()
            .filter_map(|r| {
                let t = r.value();
                t.embedding.as_ref().map(|e| {
                    let sim = cosine_similarity(e, query_embedding);
                    (sim, t.def.clone())
                })
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut result: Vec<ToolDefinition> =
            scored.into_iter().take(limit).map(|(_, d)| d).collect();

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

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b).max(f32::EPSILON)
}
