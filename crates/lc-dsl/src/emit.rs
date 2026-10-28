//! Emit a VI document as DSL text.
//!
//! The emitter is best-effort and optimised for round-tripping documents that
//! were built from DSL in the first place. It is also the export path agents use
//! to read a VI that was edited graphically, so it favours readable, explicit
//! statements over maximal compression: every computed node becomes a `let`,
//! indicators become sink statements, and structures become block expressions.

use std::collections::{BTreeMap, BTreeSet};

use lc_model::document::{
    is_structure, kinds, Control, ControlKind, Node, Scope, SourceRef, ViDocument,
};
use lc_model::value::NumericValue;
use lc_model::{Type, Value};
use serde_json::Value as Json;

use crate::ast::escape;

pub fn emit(doc: &ViDocument) -> String {
    let mut emitter = Emitter::new(doc);
    emitter.emit_doc()
}

struct Emitter<'a> {
    doc: &'a ViDocument,
    nodes: BTreeMap<&'a str, &'a Node>,
    names: BTreeMap<String, String>,
    primary: BTreeMap<String, String>,
    used: BTreeSet<String>,
}

impl<'a> Emitter<'a> {
    fn new(doc: &'a ViDocument) -> Self {
        let mut emitter = Self {
            doc,
            nodes: BTreeMap::new(),
            names: BTreeMap::new(),
            primary: BTreeMap::new(),
            used: BTreeSet::new(),
        };
        emitter.collect(&doc.diagram.root);
        emitter
    }

    fn collect(&mut self, scope: &'a Scope) {
        for node in &scope.nodes {
            self.nodes.insert(node.id.as_str(), node);
        }
        for node in &scope.nodes {
            let base = node
                .props
                .get("control")
                .and_then(Json::as_str)
                .unwrap_or(&node.key);
            let name = self.unique_name(base);
            self.names.insert(node.id.clone(), name);
            self.primary.insert(node.id.clone(), primary_port(node));
            for frame in &node.frames {
                self.collect(&frame.scope);
            }
        }
    }

    fn unique_name(&mut self, base: &str) -> String {
        let base = sanitize(base);
        if self.used.insert(base.clone()) {
            return base;
        }
        let mut i = 2;
        loop {
            let name = format!("{base}_{i}");
            if self.used.insert(name.clone()) {
                return name;
            }
            i += 1;
        }
    }

    fn emit_doc(&mut self) -> String {
        let mut out = String::new();
        out.push_str(&format!("vi \"{}\" {{\n", escape(&self.doc.name)));
        if !self.doc.description.is_empty() {
            out.push_str(&format!(
                "  description \"{}\"\n\n",
                escape(&self.doc.description)
            ));
        }
        out.push_str("  panel {\n");
        for control in &self.doc.panel.controls {
            out.push_str(&self.control_line(control));
        }
        out.push_str("  }\n\n");
        out.push_str("  diagram {\n");
        for line in self.scope_lines(&self.doc.diagram.root, &BTreeSet::new()) {
            out.push_str(&indent(&line, 2));
        }
        out.push_str("  }\n}\n");
        out
    }

    fn control_line(&self, control: &Control) -> String {
        let kind = if control.is_indicator() {
            "indicator"
        } else {
            "control"
        };
        let mut out = format!(
            "    {kind} {} \"{}\" @({}, {})",
            sanitize(&control.key),
            escape(&control.label),
            trim_f32(control.bounds.x),
            trim_f32(control.bounds.y)
        );
        let mut props: Vec<String> = Vec::new();
        if let Some(name) = type_name(&control.ty) {
            props.push(format!("type: {name}"));
        }
        let default_widget = default_widget(control.is_indicator(), &control.ty);
        if control.kind != default_widget {
            props.push(format!("widget: {}", widget_name(control.kind)));
        }
        if let Type::Enum { values } = &control.ty {
            props.push(format!(
                "options: [{}]",
                values
                    .iter()
                    .map(|v| format!("\"{}\"", escape(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if control.default != Value::default_for(&control.ty) {
            if let Some(lit) = value_literal(&control.default) {
                props.push(format!("default: {lit}"));
            }
        }
        if let Some(unit) = &control.style.unit {
            props.push(format!("unit: \"{}\"", escape(unit)));
        }
        if let Some(v) = control.style.min {
            props.push(format!("min: {}", trim_f64(v)));
        }
        if let Some(v) = control.style.max {
            props.push(format!("max: {}", trim_f64(v)));
        }
        if let Some(v) = control.style.step {
            props.push(format!("step: {}", trim_f64(v)));
        }
        if let Some(v) = control.style.decimals {
            props.push(format!("decimals: {v}"));
        }
        if let Some(color) = &control.style.color {
            props.push(format!("color: \"{}\"", escape(color)));
        }
        if !control.style.show_label {
            props.push("showLabel: false".into());
        }
        if control.bounds.w != 160.0 {
            props.push(format!("width: {}", trim_f32(control.bounds.w)));
        }
        if control.bounds.h != 32.0 {
            props.push(format!("height: {}", trim_f32(control.bounds.h)));
        }
        if !props.is_empty() {
            out.push_str(&format!(" {{ {} }}", props.join(", ")));
        }
        out.push('\n');
        out
    }

    fn scope_lines(&self, scope: &Scope, skip: &BTreeSet<String>) -> Vec<String> {
        let mut out = Vec::new();
        for node in ordered(scope) {
            if skip.contains(&node.id) || node.kind == kinds::LOOP_CONDITION {
                continue;
            }
            match node.kind.as_str() {
                kinds::CONTROL => {}
                kinds::INDICATOR => {
                    let Some(control) = node.props.get("control").and_then(Json::as_str) else {
                        continue;
                    };
                    if let Some(src) = node.inputs.get("value") {
                        out.push(format!(
                            "{} -> {}",
                            self.source_expr(src),
                            sanitize(control)
                        ));
                    }
                }
                k if is_structure(k) => out.extend(self.structure_lines(node)),
                _ => {
                    let Some(name) = self.names.get(&node.id) else {
                        continue;
                    };
                    out.push(format!("let {name} = {}", self.node_expr(node)));
                }
            }
        }
        for (key, src) in &scope.outputs {
            let Some(name) = key.strip_prefix("t:") else {
                continue;
            };
            out.push(format!("tunnel {name} = {}", self.source_expr(src)));
        }
        out
    }

    fn structure_lines(&self, node: &Node) -> Vec<String> {
        match node.kind.as_str() {
            kinds::WHILE => {
                let body = node.frame("body");
                let mut skip = BTreeSet::new();
                let cond = body
                    .and_then(|f| {
                        f.scope
                            .nodes
                            .iter()
                            .find(|n| n.kind == kinds::LOOP_CONDITION)
                    })
                    .and_then(|n| {
                        skip.insert(n.id.clone());
                        n.inputs.get("value").map(|s| self.source_expr(s))
                    })
                    .unwrap_or_else(|| "true".into());
                let mut inner = body
                    .map(|f| self.scope_lines(&f.scope, &skip))
                    .unwrap_or_default();
                let mut lines = vec![format!("while {cond} {{")];
                lines.append(&mut inner);
                lines.push("}".into());
                self.bind_structure(node, lines)
            }
            kinds::FOR => {
                let var = node
                    .props
                    .get("var")
                    .and_then(Json::as_str)
                    .map(sanitize)
                    .unwrap_or_else(|| "i".into());
                let n = node
                    .inputs
                    .get("n")
                    .map(|s| self.source_expr(s))
                    .unwrap_or_else(|| "0".into());
                let body = node.frame("body");
                let mut inner = body
                    .map(|f| self.scope_lines(&f.scope, &BTreeSet::new()))
                    .unwrap_or_default();
                let mut lines = vec![format!("for {var} in 0..{n} {{")];
                lines.append(&mut inner);
                lines.push("}".into());
                self.bind_structure(node, lines)
            }
            kinds::CASE => {
                let selector = node
                    .inputs
                    .get("selector")
                    .map(|s| self.source_expr(s))
                    .unwrap_or_else(|| "0".into());
                let mut lines = vec![format!("case {selector} {{")];
                for frame in &node.frames {
                    lines.push(format!("{} {{", label_token(&frame.label)));
                    let mut inner = self.scope_lines(&frame.scope, &BTreeSet::new());
                    lines.append(&mut inner);
                    lines.push("},".into());
                }
                lines.push("}".into());
                self.bind_structure(node, lines)
            }
            kinds::EVENT => {
                let timeout = node.inputs.get("timeout").map(|s| self.source_expr(s));
                let head = match timeout {
                    Some(t) => format!("event timeout {t} {{"),
                    None => "event {".into(),
                };
                let mut lines = vec![head];
                for frame in &node.frames {
                    lines.push(format!("{} {{", label_token(&frame.label)));
                    let mut inner = self.scope_lines(&frame.scope, &BTreeSet::new());
                    lines.append(&mut inner);
                    lines.push("},".into());
                }
                lines.push("}".into());
                self.bind_structure(node, lines)
            }
            kinds::SEQUENCE => {
                let mut lines = vec!["sequence {".into()];
                for frame in &node.frames {
                    lines.push("{".into());
                    let mut inner = self.scope_lines(&frame.scope, &BTreeSet::new());
                    lines.append(&mut inner);
                    lines.push("}".into());
                }
                lines.push("}".into());
                self.bind_structure(node, lines)
            }
            _ => Vec::new(),
        }
    }

    /// Prefix a block with `let name =` when the structure has an output, or
    /// leave it as a bare statement when it does not.
    fn bind_structure(&self, node: &Node, mut lines: Vec<String>) -> Vec<String> {
        let has_output = node
            .tunnels
            .iter()
            .any(|t| t.direction == lc_model::TunnelDirection::Out);
        if !has_output {
            return lines;
        }
        let Some(name) = self.names.get(&node.id) else {
            return lines;
        };
        if let Some(first) = lines.first_mut() {
            *first = format!("let {name} = {first}");
        }
        lines
    }

    fn node_expr(&self, node: &Node) -> String {
        match node.kind.as_str() {
            kinds::CONTROL => self
                .names
                .get(&node.id)
                .cloned()
                .unwrap_or_else(|| "value".into()),
            kinds::CONSTANT => node
                .props
                .get("value")
                .and_then(json_value_literal)
                .unwrap_or_else(|| "0".into()),
            "build_array" => {
                let mut ports: Vec<&String> = node.inputs.keys().collect();
                lc_nodes::sort_ports(&mut ports);
                let items = ports
                    .iter()
                    .filter_map(|p| node.inputs.get(*p))
                    .map(|s| self.source_expr(s))
                    .collect::<Vec<_>>();
                format!("[{}]", items.join(", "))
            }
            "bundle" => {
                let fields: Vec<String> = node
                    .props
                    .get("fields")
                    .and_then(Json::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Json::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut ports: Vec<&String> = node.inputs.keys().collect();
                lc_nodes::sort_ports(&mut ports);
                let args = ports
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        let src = node.inputs.get(*p)?;
                        let name = fields.get(i).cloned().unwrap_or_else(|| (*p).clone());
                        Some(format!("{name}: {}", self.source_expr(src)))
                    })
                    .collect::<Vec<_>>();
                format!("bundle({})", args.join(", "))
            }
            "unbundle_by_name" => {
                let field = node
                    .props
                    .get("fields")
                    .and_then(Json::as_array)
                    .and_then(|a| a.first())
                    .and_then(Json::as_str)
                    .unwrap_or("field");
                let base = node
                    .inputs
                    .get("cluster")
                    .map(|s| self.source_expr(s))
                    .unwrap_or_else(|| "cluster".into());
                format!("{base}.{field}")
            }
            _ => {
                let mut ports: Vec<&String> = node.inputs.keys().collect();
                lc_nodes::sort_ports(&mut ports);
                let args = ports
                    .iter()
                    .filter_map(|p| {
                        let src = node.inputs.get(*p)?;
                        Some(format!("{}: {}", p, self.source_expr(src)))
                    })
                    .collect::<Vec<_>>();
                format!("{}({})", node.kind, args.join(", "))
            }
        }
    }

    fn source_expr(&self, src: &SourceRef) -> String {
        match src {
            SourceRef::Node { node, port } => {
                let name = self
                    .names
                    .get(node)
                    .cloned()
                    .unwrap_or_else(|| "value".into());
                let primary = self.primary.get(node).map(String::as_str).unwrap_or("");
                if port.is_empty() || port == primary {
                    name
                } else if let Some(tunnel) = port.strip_prefix("t:") {
                    format!("{name}.{tunnel}")
                } else {
                    format!("{name}.{port}")
                }
            }
            SourceRef::Tunnel { structure, name } => {
                // Inside a structure body the DSL refers to the outer binding
                // directly; the builder recreates the tunnel.
                if let Some(node) = self.nodes.get(structure.as_str()) {
                    if let Some(outer) = node.inputs.get(&format!("t:{name}")) {
                        return self.source_expr(outer);
                    }
                    if let Some(t) = node.tunnel(name) {
                        return default_literal(&t.ty);
                    }
                }
                sanitize(name)
            }
            SourceRef::ShiftRegister { structure, name } => {
                if let Some(node) = self.nodes.get(structure.as_str()) {
                    if let Some(sr) = node.shift_registers.iter().find(|s| &s.name == name) {
                        return self.source_expr(&sr.init);
                    }
                }
                sanitize(name)
            }
            SourceRef::Iteration { structure } => self
                .nodes
                .get(structure.as_str())
                .and_then(|n| n.props.get("var"))
                .and_then(Json::as_str)
                .map(sanitize)
                .unwrap_or_else(|| "iteration".into()),
            SourceRef::EventData { .. } => "event_data".into(),
            SourceRef::Default { ty } => default_literal(ty),
        }
    }
}

// ---- Ordering -----------------------------------------------------------

fn ordered(scope: &Scope) -> Vec<&Node> {
    let by_id: BTreeMap<&str, &Node> = scope.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut visited: BTreeSet<&str> = BTreeSet::new();
    let mut visiting: BTreeSet<&str> = BTreeSet::new();
    let mut out: Vec<&Node> = Vec::new();
    for node in &scope.nodes {
        visit(node, &by_id, &mut visited, &mut visiting, &mut out);
    }
    out
}

fn visit<'a>(
    node: &'a Node,
    by_id: &BTreeMap<&'a str, &'a Node>,
    visited: &mut BTreeSet<&'a str>,
    visiting: &mut BTreeSet<&'a str>,
    out: &mut Vec<&'a Node>,
) {
    if visited.contains(node.id.as_str()) || visiting.contains(node.id.as_str()) {
        return;
    }
    visiting.insert(node.id.as_str());
    for src in node.inputs.values() {
        if let SourceRef::Node { node: dep, .. } = src {
            if let Some(dep) = by_id.get(dep.as_str()) {
                visit(dep, by_id, visited, visiting, out);
            }
        }
    }
    visiting.remove(node.id.as_str());
    visited.insert(node.id.as_str());
    out.push(node);
}

// ---- Naming helpers -----------------------------------------------------

fn primary_port(node: &Node) -> String {
    match node.kind.as_str() {
        kinds::CONTROL | kinds::CONSTANT => "value".into(),
        kinds::INDICATOR => String::new(),
        k if is_structure(k) => {
            let outs: Vec<&str> = node
                .tunnels
                .iter()
                .filter(|t| t.direction == lc_model::TunnelDirection::Out)
                .map(|t| t.name.as_str())
                .collect();
            if outs.len() == 1 {
                format!("t:{}", outs[0])
            } else {
                String::new()
            }
        }
        kind => lc_nodes::Registry::global()
            .get(kind)
            .and_then(|d| d.outputs.first())
            .map(|p| p.name.to_string())
            .unwrap_or_default(),
    }
}

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

fn label_token(label: &str) -> String {
    if label.is_empty() {
        return "\"\"".into();
    }
    let simple = label
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'));
    let numeric_list = label.split(',').all(|part| {
        let part = part.trim();
        if part.parse::<f64>().is_ok() {
            return true;
        }
        match part.split_once("..") {
            Some((a, b)) => a.trim().parse::<f64>().is_ok() && b.trim().parse::<f64>().is_ok(),
            None => false,
        }
    });
    if simple || numeric_list {
        label.to_string()
    } else {
        format!("\"{}\"", escape(label))
    }
}

// ---- Type and value rendering ------------------------------------------

fn widget_name(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::Numeric => "numeric",
        ControlKind::Slider => "slider",
        ControlKind::Toggle => "toggle",
        ControlKind::Led => "led",
        ControlKind::String => "string",
        ControlKind::Path => "path",
        ControlKind::Button => "button",
        ControlKind::Chart => "chart",
        ControlKind::Graph => "graph",
        ControlKind::Enum => "enum",
        ControlKind::Table => "table",
        ControlKind::Cluster => "cluster",
        ControlKind::Array => "array",
    }
}

fn default_widget(indicator: bool, ty: &Type) -> ControlKind {
    match ty {
        Type::Bool => {
            if indicator {
                ControlKind::Led
            } else {
                ControlKind::Toggle
            }
        }
        Type::String => ControlKind::String,
        Type::Path => ControlKind::Path,
        Type::Enum { .. } => ControlKind::Enum,
        Type::Cluster { .. } => ControlKind::Cluster,
        Type::Array { element, .. } => match element.as_ref() {
            Type::Numeric { .. } if indicator => ControlKind::Chart,
            Type::Array { .. } => ControlKind::Table,
            _ => ControlKind::Array,
        },
        _ => ControlKind::Numeric,
    }
}

fn type_name(ty: &Type) -> Option<String> {
    Some(match ty {
        Type::Void => "void".into(),
        Type::Bool => "bool".into(),
        Type::Numeric { numeric } => numeric.name().to_ascii_lowercase(),
        Type::String => "string".into(),
        Type::Path => "path".into(),
        Type::Timestamp => "timestamp".into(),
        Type::Error => "error".into(),
        Type::Variant => "variant".into(),
        Type::Waveform => "waveform".into(),
        Type::Array { .. } => format!("{}{}", type_name(ty.element()?)?, "[]".repeat(ty.dims())),
        Type::Cluster { .. } | Type::Enum { .. } | Type::Refnum { .. } | Type::Any => return None,
    })
}

fn value_literal(value: &Value) -> Option<String> {
    Some(match value {
        Value::Void => "0".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(NumericValue::Int(v)) => v.to_string(),
        Value::Num(NumericValue::UInt(v)) => v.to_string(),
        Value::Num(NumericValue::Float(v)) => float_literal(*v),
        Value::Num(NumericValue::Complex { re, .. }) => float_literal(*re),
        Value::Str(s) => format!("\"{}\"", escape(s)),
        Value::Array(array) => {
            let items = array
                .data
                .iter()
                .filter_map(value_literal)
                .collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        }
        Value::Cluster(cluster) => {
            let items = cluster
                .fields
                .iter()
                .enumerate()
                .filter_map(|(i, v)| Some(format!("f{i}: {}", value_literal(v)?)))
                .collect::<Vec<_>>();
            format!("{{{}}}", items.join(", "))
        }
        Value::Timestamp(_) | Value::Error(_) | Value::Waveform(_) | Value::Variant(_) => {
            return None
        }
    })
}

fn json_value_literal(value: &Json) -> Option<String> {
    if let Ok(v) = serde_json::from_value::<Value>(value.clone()) {
        return value_literal(&v);
    }
    match value {
        Json::Number(n) => Some(n.to_string()),
        Json::Bool(b) => Some(b.to_string()),
        Json::String(s) => Some(format!("\"{}\"", escape(s))),
        _ => None,
    }
}

fn default_literal(ty: &Type) -> String {
    value_literal(&Value::default_for(ty)).unwrap_or_else(|| "0".into())
}

fn float_literal(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

fn trim_f64(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn trim_f32(v: f32) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn indent(line: &str, level: usize) -> String {
    let pad = "  ".repeat(level);
    if line.is_empty() {
        format!("{pad}\n")
    } else {
        format!("{pad}{line}\n")
    }
}
