use petgraph::graph::DiGraph;
use petgraph::visit::Bfs;
use std::collections::HashMap;
use std::sync::RwLock;

pub struct ToolGraph {
    graph: RwLock<DiGraph<String, f32>>,
    name_to_node: RwLock<HashMap<String, petgraph::graph::NodeIndex>>,
}

impl Default for ToolGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolGraph {
    pub fn new() -> Self {
        Self {
            graph: RwLock::new(DiGraph::new()),
            name_to_node: RwLock::new(HashMap::new()),
        }
    }

    /// Register a tool in the graph.
    pub fn add_tool(&self, name: &str) {
        let mut g = self.graph.write().unwrap();
        let mut map = self.name_to_node.write().unwrap();
        if !map.contains_key(name) {
            let node = g.add_node(name.to_string());
            map.insert(name.to_string(), node);
        }
    }

    /// Add a directed relationship between tools (e.g., read_file → edit_file).
    /// Weight indicates relationship strength (1.0 = strong prerequisite).
    pub fn add_relationship(&self, from: &str, to: &str, weight: f32) {
        self.add_tool(from);
        self.add_tool(to);
        let map = self.name_to_node.read().unwrap();
        let mut g = self.graph.write().unwrap();
        if let (Some(&from_n), Some(&to_n)) = (map.get(from), map.get(to)) {
            g.add_edge(from_n, to_n, weight);
        }
    }

    /// Find tools related to the given tool within `depth` hops.
    /// Returns tool names sorted by proximity.
    pub fn find_related(&self, tool: &str, depth: usize) -> Vec<String> {
        let map = self.name_to_node.read().unwrap();
        let g = self.graph.read().unwrap();
        let start = match map.get(tool) {
            Some(&n) => n,
            None => return vec![],
        };

        let mut bfs = Bfs::new(&*g, start);
        let mut related = Vec::new();
        let mut distances: HashMap<petgraph::graph::NodeIndex, usize> = HashMap::new();
        distances.insert(start, 0);

        while let Some(node) = bfs.next(&*g) {
            let dist = *distances.get(&node).unwrap_or(&0);
            if dist > 0 && dist <= depth {
                if let Some(name) = g.node_weight(node) {
                    related.push(name.clone());
                }
            }
            if dist >= depth {
                continue;
            }
            for neighbor in g.neighbors(node) {
                distances.entry(neighbor).or_insert(dist + 1);
            }
        }

        related
    }
}

/// Build a default tool relationship graph for Volt's built-in tools.
pub fn build_default_tool_graph(graph: &ToolGraph) {
    // File I/O relationships
    graph.add_relationship("read", "edit", 0.9);
    graph.add_relationship("read", "write", 0.8);
    graph.add_relationship("glob", "read", 0.9);
    graph.add_relationship("grep", "read", 0.9);
    graph.add_relationship("edit", "write", 0.7);

    // Web relationships
    graph.add_relationship("web_fetch", "web_scrape", 0.9);
    graph.add_relationship("web_fetch", "web_scrape_all", 0.8);
    graph.add_relationship("web_scrape", "web_fetch", 0.5);

    // Code workflow
    graph.add_relationship("read", "bash", 0.6);
    graph.add_relationship("write", "bash", 0.6);
    graph.add_relationship("bash", "read", 0.4);

    // Data tools
    graph.add_relationship("json_validate", "json_query", 0.8);
    graph.add_relationship("json_query", "json_prettify", 0.5);
    graph.add_relationship("csv_read", "csv_write", 0.8);

    // Git workflow
    graph.add_relationship("git_status", "git_diff_unstaged", 0.9);
    graph.add_relationship("git_diff_unstaged", "git_add", 0.8);
    graph.add_relationship("git_add", "git_commit", 0.9);
    graph.add_relationship("git_diff_staged", "git_commit", 0.7);
    graph.add_relationship("git_log", "git_show", 0.8);
}
