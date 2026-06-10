//! Visual workflow canvas — SVG node editor for `WorkflowGraph`.
//!
//! Renders nodes as rounded rectangles and edges as cubic bezier
//! curves. Pointer events on the SVG drive dragging, selection, and
//! edge creation. All state is held in Dioxus signals so the parent
//! page can save/load the graph via the runtime bridge.
//!
//! The canvas is intentionally simple: a Phase 1 deliverable that
//! proves the visual editor works. Phase 2 will add live execution
//! feedback, Phase 3 will add tool-as-node wiring, Phase 5 will add
//! trigger nodes.

use super::state::{
    VoltState, COLOR_ACCENT, COLOR_BG, COLOR_BORDER, COLOR_DANGER, COLOR_INFO,
    COLOR_PANEL, COLOR_PANEL_HOVER, COLOR_SUCCESS, COLOR_TEXT, COLOR_TEXT_DIM,
    COLOR_TEXT_MUTED, COLOR_WARNING, FONT_MONO,
};
use crate::workflow::{
    NodeKind, NodePosition, Viewport, WorkflowEdge, WorkflowGraph, WorkflowNode,
};
use dioxus::prelude::*;
use serde_json::json;

// =============================================================================
// Geometry constants
// =============================================================================

/// Width of a node body in canvas-space units.
pub const NODE_W: f64 = 200.0;
/// Height of a node body in canvas-space units.
pub const NODE_H: f64 = 90.0;

// =============================================================================
// Drag/interaction state (held in canvas-local signals)
// =============================================================================

/// What the user is currently doing.
#[derive(Debug, Clone, PartialEq)]
enum DragMode {
    /// Not interacting.
    None,
    /// Dragging a node body.
    Node { id: String, grab_x: f64, grab_y: f64 },
    /// Dragging the canvas (panning).
    Pan { grab_x: f64, grab_y: f64 },
    /// Drawing an edge from a node's output port to the cursor.
    Edge { from_id: String },
}

#[derive(Debug, Clone, PartialEq, Default)]
enum EdgeEnd {
    /// Not currently drawing an edge.
    #[default]
    None,
    /// Cursor is over empty space — don't create a wire on release.
    Empty,
    /// Cursor is hovering a node's input port — drop will create edge.
    Target { node_id: String },
}

// =============================================================================
// WorkflowCanvas — the main visual component
// =============================================================================

/// Visual workflow editor. Reads/writes the graph through
/// `state.canvas_graph_json`. The parent page is responsible for
/// loading/saving via the runtime bridge.
#[component]
pub fn WorkflowCanvas() -> Element {
    let mut state: VoltState = use_context();
    // Local interaction state — not persisted, not shared.
    let mut drag = use_signal(|| DragMode::None);
    let mut edge_end = use_signal(EdgeEnd::default);
    // Cursor position in canvas-space (for live edge drawing).
    let mut cursor = use_signal::<Option<(f64, f64)>>(|| None);
    // Bumped whenever we mutate the graph in-place, so save handlers
    // can detect unsaved changes. Just a counter, not a content hash.
    let mut dirty = use_signal(|| 0u32);

    // Snapshot signals into local values at the top of render. This
    // is the "stable read" pattern that avoids double-mutate
    // issues when pointer events fire mid-render.
    let graph = parse_graph(&state.canvas_graph_json.peek());
    let viewport: Viewport = graph.viewport;
    let dirty_count = *dirty.peek();

    let current_drag = drag.read().clone();
    let current_cursor = cursor.read().clone();

    // Render edges and nodes as Elements here so we can keep them
    // inline in the parent rsx (avoids the child-component
    // PartialEq/borrow dance).
    let mut edge_elements: Vec<Element> = Vec::new();
    for edge in &graph.edges {
        edge_elements.push(render_edge(edge, &graph));
    }
    let mut node_elements: Vec<Element> = Vec::new();
    for node in &graph.nodes {
        node_elements.push(render_node(
            node,
            drag,
            edge_end,
            cursor,
            state.canvas_graph_json,
        ));
    }
    let live_edge_element = if let DragMode::Edge { from_id } = &current_drag {
        if let Some((cx, cy)) = current_cursor {
            Some(render_live_edge(from_id, cx, cy, &graph))
        } else {
            None
        }
    } else {
        None
    };

    rsx! {
        div { style: "position: relative; width: 100%; height: 600px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; overflow: hidden;",
            // Toolbar
            div { style: "position: absolute; top: 8px; left: 8px; right: 8px; z-index: 10; display: flex; gap: 6px; align-items: center; padding: 6px 10px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px;",
                span { style: "font-size: 12px; color: {COLOR_TEXT_DIM}; font-family: {FONT_MONO};",
                    "{graph.nodes.len()} nodes · {graph.edges.len()} edges"
                }
                if dirty_count > 0 {
                    span { style: "font-size: 11px; padding: 2px 6px; border-radius: 3px; background-color: rgba(245,158,11,0.15); color: {COLOR_WARNING};", "unsaved" }
                }
                div { style: "flex: 1;" }
                button { style: "padding: 4px 10px; background-color: {COLOR_PANEL_HOVER}; color: {COLOR_TEXT}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; font-size: 12px; cursor: pointer;",
                    onclick: move |_| {
                        let mut g = parse_graph(&state.canvas_graph_json.peek());
                        g.auto_layout();
                        g.viewport = Viewport::default();
                        if let Ok(s) = g.to_pretty_json() {
                            state.canvas_graph_json.set(s);
                        }
                        dirty.with_mut(|d| *d += 1);
                    },
                    "Auto-layout"
                }
                button { style: "padding: 4px 10px; background-color: {COLOR_PANEL_HOVER}; color: {COLOR_TEXT}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; font-size: 12px; cursor: pointer;",
                    onclick: move |_| {
                        let mut g = parse_graph(&state.canvas_graph_json.peek());
                        g.viewport = Viewport::default();
                        if let Ok(s) = g.to_pretty_json() {
                            state.canvas_graph_json.set(s);
                        }
                        dirty.with_mut(|d| *d += 1);
                    },
                    "Reset view"
                }
            }

            // SVG canvas
            svg {
                width: "100%",
                height: "100%",
                style: "display: block; cursor: default; touch-action: none; user-select: none;",
                onclick: move |_| {
                    drag.set(DragMode::None);
                    edge_end.set(EdgeEnd::None);
                },
                onmousedown: move |e| {
                    let p = e.element_coordinates();
                    let cx = p.x;
                    let cy = p.y;
                    if e.modifiers().shift() {
                        drag.set(DragMode::Pan { grab_x: cx, grab_y: cy });
                    } else {
                        drag.set(DragMode::None);
                    }
                },
                onmousemove: move |e| {
                    let p = e.element_coordinates();
                    let cx = p.x;
                    let cy = p.y;
                    cursor.set(Some((cx, cy)));
                    let d = drag.read().clone();
                    match &d {
                        DragMode::Node { id, grab_x, grab_y } => {
                            let dx = cx - *grab_x;
                            let dy = cy - *grab_y;
                            let mut g = parse_graph(&state.canvas_graph_json.peek());
                            if let Some(n) = g.nodes.iter_mut().find(|n| &n.id == id) {
                                n.position.x += dx;
                                n.position.y += dy;
                            }
                            drag.set(DragMode::Node { id: id.clone(), grab_x: cx, grab_y: cy });
                            if let Ok(s) = g.to_pretty_json() {
                                state.canvas_graph_json.set(s);
                            }
                            dirty.with_mut(|d| *d += 1);
                        }
                        DragMode::Pan { grab_x, grab_y } => {
                            let mut g = parse_graph(&state.canvas_graph_json.peek());
                            g.viewport.pan_x += cx - *grab_x;
                            g.viewport.pan_y += cy - *grab_y;
                            drag.set(DragMode::Pan { grab_x: cx, grab_y: cy });
                            if let Ok(s) = g.to_pretty_json() {
                                state.canvas_graph_json.set(s);
                            }
                            dirty.with_mut(|d| *d += 1);
                        }
                        DragMode::Edge { .. } => {
                            // Cursor position already updated above.
                        }
                        DragMode::None => {}
                    }
                },
                onmouseup: move |_| {
                    let d = drag.read().clone();
                    let e = edge_end.read().clone();
                    if let DragMode::Edge { from_id } = d {
                        if let EdgeEnd::Target { node_id } = e {
                            if from_id != node_id {
                                let mut g = parse_graph(&state.canvas_graph_json.peek());
                                let _ = g.add_edge(WorkflowEdge {
                                    from: from_id,
                                    to: node_id,
                                    label: None,
                                });
                                if let Ok(s) = g.to_pretty_json() {
                                    state.canvas_graph_json.set(s);
                                }
                                dirty.with_mut(|d| *d += 1);
                            }
                        }
                    }
                    drag.set(DragMode::None);
                    edge_end.set(EdgeEnd::None);
                },
                onwheel: move |e| {
                    let delta = e.delta().strip_units().y;
                    let factor = if delta > 0.0 { 0.9 } else { 1.1 };
                    let mut g = parse_graph(&state.canvas_graph_json.peek());
                    let new_zoom = (g.viewport.zoom * factor).clamp(0.25, 2.0);
                    g.viewport.zoom = new_zoom;
                    if let Ok(s) = g.to_pretty_json() {
                        state.canvas_graph_json.set(s);
                    }
                    dirty.with_mut(|d| *d += 1);
                },

                // Grid pattern (a faint dot grid for visual reference).
                defs {
                    pattern { id: "grid", width: "20", height: "20", pattern_units: "userSpaceOnUse",
                        circle { cx: "10", cy: "10", r: "1", fill: "{COLOR_BORDER}" }
                    }
                }
                // The grid + content are wrapped in a <g> so pan/zoom
                // can be applied via a single transform.
                g {
                    transform: "translate({viewport.pan_x},{viewport.pan_y}) scale({viewport.zoom})",
                    rect { x: "-10000", y: "-10000", width: "20000", height: "20000", fill: "url(#grid)" }

                    // Edges
                    for e_el in edge_elements {
                        {e_el}
                    }

                    // Live edge being drawn
                    if let Some(live) = live_edge_element {
                        {live}
                    }

                    // Nodes
                    for n_el in node_elements {
                        {n_el}
                    }
                }
            }

            // Help hint (bottom-left)
            div { style: "position: absolute; bottom: 8px; left: 8px; z-index: 8; padding: 6px 10px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; font-size: 11px; color: {COLOR_TEXT_MUTED};",
                "Drag node to move · Right port to connect · Shift-drag to pan · Wheel to zoom"
            }
        }
    }
}

// =============================================================================
// Rendering helpers (return Elements directly)
// =============================================================================

fn render_edge(edge: &WorkflowEdge, graph: &WorkflowGraph) -> Element {
    let from = graph.nodes.iter().find(|n| n.id == edge.from);
    let to = graph.nodes.iter().find(|n| n.id == edge.to);
    let (Some(from), Some(to)) = (from, to) else {
        return rsx! { g {} };
    };
    let (x1, y1) = output_port_pos(&from.position);
    let (x2, y2) = input_port_pos(&to.position);
    let dx = (x2 - x1).abs().max(40.0);
    let path = format!(
        "M {x1:.1} {y1:.1} C {hx1:.1} {y1:.1} {hx2:.1} {y2:.1} {x2:.1} {y2:.1}",
        x1 = x1,
        y1 = y1,
        hx1 = x1 + dx,
        hx2 = x2 - dx,
        x2 = x2,
        y2 = y2,
    );
    rsx! {
        g {
            path { d: "{path}", stroke: "transparent", stroke_width: "12", fill: "none" }
            path { d: "{path}", stroke: "{COLOR_ACCENT}", stroke_width: "2", fill: "none" }
            circle { cx: "{x2}", cy: "{y2}", r: "4", fill: "{COLOR_ACCENT}" }
        }
    }
}

fn render_live_edge(from_id: &str, cursor_x: f64, cursor_y: f64, graph: &WorkflowGraph) -> Element {
    let Some(from) = graph.nodes.iter().find(|n| n.id == from_id) else {
        return rsx! { g {} };
    };
    let (x1, y1) = output_port_pos(&from.position);
    let dx = (cursor_x - x1).abs().max(40.0);
    let path = format!(
        "M {x1:.1} {y1:.1} C {hx1:.1} {y1:.1} {hx2:.1} {cursor_y:.1} {cursor_x:.1} {cursor_y:.1}",
        x1 = x1, y1 = y1, hx1 = x1 + dx, hx2 = cursor_x - dx
    );
    rsx! {
        path { d: "{path}", stroke: "{COLOR_ACCENT}", stroke_width: "2", stroke_dasharray: "4 4", fill: "none" }
    }
}

fn render_node(
    node: &WorkflowNode,
    mut drag: Signal<DragMode>,
    mut edge_end: Signal<EdgeEnd>,
    _cursor: Signal<Option<(f64, f64)>>,
    mut graph_signal: Signal<String>,
) -> Element {
    let border_color = COLOR_BORDER;
    let kind_label = match node.kind {
        NodeKind::Agent => "Agent",
        NodeKind::Tool => "Tool",
        NodeKind::Trigger => "Trigger",
        NodeKind::Code => "Code",
        NodeKind::Note => "Note",
    };
    let kind_color = match node.kind {
        NodeKind::Agent => COLOR_ACCENT,
        NodeKind::Tool => COLOR_SUCCESS,
        NodeKind::Trigger => COLOR_WARNING,
        NodeKind::Code => COLOR_INFO,
        NodeKind::Note => COLOR_TEXT_MUTED,
    };
    let (ix, iy) = input_port_pos(&node.position);
    let (ox, oy) = output_port_pos(&node.position);
    let id_press = node.id.clone();
    let id_enter = node.id.clone();
    let id_leave = node.id.clone();
    let id_click = node.id.clone();
    let id_output = node.id.clone();
    let pos_x = node.position.x;
    let pos_y = node.position.y;
    let label = node.label.clone();
    let agent = node.agent_name.clone().unwrap_or_default();
    let task_preview = truncate(&node.task, 28);

    rsx! {
        g {
            rect {
                x: "{pos_x}",
                y: "{pos_y}",
                width: "{NODE_W}",
                height: "{NODE_H}",
                rx: "8",
                ry: "8",
                fill: "{COLOR_PANEL}",
                stroke: "{border_color}",
                stroke_width: "1",
                style: "cursor: move;",
                onmousedown: move |e| {
                    e.stop_propagation();
                    let p = e.element_coordinates();
                    let cx = p.x;
                    let cy = p.y;
                    drag.set(DragMode::Node {
                        id: id_press.clone(),
                        grab_x: cx,
                        grab_y: cy,
                    });
                },
                onclick: move |e| {
                    e.stop_propagation();
                    let cur = graph_signal.peek().clone();
                    let mut g = parse_graph(&cur);
                    if let Some(n) = g.nodes.iter_mut().find(|n| n.id == id_click) {
                        n.notes = Some(format!(
                            "__selected_at:{}__",
                            chrono::Utc::now().timestamp_millis()
                        ));
                    }
                    if let Ok(json) = g.to_pretty_json() {
                        graph_signal.set(json);
                    }
                }
            }
            rect { x: "{pos_x + 8.0}", y: "{pos_y + 8.0}", width: "62.0", height: "16.0", rx: "3", ry: "3", fill: "{kind_color}", opacity: "0.18" }
            text { x: "{pos_x + 12.0}", y: "{pos_y + 20.0}", fill: "{kind_color}", font_size: "10", font_family: "{FONT_MONO}", font_weight: "600",
                "{kind_label}"
            }
            text { x: "{pos_x + 8.0}", y: "{pos_y + 44.0}", fill: "{COLOR_TEXT}", font_size: "13", font_weight: "600",
                "{label}"
            }
            text { x: "{pos_x + 8.0}", y: "{pos_y + 62.0}", fill: "{COLOR_TEXT_DIM}", font_size: "10", font_family: "{FONT_MONO}",
                "{agent}"
            }
            text { x: "{pos_x + 8.0}", y: "{pos_y + 78.0}", fill: "{COLOR_TEXT_MUTED}", "font-size": "10",
                "{task_preview}"
            }

            circle { cx: "{ix}", cy: "{iy}", r: "6", fill: "{COLOR_BG}", stroke: "{COLOR_ACCENT}", stroke_width: "2",
                onmouseenter: move |_| edge_end.set(EdgeEnd::Target { node_id: id_enter.clone() }),
                onmouseleave: move |_| {
                    let cur = edge_end.read().clone();
                    if let EdgeEnd::Target { node_id } = cur {
                        if node_id == id_leave {
                            edge_end.set(EdgeEnd::Empty);
                        }
                    }
                },
                onmousedown: move |e| e.stop_propagation(),
            }
            circle { cx: "{ox}", cy: "{oy}", r: "6", fill: "{COLOR_BG}", stroke: "{COLOR_ACCENT}", stroke_width: "2",
                style: "cursor: crosshair;",
                onmousedown: move |e| {
                    e.stop_propagation();
                    drag.set(DragMode::Edge { from_id: id_output.clone() });
                }
            }
        }
    }
}

fn input_port_pos(p: &NodePosition) -> (f64, f64) {
    (p.x, p.y + NODE_H / 2.0)
}

fn output_port_pos(p: &NodePosition) -> (f64, f64) {
    (p.x + NODE_W, p.y + NODE_H / 2.0)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('\u{2026}');
        out
    }
}

// =============================================================================
// Graph parsing — tolerant, falls back to empty on parse error
// =============================================================================

fn parse_graph(json: &str) -> WorkflowGraph {
    if json.trim().is_empty() {
        return WorkflowGraph::new("untitled");
    }
    WorkflowGraph::from_json(json).unwrap_or_else(|_| WorkflowGraph::new("untitled"))
}

// =============================================================================
// Inspector — selected-node properties panel
// =============================================================================

/// Property editor for the currently selected node. Reads the
/// selected node ID from the `notes` field hack (see `render_node`)
/// — Phase 2 will replace with a proper selection signal.
#[component]
pub fn CanvasInspector() -> Element {
    let mut state: VoltState = use_context();
    let graph = parse_graph(&state.canvas_graph_json.peek());
    // Find a node whose notes contain the selected-at marker.
    let selected: Option<WorkflowNode> = graph.nodes.iter().find(|n| {
        n.notes
            .as_deref()
            .map(|s| s.starts_with("__selected_at:"))
            .unwrap_or(false)
    }).cloned();

    match selected {
        Some(node) => {
            let node_id_for_label = node.id.clone();
            let node_id_for_agent = node.id.clone();
            let node_id_for_model = node.id.clone();
            let node_id_for_task = node.id.clone();
            let node_id_for_delete = node.id.clone();
            let label = node.label.clone();
            let agent = node.agent_name.clone().unwrap_or_default();
            let model = node.model.clone().unwrap_or_default();
            let task = node.task.clone();

            // Brace-escape the placeholder text in JSX by using a
            // local string so rsx doesn't interpret `{input}`.
            let task_placeholder = "Task template (use {input} or {other_node_id})";

            rsx! {
                div { style: "padding: 12px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px;",
                    div { style: "display: flex; flex-direction: column; gap: 8px;",
                        div { style: "font-size: 12px; color: {COLOR_TEXT_DIM};", "Selected" }
                        input { style: "padding: 6px 8px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; color: {COLOR_TEXT}; font-size: 12px;",
                            placeholder: "Label",
                            value: "{label}",
                            oninput: move |e| {
                                let mut g = parse_graph(&state.canvas_graph_json.peek());
                                if let Some(n) = g.nodes.iter_mut().find(|n| n.id == node_id_for_label) {
                                    n.label = e.value().to_string();
                                }
                                if let Ok(s) = g.to_pretty_json() {
                                    state.canvas_graph_json.set(s);
                                }
                            }
                        }
                        input { style: "padding: 6px 8px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; color: {COLOR_TEXT}; font-size: 12px; font-family: {FONT_MONO};",
                            placeholder: "Agent name",
                            value: "{agent}",
                            oninput: move |e| {
                                let mut g = parse_graph(&state.canvas_graph_json.peek());
                                if let Some(n) = g.nodes.iter_mut().find(|n| n.id == node_id_for_agent) {
                                    n.agent_name = Some(e.value().to_string());
                                }
                                if let Ok(s) = g.to_pretty_json() {
                                    state.canvas_graph_json.set(s);
                                }
                            }
                        }
                        input { style: "padding: 6px 8px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; color: {COLOR_TEXT}; font-size: 12px; font-family: {FONT_MONO};",
                            placeholder: "Model (e.g. qwen/qwen3-32b)",
                            value: "{model}",
                            oninput: move |e| {
                                let mut g = parse_graph(&state.canvas_graph_json.peek());
                                if let Some(n) = g.nodes.iter_mut().find(|n| n.id == node_id_for_model) {
                                    n.model = Some(e.value().to_string());
                                }
                                if let Ok(s) = g.to_pretty_json() {
                                    state.canvas_graph_json.set(s);
                                }
                            }
                        }
                        textarea { style: "padding: 6px 8px; background-color: {COLOR_BG}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; color: {COLOR_TEXT}; font-size: 12px; font-family: {FONT_MONO}; min-height: 60px; resize: vertical;",
                            placeholder: "{task_placeholder}",
                            value: "{task}",
                            oninput: move |e| {
                                let mut g = parse_graph(&state.canvas_graph_json.peek());
                                if let Some(n) = g.nodes.iter_mut().find(|n| n.id == node_id_for_task) {
                                    n.task = e.value().to_string();
                                }
                                if let Ok(s) = g.to_pretty_json() {
                                    state.canvas_graph_json.set(s);
                                }
                            }
                        }
                        button { style: "padding: 4px 10px; background-color: transparent; color: {COLOR_DANGER}; border: 1px solid {COLOR_DANGER}; border-radius: 4px; font-size: 12px; cursor: pointer;",
                            onclick: move |_| {
                                let mut g = parse_graph(&state.canvas_graph_json.peek());
                                g.remove_node(&node_id_for_delete);
                                if let Ok(s) = g.to_pretty_json() {
                                    state.canvas_graph_json.set(s);
                                }
                            },
                            "Delete node"
                        }
                    }
                }
            }
        }
        None => rsx! {
            div { style: "padding: 12px; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; font-size: 12px; color: {COLOR_TEXT_MUTED};",
                "Click a node to edit its properties"
            }
        },
    }
}

// =============================================================================
// Quick-add menu — adds a new agent node at a default position
// =============================================================================

#[component]
pub fn CanvasQuickAdd() -> Element {
    let mut state: VoltState = use_context();
    rsx! {
        div { style: "display: flex; gap: 6px; align-items: center;",
            button { style: "padding: 4px 10px; background-color: {COLOR_PANEL_HOVER}; color: {COLOR_TEXT}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; font-size: 12px; cursor: pointer;",
                onclick: move |_| {
                    let mut g = parse_graph(&state.canvas_graph_json.peek());
                    let id = next_node_id(&g);
                    g.add_node(WorkflowNode {
                        id: id.clone(),
                        label: format!("Agent {}", g.nodes.len() + 1),
                        kind: NodeKind::Agent,
                        role: None,
                        agent_name: Some(format!("agent-{}", g.nodes.len() + 1)),
                        model: Some("qwen/qwen3-32b".into()),
                        system_prompt: None,
                        task: "Process {input}".into(),
                        config: json!(null),
                        position: NodePosition { x: 100.0 + (g.nodes.len() as f64) * 40.0, y: 100.0 },
                        notes: None,
                    });
                    if let Ok(s) = g.to_pretty_json() {
                        state.canvas_graph_json.set(s);
                    }
                },
                "+ Agent"
            }
            button { style: "padding: 4px 10px; background-color: {COLOR_PANEL_HOVER}; color: {COLOR_TEXT}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; font-size: 12px; cursor: pointer;",
                onclick: move |_| {
                    let mut g = parse_graph(&state.canvas_graph_json.peek());
                    let id = next_node_id(&g);
                    g.add_node(WorkflowNode {
                        id: id.clone(),
                        label: format!("Note {}", g.nodes.len() + 1),
                        kind: NodeKind::Note,
                        role: None,
                        agent_name: None,
                        model: None,
                        system_prompt: None,
                        task: String::new(),
                        config: json!(null),
                        position: NodePosition { x: 100.0 + (g.nodes.len() as f64) * 40.0, y: 100.0 },
                        notes: Some("Double-click to edit".into()),
                    });
                    if let Ok(s) = g.to_pretty_json() {
                        state.canvas_graph_json.set(s);
                    }
                },
                "+ Note"
            }
        }
    }
}

fn next_node_id(g: &WorkflowGraph) -> String {
    let mut n = 1u32;
    loop {
        let id = format!("node_{}", n);
        if !g.nodes.iter().any(|node| node.id == id) {
            return id;
        }
        n += 1;
    }
}
