use crate::embedding::EmbeddingClient;
use crate::models::{PermissionLevel, ToolDefinition, ToolResult};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

type ToolFn = Arc<dyn Fn(Value) -> ToolResult + Send + Sync>;

pub struct ToolRegistry {
    tools: RwLock<std::collections::HashMap<String, RegisteredTool>>,
}

struct RegisteredTool {
    def: ToolDefinition,
    exec: ToolFn,
    permission: PermissionLevel,
    embedding: Option<Vec<f32>>,
}

impl ToolRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tools: RwLock::new(std::collections::HashMap::new()),
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
        self.register_with_permission(name, description, input_schema, category, exec, PermissionLevel::Allow)
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
        let mut tools = self.tools.write().await;
        tools.insert(
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
    }

    pub async fn get_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        tools.values().map(|t| t.def.clone()).collect()
    }

    pub async fn get_definition(&self, name: &str) -> Option<ToolDefinition> {
        let tools = self.tools.read().await;
        tools.get(name).map(|t| t.def.clone())
    }

    pub async fn get_permission(&self, name: &str) -> PermissionLevel {
        let tools = self.tools.read().await;
        tools
            .get(name)
            .map(|t| t.permission)
            .unwrap_or(PermissionLevel::Allow)
    }

    pub async fn execute(&self, name: &str, args: &Value) -> anyhow::Result<ToolResult> {
        let tools = self.tools.read().await;
        let tool = tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("tool '{}' not found", name))?;
        Ok((tool.exec)(args.clone()))
    }

    pub async fn compute_embeddings(&self, embedder: &EmbeddingClient) {
        let items: Vec<(String, String)> = {
            let tools = self.tools.read().await;
            tools
                .values()
                .map(|t| {
                    let text = format!("{}: {}", t.def.name, t.def.description);
                    (t.def.name.clone(), text)
                })
                .collect()
        };

        let results: Vec<(String, Option<Vec<f32>>)> =
            futures::future::join_all(items.into_iter().map(|(name, text)| {
                async move {
                    let emb = embedder.embed_description(&text).await.ok();
                    (name, emb)
                }
            }))
            .await;

        let mut tools = self.tools.write().await;
        for (name, emb) in results {
            if let Some(tool) = tools.get_mut(&name) {
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
        let tools = self.tools.read().await;
        let mut scored: Vec<(f32, &RegisteredTool)> = tools
            .values()
            .filter_map(|t| {
                t.embedding.as_ref().map(|e| {
                    let sim = cosine_similarity(e, query_embedding);
                    (sim, t)
                })
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut result: Vec<ToolDefinition> = scored.into_iter().take(limit).map(|(_, t)| t.def.clone()).collect();

        // Ensure essential tools are always included
        for name in essential {
            if !result.iter().any(|d| d.name == *name) {
                if let Some(t) = tools.get(*name) {
                    result.push(t.def.clone());
                }
            }
        }

        result
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b).max(f32::EPSILON)
}