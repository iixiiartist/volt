//! Visual workflow definitions — node graph + layout persistence.
//!
//! This module owns the on-disk workflow file format used by the visual
//! editor in the web UI. It is intentionally separate from
//! `crate::orchestrator::DagWorkflow`: the orchestrator's type is the
//! *runtime* representation (agents + task templates + edges), while
//! `WorkflowGraph` is the *editor* representation (any node kind, layout
//! positions, viewport state, metadata).
//!
//! A `WorkflowGraph` is saved as `.workflow.json` and round-trips through
//! the editor unchanged. `to_dag_workflow()` projects it into the
//! orchestrator's representation for execution.

use anyhow::{bail, Context, Result};
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// =============================================================================
// File format
// =============================================================================

/// Format version of the on-disk workflow file. Bumped when the
/// representation changes in an incompatible way; the loader rejects
/// files with a higher major version.
pub const WORKFLOW_FILE_VERSION: u32 = 1;

/// What the node does at runtime. The editor knows the full set; the
/// orchestrator only consumes `Agent` nodes for now — other kinds are
/// rendered in the canvas but skipped during execution (with a warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// An LLM agent task (mapped to `DagNode`).
    Agent,
    /// A direct tool invocation (Phase 3).
    Tool,
    /// A workflow entry point — no input port (Phase 5).
    Trigger,
    /// Inline code (Phase 3).
    Code,
    /// Comment / annotation, not executed.
    Note,
}

impl Default for NodeKind {
    fn default() -> Self {
        Self::Agent
    }
}

/// One node in the editor. Position is the canvas-space (x, y) of the
/// top-left corner of the node's bounding box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    /// Stable ID used in edges. Editor-generated (e.g. "node_3").
    pub id: String,
    /// Display label shown on the node body.
    pub label: String,
    /// What this node does.
    #[serde(default)]
    pub kind: NodeKind,
    /// Agent name (for `Agent` nodes) or tool name (for `Tool` nodes).
    pub agent_name: Option<String>,
    /// Model ID (for `Agent` nodes). E.g. "qwen/qwen3-32b".
    pub model: Option<String>,
    /// System prompt (for `Agent` nodes).
    pub system_prompt: Option<String>,
    /// Task template with `{input}` / `{node_id}` placeholders.
    pub task: String,
    /// Free-form per-node config (tool args, code body, etc.).
    #[serde(default)]
    pub config: serde_json::Value,
    /// Canvas position.
    #[serde(default)]
    pub position: NodePosition,
    /// Per-node notes (annotations).
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub from: String,
    pub to: String,
    /// Optional label rendered on the wire.
    #[serde(default)]
    pub label: Option<String>,
}

/// Canvas viewport (zoom + pan). Persisted so re-opening a workflow
/// restores the user's last view.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

/// A visual workflow definition. This is the on-disk format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowGraph {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub viewport: Viewport,
    /// When the file was last modified (UTC, RFC 3339).
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl WorkflowGraph {
    /// Empty graph with a default name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: WORKFLOW_FILE_VERSION,
            name: name.into(),
            description: String::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: Viewport::default(),
            updated_at: None,
        }
    }

    /// Parse a workflow file from JSON.
    pub fn from_json(text: &str) -> Result<Self> {
        let g: WorkflowGraph = serde_json::from_str(text)
            .with_context(|| "failed to parse workflow JSON")?;
        if g.version > WORKFLOW_FILE_VERSION {
            bail!(
                "workflow file version {} is newer than supported ({})",
                g.version,
                WORKFLOW_FILE_VERSION
            );
        }
        Ok(g)
    }

    /// Serialize to pretty JSON for saving.
    pub fn to_pretty_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    // -----------------------------------------------------------------
    // Mutation helpers
    // -----------------------------------------------------------------

    pub fn add_node(&mut self, node: WorkflowNode) {
        self.nodes.push(node);
    }

    pub fn remove_node(&mut self, id: &str) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from != id && e.to != id);
        self.nodes.len() != before
    }

    pub fn add_edge(&mut self, edge: WorkflowEdge) -> Result<()> {
        if edge.from == edge.to {
            bail!("self-loops are not allowed: {} -> {}", edge.from, edge.to);
        }
        if !self.nodes.iter().any(|n| n.id == edge.from) {
            bail!("edge references unknown source node: {}", edge.from);
        }
        if !self.nodes.iter().any(|n| n.id == edge.to) {
            bail!("edge references unknown target node: {}", edge.to);
        }
        if self.edges.iter().any(|e| e.from == edge.from && e.to == edge.to) {
            // Idempotent — silently no-op for repeated connections.
            return Ok(());
        }
        self.edges.push(edge);
        Ok(())
    }

    pub fn remove_edge(&mut self, from: &str, to: &str) -> bool {
        let before = self.edges.len();
        self.edges.retain(|e| !(e.from == from && e.to == to));
        self.edges.len() != before
    }

    // -----------------------------------------------------------------
    // Graph analysis
    // -----------------------------------------------------------------

    /// Build a `petgraph::DiGraph` for layout/cycle analysis. Node
    /// weights are `String` (the node ID).
    pub fn to_petgraph(&self) -> DiGraph<String, ()> {
        let mut g = DiGraph::<String, ()>::new();
        let mut idxs: HashMap<String, NodeIndex> = HashMap::new();
        for n in &self.nodes {
            let i = g.add_node(n.id.clone());
            idxs.insert(n.id.clone(), i);
        }
        for e in &self.edges {
            if let (Some(&a), Some(&b)) = (idxs.get(&e.from), idxs.get(&e.to)) {
                g.add_edge(a, b, ());
            }
        }
        g
    }

    /// True if the graph has a cycle. Used to prevent running invalid
    /// workflows.
    pub fn has_cycle(&self) -> bool {
        let g = self.to_petgraph();
        petgraph::algo::is_cyclic_directed(&g)
    }

    /// Kahn's algorithm topological order. Returns an error if a cycle
    /// is detected.
    pub fn topological_order(&self) -> Result<Vec<String>> {
        let g = self.to_petgraph();
        match petgraph::algo::toposort(&g, None) {
            Ok(order) => Ok(order
                .into_iter()
                .map(|i| g[i].clone())
                .collect()),
            Err(_) => bail!("workflow contains a cycle"),
        }
    }

    /// Project into the orchestrator's `DagWorkflow` representation.
    /// Only `Agent` nodes are kept; other kinds are dropped (the
    /// orchestrator can only run agents). Returns a `DagWorkflow` plus
    /// the list of node IDs that were dropped (for user feedback).
    pub fn to_dag_workflow(
        &self,
    ) -> Result<(crate::orchestrator::DagWorkflow, Vec<String>)> {
        use crate::orchestrator::{AgentSpec, DagEdge, DagNode, DagWorkflow};

        let mut nodes = Vec::new();
        let mut dropped = Vec::new();
        for n in &self.nodes {
            match n.kind {
                NodeKind::Agent => {
                    let agent = AgentSpec {
                        name: n
                            .agent_name
                            .clone()
                            .unwrap_or_else(|| n.id.clone()),
                        model: n.model.clone().unwrap_or_default(),
                        system_prompt: n.system_prompt.clone(),
                        max_iterations: 8,
                        temperature: 0.3,
                        allow_all: false,
                        mode: None,
                        use_synthesizer: false,
                    };
                    nodes.push(DagNode {
                        id: n.id.clone(),
                        agent,
                        task_template: n.task.clone(),
                    });
                }
                _ => dropped.push(n.id.clone()),
            }
        }

        let id_set: std::collections::HashSet<&str> =
            nodes.iter().map(|n| n.id.as_str()).collect();
        let edges: Vec<DagEdge> = self
            .edges
            .iter()
            .filter(|e| id_set.contains(e.from.as_str()) && id_set.contains(e.to.as_str()))
            .map(|e| DagEdge {
                from: e.from.clone(),
                to: e.to.clone(),
            })
            .collect();

        Ok((DagWorkflow { nodes, edges }, dropped))
    }

    // -----------------------------------------------------------------
    // Auto-layout
    // -----------------------------------------------------------------

    /// Assign positions to all nodes using a simple layered layout.
    /// Nodes with no incoming edges go in column 0; each subsequent
    /// column is one to the right of the latest predecessor. Rows
    /// within a column are stacked vertically.
    ///
    /// Overwrites any existing positions. Returns the count of nodes
    /// repositioned.
    pub fn auto_layout(&mut self) -> usize {
        use std::collections::HashMap;
        let order = match self.topological_order() {
            Ok(o) => o,
            Err(_) => return 0,
        };

        let mut level: HashMap<String, usize> = HashMap::new();
        let mut in_degree: HashMap<String, usize> = self
            .nodes
            .iter()
            .map(|n| (n.id.clone(), 0))
            .collect();
        for e in &self.edges {
            *in_degree.entry(e.to.clone()).or_insert(0) += 1;
        }
        for id in &order {
            let pred_level = self
                .edges
                .iter()
                .filter(|e| &e.to == id)
                .filter_map(|e| level.get(&e.from).copied())
                .max()
                .unwrap_or(0);
            // Level = max predecessor level + 1, or 0 if no predecessors.
            let l = if in_degree.get(id).copied().unwrap_or(0) == 0 {
                0
            } else {
                pred_level + 1
            };
            level.insert(id.clone(), l);
        }

        // Bucket nodes by level.
        let mut by_level: HashMap<usize, Vec<String>> = HashMap::new();
        for (id, l) in &level {
            by_level.entry(*l).or_default().push(id.clone());
        }

        const COL_W: f64 = 260.0;
        const ROW_H: f64 = 140.0;
        const ORIGIN_X: f64 = 60.0;
        const ORIGIN_Y: f64 = 60.0;
        let mut count = 0;
        for (l, ids) in &by_level {
            for (row, id) in ids.iter().enumerate() {
                if let Some(n) = self.nodes.iter_mut().find(|n| &n.id == id) {
                    n.position = NodePosition {
                        x: ORIGIN_X + (*l as f64) * COL_W,
                        y: ORIGIN_Y + (row as f64) * ROW_H,
                    };
                    count += 1;
                }
            }
        }
        count
    }
}

// =============================================================================
// File I/O
// =============================================================================

/// Where to look for workflow files by default. The user can override
/// with the `VOLT_WORKFLOWS_DIR` env var.
pub fn default_workflows_dir() -> PathBuf {
    if let Ok(p) = std::env::var("VOLT_WORKFLOWS_DIR") {
        return PathBuf::from(p);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".volt").join("workflows")
}

/// Ensure the workflows directory exists. Returns the path.
pub fn ensure_workflows_dir() -> Result<PathBuf> {
    let dir = default_workflows_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create workflows dir at {:?}", dir))?;
    }
    Ok(dir)
}

/// Save a workflow to disk. Writes to `<workflows_dir>/<name>.workflow.json`.
/// If a file with that name already exists, overwrites it.
pub fn save(graph: &WorkflowGraph) -> Result<PathBuf> {
    let dir = ensure_workflows_dir()?;
    let safe = sanitize_filename(&graph.name);
    let path = dir.join(format!("{}.workflow.json", safe));
    let json = graph.to_pretty_json()?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write workflow to {:?}", path))?;
    Ok(path)
}

/// Load a workflow by file path.
pub fn load(path: &Path) -> Result<WorkflowGraph> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow file {:?}", path))?;
    let g = WorkflowGraph::from_json(&text)?;
    Ok(g)
}

/// List all `.workflow.json` files in the default workflows dir.
pub fn list_all() -> Result<Vec<PathBuf>> {
    let dir = default_workflows_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read workflows dir {:?}", dir))?
    {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json")
            && p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.ends_with(".workflow.json"))
                .unwrap_or(false)
        {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Replace filesystem-unsafe characters in a workflow name.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_whitespace() => '_',
            c => c,
        })
        .collect::<String>()
        .to_lowercase()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_graph() -> WorkflowGraph {
        let mut g = WorkflowGraph::new("test");
        g.add_node(WorkflowNode {
            id: "a".into(),
            label: "A".into(),
            kind: NodeKind::Agent,
            agent_name: Some("agent-a".into()),
            model: Some("qwen/qwen3-32b".into()),
            system_prompt: None,
            task: "do {input}".into(),
            config: serde_json::Value::Null,
            position: NodePosition { x: 0.0, y: 0.0 },
            notes: None,
        });
        g.add_node(WorkflowNode {
            id: "b".into(),
            label: "B".into(),
            kind: NodeKind::Agent,
            agent_name: Some("agent-b".into()),
            model: Some("qwen/qwen3-32b".into()),
            system_prompt: None,
            task: "process {a}".into(),
            config: serde_json::Value::Null,
            position: NodePosition::default(),
            notes: None,
        });
        g.add_edge(WorkflowEdge {
            from: "a".into(),
            to: "b".into(),
            label: None,
        })
        .unwrap();
        g
    }

    #[test]
    fn round_trip() {
        let g = sample_graph();
        let json = g.to_pretty_json().unwrap();
        let g2 = WorkflowGraph::from_json(&json).unwrap();
        assert_eq!(g2.nodes.len(), 2);
        assert_eq!(g2.edges.len(), 1);
        assert_eq!(g2.edges[0].from, "a");
    }

    #[test]
    fn rejects_self_loop() {
        let mut g = WorkflowGraph::new("loop");
        g.add_node(WorkflowNode {
            id: "x".into(),
            label: "X".into(),
            kind: NodeKind::Agent,
            agent_name: Some("x".into()),
            model: None,
            system_prompt: None,
            task: "t".into(),
            config: serde_json::Value::Null,
            position: NodePosition::default(),
            notes: None,
        });
        let err = g
            .add_edge(WorkflowEdge {
                from: "x".into(),
                to: "x".into(),
                label: None,
            })
            .unwrap_err();
        assert!(err.to_string().contains("self-loop"));
    }

    #[test]
    fn rejects_unknown_node_edge() {
        let mut g = WorkflowGraph::new("bad");
        g.add_node(WorkflowNode {
            id: "a".into(),
            label: "A".into(),
            kind: NodeKind::Agent,
            agent_name: Some("a".into()),
            model: None,
            system_prompt: None,
            task: "t".into(),
            config: serde_json::Value::Null,
            position: NodePosition::default(),
            notes: None,
        });
        let err = g
            .add_edge(WorkflowEdge {
                from: "a".into(),
                to: "ghost".into(),
                label: None,
            })
            .unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn detects_cycle() {
        let mut g = WorkflowGraph::new("cyc");
        for id in ["a", "b"] {
            g.add_node(WorkflowNode {
                id: id.into(),
                label: id.into(),
                kind: NodeKind::Agent,
                agent_name: Some(id.into()),
                model: None,
                system_prompt: None,
                task: "t".into(),
                config: serde_json::Value::Null,
                position: NodePosition::default(),
                notes: None,
            });
        }
        g.add_edge(WorkflowEdge {
            from: "a".into(),
            to: "b".into(),
            label: None,
        })
        .unwrap();
        g.add_edge(WorkflowEdge {
            from: "b".into(),
            to: "a".into(),
            label: None,
        })
        .unwrap();
        assert!(g.has_cycle());
        assert!(g.topological_order().is_err());
    }

    #[test]
    fn auto_layout_assigns_columns() {
        let mut g = sample_graph();
        let n = g.auto_layout();
        assert_eq!(n, 2);
        let a = g.nodes.iter().find(|n| n.id == "a").unwrap();
        let b = g.nodes.iter().find(|n| n.id == "b").unwrap();
        assert!(b.position.x > a.position.x);
    }

    #[test]
    fn projects_to_dag_workflow() {
        let g = sample_graph();
        let (dag, dropped) = g.to_dag_workflow().unwrap();
        assert_eq!(dag.nodes.len(), 2);
        assert!(dropped.is_empty());
        assert_eq!(dag.nodes[0].id, "a");
        assert_eq!(dag.edges.len(), 1);
    }

    #[test]
    fn to_petgraph_round_trip() {
        let g = sample_graph();
        let pg = g.to_petgraph();
        assert_eq!(pg.node_count(), 2);
        assert_eq!(pg.edge_count(), 1);
    }
}
