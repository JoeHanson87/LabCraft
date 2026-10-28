//! Static type checking and the deterministic dataflow interpreter.
//!
//! Compilation resolves every port to a concrete [`Type`] and records coercion
//! points. Execution walks a scope in dependency order, gives structures their
//! own frame semantics, and keeps going in the face of errors long enough to
//! report a useful diagnostic (with machine-actionable fixes).

mod interp;

use std::collections::{BTreeMap, BTreeSet};

use lc_model::document::{
    is_structure, kinds, Node, Scope, SourceRef, TunnelDirection, TunnelMode, ViDocument,
};
use lc_model::{Diagnostic, Severity, Type, Value};
use lc_nodes::Registry;

pub use interp::{run, run_tests, Event, RunError, RunOptions, RunOutput, TestReport, TraceEvent};

/// A fully type-checked VI. The compiled snapshot is immutable; editing the
/// document during a run does not affect the running instance.
#[derive(Debug, Clone)]
pub struct CompiledVi {
    pub doc: ViDocument,
    /// node id -> port -> type
    pub node_types: BTreeMap<String, BTreeMap<String, Type>>,
    /// input port -> target type, for coercion.
    pub input_types: BTreeMap<String, BTreeMap<String, Type>>,
}

impl CompiledVi {
    pub fn output_type(&self, node: &str, port: &str) -> Option<&Type> {
        self.node_types.get(node).and_then(|m| m.get(port))
    }
    pub fn input_type(&self, node: &str, port: &str) -> Option<&Type> {
        self.input_types.get(node).and_then(|m| m.get(port))
    }
}

pub struct CompileReport {
    pub compiled: Option<CompiledVi>,
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileReport {
    pub fn ok(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

struct ScopeCtx {
    tunnels: BTreeMap<(String, String), Type>,
    shifts: BTreeMap<(String, String), Type>,
    iteration: Option<String>,
    event: Option<String>,
}

impl ScopeCtx {
    fn root() -> Self {
        Self {
            tunnels: BTreeMap::new(),
            shifts: BTreeMap::new(),
            iteration: None,
            event: None,
        }
    }
}

struct Checker<'a> {
    doc: &'a ViDocument,
    reg: &'a Registry,
    compiled: CompiledVi,
    diagnostics: Vec<Diagnostic>,
    /// Nodes whose own check failed. Wires out of these nodes are not reported
    /// again as broken sources; the root diagnostic is enough to act on.
    failed: BTreeSet<String>,
}

pub fn compile(doc: &ViDocument) -> CompileReport {
    let reg = Registry::global();
    let mut checker = Checker {
        doc,
        reg,
        compiled: CompiledVi {
            doc: doc.clone(),
            node_types: BTreeMap::new(),
            input_types: BTreeMap::new(),
        },
        diagnostics: lc_model::validate::validate_structure(doc),
        failed: BTreeSet::new(),
    };
    checker.check_scope(&doc.diagram.root, &ScopeCtx::root());

    // Types are always returned so tools can refine partially-built graphs;
    // `ok()` tells callers whether execution is safe.
    CompileReport {
        compiled: Some(checker.compiled),
        diagnostics: checker.diagnostics,
    }
}

impl<'a> Checker<'a> {
    fn check_scope(&mut self, scope: &Scope, ctx: &ScopeCtx) {
        let order = topo_order(scope);
        for id in order {
            let Some(node) = scope.node(&id) else {
                continue;
            };
            self.check_node(node, scope, ctx);
        }
    }

    fn check_node(&mut self, node: &Node, scope: &Scope, ctx: &ScopeCtx) {
        if is_structure(&node.kind) {
            let before = self.diagnostics.len();
            self.check_structure(node, scope, ctx);
            if self.has_new_error(before) {
                self.failed.insert(node.id.clone());
            }
            return;
        }

        let Some(def) = self.reg.get(&node.kind) else {
            self.error(
                "UNKNOWN_NODE",
                format!("unknown node kind `{}`", node.kind),
                node,
                None,
            );
            self.failed.insert(node.id.clone());
            return;
        };

        let mut input_types = BTreeMap::new();
        for (port, src) in &node.inputs {
            match self.source_type(src, scope, ctx) {
                Ok(ty) => {
                    input_types.insert(port.clone(), ty);
                }
                Err(msg) => {
                    if !self.source_failed(src) {
                        self.error("BROKEN_SOURCE", msg, node, Some(port));
                    }
                }
            }
        }

        // Required pins and unknown ports.
        if !def.variadic {
            for pin in &def.inputs {
                if pin.required && !node.inputs.contains_key(pin.name) {
                    self.error(
                        "BROKEN_WIRE",
                        format!("required input `{}` is not wired", pin.name),
                        node,
                        Some(pin.name),
                    )
                    .fix("connect_source");
                }
            }
            for port in node.inputs.keys() {
                if !def.inputs.iter().any(|p| p.name == port) {
                    self.error(
                        "UNKNOWN_PORT",
                        format!("node `{}` has no input port `{port}`", def.id),
                        node,
                        Some(port),
                    )
                    .fix("remove_input");
                }
            }
        } else if node.inputs.is_empty() {
            self.error(
                "BROKEN_WIRE",
                format!("`{}` needs at least one input", def.id),
                node,
                None,
            )
            .fix("connect_source");
        }

        let resolved = match lc_nodes::resolve(def, node, self.doc, &input_types) {
            Ok(r) => r,
            Err(msg) => {
                self.error("TYPE_RESOLUTION", msg, node, None);
                self.failed.insert(node.id.clone());
                return;
            }
        };

        for (port, target) in &resolved.inputs {
            if let Some(src) = input_types.get(port) {
                if !target.accepts(src) {
                    self.error(
                        "TYPE_MISMATCH",
                        format!(
                            "input `{port}` expects {} but receives {}",
                            target.display(),
                            src.display()
                        ),
                        node,
                        Some(port),
                    )
                    .fix("insert_coercion");
                }
            }
        }

        self.compiled
            .node_types
            .insert(node.id.clone(), resolved.outputs.clone());
        self.compiled
            .input_types
            .insert(node.id.clone(), resolved.inputs);
    }

    fn check_structure(&mut self, node: &Node, scope: &Scope, ctx: &ScopeCtx) {
        if node.kind == kinds::LOOP_CONDITION {
            return;
        }
        let mut tunnel_in = BTreeMap::new();
        let mut tunnel_out = BTreeMap::new();
        for t in &node.tunnels {
            match t.direction {
                TunnelDirection::In => {
                    tunnel_in.insert(t.name.clone(), t.ty.clone());
                    if let Some(src) = node.inputs.get(&format!("t:{}", t.name)) {
                        if let Ok(src_ty) = self.source_type(src, scope, ctx) {
                            if !t.ty.accepts(&src_ty) {
                                self.error(
                                    "TUNNEL_TYPE_MISMATCH",
                                    format!(
                                        "tunnel `{}` expects {} but receives {}",
                                        t.name,
                                        t.ty.display(),
                                        src_ty.display()
                                    ),
                                    node,
                                    Some(&format!("t:{}", t.name)),
                                )
                                .fix("insert_coercion");
                            }
                        }
                    } else {
                        self.error(
                            "TUNNEL_UNWIRED",
                            format!("input tunnel `{}` is not wired", t.name),
                            node,
                            Some(&format!("t:{}", t.name)),
                        )
                        .fix("connect_tunnel");
                    }
                }
                TunnelDirection::Out => {
                    tunnel_out.insert(t.name.clone(), t.ty.clone());
                }
            }
        }

        if node.kind == kinds::FOR && !node.inputs.contains_key("n") {
            // N defaults to the shortest auto-indexed array, so this is legal.
        }

        let mut body_ctx = ScopeCtx {
            tunnels: ctx.tunnels.clone(),
            shifts: ctx.shifts.clone(),
            iteration: ctx.iteration.clone(),
            event: ctx.event.clone(),
        };
        for t in &node.tunnels {
            let inside = match (t.direction, t.mode) {
                (_, TunnelMode::AutoIndex) => t.ty.element().cloned().unwrap_or(Type::Void),
                _ => t.ty.clone(),
            };
            body_ctx
                .tunnels
                .insert((node.id.clone(), t.name.clone()), inside);
        }
        for sr in &node.shift_registers {
            body_ctx
                .shifts
                .insert((node.id.clone(), sr.name.clone()), sr.ty.clone());
        }
        if matches!(node.kind.as_str(), kinds::WHILE | kinds::FOR) {
            body_ctx.iteration = Some(node.id.clone());
        }
        if node.kind == kinds::EVENT {
            body_ctx.event = Some(node.id.clone());
        }

        let frames: Vec<&lc_model::Frame> = match node.kind.as_str() {
            kinds::WHILE | kinds::FOR => node.frames.iter().filter(|f| f.label == "body").collect(),
            _ => node.frames.iter().collect(),
        };

        if frames.is_empty() && node.kind != kinds::SEQUENCE {
            return;
        }

        for frame in frames {
            let before = self.diagnostics.len();
            self.check_scope(&frame.scope, &body_ctx);
            // Surface frame-scope diagnostics with frame context.
            for d in &mut self.diagnostics[before..] {
                if d.node.is_none() {
                    d.node = Some(node.id.clone());
                }
            }

            for t in &node.tunnels {
                if t.direction != TunnelDirection::Out {
                    continue;
                }
                let key = format!("t:{}", t.name);
                match frame.scope.outputs.get(&key) {
                    Some(src) => {
                        let expected = match t.mode {
                            TunnelMode::AutoIndex => t.ty.element().cloned().unwrap_or(Type::Void),
                            _ => t.ty.clone(),
                        };
                        match self.source_type(src, &frame.scope, &body_ctx) {
                            Ok(actual) => {
                                if !expected.accepts(&actual) {
                                    self.error(
                                        "TUNNEL_OUTPUT_MISMATCH",
                                        format!(
                                            "tunnel `{}` produces {} but needs {}",
                                            t.name,
                                            actual.display(),
                                            expected.display()
                                        ),
                                        node,
                                        Some(&key),
                                    )
                                    .fix("insert_coercion");
                                }
                            }
                            Err(msg) => {
                                self.error("BROKEN_TUNNEL_OUTPUT", msg, node, Some(&key));
                            }
                        }
                    }
                    None => {
                        let allowed =
                            t.mode == TunnelMode::UseDefaultIfUnwired || node.kind == kinds::CASE;
                        if !allowed {
                            self.error(
                                "TUNNEL_OUTPUT_UNWIRED",
                                format!(
                                    "frame `{}` does not provide tunnel `{}`",
                                    frame.label, t.name
                                ),
                                node,
                                Some(&key),
                            )
                            .fix("connect_tunnel_output");
                        }
                    }
                }
            }

            for sr in &node.shift_registers {
                let key = format!("sr:{}", sr.name);
                if !frame.scope.outputs.contains_key(&key) && node.kind != kinds::CASE {
                    self.error(
                        "SHIFT_REGISTER_UNWIRED",
                        format!(
                            "shift register `{}` is not driven in frame `{}`",
                            sr.name, frame.label
                        ),
                        node,
                        Some(&key),
                    )
                    .fix("connect_shift_register");
                }
            }
        }

        if node.kind == kinds::WHILE {
            let has_condition = node.frames.iter().filter(|f| f.label == "body").any(|f| {
                f.scope
                    .nodes
                    .iter()
                    .any(|n| n.kind == kinds::LOOP_CONDITION)
            });
            if !has_condition {
                self.error(
                    "WHILE_MISSING_CONDITION",
                    "While loop body has no loop condition terminal",
                    node,
                    None,
                )
                .fix("add_loop_condition");
            }
        }

        let outputs: BTreeMap<String, Type> = node
            .tunnels
            .iter()
            .filter(|t| t.direction == TunnelDirection::Out)
            .map(|t| (format!("t:{}", t.name), t.ty.clone()))
            .collect();
        self.compiled.node_types.insert(node.id.clone(), outputs);
        self.compiled.input_types.insert(node.id.clone(), tunnel_in);
    }

    fn has_new_error(&self, from: usize) -> bool {
        self.diagnostics[from..]
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    fn source_failed(&self, src: &SourceRef) -> bool {
        matches!(src, SourceRef::Node { node, .. } if self.failed.contains(node))
    }

    fn source_type(&self, src: &SourceRef, scope: &Scope, ctx: &ScopeCtx) -> Result<Type, String> {
        match src {
            SourceRef::Node { node, port } => {
                if scope.node(node).is_none() {
                    return Err(format!("source node `{node}` is not in this scope"));
                }
                self.compiled
                    .node_types
                    .get(node)
                    .and_then(|m| m.get(port))
                    .cloned()
                    .ok_or_else(|| format!("node `{node}` has no output port `{port}`"))
            }
            SourceRef::Tunnel { structure, name } => ctx
                .tunnels
                .get(&(structure.clone(), name.clone()))
                .cloned()
                .ok_or_else(|| format!("tunnel `{name}` is not available in this scope")),
            SourceRef::ShiftRegister { structure, name } => ctx
                .shifts
                .get(&(structure.clone(), name.clone()))
                .cloned()
                .ok_or_else(|| format!("shift register `{name}` is not available in this scope")),
            SourceRef::Iteration { structure } => {
                if ctx.iteration.as_deref() == Some(structure.as_str()) {
                    Ok(Type::i32())
                } else {
                    Err("iteration terminal used outside its loop".into())
                }
            }
            SourceRef::EventData { structure, .. } => {
                if ctx.event.as_deref() == Some(structure.as_str()) {
                    Ok(Type::Variant)
                } else {
                    Err("event data used outside its event structure".into())
                }
            }
            SourceRef::Default { ty } => Ok(ty.clone()),
        }
    }

    fn error(
        &mut self,
        code: &str,
        message: impl Into<String>,
        node: &Node,
        port: Option<&str>,
    ) -> Diagnostic {
        let mut d = Diagnostic::error(code, message).at(&node.id);
        if let Some(p) = port {
            d = d.port(p);
        }
        self.diagnostics.push(d);
        self.diagnostics.last().expect("just pushed").clone()
    }
}

/// Deterministic dependency order: Kahn's algorithm with a stable queue.
pub(crate) fn topo_order(scope: &Scope) -> Vec<String> {
    let ids: BTreeSet<&str> = scope.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut indegree: BTreeMap<String, usize> =
        scope.nodes.iter().map(|n| (n.id.clone(), 0usize)).collect();
    for node in &scope.nodes {
        for src in node.inputs.values() {
            if let SourceRef::Node { node: from, .. } = src {
                if ids.contains(from.as_str()) {
                    *indegree.entry(node.id.clone()).or_default() += 1;
                }
            }
        }
    }
    let mut ready: Vec<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(k, _)| k.clone())
        .collect();
    ready.sort();
    let mut out = Vec::new();
    while let Some(id) = ready.pop() {
        out.push(id.clone());
        for node in &scope.nodes {
            let mut dependent = false;
            for src in node.inputs.values() {
                if let SourceRef::Node { node: from, .. } = src {
                    if from == &id {
                        dependent = true;
                    }
                }
            }
            if dependent {
                if let Some(d) = indegree.get_mut(&node.id) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        ready.push(node.id.clone());
                    }
                }
            }
        }
        ready.sort();
    }
    // Cycles are reported structurally; still return the remaining nodes so
    // diagnostics cover them.
    for n in &scope.nodes {
        if !out.contains(&n.id) {
            out.push(n.id.clone());
        }
    }
    out
}

/// Convenience for tools: compile and execute with default options.
pub fn compile_and_run(
    doc: &ViDocument,
    inputs: BTreeMap<String, Value>,
) -> Result<RunOutput, RunError> {
    let report = compile(doc);
    if report.compiled.is_none() {
        return Err(RunError::Compile(report.diagnostics));
    }
    let opts = RunOptions {
        inputs,
        ..Default::default()
    };
    run(doc, &opts)
}

/// All default panel values, handy for tests and MCP runs.
pub fn default_inputs(doc: &ViDocument) -> BTreeMap<String, Value> {
    doc.panel
        .controls
        .iter()
        .filter(|c| !c.is_indicator())
        .map(|c| (c.key.clone(), c.default.clone()))
        .collect()
}
