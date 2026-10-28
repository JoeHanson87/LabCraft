use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lc_model::document::{
    is_structure, kinds, Node, Scope, SourceRef, TunnelDirection, TunnelMode,
};
use lc_model::{ArrayValue, Diagnostic, Type, Value, Waveform};
use lc_nodes::ExecCtx;

use crate::{default_inputs, CompileReport, CompiledVi};

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub inputs: BTreeMap<String, Value>,
    pub timeout_ms: u64,
    pub max_iterations: u64,
    pub max_wait_ms: u64,
    pub allow_file: bool,
    pub file_root: Option<PathBuf>,
    pub events: Vec<Event>,
    pub trace: bool,
}

impl RunOptions {
    pub fn defaults() -> Self {
        Self {
            inputs: BTreeMap::new(),
            timeout_ms: 15_000,
            max_iterations: 1_000_000,
            max_wait_ms: 1_000,
            allow_file: true,
            file_root: None,
            events: Vec::new(),
            trace: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub data: Value,
}

#[derive(Debug, Clone, Default, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub node: String,
    pub key: String,
    pub kind: String,
    pub iteration: u64,
    pub elapsed_ms: f64,
    pub duration_us: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutput {
    pub indicators: BTreeMap<String, Value>,
    pub logs: Vec<String>,
    pub messages: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<TraceEvent>,
    pub iterations: u64,
    pub duration_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunError {
    Compile(Vec<Diagnostic>),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Compile(diags) => {
                write!(f, "VI does not compile ({} diagnostics)", diags.len())
            }
        }
    }
}

impl std::error::Error for RunError {}

#[derive(Debug, Clone, Default)]
struct ScopeEnv {
    values: BTreeMap<(String, String), Value>,
    tunnels: BTreeMap<String, Value>,
    shifts: BTreeMap<String, Value>,
    iteration: Option<i64>,
    event: Option<Value>,
}

#[derive(Debug, Clone)]
struct RtError {
    message: String,
    node: Option<String>,
    key: Option<String>,
    port: Option<String>,
}

impl RtError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            node: None,
            key: None,
            port: None,
        }
    }
    fn at(mut self, node: &Node) -> Self {
        self.node = Some(node.id.clone());
        self.key = Some(node.key.clone());
        self
    }
    fn port(mut self, port: impl Into<String>) -> Self {
        self.port = Some(port.into());
        self
    }
    fn diagnostic(&self) -> Diagnostic {
        let mut d = Diagnostic::error("RUNTIME_ERROR", &self.message);
        if let Some(n) = &self.node {
            d = d.at(n);
        }
        if let Some(p) = &self.port {
            d = d.port(p);
        }
        d
    }
}

struct Interp<'a> {
    compiled: &'a CompiledVi,
    ctx: ExecCtx,
    timeout: Duration,
    max_iterations: u64,
    iterations: u64,
    trace: Vec<TraceEvent>,
    trace_enabled: bool,
    events: VecDeque<Event>,
    started: Instant,
}

pub fn run(doc: &lc_model::ViDocument, opts: &RunOptions) -> Result<RunOutput, RunError> {
    let report: CompileReport = crate::compile(doc);
    if !report.ok() {
        return Err(RunError::Compile(report.diagnostics));
    }
    let compiled = report.compiled.expect("compile always returns types");

    let mut panel = default_inputs(doc);
    for c in &doc.panel.controls {
        panel
            .entry(c.key.clone())
            .or_insert_with(|| c.default.clone());
    }
    for (k, v) in &opts.inputs {
        panel.insert(k.clone(), v.clone());
    }

    let mut ctx = ExecCtx::new();
    ctx.panel = panel;
    ctx.indicators = doc
        .panel
        .controls
        .iter()
        .filter(|c| c.is_indicator())
        .map(|c| (c.key.clone(), c.default.clone()))
        .collect();
    ctx.file_root = opts.file_root.clone();
    ctx.allow_file = opts.allow_file;
    ctx.max_wait_ms = if opts.max_wait_ms == 0 {
        1_000
    } else {
        opts.max_wait_ms
    };

    let mut interp = Interp {
        compiled: &compiled,
        ctx,
        timeout: Duration::from_millis(if opts.timeout_ms == 0 {
            15_000
        } else {
            opts.timeout_ms
        }),
        max_iterations: if opts.max_iterations == 0 {
            1_000_000
        } else {
            opts.max_iterations
        },
        iterations: 0,
        trace: Vec::new(),
        trace_enabled: opts.trace,
        events: opts.events.iter().cloned().collect(),
        started: Instant::now(),
    };

    let mut env = ScopeEnv::default();
    let mut output = RunOutput::default();
    match interp.exec_scope(&compiled.doc.diagram.root, &mut env) {
        Ok(()) => {}
        Err(e) => {
            output.error = Some(e.message.clone());
            output.diagnostics.push(e.diagnostic());
        }
    }
    output.indicators = interp.ctx.indicators.clone();
    output.logs = interp.ctx.logs.clone();
    output.messages = interp.ctx.messages.clone();
    output.trace = interp.trace.clone();
    output.iterations = interp.iterations;
    output.duration_ms = interp.started.elapsed().as_secs_f64() * 1000.0;
    Ok(output)
}

impl<'a> Interp<'a> {
    fn tick(&mut self, node: &Node) -> Result<(), RtError> {
        self.iterations += 1;
        if self.started.elapsed() > self.timeout {
            return Err(RtError::new("execution timed out").at(node));
        }
        if self.iterations > self.max_iterations {
            return Err(RtError::new("iteration limit exceeded").at(node));
        }
        Ok(())
    }

    fn exec_scope(&mut self, scope: &Scope, env: &mut ScopeEnv) -> Result<(), RtError> {
        for id in crate::topo_order(scope) {
            let Some(node) = scope.node(&id) else {
                continue;
            };
            self.tick(node)?;
            let started = Instant::now();
            let result = self.exec_node(node, env);
            let duration_us = started.elapsed().as_micros() as u64;
            match &result {
                Ok(outputs) => {
                    for (port, v) in outputs {
                        env.values
                            .insert((node.id.clone(), port.clone()), v.clone());
                    }
                }
                Err(e) => {
                    if self.trace_enabled {
                        self.trace.push(TraceEvent {
                            node: node.id.clone(),
                            key: node.key.clone(),
                            kind: node.kind.clone(),
                            iteration: self.iterations,
                            elapsed_ms: self.started.elapsed().as_secs_f64() * 1000.0,
                            duration_us,
                            inputs: BTreeMap::new(),
                            outputs: BTreeMap::new(),
                            error: Some(e.message.clone()),
                        });
                    }
                    return Err(e.clone());
                }
            }
            if self.trace_enabled {
                self.trace.push(TraceEvent {
                    node: node.id.clone(),
                    key: node.key.clone(),
                    kind: node.kind.clone(),
                    iteration: self.iterations,
                    elapsed_ms: self.started.elapsed().as_secs_f64() * 1000.0,
                    duration_us,
                    inputs: BTreeMap::new(),
                    outputs: result.unwrap_or_default(),
                    error: None,
                });
            }
        }
        Ok(())
    }

    fn exec_node(
        &mut self,
        node: &Node,
        env: &mut ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        if is_structure(&node.kind) {
            return self.exec_structure(node, env);
        }
        let def = lc_nodes::Registry::global()
            .get(&node.kind)
            .ok_or_else(|| RtError::new(format!("unknown node kind `{}`", node.kind)).at(node))?;

        let mut inputs = BTreeMap::new();
        for (port, src) in &node.inputs {
            let value = self
                .eval_source(src, env)
                .map_err(|e| e.at(node).port(port))?;
            let target = self
                .compiled
                .input_type(&node.id, port)
                .cloned()
                .unwrap_or_else(|| value.type_of());
            let coerced = value
                .coerce_to(&target)
                .map_err(|msg| RtError::new(msg).at(node).port(port))?;
            inputs.insert(port.clone(), coerced);
        }
        for pin in &def.inputs {
            if !inputs.contains_key(pin.name) {
                if let Some(default) = &pin.default {
                    inputs.insert(pin.name.to_string(), default.clone());
                }
            }
        }

        // Error-cluster propagation: a node with a wired error input that is
        // already in error is skipped and passes the cluster through.
        if let Some(err) = inputs
            .get("error")
            .or_else(|| inputs.get("error_in"))
            .and_then(|v| match v {
                Value::Error(e) if e.status => Some(e.clone()),
                _ => None,
            })
        {
            let mut out = BTreeMap::new();
            if let Some(types) = self.compiled.node_types.get(&node.id) {
                for (port, ty) in types {
                    if matches!(ty, Type::Error) {
                        out.insert(port.clone(), Value::Error(err.clone()));
                    } else {
                        out.insert(port.clone(), Value::default_for(ty));
                    }
                }
            }
            return Ok(out);
        }

        let resolved = lc_nodes::Resolved {
            inputs: self
                .compiled
                .input_types
                .get(&node.id)
                .cloned()
                .unwrap_or_default(),
            outputs: self
                .compiled
                .node_types
                .get(&node.id)
                .cloned()
                .unwrap_or_default(),
            cluster_fields: cluster_fields(self.compiled, node),
        };

        let result = lc_nodes::execute(def, node, &resolved, &inputs, &mut self.ctx);
        match result {
            Ok(mut outputs) => {
                // Ensure every declared output has a value of the right type.
                if let Some(types) = self.compiled.node_types.get(&node.id) {
                    for (port, ty) in types {
                        let v = outputs
                            .remove(port)
                            .unwrap_or_else(|| Value::default_for(ty));
                        let v = v.coerce_to(ty).unwrap_or_else(|_| Value::default_for(ty));
                        outputs.insert(port.clone(), v);
                    }
                }
                Ok(outputs)
            }
            Err(msg) => {
                let has_error_port = self
                    .compiled
                    .node_types
                    .get(&node.id)
                    .map(|m| m.values().any(|t| matches!(t, Type::Error)))
                    .unwrap_or(false);
                if has_error_port {
                    let mut out = BTreeMap::new();
                    if let Some(types) = self.compiled.node_types.get(&node.id) {
                        for (port, ty) in types {
                            if matches!(ty, Type::Error) {
                                out.insert(
                                    port.clone(),
                                    Value::Error(lc_model::ErrorCluster::err(1, msg.clone())),
                                );
                            } else {
                                out.insert(port.clone(), Value::default_for(ty));
                            }
                        }
                    }
                    Ok(out)
                } else {
                    Err(RtError::new(msg).at(node))
                }
            }
        }
    }

    fn exec_structure(
        &mut self,
        node: &Node,
        env: &mut ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        match node.kind.as_str() {
            kinds::WHILE => self.exec_while(node, env),
            kinds::FOR => self.exec_for(node, env),
            kinds::CASE => self.exec_case(node, env),
            kinds::SEQUENCE => self.exec_sequence(node, env),
            kinds::EVENT => self.exec_event(node, env),
            kinds::LOOP_CONDITION => Ok(BTreeMap::new()),
            other => Err(RtError::new(format!("unsupported structure `{other}`")).at(node)),
        }
    }

    fn eval_source(&self, src: &SourceRef, env: &ScopeEnv) -> Result<Value, RtError> {
        match src {
            SourceRef::Node { node, port } => env
                .values
                .get(&(node.clone(), port.clone()))
                .cloned()
                .ok_or_else(|| RtError::new(format!("no value on wire from `{node}.{port}`"))),
            SourceRef::Tunnel { name, .. } => env
                .tunnels
                .get(name)
                .cloned()
                .ok_or_else(|| RtError::new(format!("no value on tunnel `{name}`"))),
            SourceRef::ShiftRegister { name, .. } => env
                .shifts
                .get(name)
                .cloned()
                .ok_or_else(|| RtError::new(format!("no value on shift register `{name}`"))),
            SourceRef::Iteration { .. } => Ok(Value::int(env.iteration.unwrap_or(0))),
            SourceRef::EventData { .. } => Ok(env.event.clone().unwrap_or(Value::Void)),
            SourceRef::Default { ty } => Ok(Value::default_for(ty)),
        }
    }

    fn input_of(&self, node: &Node, port: &str, env: &ScopeEnv) -> Result<Option<Value>, RtError> {
        let Some(src) = node.inputs.get(port) else {
            return Ok(None);
        };
        let v = self.eval_source(src, env)?;
        let target = self
            .compiled
            .input_type(&node.id, &format!("t:{port}"))
            .or_else(|| self.compiled.input_type(&node.id, port))
            .cloned();
        match target {
            Some(t) => v
                .coerce_to(&t)
                .map(Some)
                .map_err(|m| RtError::new(m).at(node).port(port)),
            None => Ok(Some(v)),
        }
    }

    fn tunnel_inputs(
        &self,
        node: &Node,
        env: &ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        let mut out = BTreeMap::new();
        for t in &node.tunnels {
            if t.direction != TunnelDirection::In {
                continue;
            }
            let v = self
                .input_of(node, &format!("t:{}", t.name), env)?
                .unwrap_or_else(|| Value::default_for(&t.ty));
            out.insert(t.name.clone(), v);
        }
        Ok(out)
    }

    fn shift_init(&self, node: &Node, env: &ScopeEnv) -> Result<BTreeMap<String, Value>, RtError> {
        let mut out = BTreeMap::new();
        for sr in &node.shift_registers {
            let v = self.eval_source(&sr.init, env)?;
            let v = v
                .coerce_to(&sr.ty)
                .unwrap_or_else(|_| Value::default_for(&sr.ty));
            out.insert(sr.name.clone(), v);
        }
        Ok(out)
    }

    fn body_env(
        &self,
        tunnels: BTreeMap<String, Value>,
        shifts: BTreeMap<String, Value>,
        iteration: Option<i64>,
        event: Option<Value>,
    ) -> ScopeEnv {
        ScopeEnv {
            values: BTreeMap::new(),
            tunnels,
            shifts,
            iteration,
            event,
        }
    }

    fn exec_while(
        &mut self,
        node: &Node,
        env: &mut ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        let Some(frame) = node.frames.iter().find(|f| f.label == "body") else {
            return Err(RtError::new("While loop has no body").at(node));
        };
        let outside = self.tunnel_inputs(node, env)?;
        let mut shifts = self.shift_init(node, env)?;
        let mut last: BTreeMap<String, Value> = BTreeMap::new();
        let mut collected: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut iteration: i64 = 0;
        loop {
            let mut body_tunnels = BTreeMap::new();
            let mut exhausted = false;
            for t in &node.tunnels {
                if t.direction != TunnelDirection::In {
                    continue;
                }
                let value = outside.get(&t.name).cloned().unwrap_or(Value::Void);
                if t.mode == TunnelMode::AutoIndex {
                    let elem_ty = t.ty.element().cloned().unwrap_or(Type::Void);
                    match value.as_array() {
                        Some(a) => match a.data.get(iteration as usize) {
                            Some(v) => {
                                body_tunnels.insert(t.name.clone(), v.clone());
                            }
                            None => {
                                exhausted = true;
                                body_tunnels.insert(t.name.clone(), Value::default_for(&elem_ty));
                            }
                        },
                        None => {
                            body_tunnels.insert(t.name.clone(), Value::default_for(&elem_ty));
                        }
                    }
                } else {
                    body_tunnels.insert(t.name.clone(), value);
                }
            }
            if exhausted {
                break;
            }

            let mut benv = self.body_env(body_tunnels, shifts.clone(), Some(iteration), None);
            self.exec_scope(&frame.scope, &mut benv)?;

            let cond = self.loop_condition(node, &frame.scope, &benv);
            for t in &node.tunnels {
                if t.direction != TunnelDirection::Out {
                    continue;
                }
                let key = format!("t:{}", t.name);
                let v = match frame.scope.outputs.get(&key) {
                    Some(src) => self.eval_source(src, &benv)?,
                    None => Value::default_for(&t.ty),
                };
                if t.mode == TunnelMode::AutoIndex {
                    collected.entry(t.name.clone()).or_default().push(v);
                } else {
                    last.insert(t.name.clone(), v);
                }
            }
            for sr in &node.shift_registers {
                let key = format!("sr:{}", sr.name);
                if let Some(src) = frame.scope.outputs.get(&key) {
                    let v = self.eval_source(src, &benv)?;
                    shifts.insert(sr.name.clone(), v.coerce_to(&sr.ty).unwrap_or(Value::Void));
                }
            }

            let stop_if_true = node
                .props
                .get("stopIfTrue")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let stop = cond.unwrap_or(false);
            if stop == stop_if_true {
                break;
            }
            iteration += 1;
            if iteration as u64 > self.max_iterations {
                return Err(RtError::new("while loop exceeded iteration limit").at(node));
            }
        }

        let mut out = BTreeMap::new();
        for t in &node.tunnels {
            if t.direction != TunnelDirection::Out {
                continue;
            }
            let value = if t.mode == TunnelMode::AutoIndex {
                let vals = collected.remove(&t.name).unwrap_or_default();
                array_for(&t.ty, vals)
            } else {
                last.get(&t.name)
                    .cloned()
                    .unwrap_or_else(|| Value::default_for(&t.ty))
            };
            out.insert(format!("t:{}", t.name), value);
        }
        Ok(out)
    }

    fn loop_condition(&self, _node: &Node, scope: &Scope, env: &ScopeEnv) -> Option<bool> {
        let cond = scope
            .nodes
            .iter()
            .find(|n| n.kind == kinds::LOOP_CONDITION)?;
        let src = cond.inputs.get("value")?;
        self.eval_source(src, env).ok().map(|v| v.as_bool())
    }

    fn exec_for(
        &mut self,
        node: &Node,
        env: &mut ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        let Some(frame) = node.frames.iter().find(|f| f.label == "body") else {
            return Err(RtError::new("For loop has no body").at(node));
        };
        let outside = self.tunnel_inputs(node, env)?;
        let mut shifts = self.shift_init(node, env)?;
        let mut n = match self.input_of(node, "n", env)? {
            Some(v) => v.as_i64().max(0) as u64,
            None => {
                // Number of iterations defaults to the shortest auto-indexed array.
                let mut min = u64::MAX;
                for t in &node.tunnels {
                    if t.direction == TunnelDirection::In && t.mode == TunnelMode::AutoIndex {
                        if let Some(a) = outside.get(&t.name).and_then(|v| v.as_array()) {
                            min = min.min(a.data.len() as u64);
                        }
                    }
                }
                if min == u64::MAX {
                    0
                } else {
                    min
                }
            }
        };
        n = n.min(self.max_iterations);

        let mut last: BTreeMap<String, Value> = BTreeMap::new();
        let mut collected: BTreeMap<String, Vec<Value>> = BTreeMap::new();

        for i in 0..n {
            let mut body_tunnels = BTreeMap::new();
            for t in &node.tunnels {
                if t.direction != TunnelDirection::In {
                    continue;
                }
                let value = outside.get(&t.name).cloned().unwrap_or(Value::Void);
                if t.mode == TunnelMode::AutoIndex {
                    let elem_ty = t.ty.element().cloned().unwrap_or(Type::Void);
                    let v = value
                        .as_array()
                        .and_then(|a| a.data.get(i as usize).cloned())
                        .unwrap_or_else(|| Value::default_for(&elem_ty));
                    body_tunnels.insert(t.name.clone(), v);
                } else {
                    body_tunnels.insert(t.name.clone(), value);
                }
            }
            let mut benv = self.body_env(body_tunnels, shifts.clone(), Some(i as i64), None);
            self.exec_scope(&frame.scope, &mut benv)?;

            for t in &node.tunnels {
                if t.direction != TunnelDirection::Out {
                    continue;
                }
                let key = format!("t:{}", t.name);
                let v = match frame.scope.outputs.get(&key) {
                    Some(src) => self.eval_source(src, &benv)?,
                    None => Value::default_for(&t.ty),
                };
                if t.mode == TunnelMode::AutoIndex {
                    collected.entry(t.name.clone()).or_default().push(v);
                } else {
                    last.insert(t.name.clone(), v);
                }
            }
            for sr in &node.shift_registers {
                let key = format!("sr:{}", sr.name);
                if let Some(src) = frame.scope.outputs.get(&key) {
                    let v = self.eval_source(src, &benv)?;
                    shifts.insert(sr.name.clone(), v.coerce_to(&sr.ty).unwrap_or(Value::Void));
                }
            }
        }

        let mut out = BTreeMap::new();
        for t in &node.tunnels {
            if t.direction != TunnelDirection::Out {
                continue;
            }
            let value = if t.mode == TunnelMode::AutoIndex {
                array_for(&t.ty, collected.remove(&t.name).unwrap_or_default())
            } else {
                last.get(&t.name)
                    .cloned()
                    .unwrap_or_else(|| Value::default_for(&t.ty))
            };
            out.insert(format!("t:{}", t.name), value);
        }
        Ok(out)
    }

    fn exec_case(
        &mut self,
        node: &Node,
        env: &mut ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        let selector = self.input_of(node, "selector", env)?.unwrap_or(Value::Void);
        let tunnels = self.tunnel_inputs(node, env)?;

        let chosen = node
            .frames
            .iter()
            .find(|f| label_matches(&f.label, &selector))
            .or_else(|| node.frames.iter().find(|f| f.label == "default"))
            .or_else(|| node.frames.first());

        let Some(frame) = chosen else {
            return Err(RtError::new("case structure has no matching frame").at(node));
        };

        let mut benv = self.body_env(tunnels.clone(), BTreeMap::new(), None, None);
        self.exec_scope(&frame.scope, &mut benv)?;

        let mut out = BTreeMap::new();
        for t in &node.tunnels {
            if t.direction != TunnelDirection::Out {
                continue;
            }
            let key = format!("t:{}", t.name);
            let v = match frame.scope.outputs.get(&key) {
                Some(src) => self.eval_source(src, &benv)?,
                None => Value::default_for(&t.ty),
            };
            out.insert(format!("t:{}", t.name), v);
        }
        Ok(out)
    }

    fn exec_sequence(
        &mut self,
        node: &Node,
        env: &mut ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        let mut tunnels = self.tunnel_inputs(node, env)?;
        for frame in &node.frames {
            let mut benv = self.body_env(tunnels.clone(), BTreeMap::new(), None, None);
            self.exec_scope(&frame.scope, &mut benv)?;
            for t in &node.tunnels {
                let key = format!("t:{}", t.name);
                if let Some(src) = frame.scope.outputs.get(&key) {
                    let v = self.eval_source(src, &benv)?;
                    let target = if t.mode == TunnelMode::AutoIndex {
                        t.ty.element().cloned().unwrap_or(t.ty.clone())
                    } else {
                        t.ty.clone()
                    };
                    tunnels.insert(t.name.clone(), v.coerce_to(&target).unwrap_or(v));
                }
            }
        }
        let mut out = BTreeMap::new();
        for t in &node.tunnels {
            if t.direction != TunnelDirection::Out {
                continue;
            }
            let v = tunnels
                .get(&t.name)
                .cloned()
                .unwrap_or_else(|| Value::default_for(&t.ty));
            out.insert(format!("t:{}", t.name), v);
        }
        Ok(out)
    }

    fn exec_event(
        &mut self,
        node: &Node,
        env: &mut ScopeEnv,
    ) -> Result<BTreeMap<String, Value>, RtError> {
        let tunnels = self.tunnel_inputs(node, env)?;
        let event = self.events.pop_front();
        let frame = match &event {
            Some(e) => node
                .frames
                .iter()
                .find(|f| f.label == e.name)
                .or_else(|| node.frames.iter().find(|f| f.label == "default")),
            None => node
                .frames
                .iter()
                .find(|f| f.label == "timeout")
                .or_else(|| node.frames.first()),
        };
        let Some(frame) = frame else {
            return Err(RtError::new("event structure has no matching frame").at(node));
        };
        let data = event.map(|e| e.data).unwrap_or(Value::Void);
        let mut benv = self.body_env(tunnels, BTreeMap::new(), None, Some(data));
        self.exec_scope(&frame.scope, &mut benv)?;

        let mut out = BTreeMap::new();
        for t in &node.tunnels {
            if t.direction != TunnelDirection::Out {
                continue;
            }
            let key = format!("t:{}", t.name);
            let v = match frame.scope.outputs.get(&key) {
                Some(src) => self.eval_source(src, &benv)?,
                None => Value::default_for(&t.ty),
            };
            out.insert(format!("t:{}", t.name), v);
        }
        Ok(out)
    }
}

fn array_for(outside: &Type, values: Vec<Value>) -> Value {
    let dims = match outside {
        Type::Array { dims, .. } => {
            let mut d = vec![values.len()];
            // Higher-dimensional auto-indexing is not represented by a
            // dynamic count yet; flatten to the collected length.
            d.resize((*dims).max(1), 1);
            d
        }
        _ => vec![values.len()],
    };
    Value::Array(ArrayValue::new(dims, values))
}

fn label_matches(label: &str, selector: &Value) -> bool {
    match selector {
        Value::Bool(b) => {
            let l = label.trim().to_lowercase();
            (l == "true" && *b) || (l == "false" && !*b)
        }
        Value::Str(s) => label == s,
        Value::Num(_) => {
            let n = selector.as_f64();
            let l = label.trim();
            if l == "default" {
                return false;
            }
            for part in l.split(',') {
                let part = part.trim();
                if let Some((a, b)) = part.split_once("..") {
                    if let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
                        if n >= a && n <= b {
                            return true;
                        }
                    }
                } else if let Ok(v) = part.parse::<f64>() {
                    if (v - n).abs() < f64::EPSILON {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

fn cluster_fields(compiled: &CompiledVi, node: &Node) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    if node.kind == "unbundle_by_name" {
        let names = lc_nodes::prop_string_list(node, "fields");
        if let Some(Type::Cluster { fields }) = compiled.input_type(&node.id, "cluster") {
            for (i, f) in fields.iter().enumerate() {
                if names.contains(&f.name) {
                    out.insert(f.name.clone(), i);
                }
            }
        }
    } else if node.kind == "unbundle" {
        for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
            out.insert((*name).to_string(), i);
        }
    }
    out
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    pub total: usize,
    pub passed: usize,
    pub results: Vec<TestResult>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub failures: Vec<String>,
    pub outputs: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Run every test vector attached to the VI and compare indicator values.
pub fn run_tests(doc: &lc_model::ViDocument) -> TestReport {
    let mut report = TestReport::default();
    for case in &doc.tests {
        report.total += 1;
        let mut inputs = default_inputs(doc);
        for (k, v) in &case.inputs {
            inputs.insert(k.clone(), v.clone());
        }
        let opts = RunOptions {
            inputs,
            ..RunOptions::defaults()
        };
        let mut failures = Vec::new();
        let mut outputs = BTreeMap::new();
        let mut error = None;
        match run(doc, &opts) {
            Err(e) => {
                error = Some(e.to_string());
                failures.push("VI failed to compile".into());
            }
            Ok(out) => {
                outputs = out.indicators.clone();
                if let Some(e) = &out.error {
                    error = Some(e.clone());
                    failures.push(format!("runtime error: {e}"));
                }
                for (key, expected) in &case.expected {
                    let actual = out
                        .indicators
                        .get(key)
                        .or_else(|| out.indicators.get(key))
                        .cloned()
                        .unwrap_or(Value::Void);
                    if !value_matches(expected, &actual, case.tolerance) {
                        failures.push(format!(
                            "`{key}` expected {} got {}",
                            expected.display(),
                            actual.display()
                        ));
                    }
                }
            }
        }
        let passed = failures.is_empty();
        if passed {
            report.passed += 1;
        }
        report.results.push(TestResult {
            name: case.name.clone(),
            passed,
            failures,
            outputs,
            error,
        });
    }
    report
}

fn value_matches(expected: &Value, actual: &Value, tolerance: f64) -> bool {
    match (expected, actual) {
        (Value::Num(_), Value::Num(_)) => {
            (expected.as_f64() - actual.as_f64()).abs() <= tolerance.max(f64::EPSILON)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.dims == b.dims
                && a.data.len() == b.data.len()
                && a.data
                    .iter()
                    .zip(&b.data)
                    .all(|(x, y)| value_matches(x, y, tolerance))
        }
        (Value::Cluster(a), Value::Cluster(b)) => {
            a.fields.len() == b.fields.len()
                && a.fields
                    .iter()
                    .zip(&b.fields)
                    .all(|(x, y)| value_matches(x, y, tolerance))
        }
        _ => expected == actual,
    }
}

/// A minimal waveform helper used by examples and tests.
#[allow(dead_code)]
pub fn sine_waveform(n: usize, dt: f64, amplitude: f64) -> Waveform {
    Waveform {
        t0: lc_model::Timestamp::default(),
        dt,
        y: (0..n)
            .map(|i| amplitude * (i as f64 * dt * std::f64::consts::TAU).sin())
            .collect(),
        attributes: BTreeMap::new(),
    }
}
