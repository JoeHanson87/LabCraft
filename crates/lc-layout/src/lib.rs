//! Automatic diagram layout and orthogonal wire routing.
//!
//! The layout is deliberately deterministic: the same document always produces
//! the same coordinates, which keeps golden tests, screenshots and agent diffs
//! stable. It is a layered placement without crossing minimisation yet; the
//! router produces clean three-segment orthogonal runs.

use lc_model::document::{is_structure, kinds, Node, Point, Rect, Scope, ViDocument};
use lc_model::Type;
use lc_nodes::Registry;

pub const HEADER_H: f32 = 28.0;
pub const PIN_SPACING: f32 = 20.0;
pub const LAYER_GAP: f32 = 110.0;
pub const ROW_GAP: f32 = 34.0;

#[derive(Debug, Clone)]
pub struct Geometry {
    pub rect: Rect,
    pub inputs: Vec<(String, Point)>,
    pub outputs: Vec<(String, Point)>,
}

impl Geometry {
    pub fn input(&self, port: &str) -> Option<Point> {
        self.inputs
            .iter()
            .find(|(p, _)| p == port)
            .map(|(_, pt)| *pt)
    }
    pub fn output(&self, port: &str) -> Option<Point> {
        self.outputs
            .iter()
            .find(|(p, _)| p == port)
            .map(|(_, pt)| *pt)
    }
}

/// Compute node size and pin coordinates from the registry schema. Ports are
/// distributed down the left and right edges below the header.
pub fn geometry(node: &Node, reg: &Registry) -> Geometry {
    let (mut w, mut h) = default_size(node, reg);
    if let Some(size) = node.size {
        w = size.x.max(90.0);
        h = size.y.max(44.0);
    }
    let (input_names, output_names) = port_names(node, reg);
    let rect = Rect::new(node.pos.x, node.pos.y, w, h);

    let ins = pin_span(node, h, input_names.len());
    let outs = pin_span(node, h, output_names.len());

    let inputs = input_names
        .into_iter()
        .zip(ins)
        .map(|(name, p)| (name, Point::new(node.pos.x, p.y)))
        .collect();
    let outputs = output_names
        .into_iter()
        .zip(outs)
        .map(|(name, p)| (name, Point::new(node.pos.x + w, p.y)))
        .collect();

    Geometry {
        rect,
        inputs,
        outputs,
    }
}

fn pin_span(node: &Node, h: f32, n: usize) -> Vec<Point> {
    if n == 0 {
        return Vec::new();
    }
    let top = node.pos.y + HEADER_H + 14.0;
    let bottom = node.pos.y + h - 12.0;
    let step = if n == 1 {
        0.0
    } else {
        (bottom - top).max(0.0) / (n - 1) as f32
    };
    (0..n)
        .map(|i| Point::new(0.0, top + step * i as f32))
        .collect()
}

pub fn port_names(node: &Node, reg: &Registry) -> (Vec<String>, Vec<String>) {
    if node.kind == kinds::CONTROL {
        return (vec![], vec!["value".into()]);
    }
    if node.kind == kinds::INDICATOR {
        return (vec!["value".into()], vec![]);
    }
    if node.kind == kinds::CONSTANT {
        return (vec![], vec!["value".into()]);
    }
    if is_structure(&node.kind) {
        let mut ins = Vec::new();
        if node.kind == kinds::FOR {
            ins.push("n".to_string());
        }
        if node.kind == kinds::CASE {
            ins.push("selector".to_string());
        }
        if node.kind == kinds::EVENT {
            ins.push("timeout".to_string());
        }
        for t in &node.tunnels {
            if t.direction == lc_model::TunnelDirection::In {
                ins.push(format!("t:{}", t.name));
            }
        }
        let outs = node
            .tunnels
            .iter()
            .filter(|t| t.direction == lc_model::TunnelDirection::Out)
            .map(|t| format!("t:{}", t.name))
            .collect();
        return (ins, outs);
    }
    let Some(def) = reg.get(&node.kind) else {
        return (Vec::new(), Vec::new());
    };
    let outs = def.outputs.iter().map(|p| p.name.to_string()).collect();
    if def.variadic {
        let mut ins: Vec<String> = node.inputs.keys().cloned().collect();
        let mut refs: Vec<&String> = ins.iter().collect();
        lc_nodes::sort_ports(&mut refs);
        ins = refs.into_iter().cloned().collect();
        return (ins, outs);
    }
    let ins = def.inputs.iter().map(|p| p.name.to_string()).collect();
    (ins, outs)
}

pub fn default_size(node: &Node, reg: &Registry) -> (f32, f32) {
    match node.kind.as_str() {
        kinds::CONSTANT => (130.0, 34.0),
        kinds::CONTROL | kinds::INDICATOR => (140.0, 48.0),
        kinds::WHILE | kinds::FOR | kinds::CASE | kinds::SEQUENCE | kinds::EVENT => (420.0, 260.0),
        _ => {
            let (ins, outs) = port_names(node, reg);
            let rows = ins.len().max(outs.len()).max(2);
            (
                180.0,
                (HEADER_H + 22.0 + rows as f32 * PIN_SPACING).max(72.0),
            )
        }
    }
}

/// Lay out the whole VI. Existing positions are replaced.
pub fn layout_vi(doc: &mut ViDocument) {
    let reg = Registry::global();
    let origin = Point::new(60.0, 80.0);
    layout_scope(&mut doc.diagram.root, origin, reg);
    layout_panel(doc);
}

/// Position panel controls in a tidy two-column flow.
pub fn layout_panel(doc: &mut ViDocument) {
    let mut y = 72.0f32;
    let mut x = 40.0f32;
    let mut col = 0;
    for c in &mut doc.panel.controls {
        c.bounds.x = x;
        c.bounds.y = y;
        if c.bounds.w < 100.0 {
            c.bounds.w = 160.0;
        }
        c.bounds.h = c.bounds.h.max(32.0);
        col += 1;
        if col == 2 {
            col = 0;
            x = 40.0;
            y += 72.0;
        } else {
            x = 260.0;
        }
    }
}

pub fn layout_scope(scope: &mut Scope, origin: Point, reg: &Registry) {
    let layers = layer_map(scope);
    let max_layer = layers.values().copied().max().unwrap_or(0);
    let mut per_layer: Vec<Vec<usize>> = vec![Vec::new(); max_layer + 1];
    for (idx, node) in scope.nodes.iter().enumerate() {
        let layer = layers.get(&node.id).copied().unwrap_or(0);
        per_layer[layer].push(idx);
    }
    for (layer, indices) in per_layer.iter_mut().enumerate() {
        indices.sort_by(|a, b| scope.nodes[*a].key.cmp(&scope.nodes[*b].key));
        let mut y = origin.y;
        for idx in indices.iter() {
            let node = &mut scope.nodes[*idx];
            let (w, _h) = default_size(node, reg);
            node.pos = Point::new(origin.x + layer as f32 * (w + LAYER_GAP), y);
            if node.size.is_none() && is_structure(&node.kind) {
                node.size = Some(Point::new(420.0, 260.0));
            }
            y += 120.0 + ROW_GAP;
        }
    }

    // Lay out nested frames and grow structures to fit.
    for node in &mut scope.nodes {
        if node.frames.is_empty() {
            continue;
        }
        let inner_origin = Point::new(node.pos.x + 28.0, node.pos.y + 78.0);
        for (i, frame) in node.frames.iter_mut().enumerate() {
            let offset = if node.kind == kinds::SEQUENCE {
                Point::new(inner_origin.x + i as f32 * 460.0, inner_origin.y)
            } else {
                inner_origin
            };
            layout_scope(&mut frame.scope, offset, reg);
        }
        if node.size.is_none()
            || node
                .size
                .map(|s| s.x < 420.0 || s.y < 260.0)
                .unwrap_or(false)
        {
            let mut max_x = node.pos.x + 420.0;
            let mut max_y = node.pos.y + 220.0;
            for frame in &node.frames {
                for n in &frame.scope.nodes {
                    let (w, h) = default_size(n, reg);
                    max_x = max_x.max(n.pos.x + w + 40.0);
                    max_y = max_y.max(n.pos.y + h + 60.0);
                }
            }
            node.size = Some(Point::new(max_x - node.pos.x, max_y - node.pos.y));
        }
    }
}

/// Longest-path layering within one scope.
fn layer_map(scope: &Scope) -> std::collections::BTreeMap<String, usize> {
    use std::collections::BTreeMap;
    let mut layers: BTreeMap<String, usize> =
        scope.nodes.iter().map(|n| (n.id.clone(), 0usize)).collect();
    let mut changed = true;
    let mut guard = 0;
    while changed && guard < scope.nodes.len() + 2 {
        changed = false;
        guard += 1;
        for node in &scope.nodes {
            let mut want = 0usize;
            for src in node.inputs.values() {
                if let lc_model::SourceRef::Node { node: from, .. } = src {
                    if let Some(l) = layers.get(from) {
                        want = want.max(*l + 1);
                    }
                }
            }
            if let Some(cur) = layers.get_mut(&node.id) {
                if want > *cur {
                    *cur = want;
                    changed = true;
                }
            }
        }
    }
    layers
}

/// Three-segment orthogonal wire path with a little breathing room.
pub fn route_wire(from: Point, to: Point) -> Vec<Point> {
    let min_stub = 42.0;
    let base_x = if to.x > from.x + min_stub * 2.0 {
        (from.x + to.x) / 2.0
    } else {
        from.x + min_stub
    };
    // Give wires with different vertical spans their own channel so parallel
    // runs do not overlap and read as an electrical junction.
    let lane = ((to.y - from.y) / 16.0).clamp(-26.0, 26.0);
    let mid_x = base_x + lane;
    if (from.y - to.y).abs() < 1.0 {
        return vec![from, to];
    }
    vec![from, Point::new(mid_x, from.y), Point::new(mid_x, to.y), to]
}

/// Resolve the endpoints of every wire in a scope. Returns (from, to, source
/// port, destination port).
pub fn wires(scope: &Scope, reg: &Registry) -> Vec<(Point, Point, String, String)> {
    let mut out = Vec::new();
    for node in &scope.nodes {
        let to_geom = geometry(node, reg);
        for (port, src) in &node.inputs {
            let lc_model::SourceRef::Node {
                node: from,
                port: from_port,
            } = src
            else {
                continue;
            };
            let Some(src_node) = scope.node(from) else {
                continue;
            };
            let from_geom = geometry(src_node, reg);
            let (Some(a), Some(b)) = (from_geom.output(from_port), to_geom.input(port)) else {
                continue;
            };
            out.push((a, b, from_port.clone(), port.clone()));
        }
    }
    out
}

/// Does this node's output type support a coercion dot in the UI?
pub fn is_coercion(from: &Type, to: &Type) -> bool {
    from != to && to.accepts(from)
}
