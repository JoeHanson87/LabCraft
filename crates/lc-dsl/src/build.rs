//! Turn the DSL syntax tree into a typed VI document.
//!
//! The builder is deliberately forgiving about types: it creates explicit
//! constant nodes, wires pipelines, adds structure tunnels on demand, then runs
//! the type checker a few times to refine tunnel types. The final diagnostics
//! are returned to the caller so agents see real problems instead of a silent
//! half-built VI.

use std::collections::{BTreeMap, BTreeSet};

use lc_model::document::{
    kinds, Control, Node, Point, Scope, SourceRef, Tunnel, TunnelDirection, TunnelMode, ViDocument,
};
use lc_model::{ClusterField, Diagnostic, NumericType, Type, Value};
use lc_model::{ConnectorPin, Direction};
use lc_nodes::{NodeDef, PinType, Registry};
use serde_json::json;

use crate::ast::*;
use crate::parser::parse;
use crate::DslError;

pub struct BuildOutcome {
    pub doc: ViDocument,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn build(src: &str) -> Result<BuildOutcome, DslError> {
    let program = parse(src)?;
    let mut builder = Builder::new();
    builder.build_program(program)?;
    let mut doc = builder.finish();
    lc_layout::layout_vi(&mut doc);
    refine_types(&mut doc);
    lc_layout::layout_vi(&mut doc);
    let diagnostics = lc_runtime::compile(&doc).diagnostics;
    Ok(BuildOutcome { doc, diagnostics })
}

#[derive(Default)]
struct ScopeDraft {
    nodes: Vec<Node>,
    outputs: BTreeMap<String, SourceRef>,
}

#[derive(Clone)]
struct Binding {
    source: SourceRef,
    ty: Option<Type>,
    scope_uid: u64,
    name: String,
    indicator: bool,
}

#[derive(Clone)]
struct RefVal {
    source: SourceRef,
    ty: Option<Type>,
    node_id: Option<String>,
    indicator: bool,
    created: bool,
}

#[derive(Clone)]
struct StructFrame {
    node_id: String,
    parent_scope_uid: u64,
}

struct Builder {
    scopes: BTreeMap<u64, ScopeDraft>,
    next_scope: u64,
    symbols: Vec<BTreeMap<String, Binding>>,
    children: BTreeMap<String, Vec<(String, u64)>>,
    used_keys: BTreeSet<String>,
    panel: Vec<Control>,
    panel_by_key: BTreeMap<String, Control>,
    name: String,
    description: String,
}

impl Builder {
    fn new() -> Self {
        let mut scopes = BTreeMap::new();
        scopes.insert(
            0,
            ScopeDraft {
                nodes: Vec::new(),
                outputs: BTreeMap::new(),
            },
        );
        Self {
            scopes,
            next_scope: 1,
            symbols: vec![BTreeMap::new()],
            children: BTreeMap::new(),
            used_keys: BTreeSet::new(),
            panel: Vec::new(),
            panel_by_key: BTreeMap::new(),
            name: String::new(),
            description: String::new(),
        }
    }

    fn build_program(&mut self, program: Program) -> Result<(), DslError> {
        self.name = program.name;
        self.description = program.description;
        self.build_panel(&program.panel)?;
        self.build_stmts(&program.body, 0, &[])?;
        Ok(())
    }

    // ---- Panel ---------------------------------------------------------

    fn build_panel(&mut self, decls: &[PanelDecl]) -> Result<(), DslError> {
        for d in decls {
            if self.panel_by_key.contains_key(&d.key) {
                return Err(DslError::new(
                    format!("duplicate panel control key `{}`", d.key),
                    d.line,
                    d.col,
                ));
            }
            let widget = d.props.get("widget").and_then(LitValue::as_str);
            let ty = match d.props.get("type").and_then(LitValue::as_str) {
                Some(name) => parse_type_name(name).map_err(|m| {
                    DslError::new(m, d.line, d.col)
                        .hint("supported: dbl, i32, bool, string, path, dbl[]")
                })?,
                None => kind_default_type(d.indicator, widget),
            };
            let mut ty = ty;
            if let Some(options) = d.props.get("options").and_then(LitValue::as_list) {
                ty = Type::Enum {
                    values: options
                        .iter()
                        .map(|o| o.as_str().unwrap_or("").to_string())
                        .collect(),
                };
            }
            let kind = choose_widget(d.indicator, widget, &ty);
            let mut control = Control::new(d.key.clone(), kind, d.label.clone());
            control.indicator = d.indicator;
            control.ty = ty.clone();
            control.default = Value::default_for(&ty);
            if let Some((x, y)) = d.pos {
                control.bounds.x = x;
                control.bounds.y = y;
            }
            control.bounds.w = control.bounds.w.max(160.0);
            control.bounds.h = control.bounds.h.max(32.0);
            if let Some(default) = d.props.get("default") {
                let value = lit_to_value(default)
                    .map_err(|m| DslError::new(m, d.line, d.col))?
                    .0;
                control.default = value.coerce_to(&ty).unwrap_or(value);
            }
            if let Some(unit) = d.props.get("unit").and_then(LitValue::as_str) {
                control.style.unit = Some(unit.to_string());
            }
            if let Some(v) = d.props.get("min").and_then(LitValue::as_f64) {
                control.style.min = Some(v);
            }
            if let Some(v) = d.props.get("max").and_then(LitValue::as_f64) {
                control.style.max = Some(v);
            }
            if let Some(v) = d.props.get("step").and_then(LitValue::as_f64) {
                control.style.step = Some(v);
            }
            if let Some(v) = d.props.get("decimals").and_then(LitValue::as_f64) {
                control.style.decimals = Some(v.max(0.0) as u8);
            }
            if let Some(v) = d.props.get("color").and_then(LitValue::as_str) {
                control.style.color = Some(v.to_string());
            }
            if let Some(v) = d.props.get("showLabel").and_then(LitValue::as_bool) {
                control.style.show_label = v;
            }
            if let Some(v) = d.props.get("width").and_then(LitValue::as_f64) {
                control.bounds.w = v as f32;
            }
            if let Some(v) = d.props.get("height").and_then(LitValue::as_f64) {
                control.bounds.h = v as f32;
            }
            if let Some(options) = d.props.get("options").and_then(LitValue::as_list) {
                control.style.options = options
                    .iter()
                    .map(|o| o.as_str().unwrap_or("").to_string())
                    .collect();
            }
            for (k, v) in &d.props {
                if matches!(
                    k.as_str(),
                    "type"
                        | "default"
                        | "min"
                        | "max"
                        | "step"
                        | "decimals"
                        | "unit"
                        | "color"
                        | "showLabel"
                        | "options"
                        | "widget"
                        | "width"
                        | "height"
                ) {
                    continue;
                }
                if let Ok(value) = lit_to_json(v) {
                    control.props.insert(k.clone(), value);
                }
            }
            self.panel_by_key.insert(d.key.clone(), control.clone());
            self.panel.push(control);
        }
        Ok(())
    }

    // ---- Scopes & nodes ------------------------------------------------

    fn new_scope(&mut self) -> u64 {
        let uid = self.next_scope;
        self.next_scope += 1;
        self.scopes.insert(
            uid,
            ScopeDraft {
                nodes: Vec::new(),
                outputs: BTreeMap::new(),
            },
        );
        uid
    }

    fn push_node(&mut self, scope_uid: u64, node: Node) {
        self.scopes
            .entry(scope_uid)
            .or_insert_with(|| ScopeDraft {
                nodes: Vec::new(),
                outputs: BTreeMap::new(),
            })
            .nodes
            .push(node);
    }

    fn find_node(&self, id: &str) -> Option<&Node> {
        for scope in self.scopes.values() {
            if let Some(n) = scope.nodes.iter().find(|n| n.id == id) {
                return Some(n);
            }
        }
        None
    }

    fn find_node_mut(&mut self, id: &str) -> Option<&mut Node> {
        for scope in self.scopes.values_mut() {
            if let Some(i) = scope.nodes.iter().position(|n| n.id == id) {
                return scope.nodes.get_mut(i);
            }
        }
        None
    }

    fn unique_key(&mut self, base: &str) -> String {
        let base = sanitize(base);
        if self.used_keys.insert(base.clone()) {
            return base;
        }
        let mut i = 2;
        loop {
            let key = format!("{base}_{i}");
            if self.used_keys.insert(key.clone()) {
                return key;
            }
            i += 1;
        }
    }

    fn rename_key(&mut self, id: &str, name: &str) {
        let key = self.unique_key(name);
        if let Some(node) = self.find_node_mut(id) {
            node.key = key;
        }
    }

    fn build_stmts(
        &mut self,
        stmts: &[Stmt],
        scope_uid: u64,
        chain: &[StructFrame],
    ) -> Result<(), DslError> {
        for stmt in stmts {
            self.build_stmt(stmt, scope_uid, chain)?;
        }
        Ok(())
    }

    fn build_stmt(
        &mut self,
        stmt: &Stmt,
        scope_uid: u64,
        chain: &[StructFrame],
    ) -> Result<(), DslError> {
        match &stmt.kind {
            StmtKind::Let { name, value } => {
                let rv = self.build_expr(value, scope_uid, chain, stmt.line, stmt.col)?;
                if rv.indicator {
                    return Err(DslError::new(
                        format!("`{name}` is an indicator and cannot be used as a value"),
                        stmt.line,
                        stmt.col,
                    ));
                }
                if rv.created {
                    if let Some(id) = &rv.node_id {
                        self.rename_key(id, name);
                    }
                }
                let binding = Binding {
                    source: rv.source,
                    ty: rv.ty,
                    scope_uid,
                    name: name.clone(),
                    indicator: false,
                };
                self.symbols
                    .last_mut()
                    .expect("symbol scope")
                    .insert(name.clone(), binding);
            }
            StmtKind::Tunnel { name, value } => {
                self.add_tunnel_output(name, value, scope_uid, chain, stmt.line, stmt.col)?;
            }
            StmtKind::While { .. }
            | StmtKind::For { .. }
            | StmtKind::Case { .. }
            | StmtKind::Event { .. }
            | StmtKind::Sequence { .. } => {
                let _ = self.build_structure(stmt, scope_uid, chain)?;
            }
            StmtKind::Expr(expr) => {
                let _ = self.build_expr(expr, scope_uid, chain, stmt.line, stmt.col)?;
            }
        }
        Ok(())
    }

    fn build_expr(
        &mut self,
        expr: &Expr,
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        match expr {
            Expr::Lit(lit) => self.constant(lit, scope_uid, line, col),
            Expr::Ident(name) => self.resolve_ident(name, scope_uid, chain, line, col),
            Expr::Call { name, args } => {
                self.build_call(name, args, None, scope_uid, chain, line, col)
            }
            Expr::Array(items) => {
                let mut inputs = BTreeMap::new();
                let mut tys = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let rv = self.build_expr(item, scope_uid, chain, line, col)?;
                    inputs.insert(format!("in{i}"), rv.source);
                    tys.push(rv.ty);
                }
                if inputs.is_empty() {
                    return Err(DslError::new(
                        "an array literal needs at least one element",
                        line,
                        col,
                    ));
                }
                let key = self.unique_key("array");
                let mut node = Node::new("build_array", key);
                node.inputs = inputs;
                let ty = ty_of_first(&tys)
                    .map(Type::array_of)
                    .or(Some(Type::array_of(Type::dbl())));
                let id = node.id.clone();
                self.push_node(scope_uid, node);
                Ok(RefVal {
                    source: SourceRef::Node {
                        node: id.clone(),
                        port: "array".into(),
                    },
                    ty,
                    node_id: Some(id),
                    indicator: false,
                    created: true,
                })
            }
            Expr::Cluster(fields) => {
                if fields.len() > 4 {
                    return Err(DslError::new(
                        "cluster literals support at most 4 fields in v1; use a bundle node for larger clusters",
                        line,
                        col,
                    ));
                }
                let ports = ["a", "b", "c", "d"];
                let mut node = Node::new("bundle", self.unique_key("cluster"));
                let mut names = Vec::new();
                let mut cluster_fields = Vec::new();
                for (i, (name, value)) in fields.iter().enumerate() {
                    let rv = self.build_expr(value, scope_uid, chain, line, col)?;
                    node.inputs.insert(ports[i].to_string(), rv.source);
                    names.push(name.clone());
                    cluster_fields.push(ClusterField {
                        name: name.clone(),
                        ty: rv.ty.unwrap_or(Type::Any),
                    });
                }
                node.props.insert("fields".into(), json!(names));
                let id = node.id.clone();
                self.push_node(scope_uid, node);
                Ok(RefVal {
                    source: SourceRef::Node {
                        node: id.clone(),
                        port: "cluster".into(),
                    },
                    ty: Some(Type::Cluster {
                        fields: cluster_fields,
                    }),
                    node_id: Some(id),
                    indicator: false,
                    created: true,
                })
            }
            Expr::Pipe { start, steps } => {
                let mut current = self.build_expr(start, scope_uid, chain, line, col)?;
                for (i, step) in steps.iter().enumerate() {
                    if current.indicator {
                        return Err(DslError::new(
                            "an indicator must be the last step of a pipeline",
                            line,
                            col,
                        ));
                    }
                    let is_last = i + 1 == steps.len();
                    if step.args.is_empty()
                        && self
                            .panel_by_key
                            .get(&step.name)
                            .map(|c| c.is_indicator())
                            .unwrap_or(false)
                    {
                        current =
                            self.create_indicator(&step.name, current, scope_uid, line, col)?;
                        continue;
                    }
                    if Registry::global().get(&step.name).is_some() {
                        current = self.build_call(
                            &step.name,
                            &step.args,
                            Some(current),
                            scope_uid,
                            chain,
                            line,
                            col,
                        )?;
                    } else if self.panel_by_key.contains_key(&step.name) {
                        return Err(DslError::new(
                            format!(
                                "`{}` is a control, so it cannot receive a wire; use `-> {}` on an indicator",
                                step.name, step.name
                            ),
                            line,
                            col,
                        ));
                    } else if self.lookup_binding(&step.name).is_some() {
                        return Err(DslError::new(
                            format!(
                                "`{}` is a variable and cannot be a pipeline destination",
                                step.name
                            ),
                            line,
                            col,
                        ));
                    } else {
                        let hint = nearest_name(&step.name, Registry::global());
                        let mut err = DslError::new(
                            format!("unknown node or indicator `{}` after `->`", step.name),
                            line,
                            col,
                        );
                        if let Some(h) = hint {
                            err = err.hint(h);
                        }
                        return Err(err);
                    }
                    if !is_last {
                        let _ = &current;
                    }
                }
                Ok(current)
            }
            Expr::Field { base, name } => {
                let base_rv = self.build_expr(base, scope_uid, chain, line, col)?;
                if let Some(id) = &base_rv.node_id {
                    if let Some(node) = self.find_node(id) {
                        if let Some(def) = Registry::global().get(&node.kind) {
                            if let Some(pin) = def.outputs.iter().find(|p| p.name == name) {
                                return Ok(RefVal {
                                    source: SourceRef::Node {
                                        node: id.clone(),
                                        port: pin.name.to_string(),
                                    },
                                    ty: Some(pin_type_to_type(&pin.ty)),
                                    node_id: Some(id.clone()),
                                    indicator: false,
                                    created: false,
                                });
                            }
                        }
                        if lc_model::document::is_structure(&node.kind) {
                            let port = format!("t:{name}");
                            if let Some(t) = node.tunnel(name) {
                                return Ok(RefVal {
                                    source: SourceRef::Node {
                                        node: id.clone(),
                                        port,
                                    },
                                    ty: Some(t.ty.clone()),
                                    node_id: Some(id.clone()),
                                    indicator: false,
                                    created: false,
                                });
                            }
                        }
                    }
                }
                // Cluster field access.
                let mut node = Node::new("unbundle_by_name", self.unique_key("field"));
                node.inputs.insert("cluster".into(), base_rv.source.clone());
                node.props.insert("fields".into(), json!([name]));
                let ty = match &base_rv.ty {
                    Some(Type::Cluster { fields }) => fields
                        .iter()
                        .find(|f| &f.name == name)
                        .map(|f| f.ty.clone())
                        .or(Some(Type::Any)),
                    _ => Some(Type::Any),
                };
                let id = node.id.clone();
                self.push_node(scope_uid, node);
                Ok(RefVal {
                    source: SourceRef::Node {
                        node: id.clone(),
                        port: name.clone(),
                    },
                    ty,
                    node_id: Some(id),
                    indicator: false,
                    created: true,
                })
            }
            Expr::Structure(stmt) => self.build_structure(stmt, scope_uid, chain),
        }
    }

    fn constant(
        &mut self,
        lit: &LitValue,
        scope_uid: u64,
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let (value, ty) = lit_to_value(lit).map_err(|m| DslError::new(m, line, col))?;
        let key = self.unique_key("const");
        let mut node = Node::new(kinds::CONSTANT, key);
        node.props.insert(
            "type".into(),
            serde_json::to_value(&ty).unwrap_or(json!("void")),
        );
        node.props.insert(
            "value".into(),
            serde_json::to_value(&value).unwrap_or(json!(null)),
        );
        node.size = Some(Point::new(130.0, 34.0));
        let id = node.id.clone();
        self.push_node(scope_uid, node);
        Ok(RefVal {
            source: SourceRef::Node {
                node: id.clone(),
                port: "value".into(),
            },
            ty: Some(ty),
            node_id: Some(id),
            indicator: false,
            created: true,
        })
    }

    fn lookup_binding(&self, name: &str) -> Option<&Binding> {
        self.symbols.iter().rev().find_map(|m| m.get(name))
    }

    fn resolve_ident(
        &mut self,
        name: &str,
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        if let Some(binding) = self.lookup_binding(name) {
            if binding.indicator {
                return Err(DslError::new(
                    format!("indicator `{name}` cannot be read as a value"),
                    line,
                    col,
                ));
            }
            let binding = binding.clone();
            let source = self.source_in_scope(&binding, scope_uid, chain, line, col)?;
            let node_id = match &source {
                SourceRef::Node { node, .. } => Some(node.clone()),
                _ => None,
            };
            return Ok(RefVal {
                source,
                ty: binding.ty,
                node_id,
                indicator: false,
                created: false,
            });
        }
        if let Some(control) = self.panel_by_key.get(name) {
            if control.is_indicator() {
                return Err(DslError::new(
                    format!("indicator `{name}` has no value until it is wired"),
                    line,
                    col,
                ));
            }
            let control = control.clone();
            let key = self.unique_key(name);
            let mut node = Node::new(kinds::CONTROL, key);
            node.props.insert("control".into(), json!(name));
            node.size = Some(Point::new(150.0, 48.0));
            let id = node.id.clone();
            self.push_node(scope_uid, node);
            let binding = Binding {
                source: SourceRef::Node {
                    node: id.clone(),
                    port: "value".into(),
                },
                ty: Some(control.ty.clone()),
                scope_uid,
                name: name.to_string(),
                indicator: false,
            };
            self.symbols
                .last_mut()
                .expect("symbol scope")
                .insert(name.to_string(), binding);
            return Ok(RefVal {
                source: SourceRef::Node {
                    node: id.clone(),
                    port: "value".into(),
                },
                ty: Some(control.ty),
                node_id: Some(id),
                indicator: false,
                created: false,
            });
        }
        let mut err = DslError::new(format!("unknown identifier `{name}`"), line, col);
        if let Some(h) = nearest_name(name, Registry::global()) {
            err = err.hint(h);
        } else {
            err = err.hint(format!(
                "declare `{name}` in the panel block, bind it with `let {name} = ...`, or use a node name"
            ));
        }
        Err(err)
    }

    fn source_in_scope(
        &mut self,
        binding: &Binding,
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<SourceRef, DslError> {
        if binding.scope_uid == scope_uid {
            return Ok(binding.source.clone());
        }
        let Some(frame) = chain.last() else {
            return Err(DslError::new(
                format!(
                    "`{}` is defined in another scope and cannot be reached from here",
                    binding.name
                ),
                line,
                col,
            ));
        };
        let parent = &chain[..chain.len() - 1];
        let parent_src =
            self.source_in_scope(binding, frame.parent_scope_uid, parent, line, col)?;
        let ty = binding.ty.clone().unwrap_or(Type::Any);
        let tunnel = self.ensure_input_tunnel(frame, &binding.name, ty, parent_src, line, col)?;
        Ok(SourceRef::Tunnel {
            structure: frame.node_id.clone(),
            name: tunnel,
        })
    }

    fn ensure_input_tunnel(
        &mut self,
        frame: &StructFrame,
        base_name: &str,
        ty: Type,
        source: SourceRef,
        line: usize,
        col: usize,
    ) -> Result<String, DslError> {
        let node = self.find_node_mut(&frame.node_id).ok_or_else(|| {
            DslError::new("internal error: structure node disappeared", line, col)
        })?;
        if let Some(t) = node.tunnels.iter().find(|t| {
            t.direction == TunnelDirection::In
                && node.inputs.get(&format!("t:{}", t.name)) == Some(&source)
        }) {
            return Ok(t.name.clone());
        }
        let name = unique_tunnel_name(&node.tunnels, base_name);
        node.tunnels.push(Tunnel {
            name: name.clone(),
            ty,
            mode: TunnelMode::LastValue,
            direction: TunnelDirection::In,
        });
        node.inputs.insert(format!("t:{name}"), source);
        Ok(name)
    }

    fn add_tunnel_output(
        &mut self,
        name: &str,
        value: &Expr,
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<(), DslError> {
        let Some(frame) = chain.last().cloned() else {
            return Err(DslError::new(
                "`tunnel` can only be used inside a loop, case, event or sequence",
                line,
                col,
            ));
        };
        let rv = self.build_expr(value, scope_uid, chain, line, col)?;
        self.scopes
            .entry(scope_uid)
            .or_default()
            .outputs
            .insert(format!("t:{name}"), rv.source);
        let ty = rv.ty.clone().unwrap_or(Type::Any);
        let node = self.find_node_mut(&frame.node_id).ok_or_else(|| {
            DslError::new("internal error: structure node disappeared", line, col)
        })?;
        if let Some(t) = node.tunnels.iter_mut().find(|t| t.name == name) {
            if t.direction != TunnelDirection::Out {
                return Err(DslError::new(
                    format!("tunnel `{name}` already exists as an input tunnel"),
                    line,
                    col,
                ));
            }
            if matches!(t.ty, Type::Any | Type::Void) {
                t.ty = ty;
            }
        } else {
            node.tunnels.push(Tunnel {
                name: name.to_string(),
                ty,
                mode: TunnelMode::LastValue,
                direction: TunnelDirection::Out,
            });
        }
        Ok(())
    }

    fn create_indicator(
        &mut self,
        key: &str,
        piped: RefVal,
        scope_uid: u64,
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let Some(control) = self.panel_by_key.get(key).cloned() else {
            return Err(DslError::new(
                format!("unknown indicator `{key}`; declare it in the panel block"),
                line,
                col,
            ));
        };
        if !control.is_indicator() {
            return Err(DslError::new(
                format!("`{key}` is a control, not an indicator"),
                line,
                col,
            ));
        }
        let node_key = self.unique_key(key);
        let mut node = Node::new(kinds::INDICATOR, node_key);
        node.props.insert("control".into(), json!(key));
        node.inputs.insert("value".into(), piped.source);
        let id = node.id.clone();
        self.push_node(scope_uid, node);
        Ok(RefVal {
            source: SourceRef::Node {
                node: id.clone(),
                port: String::new(),
            },
            ty: Some(control.ty.clone()),
            node_id: Some(id),
            indicator: true,
            created: true,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn build_call(
        &mut self,
        name: &str,
        args: &[Arg],
        piped: Option<RefVal>,
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let Some(def) = Registry::global().get(name) else {
            let mut err = DslError::new(format!("unknown node `{name}`"), line, col);
            if let Some(h) = nearest_name(name, Registry::global()) {
                err = err.hint(h);
            }
            return Err(err);
        };

        // Bundle is special: named arguments are field names, not port names.
        if def.id == "bundle" {
            return self.build_bundle(args, piped, scope_uid, chain, line, col);
        }

        let ports: Vec<String> = if def.variadic {
            (0..args.len().max(1)).map(|i| format!("in{i}")).collect()
        } else {
            def.inputs.iter().map(|p| p.name.to_string()).collect()
        };

        let mut assigned: BTreeMap<String, (SourceRef, Option<Type>)> = BTreeMap::new();
        for arg in args {
            if let Some(port) = &arg.name {
                if !ports.contains(port) {
                    return Err(DslError::new(
                        format!(
                            "`{}` has no input `{port}`; inputs are: {}",
                            def.id,
                            ports.join(", ")
                        ),
                        line,
                        col,
                    ));
                }
                let rv = self.build_expr(&arg.value, scope_uid, chain, line, col)?;
                assigned.insert(port.clone(), (rv.source, rv.ty));
            }
        }
        if let Some(piped) = &piped {
            let port = ports
                .iter()
                .find(|p| !assigned.contains_key(*p))
                .cloned()
                .ok_or_else(|| {
                    DslError::new(
                        format!(
                            "`{}` has no free input for the piped value; inputs are: {}",
                            def.id,
                            ports.join(", ")
                        ),
                        line,
                        col,
                    )
                })?;
            assigned.insert(port, (piped.source.clone(), piped.ty.clone()));
        }
        for arg in args {
            if arg.name.is_some() {
                continue;
            }
            let port = ports
                .iter()
                .find(|p| !assigned.contains_key(*p))
                .cloned()
                .ok_or_else(|| {
                    DslError::new(
                        format!(
                            "too many arguments for `{}`; it has {} input(s): {}",
                            def.id,
                            ports.len(),
                            ports.join(", ")
                        ),
                        line,
                        col,
                    )
                })?;
            let rv = self.build_expr(&arg.value, scope_uid, chain, line, col)?;
            assigned.insert(port, (rv.source, rv.ty));
        }

        let key = self.unique_key(&def.id.replace('.', "_"));
        let mut node = Node::new(def.id, key);
        for (port, (source, _)) in &assigned {
            node.inputs.insert(port.clone(), source.clone());
        }
        let ty = guess_output(def, &assigned);
        let port = def
            .outputs
            .first()
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "result".into());
        let id = node.id.clone();
        self.push_node(scope_uid, node);
        Ok(RefVal {
            source: SourceRef::Node {
                node: id.clone(),
                port,
            },
            ty,
            node_id: Some(id),
            indicator: false,
            created: true,
        })
    }

    fn build_bundle(
        &mut self,
        args: &[Arg],
        piped: Option<RefVal>,
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let ports = ["a", "b", "c", "d"];
        if args.len() > 4 {
            return Err(DslError::new(
                "bundle supports at most 4 fields in v1",
                line,
                col,
            ));
        }
        let mut node = Node::new("bundle", self.unique_key("bundle"));
        let mut names = Vec::new();
        let mut fields = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            let field = arg.name.clone().unwrap_or_else(|| ports[i].to_string());
            let rv = self.build_expr(&arg.value, scope_uid, chain, line, col)?;
            node.inputs.insert(ports[i].to_string(), rv.source);
            names.push(field.clone());
            fields.push(ClusterField {
                name: field,
                ty: rv.ty.unwrap_or(Type::Any),
            });
        }
        if let Some(piped) = piped {
            let i = args.len();
            if i >= 4 {
                return Err(DslError::new(
                    "bundle supports at most 4 fields in v1",
                    line,
                    col,
                ));
            }
            node.inputs.insert(ports[i].to_string(), piped.source);
            names.push(ports[i].to_string());
            fields.push(ClusterField {
                name: ports[i].to_string(),
                ty: piped.ty.unwrap_or(Type::Any),
            });
        }
        node.props.insert("fields".into(), json!(names));
        let id = node.id.clone();
        self.push_node(scope_uid, node);
        Ok(RefVal {
            source: SourceRef::Node {
                node: id.clone(),
                port: "cluster".into(),
            },
            ty: Some(Type::Cluster { fields }),
            node_id: Some(id),
            indicator: false,
            created: true,
        })
    }

    // ---- Structures ----------------------------------------------------

    fn build_structure(
        &mut self,
        stmt: &Stmt,
        scope_uid: u64,
        chain: &[StructFrame],
    ) -> Result<RefVal, DslError> {
        match &stmt.kind {
            StmtKind::While { cond, body } => {
                self.build_while(cond, body, scope_uid, chain, stmt.line, stmt.col)
            }
            StmtKind::For {
                var,
                start,
                end,
                body,
            } => self.build_for(var, start, end, body, scope_uid, chain, stmt.line, stmt.col),
            StmtKind::Case { selector, frames } => {
                self.build_case(selector, frames, scope_uid, chain, stmt.line, stmt.col)
            }
            StmtKind::Event { timeout, frames } => self.build_event(
                timeout.as_ref(),
                frames,
                scope_uid,
                chain,
                stmt.line,
                stmt.col,
            ),
            StmtKind::Sequence { frames } => {
                self.build_sequence(frames, scope_uid, chain, stmt.line, stmt.col)
            }
            _ => Err(DslError::new(
                "internal error: not a structure",
                stmt.line,
                stmt.col,
            )),
        }
    }

    fn build_while(
        &mut self,
        cond: &Expr,
        body: &[Stmt],
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let key = self.unique_key("while");
        let mut node = Node::new(kinds::WHILE, key);
        node.props.insert("stopIfTrue".into(), json!(true));
        let id = node.id.clone();
        self.push_node(scope_uid, node);

        let body_uid = self.new_scope();
        let frame = StructFrame {
            node_id: id.clone(),
            parent_scope_uid: scope_uid,
        };
        let mut chain2 = chain.to_vec();
        chain2.push(frame);

        self.symbols.push(BTreeMap::new());
        self.symbols.last_mut().expect("symbol scope").insert(
            "iteration".into(),
            Binding {
                source: SourceRef::Iteration {
                    structure: id.clone(),
                },
                ty: Some(Type::i32()),
                scope_uid: body_uid,
                name: "iteration".into(),
                indicator: false,
            },
        );
        self.build_stmts(body, body_uid, &chain2)?;

        // The stop condition lives in the body so it can see iteration and
        // body-local values, exactly like a LabVIEW loop condition terminal.
        let cond_rv = self.build_expr(cond, body_uid, &chain2, line, col)?;
        let mut cond_node = Node::new(kinds::LOOP_CONDITION, self.unique_key("condition"));
        cond_node.inputs.insert("value".into(), cond_rv.source);
        self.push_node(body_uid, cond_node);
        self.symbols.pop();

        self.children
            .entry(id.clone())
            .or_default()
            .push(("body".into(), body_uid));

        Ok(self.structure_ref(&id, line, col))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_for(
        &mut self,
        var: &str,
        start: &Expr,
        end: &Expr,
        body: &[Stmt],
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let starts_at_zero =
            matches!(start, Expr::Lit(LitValue::Num { value, .. }) if *value == 0.0);
        let n_source = if starts_at_zero {
            self.build_expr(end, scope_uid, chain, line, col)?.source
        } else {
            let end_rv = self.build_expr(end, scope_uid, chain, line, col)?;
            let start_rv = self.build_expr(start, scope_uid, chain, line, col)?;
            let key = self.unique_key("count");
            let mut count = Node::new("subtract", key);
            count.inputs.insert("x".into(), end_rv.source);
            count.inputs.insert("y".into(), start_rv.source);
            let count_id = count.id.clone();
            self.push_node(scope_uid, count);
            SourceRef::Node {
                node: count_id,
                port: "difference".into(),
            }
        };

        let key = self.unique_key("for");
        let mut node = Node::new(kinds::FOR, key);
        node.inputs.insert("n".into(), n_source);
        node.props.insert("var".into(), json!(var));
        let id = node.id.clone();
        self.push_node(scope_uid, node);

        let body_uid = self.new_scope();
        let frame = StructFrame {
            node_id: id.clone(),
            parent_scope_uid: scope_uid,
        };
        let mut chain2 = chain.to_vec();
        chain2.push(frame);

        self.symbols.push(BTreeMap::new());
        self.symbols.last_mut().expect("symbol scope").insert(
            "iteration".into(),
            Binding {
                source: SourceRef::Iteration {
                    structure: id.clone(),
                },
                ty: Some(Type::i32()),
                scope_uid: body_uid,
                name: "iteration".into(),
                indicator: false,
            },
        );
        let var_binding = if starts_at_zero {
            Binding {
                source: SourceRef::Iteration {
                    structure: id.clone(),
                },
                ty: Some(Type::i32()),
                scope_uid: body_uid,
                name: var.to_string(),
                indicator: false,
            }
        } else {
            let start_rv = self.build_expr(start, body_uid, &chain2, line, col)?;
            let key = self.unique_key("index");
            let mut add = Node::new("add", key);
            add.inputs.insert(
                "x".into(),
                SourceRef::Iteration {
                    structure: id.clone(),
                },
            );
            add.inputs.insert("y".into(), start_rv.source);
            let add_id = add.id.clone();
            self.push_node(body_uid, add);
            Binding {
                source: SourceRef::Node {
                    node: add_id,
                    port: "sum".into(),
                },
                ty: Some(Type::i32()),
                scope_uid: body_uid,
                name: var.to_string(),
                indicator: false,
            }
        };
        self.symbols
            .last_mut()
            .expect("symbol scope")
            .insert(var.to_string(), var_binding);

        self.build_stmts(body, body_uid, &chain2)?;
        self.symbols.pop();
        self.children
            .entry(id.clone())
            .or_default()
            .push(("body".into(), body_uid));
        Ok(self.structure_ref(&id, line, col))
    }

    fn build_case(
        &mut self,
        selector: &Expr,
        frames: &[Frame],
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let sel = self.build_expr(selector, scope_uid, chain, line, col)?;
        let key = self.unique_key("case");
        let mut node = Node::new(kinds::CASE, key);
        node.inputs.insert("selector".into(), sel.source);
        let id = node.id.clone();
        self.push_node(scope_uid, node);

        let mut labels = Vec::new();
        for frame in frames {
            if labels.contains(&frame.label) {
                return Err(DslError::new(
                    format!("duplicate case frame `{}`", frame.label),
                    line,
                    col,
                ));
            }
            labels.push(frame.label.clone());
            let body_uid = self.new_scope();
            let sf = StructFrame {
                node_id: id.clone(),
                parent_scope_uid: scope_uid,
            };
            let mut chain2 = chain.to_vec();
            chain2.push(sf);
            self.symbols.push(BTreeMap::new());
            self.build_stmts(&frame.body, body_uid, &chain2)?;
            self.symbols.pop();
            self.children
                .entry(id.clone())
                .or_default()
                .push((frame.label.clone(), body_uid));
        }
        Ok(self.structure_ref(&id, line, col))
    }

    fn build_event(
        &mut self,
        timeout: Option<&Expr>,
        frames: &[Frame],
        scope_uid: u64,
        chain: &[StructFrame],
        line: usize,
        col: usize,
    ) -> Result<RefVal, DslError> {
        let key = self.unique_key("event");
        let mut node = Node::new(kinds::EVENT, key);
        if let Some(t) = timeout {
            let rv = self.build_expr(t, scope_uid, chain, line, col)?;
            node.inputs.insert("timeout".into(), rv.source);
        }
        let id = node.id.clone();
        self.push_node(scope_uid, node);

        for frame in frames {
            let body_uid = self.new_scope();
            let sf = StructFrame {
                node_id: id.clone(),
                parent_scope_uid: scope_uid,
            };
            let mut chain2 = chain.to_vec();
            chain2.push(sf);
            self.symbols.push(BTreeMap::new());
            self.symbols.last_mut().expect("symbol scope").insert(
                "event_data".into(),
                Binding {
                    source: SourceRef::EventData {
                        structure: id.clone(),
                        field: None,
                    },
                    ty: Some(Type::Variant),
                    scope_uid: body_uid,
                    name: "event_data".into(),
                    indicator: false,
                },
            );
            self.build_stmts(&frame.body, body_uid, &chain2)?;
            self.symbols.pop();
            self.children
                .entry(id.clone())
                .or_default()
                .push((frame.label.clone(), body_uid));
        }
        Ok(self.structure_ref(&id, line, col))
    }

    fn build_sequence(
        &mut self,
        frames: &[Vec<Stmt>],
        scope_uid: u64,
        chain: &[StructFrame],
        _line: usize,
        _col: usize,
    ) -> Result<RefVal, DslError> {
        let key = self.unique_key("sequence");
        let node = Node::new(kinds::SEQUENCE, key);
        let id = node.id.clone();
        self.push_node(scope_uid, node);
        for (i, body) in frames.iter().enumerate() {
            let body_uid = self.new_scope();
            let sf = StructFrame {
                node_id: id.clone(),
                parent_scope_uid: scope_uid,
            };
            let mut chain2 = chain.to_vec();
            chain2.push(sf);
            self.symbols.push(BTreeMap::new());
            self.build_stmts(body, body_uid, &chain2)?;
            self.symbols.pop();
            self.children
                .entry(id.clone())
                .or_default()
                .push((i.to_string(), body_uid));
        }
        Ok(self.structure_ref(&id, _line, _col))
    }

    fn structure_ref(&mut self, id: &str, _line: usize, _col: usize) -> RefVal {
        let mut out = Vec::new();
        if let Some(node) = self.find_node(id) {
            for t in &node.tunnels {
                if t.direction == TunnelDirection::Out {
                    out.push((t.name.clone(), t.ty.clone()));
                }
            }
        }
        if out.len() == 1 {
            let (name, ty) = out.remove(0);
            RefVal {
                source: SourceRef::Node {
                    node: id.to_string(),
                    port: format!("t:{name}"),
                },
                ty: Some(ty),
                node_id: Some(id.to_string()),
                indicator: false,
                created: true,
            }
        } else {
            RefVal {
                source: SourceRef::Node {
                    node: id.to_string(),
                    port: String::new(),
                },
                ty: None,
                node_id: Some(id.to_string()),
                indicator: false,
                created: true,
            }
        }
    }

    // ---- Materialisation ----------------------------------------------

    fn build_scope(&self, uid: u64) -> Scope {
        let draft = self.scopes.get(&uid);
        let Some(draft) = draft else {
            return Scope::default();
        };
        let mut nodes = draft.nodes.clone();
        for node in &mut nodes {
            if let Some(frames) = self.children.get(&node.id) {
                node.frames = frames
                    .iter()
                    .map(|(label, child)| lc_model::Frame {
                        label: label.clone(),
                        scope: self.build_scope(*child),
                    })
                    .collect();
            }
        }
        Scope {
            nodes,
            outputs: draft.outputs.clone(),
        }
    }

    fn finish(mut self) -> ViDocument {
        let root = self.build_scope(0);
        let mut doc = ViDocument::new(self.name);
        doc.diagram.root = root;
        doc.description = self.description;
        doc.panel.controls = std::mem::take(&mut self.panel);
        for c in &doc.panel.controls {
            let pin = ConnectorPin {
                name: c.label.clone(),
                ty: c.ty.clone(),
                direction: if c.is_indicator() {
                    Direction::Output
                } else {
                    Direction::Input
                },
            };
            if c.kind.is_indicator() {
                doc.connector.outputs.push(pin);
            } else {
                doc.connector.inputs.push(pin);
            }
        }
        doc
    }
}

// ---- Helpers -----------------------------------------------------------

fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('n');
    }
    if s.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        s.insert(0, 'n');
    }
    s
}

fn unique_tunnel_name(tunnels: &[Tunnel], base: &str) -> String {
    let base = sanitize(base);
    if !tunnels.iter().any(|t| t.name == base) {
        return base;
    }
    let mut i = 2;
    loop {
        let name = format!("{base}_{i}");
        if !tunnels.iter().any(|t| t.name == name) {
            return name;
        }
        i += 1;
    }
}

fn nearest_name(query: &str, reg: &Registry) -> Option<String> {
    let q = query.to_lowercase();
    let mut best: Option<(usize, String)> = None;
    for info in reg.infos() {
        let d = edit_distance(&q, &info.id.to_lowercase());
        if d <= 3 && best.as_ref().map(|(bd, _)| d < *bd).unwrap_or(true) {
            best = Some((d, info.id));
        }
    }
    best.map(|(_, name)| format!("did you mean `{name}`?"))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn ty_of_first(tys: &[Option<Type>]) -> Option<Type> {
    tys.iter()
        .flatten()
        .find(|t| !matches!(t, Type::Any | Type::Void))
        .cloned()
}

fn lit_to_value(lit: &LitValue) -> Result<(Value, Type), String> {
    match lit {
        LitValue::Num { value, is_int } => {
            if *is_int {
                Ok((Value::int(*value as i64), Type::numeric(NumericType::I64)))
            } else {
                Ok((Value::num(*value), Type::dbl()))
            }
        }
        LitValue::Str(s) => Ok((Value::string(s.clone()), Type::String)),
        LitValue::Bool(b) => Ok((Value::Bool(*b), Type::Bool)),
        LitValue::Ident(s) => Err(format!("`{s}` is not a literal value")),
        LitValue::List(items) => {
            let mut data = Vec::new();
            let mut elem_ty = None;
            for item in items {
                let (value, ty) = lit_to_value(item)?;
                if elem_ty.is_none() {
                    elem_ty = Some(ty);
                }
                data.push(value);
            }
            let elem = elem_ty.unwrap_or_else(Type::dbl);
            Ok((
                Value::Array(lc_model::ArrayValue::from_vec(data)),
                Type::array_of(elem),
            ))
        }
    }
}

fn lit_to_json(lit: &LitValue) -> Result<serde_json::Value, ()> {
    Ok(match lit {
        LitValue::Num { value, is_int } => {
            if *is_int {
                json!(*value as i64)
            } else {
                json!(*value)
            }
        }
        LitValue::Str(s) | LitValue::Ident(s) => json!(s),
        LitValue::Bool(b) => json!(b),
        LitValue::List(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|i| lit_to_json(i).unwrap_or(serde_json::Value::Null))
                .collect(),
        ),
    })
}

fn parse_type_name(name: &str) -> Result<Type, String> {
    let mut base = name.trim();
    let mut dims = 0usize;
    while let Some(stripped) = base.strip_suffix("[]") {
        dims += 1;
        base = stripped.trim();
    }
    let mut ty = match base.to_ascii_lowercase().as_str() {
        "void" => Type::Void,
        "bool" | "boolean" => Type::Bool,
        "string" | "str" => Type::String,
        "path" => Type::Path,
        "timestamp" | "time" => Type::Timestamp,
        "error" | "err" => Type::Error,
        "variant" | "any" => Type::Variant,
        "waveform" => Type::Waveform,
        "i8" => Type::numeric(NumericType::I8),
        "i16" => Type::numeric(NumericType::I16),
        "i32" | "int" => Type::numeric(NumericType::I32),
        "i64" => Type::numeric(NumericType::I64),
        "u8" => Type::numeric(NumericType::U8),
        "u16" => Type::numeric(NumericType::U16),
        "u32" => Type::numeric(NumericType::U32),
        "u64" => Type::numeric(NumericType::U64),
        "sgl" | "f32" | "float" => Type::numeric(NumericType::Sgl),
        "dbl" | "f64" | "double" | "numeric" => Type::dbl(),
        "ext" => Type::numeric(NumericType::Ext),
        other => return Err(format!("unknown type `{other}`")),
    };
    if dims > 0 {
        ty = Type::Array {
            element: Box::new(ty),
            dims,
        };
    }
    Ok(ty)
}

fn kind_default_type(indicator: bool, widget: Option<&str>) -> Type {
    match widget {
        Some(w) => choose_widget(indicator, Some(w), &Type::dbl()).default_type(),
        None => Type::dbl(),
    }
}

fn choose_widget(indicator: bool, widget: Option<&str>, ty: &Type) -> lc_model::ControlKind {
    use lc_model::ControlKind::*;
    if let Some(w) = widget {
        match w.to_ascii_lowercase().as_str() {
            "led" => return Led,
            "slider" => return Slider,
            "toggle" | "switch" => return Toggle,
            "button" => return Button,
            "chart" => return Chart,
            "graph" => return Graph,
            "table" => return Table,
            "enum" | "ring" => return Enum,
            "cluster" => return Cluster,
            "array" => return Array,
            "string" | "text" => return String,
            "path" => return Path,
            "numeric" | "number" => return Numeric,
            _ => {}
        }
    }
    match ty {
        Type::Bool => {
            if indicator {
                Led
            } else {
                Toggle
            }
        }
        Type::String => String,
        Type::Path => Path,
        Type::Enum { .. } => Enum,
        Type::Cluster { .. } => Cluster,
        Type::Array { element, .. } => match element.as_ref() {
            Type::Numeric { .. } => {
                if indicator {
                    Chart
                } else {
                    Array
                }
            }
            Type::Array { .. } => Table,
            _ => Array,
        },
        _ => Numeric,
    }
}

fn pin_type_to_type(p: &PinType) -> Type {
    match p {
        PinType::Bool => Type::Bool,
        PinType::Numeric => Type::dbl(),
        PinType::Int => Type::i32(),
        PinType::String => Type::String,
        PinType::Path => Type::Path,
        PinType::Error => Type::Error,
        PinType::Variant | PinType::Any => Type::Any,
        PinType::Cluster => Type::Cluster { fields: Vec::new() },
        PinType::NumericArray => Type::array_of(Type::dbl()),
        PinType::Array(inner) => Type::array_of(pin_type_to_type(inner)),
    }
}

fn guess_output(
    def: &NodeDef,
    assigned: &BTreeMap<String, (SourceRef, Option<Type>)>,
) -> Option<Type> {
    let input = |port: &str| assigned.get(port).and_then(|(_, t)| t.clone());
    match def.id {
        "build_array" => {
            let mut ports: Vec<&String> = assigned.keys().collect();
            lc_nodes::sort_ports(&mut ports);
            let elem = ports
                .iter()
                .filter_map(|p| assigned.get(*p).and_then(|(_, t)| t.clone()))
                .find(|t| !matches!(t, Type::Any | Type::Void))
                .unwrap_or(Type::dbl());
            Some(Type::array_of(match elem {
                Type::Array { element, .. } => *element,
                other => other,
            }))
        }
        "index_array" => input("array").and_then(|t| match t {
            Type::Array { element, .. } => Some(*element),
            _ => None,
        }),
        "array_sort" | "reverse_array" | "array_subset" => input("array"),
        "array_size" | "string_length" | "array_search" => Some(Type::i32()),
        "concat_strings" | "to_string" | "format_number" | "substring" | "to_upper"
        | "to_lower" | "trim" | "replace" | "json_stringify" => Some(Type::String),
        "greater" | "less" | "greater_equal" | "less_equal" | "equal" | "not_equal" | "and"
        | "or" | "not" | "xor" | "to_bool" | "error_status" | "scan_number" => Some(Type::Bool),
        "quotient" => Some(Type::i32()),
        "min" | "add" | "subtract" | "multiply" | "divide" | "pow" | "abs" | "negate"
        | "increment" | "decrement" | "round" | "sqrt" | "array_sum" | "array_mean"
        | "to_double" | "to_int" => input("x").or_else(|| input("array")).or(Some(Type::dbl())),
        "select" => input("true").or_else(|| input("false")),
        "random" | "tick" => Some(Type::dbl()),
        "bundle" => None,
        _ => def.outputs.first().map(|p| pin_type_to_type(&p.ty)),
    }
}

// ---- Tunnel type refinement -------------------------------------------

/// Refine tunnel types by repeatedly compiling the document.
///
/// The builder starts tunnels at `Any` because a binding's type may only become
/// known after the rest of the graph is resolved. Each pass copies real types
/// from tunnel sources and frame outputs.
pub fn refine_types(doc: &mut ViDocument) {
    for _ in 0..8 {
        let compiled = lc_runtime::compile(doc).compiled;
        let Some(compiled) = compiled else { break };
        let tunnels = collect_tunnel_types(&doc.diagram.root);
        let mut updates: Vec<(String, String, Type)> = Vec::new();
        collect_updates(&doc.diagram.root, &compiled, &tunnels, &mut updates);
        if updates.is_empty() {
            break;
        }
        let mut changed = false;
        for (id, name, ty) in updates {
            if let Some(node) = find_node_mut_in(&mut doc.diagram.root, &id) {
                if let Some(t) = node.tunnels.iter_mut().find(|t| t.name == name) {
                    if t.ty != ty && !matches!(ty, Type::Any | Type::Void) {
                        t.ty = ty;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn find_node_mut_in<'a>(scope: &'a mut Scope, id: &str) -> Option<&'a mut Node> {
    if let Some(i) = scope.nodes.iter().position(|n| n.id == id) {
        return scope.nodes.get_mut(i);
    }
    for node in &mut scope.nodes {
        for frame in &mut node.frames {
            if let Some(found) = find_node_mut_in(&mut frame.scope, id) {
                return Some(found);
            }
        }
    }
    None
}

fn collect_tunnel_types(scope: &Scope) -> BTreeMap<(String, String), Type> {
    let mut out = BTreeMap::new();
    for node in &scope.nodes {
        for t in &node.tunnels {
            out.insert((node.id.clone(), t.name.clone()), t.ty.clone());
        }
        for frame in &node.frames {
            out.extend(collect_tunnel_types(&frame.scope));
        }
    }
    out
}

fn collect_updates(
    scope: &Scope,
    compiled: &lc_runtime::CompiledVi,
    tunnels: &BTreeMap<(String, String), Type>,
    out: &mut Vec<(String, String, Type)>,
) {
    for node in &scope.nodes {
        for t in &node.tunnels {
            match t.direction {
                TunnelDirection::In => {
                    let key = format!("t:{}", t.name);
                    if let Some(src) = node.inputs.get(&key) {
                        if let Some(ty) = source_type(src, compiled, tunnels) {
                            if ty != t.ty && !matches!(ty, Type::Any | Type::Void) {
                                out.push((node.id.clone(), t.name.clone(), ty));
                            }
                        }
                    }
                }
                TunnelDirection::Out => {
                    let key = format!("t:{}", t.name);
                    let mut found: Option<Type> = None;
                    let mut conflict = false;
                    for frame in &node.frames {
                        if let Some(src) = frame.scope.outputs.get(&key) {
                            if let Some(ty) = source_type(src, compiled, tunnels) {
                                let outside = if t.mode == TunnelMode::AutoIndex {
                                    match ty {
                                        Type::Array { .. } => ty,
                                        other => Type::array_of(other),
                                    }
                                } else {
                                    ty
                                };
                                match &found {
                                    None => found = Some(outside),
                                    Some(prev) if *prev != outside => conflict = true,
                                    _ => {}
                                }
                            }
                        }
                    }
                    if !conflict {
                        if let Some(ty) = found {
                            if ty != t.ty && !matches!(ty, Type::Any | Type::Void) {
                                out.push((node.id.clone(), t.name.clone(), ty));
                            }
                        }
                    }
                }
            }
        }
        for frame in &node.frames {
            collect_updates(&frame.scope, compiled, tunnels, out);
        }
    }
}

fn source_type(
    src: &SourceRef,
    compiled: &lc_runtime::CompiledVi,
    tunnels: &BTreeMap<(String, String), Type>,
) -> Option<Type> {
    match src {
        SourceRef::Node { node, port } => compiled
            .node_types
            .get(node)
            .and_then(|m| m.get(port))
            .cloned(),
        SourceRef::Tunnel { structure, name } => {
            let ty = tunnels.get(&(structure.clone(), name.clone()))?.clone();
            Some(match ty {
                Type::Array { element, .. } => *element,
                other => other,
            })
        }
        SourceRef::ShiftRegister { structure, name } => tunnels
            .get(&(structure.clone(), format!("sr:{name}")))
            .cloned(),
        SourceRef::Iteration { .. } => Some(Type::i32()),
        SourceRef::EventData { .. } => Some(Type::Variant),
        SourceRef::Default { ty } => Some(ty.clone()),
    }
}
