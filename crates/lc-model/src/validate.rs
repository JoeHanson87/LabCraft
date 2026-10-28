use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::document::{is_structure, kinds, Node, Scope, SourceRef, ViDocument};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// Machine-actionable repair hints, consumed by agents and `vi_autofix`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            message: message.into(),
            node: None,
            port: None,
            fixes: Vec::new(),
        }
    }
    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.into(),
            message: message.into(),
            node: None,
            port: None,
            fixes: Vec::new(),
        }
    }
    pub fn at(mut self, node: impl Into<String>) -> Self {
        self.node = Some(node.into());
        self
    }
    pub fn port(mut self, port: impl Into<String>) -> Self {
        self.port = Some(port.into());
        self
    }
    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fixes.push(fix.into());
        self
    }
}

/// Structural validation that does not need the node registry: identity,
/// references, scope boundaries, structure shape, and cycles.
pub fn validate_structure(doc: &ViDocument) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut control_keys = BTreeSet::new();
    let mut control_ids = BTreeSet::new();
    for c in &doc.panel.controls {
        if !control_keys.insert(c.key.clone()) {
            out.push(
                Diagnostic::error(
                    "DUPLICATE_CONTROL_KEY",
                    format!("duplicate panel control key `{}`", c.key),
                )
                .fix("rename_control_key"),
            );
        }
        if !control_ids.insert(c.id.clone()) {
            out.push(Diagnostic::error(
                "DUPLICATE_CONTROL_ID",
                format!("duplicate panel control id `{}`", c.id),
            ));
        }
        if matches!(c.ty, crate::Type::Void) {
            out.push(
                Diagnostic::warning(
                    "VOID_CONTROL_TYPE",
                    format!("control `{}` has no data type", c.key),
                )
                .fix("set_control_type"),
            );
        }
    }

    let mut node_keys: BTreeMap<String, String> = BTreeMap::new();
    let mut node_ids: BTreeSet<String> = BTreeSet::new();
    doc.for_each_node(&mut |n| {
        if let Some(prev) = node_keys.insert(n.key.clone(), n.id.clone()) {
            if prev != n.id {
                out.push(
                    Diagnostic::error(
                        "DUPLICATE_NODE_KEY",
                        format!("node key `{}` is used more than once", n.key),
                    )
                    .at(&n.id)
                    .fix("rename_node_key"),
                );
            }
        }
        if !node_ids.insert(n.id.clone()) {
            out.push(
                Diagnostic::error(
                    "DUPLICATE_NODE_ID",
                    format!("node id `{}` is duplicated", n.id),
                )
                .at(&n.id),
            );
        }
    });

    let mut stack: Vec<Node> = Vec::new();
    check_scope(doc, &doc.diagram.root, &mut stack, &mut out);
    out
}

fn check_scope(doc: &ViDocument, scope: &Scope, stack: &mut Vec<Node>, out: &mut Vec<Diagnostic>) {
    let ids: BTreeSet<&str> = scope.nodes.iter().map(|n| n.id.as_str()).collect();

    for node in &scope.nodes {
        // Control/indicator terminators must reference a real panel control.
        if node.kind == kinds::CONTROL || node.kind == kinds::INDICATOR {
            match node.control_key() {
                Some(key) if doc.control(key).is_some() => {
                    let c = doc.control(key).unwrap();
                    let wants_indicator = node.kind == kinds::INDICATOR;
                    if wants_indicator && !c.is_indicator() {
                        out.push(
                            Diagnostic::warning(
                                "CONTROL_KIND_MISMATCH",
                                format!(
                                    "`{}` is used as an indicator terminal but its widget is {:?}",
                                    key, c.kind
                                ),
                            )
                            .at(&node.id)
                            .fix("change_control_kind"),
                        );
                    }
                    if wants_indicator && !node.inputs.contains_key("value") {
                        out.push(
                            Diagnostic::error(
                                "INDICATOR_UNWIRED",
                                format!("indicator `{key}` has no value wired to it"),
                            )
                            .at(&node.id)
                            .port("value")
                            .fix("connect_source"),
                        );
                    }
                }
                Some(key) => out.push(
                    Diagnostic::error(
                        "MISSING_CONTROL",
                        format!(
                            "node `{}` references missing panel control `{key}`",
                            node.key
                        ),
                    )
                    .at(&node.id)
                    .fix("add_panel_control"),
                ),
                None => out.push(
                    Diagnostic::error(
                        "MISSING_CONTROL_KEY",
                        format!("{} node `{}` has no control key", node.kind, node.key),
                    )
                    .at(&node.id)
                    .fix("set_control_key"),
                ),
            }
        }

        if is_structure(&node.kind) && node.kind != kinds::LOOP_CONDITION {
            match node.kind.as_str() {
                kinds::WHILE | kinds::FOR => {
                    if !node.frames.iter().any(|f| f.label == "body") {
                        out.push(
                            Diagnostic::error(
                                "STRUCTURE_MISSING_BODY",
                                format!("`{}` has no `body` frame", node.key),
                            )
                            .at(&node.id)
                            .fix("add_body_frame"),
                        );
                    }
                }
                kinds::CASE => {
                    if node.frames.is_empty() {
                        out.push(
                            Diagnostic::error(
                                "CASE_HAS_NO_FRAMES",
                                format!("case structure `{}` has no frames", node.key),
                            )
                            .at(&node.id)
                            .fix("add_case_frame"),
                        );
                    }
                    if !node.frames.iter().any(|f| f.label == "default") {
                        out.push(
                            Diagnostic::warning(
                                "CASE_NO_DEFAULT",
                                format!("case structure `{}` has no default frame", node.key),
                            )
                            .at(&node.id)
                            .fix("add_default_frame"),
                        );
                    }
                }
                kinds::SEQUENCE | kinds::EVENT => {}
                _ => {}
            }
        }

        // Validate every input source reference.
        for (port, src) in &node.inputs {
            match src {
                SourceRef::Node {
                    node: from,
                    port: from_port,
                } => {
                    if !ids.contains(from.as_str()) {
                        out.push(
                            Diagnostic::error(
                                "WIRE_CROSSES_SCOPE",
                                format!(
                                    "input `{port}` of `{}` is wired to node `{from}`, which is not in the same scope",
                                    node.key
                                ),
                            )
                            .at(&node.id)
                            .port(port)
                            .fix("use_tunnel"),
                        );
                    } else if let Some(src_node) = scope.node(from) {
                        if src_node.id == node.id {
                            out.push(
                                Diagnostic::error(
                                    "SELF_WIRE",
                                    format!("`{}` is wired to itself", node.key),
                                )
                                .at(&node.id)
                                .port(port),
                            );
                        }
                        if from_port.is_empty() {
                            out.push(
                                Diagnostic::error(
                                    "MISSING_SOURCE_PORT",
                                    format!("wire into `{port}` does not name a source port"),
                                )
                                .at(&node.id)
                                .port(port)
                                .fix("set_source_port"),
                            );
                        }
                    }
                }
                SourceRef::Tunnel { structure, name } => {
                    if !stack.iter().any(|s| &s.id == structure) {
                        out.push(
                            Diagnostic::error(
                                "TUNNEL_OUTSIDE_STRUCTURE",
                                format!("tunnel `{name}` references structure `{structure}` outside its body"),
                            )
                            .at(&node.id)
                            .port(port)
                            .fix("create_tunnel"),
                        );
                    } else if let Some(s) = stack.iter().find(|s| &s.id == structure) {
                        if s.tunnel(name).is_none() {
                            out.push(
                                Diagnostic::error(
                                    "MISSING_TUNNEL",
                                    format!("structure `{}` has no tunnel `{name}`", s.key),
                                )
                                .at(&node.id)
                                .port(port)
                                .fix("add_tunnel"),
                            );
                        }
                    }
                }
                SourceRef::ShiftRegister { structure, name } => {
                    if !stack.iter().any(|s| &s.id == structure) {
                        out.push(
                            Diagnostic::error(
                                "SHIFT_REGISTER_OUTSIDE_STRUCTURE",
                                format!("shift register `{name}` used outside its loop"),
                            )
                            .at(&node.id)
                            .port(port)
                            .fix("create_shift_register"),
                        );
                    } else if let Some(s) = stack.iter().find(|s| &s.id == structure) {
                        if !s.shift_registers.iter().any(|r| &r.name == name) {
                            out.push(
                                Diagnostic::error(
                                    "MISSING_SHIFT_REGISTER",
                                    format!("loop `{}` has no shift register `{name}`", s.key),
                                )
                                .at(&node.id)
                                .port(port)
                                .fix("add_shift_register"),
                            );
                        }
                    }
                }
                SourceRef::Iteration { structure } => {
                    if !stack.iter().any(|s| &s.id == structure) {
                        out.push(
                            Diagnostic::error(
                                "ITERATION_OUTSIDE_LOOP",
                                format!("iteration terminal used outside loop `{structure}`"),
                            )
                            .at(&node.id)
                            .port(port)
                            .fix("move_into_loop"),
                        );
                    }
                }
                SourceRef::EventData { structure, .. } => {
                    if !stack.iter().any(|s| &s.id == structure) {
                        out.push(
                            Diagnostic::error(
                                "EVENT_DATA_OUTSIDE_EVENT",
                                format!("event data used outside event structure `{structure}`"),
                            )
                            .at(&node.id)
                            .port(port)
                            .fix("move_into_event"),
                        );
                    }
                }
                SourceRef::Default { .. } => {}
            }
        }

        // Descend into frames with this node on the stack.
        if !node.frames.is_empty() {
            stack.push(node.clone());
            for frame in &node.frames {
                check_scope(doc, &frame.scope, stack, out);
            }
            stack.pop();
        }
    }

    for (key, src) in &scope.outputs {
        if let SourceRef::Node { node: from, .. } = src {
            if !ids.contains(from.as_str()) {
                out.push(Diagnostic::error(
                    "FRAME_OUTPUT_CROSSES_SCOPE",
                    format!("frame output `{key}` references node `{from}` outside its scope"),
                ));
            }
        }
    }

    if let Some(cycle) = find_cycle(scope) {
        out.push(
            Diagnostic::error(
                "CYCLE_WITHOUT_STRUCTURE",
                format!(
                    "dataflow cycle through {} must be expressed with a loop structure",
                    cycle.join(" -> ")
                ),
            )
            .fix("wrap_in_loop"),
        );
    }
}

/// Detect a cycle among nodes in a single scope. Cycles inside a loop body are
/// still invalid; iteration is expressed by the loop node itself.
fn find_cycle(scope: &Scope) -> Option<Vec<String>> {
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    let mut path: Vec<&str> = Vec::new();

    fn visit<'a>(
        id: &'a str,
        scope: &'a Scope,
        state: &mut BTreeMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match state.get(id).copied().unwrap_or(0) {
            1 => {
                let start = path.iter().position(|p| *p == id).unwrap_or(0);
                let mut cycle: Vec<String> = path[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(id.to_string());
                return Some(cycle);
            }
            2 => return None,
            _ => {}
        }
        state.insert(id, 1);
        path.push(id);
        if let Some(node) = scope.node(id) {
            for src in node.inputs.values() {
                if let SourceRef::Node { node: from, .. } = src {
                    if let Some(cycle) = visit(from, scope, state, path) {
                        return Some(cycle);
                    }
                }
            }
        }
        path.pop();
        state.insert(id, 2);
        None
    }

    for node in &scope.nodes {
        if let Some(cycle) = visit(&node.id, scope, &mut state, &mut path) {
            return Some(cycle);
        }
    }
    None
}
