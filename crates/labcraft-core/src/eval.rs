use crate::model::{EventFrame, LoopKind, NodeId, NodeKind, Spec, Vi, Wire, MAX_FRAMES};
use crate::panel::Prop;
use crate::ops::{self, BinOp, Cmp, Logic, Unary};
use crate::typing::{self, TypeMap};
use crate::types::{ArrayVal, DataType, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ITERATION_LIMIT: i64 = 50_000_000;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RunError {
    #[error("node {0} ({1}) has an unwired input '{2}'")]
    Unwired(NodeId, String, String),
    #[error("division by zero at node {0}")]
    DivideByZero(NodeId),
    #[error("diagram contains a cycle")]
    Cycle,
    #[error("run aborted")]
    Aborted,
    #[error("loop {0} exceeded the iteration limit")]
    IterationLimit(NodeId),
    #[error("internal error: {0}")]
    Internal(String),
}

/// Hooks for a running VI: abort flag, live control values and indicator updates.
pub struct RunControl {
    pub abort: Arc<AtomicBool>,
    /// Latest control values, read live so a Stop button works inside a running loop.
    pub live_controls: Arc<Mutex<HashMap<NodeId, Value>>>,
    /// Global variables, shared by name across the VI and across runs.
    pub globals: Arc<Mutex<HashMap<String, Value>>>,
    pub on_indicator: Box<dyn Fn(NodeId, &Value) + Send + Sync>,
    /// Called when a Local Variable writes to a control, so the panel can show it.
    pub on_control_write: Box<dyn Fn(NodeId, &Value) + Send + Sync>,
    /// Property values written during this run, so later reads see them.
    pub properties: Arc<Mutex<HashMap<(NodeId, Prop), Value>>>,
    /// Called when a Property Node writes, so the panel can apply it.
    pub on_property: Box<dyn Fn(NodeId, Prop, &Value) + Send + Sync>,
}

impl RunControl {
    pub fn headless() -> Self {
        RunControl {
            abort: Arc::new(AtomicBool::new(false)),
            live_controls: Arc::new(Mutex::new(HashMap::new())),
            globals: Arc::new(Mutex::new(HashMap::new())),
            on_indicator: Box::new(|_, _| {}),
            on_control_write: Box::new(|_, _| {}),
            properties: Arc::new(Mutex::new(HashMap::new())),
            on_property: Box::new(|_, _, _| {}),
        }
    }
}

pub struct RunResult {
    /// Output value of every node terminal, keyed by (node, output index).
    pub outputs: HashMap<(NodeId, usize), Value>,
    /// Final value written to each indicator.
    pub indicators: Vec<(NodeId, Value)>,
}

pub fn run(vi: &Vi) -> Result<RunResult, RunError> {
    run_with(vi, &RunControl::headless())
}

/// Execute a VI in dataflow order. Loops, Case/Sequence/Event structures, auto-indexing and
/// feedback are supported; independent structures run on separate threads.
pub fn run_with(vi: &Vi, ctl: &RunControl) -> Result<RunResult, RunError> {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    let types = typing::resolve(vi);
    let mut cx = Ctx {
        vi,
        ctl,
        types: &types,
        env: HashMap::new(),
        indicators: HashMap::new(),
        tun: HashMap::new(),
        feedback: HashMap::new(),
        event_last: HashMap::new(),
        iter: vec![],
        rng: seed,
    };
    cx.eval_scope(None, None, &mut HashSet::new())?;
    let mut indicators: Vec<_> = cx.indicators.into_iter().collect();
    indicators.sort_by_key(|(id, _)| *id);
    Ok(RunResult { outputs: cx.env, indicators })
}

struct Ctx<'a> {
    vi: &'a Vi,
    ctl: &'a RunControl,
    types: &'a TypeMap,
    env: HashMap<(NodeId, usize), Value>,
    indicators: HashMap<NodeId, Value>,
    /// Values gathered by auto-indexed wires that left a loop, keyed by the wire's destination.
    tun: HashMap<(NodeId, usize), Value>,
    feedback: HashMap<NodeId, Value>,
    /// Last seen value of each control an Event structure watches.
    event_last: HashMap<(NodeId, NodeId), Value>,
    /// Active loops and their current iteration, outermost first.
    iter: Vec<(NodeId, i64)>,
    rng: u64,
}

/// Stack per-iteration values into an array (adding a leading dimension).
fn stack_values(vals: Vec<Value>) -> Value {
    let n = vals.len();
    let mut dims = vec![n];
    let mut data = Vec::new();
    for (i, v) in vals.into_iter().enumerate() {
        match v {
            Value::Array(a) => {
                if i == 0 {
                    dims.extend(a.dims.iter().copied());
                }
                data.extend(a.data);
            }
            other => data.push(other),
        }
    }
    Value::Array(ArrayVal { dims, data })
}

impl<'a> Ctx<'a> {
    fn check_abort(&self) -> Result<(), RunError> {
        if self.ctl.abort.load(Ordering::Relaxed) { Err(RunError::Aborted) } else { Ok(()) }
    }

    fn fork(&self, salt: u64) -> Ctx<'a> {
        Ctx {
            vi: self.vi,
            ctl: self.ctl,
            types: self.types,
            env: self.env.clone(),
            indicators: HashMap::new(),
            tun: self.tun.clone(),
            feedback: self.feedback.clone(),
            event_last: self.event_last.clone(),
            iter: self.iter.clone(),
            rng: self.rng.rotate_left(17 * (salt as u32 + 1)) | 1,
        }
    }

    fn merge(&mut self, child: Ctx<'a>) {
        self.env.extend(child.env);
        self.indicators.extend(child.indicators);
        self.tun.extend(child.tun);
        self.feedback.extend(child.feedback);
        self.event_last.extend(child.event_last);
    }

    /// The child of `scope` that contains (or is) `s`; `None` if `s` is outside `scope`.
    fn lift(&self, s: NodeId, scope: Option<NodeId>) -> Option<NodeId> {
        let mut cur = s;
        for _ in 0..1000 {
            let p = self.vi.node(cur)?.parent;
            if p == scope {
                return if Some(cur) == scope { None } else { Some(cur) };
            }
            cur = p?;
        }
        None
    }

    /// Sibling nodes that must finish before `x` (and everything inside it) can start.
    fn deps(&self, x: NodeId, scope: Option<NodeId>) -> Vec<NodeId> {
        let inside = self.vi.descendants(x);
        let mut out = vec![];
        for w in &self.vi.wires {
            if w.to.0 != x && !inside.contains(&w.to.0) {
                continue;
            }
            if let Some(c) = self.lift(w.from.0, scope) {
                if c != x {
                    out.push(c);
                }
            }
        }
        out
    }

    /// Run the nodes of `scope` (restricted to one `frame` if given) until all have fired.
    /// Ready structures run concurrently.
    fn eval_scope(&mut self, scope: Option<NodeId>, frame: Option<usize>, done: &mut HashSet<NodeId>) -> Result<(), RunError> {
        let mut remaining: Vec<NodeId> = self.vi.nodes.iter().filter(|n| n.parent == scope && frame.is_none_or(|f| n.frame == f)).map(|n| n.id).collect();
        // Feedback nodes present last iteration's value up front and store the new one at the end.
        let fbs: Vec<NodeId> = remaining.iter().copied().filter(|id| matches!(self.vi.node(*id).map(|n| &n.kind), Some(NodeKind::Feedback { .. }))).collect();
        remaining.retain(|id| !fbs.contains(id));
        for id in &fbs {
            let Some(NodeKind::Feedback { ty }) = self.vi.node(*id).map(|n| &n.kind) else { continue };
            let v = match self.feedback.get(id) {
                Some(v) => v.clone(),
                None => self.read_input(*id, 1, Some(ty)).ok().flatten().unwrap_or_else(|| Value::default_for(ty)),
            };
            self.feedback.insert(*id, v.clone());
            self.env.insert((*id, 0), v);
            done.insert(*id);
        }
        while !remaining.is_empty() {
            self.check_abort()?;
            let ready: Vec<NodeId> = remaining.iter().copied().filter(|id| self.deps(*id, scope).iter().all(|d| done.contains(d))).collect();
            if ready.is_empty() {
                return Err(RunError::Cycle);
            }
            let (structs, plain): (Vec<NodeId>, Vec<NodeId>) = ready.into_iter().partition(|id| self.vi.node(*id).is_some_and(|n| n.kind.is_structure()));
            for id in plain {
                self.fire(id)?;
                done.insert(id);
            }
            if structs.len() >= 2 {
                self.fire_parallel(&structs)?;
                done.extend(structs.iter().copied());
            } else {
                for id in structs {
                    self.fire(id)?;
                    done.insert(id);
                }
            }
            remaining.retain(|id| !done.contains(id));
        }
        for id in &fbs {
            let Some(NodeKind::Feedback { ty }) = self.vi.node(*id).map(|n| &n.kind) else { continue };
            if let Some(v) = self.read_input(*id, 0, Some(ty))? {
                self.feedback.insert(*id, v);
            }
        }
        Ok(())
    }

    fn fire_parallel(&mut self, ids: &[NodeId]) -> Result<(), RunError> {
        let results: Vec<(Result<(), RunError>, Ctx<'a>)> = std::thread::scope(|s| {
            let handles: Vec<_> = ids
                .iter()
                .enumerate()
                .map(|(i, &id)| {
                    let mut child = self.fork(i as u64);
                    s.spawn(move || {
                        let r = child.fire(id);
                        if r.is_err() {
                            child.ctl.abort.store(true, Ordering::Relaxed);
                        }
                        (r, child)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("structure thread panicked")).collect()
        });
        let mut first_err: Option<RunError> = None;
        for (r, child) in results {
            self.merge(child);
            if let Err(e) = r {
                if first_err.is_none() || first_err == Some(RunError::Aborted) {
                    first_err = Some(e);
                }
            }
        }
        first_err.map_or(Ok(()), Err)
    }

    fn live_value(&self, id: NodeId) -> Value {
        let node = self.vi.node(id);
        let default = match node.map(|n| &n.kind) {
            Some(NodeKind::Control { ty, .. }) => Value::default_for(ty),
            _ => Value::Bool(false),
        };
        self.ctl.live_controls.lock().ok().and_then(|m| m.get(&id).cloned()).or_else(|| node.and_then(|n| n.value.clone())).unwrap_or(default)
    }

    /// The raw value on a wire, before auto-index slicing.
    fn base_value(&self, w: &Wire) -> Result<Value, RunError> {
        let src = self.vi.node(w.from.0).ok_or_else(|| RunError::Internal("dangling wire".into()))?;
        if matches!(src.kind, NodeKind::Control { .. }) {
            return Ok(self.live_value(src.id));
        }
        if w.auto_index {
            if let Some(v) = self.tun.get(&w.to) {
                return Ok(v.clone());
            }
        }
        self.env.get(&w.from).cloned().ok_or_else(|| RunError::Internal(format!("node {} not ready", w.from.0)))
    }

    /// Hand out one element per iteration for each auto-indexed loop the wire has entered so far.
    fn apply_entering(&self, w: &Wire, mut v: Value) -> Value {
        if !w.auto_index {
            return v;
        }
        let (_, entering) = typing::crossing_loops(self.vi, w.from.0, w.to.0);
        for l in entering {
            let Some((_, i)) = self.iter.iter().find(|(id, _)| *id == l) else { break };
            let elem_default = match &v {
                Value::Array(a) => a.data.first().map(|e| match e {
                    Value::Int(_) => Value::Int(0),
                    Value::Bool(_) => Value::Bool(false),
                    Value::Str(_) => Value::Str(String::new()),
                    _ => Value::Dbl(0.0),
                }),
                _ => None,
            };
            let out_ty = match &elem_default {
                Some(Value::Int(_)) => DataType::I32,
                Some(Value::Bool(_)) => DataType::Bool,
                Some(Value::Str(_)) => DataType::Str,
                _ => DataType::DBL,
            };
            v = ops::index_array(&v, &[*i], &out_ty);
        }
        v
    }

    /// Value on the wire into `(id, idx)`, optionally coerced to `ty`.
    fn read_input(&self, id: NodeId, idx: usize, ty: Option<&DataType>) -> Result<Option<Value>, RunError> {
        let Some(w) = self.vi.wires.iter().find(|w| w.to == (id, idx)) else { return Ok(None) };
        let raw = self.apply_entering(w, self.base_value(w)?);
        match ty {
            Some(t) => raw.coerce(t).map(Some).ok_or_else(|| RunError::Internal("type mismatch on wire".into())),
            None => Ok(Some(raw)),
        }
    }

    fn fire(&mut self, id: NodeId) -> Result<(), RunError> {
        let node = self.vi.node(id).ok_or_else(|| RunError::Internal("missing node".into()))?;
        let kind = node.kind.clone();
        match &kind {
            NodeKind::Loop { kind: lk, regs } => return self.fire_loop(id, *lk, regs),
            NodeKind::Case { labels, default, outs, fixed } => return self.fire_case(id, labels, *default, outs, *fixed),
            NodeKind::Sequence { frames } => {
                let mut done = HashSet::new();
                for f in 0..*frames {
                    self.eval_scope(Some(id), Some(f), &mut done)?;
                }
                return Ok(());
            }
            NodeKind::Event { frames, outs } => return self.fire_event(id, frames, outs),
            _ => {}
        }
        let mut args: Vec<Option<Value>> = Vec::new();
        for (i, t) in kind.inputs().iter().enumerate() {
            let coerce = (t.spec == Spec::Exact).then_some(&t.ty);
            match self.read_input(id, i, coerce)? {
                Some(v) => args.push(Some(v)),
                None if t.optional => args.push(None),
                None => return Err(RunError::Unwired(id, kind.title(), t.name.clone())),
            }
        }
        if matches!(kind, NodeKind::Indicator { .. }) {
            if let Some(v) = args[0].take() {
                (self.ctl.on_indicator)(id, &v);
                self.indicators.insert(id, v);
            }
            return Ok(());
        }
        let own = node.value.clone();
        for (i, v) in self.compute(id, &kind, own.as_ref(), &args)?.into_iter().enumerate() {
            self.env.insert((id, i), v);
        }
        Ok(())
    }

    /// Auto-indexed wires entering and leaving loop `id`.
    fn auto_wires(&self, id: NodeId) -> (Vec<Wire>, Vec<Wire>) {
        let (mut entering, mut leaving) = (vec![], vec![]);
        for w in self.vi.wires.iter().filter(|w| w.auto_index) {
            let (l, e) = typing::crossing_loops(self.vi, w.from.0, w.to.0);
            if e.contains(&id) {
                entering.push(*w);
            }
            if l.contains(&id) {
                leaving.push(*w);
            }
        }
        (entering, leaving)
    }

    fn fire_loop(&mut self, id: NodeId, kind: LoopKind, regs: &[DataType]) -> Result<(), RunError> {
        let (entering, leaving) = self.auto_wires(id);
        for d in self.vi.descendants(id) {
            self.feedback.remove(&d);
        }
        for w in &leaving {
            self.tun.remove(&w.to);
        }
        let count = match kind {
            LoopKind::For => {
                let mut c = match self.read_input(id, 0, Some(&DataType::I32))? {
                    Some(Value::Int(n)) => Some(n),
                    _ => None,
                };
                for w in &entering {
                    if let Value::Array(a) = self.apply_entering(w, self.base_value(w)?) {
                        let len = a.dims.first().copied().unwrap_or(0) as i64;
                        c = Some(c.map_or(len, |x| x.min(len)));
                    }
                }
                c.ok_or_else(|| RunError::Unwired(id, "For Loop".into(), "N".into()))?
            }
            LoopKind::While => i64::MAX,
        };
        let mut state = Vec::with_capacity(regs.len());
        for (k, ty) in regs.iter().enumerate() {
            state.push(self.read_input(id, 1 + 2 * k, Some(ty))?.unwrap_or(Value::default_for(ty)));
        }
        let mut gathered: Vec<Vec<Value>> = vec![vec![]; leaving.len()];
        self.iter.push((id, 0));
        let result = self.loop_body(id, kind, regs, count, &mut state, &leaving, &mut gathered);
        self.iter.pop();
        result?;
        for (w, vals) in leaving.iter().zip(gathered) {
            self.tun.insert(w.to, stack_values(vals));
        }
        for (k, s) in state.into_iter().enumerate() {
            self.env.insert((id, 2 + 2 * k), s);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn loop_body(&mut self, id: NodeId, kind: LoopKind, regs: &[DataType], count: i64, state: &mut [Value], leaving: &[Wire], gathered: &mut [Vec<Value>]) -> Result<(), RunError> {
        let mut i = 0i64;
        while i < count {
            self.check_abort()?;
            if i >= ITERATION_LIMIT {
                return Err(RunError::IterationLimit(id));
            }
            if let Some(top) = self.iter.last_mut() {
                top.1 = i;
            }
            self.env.insert((id, 0), Value::Int(i));
            for (k, s) in state.iter().enumerate() {
                self.env.insert((id, 1 + 2 * k), s.clone());
            }
            self.eval_scope(Some(id), None, &mut HashSet::new())?;
            for (k, ty) in regs.iter().enumerate() {
                if let Some(v) = self.read_input(id, 2 + 2 * k, Some(ty))? {
                    state[k] = v;
                }
            }
            for (w, g) in leaving.iter().zip(gathered.iter_mut()) {
                g.push(self.base_value(w)?);
            }
            i += 1;
            if kind == LoopKind::While {
                match self.read_input(id, 0, Some(&DataType::Bool))? {
                    Some(Value::Bool(true)) => break,
                    Some(_) => {}
                    None => return Err(RunError::Unwired(id, "While Loop".into(), "stop".into())),
                }
            }
        }
        Ok(())
    }

    /// Which frame of a Case structure runs for the wired selector.
    fn case_frame(&self, id: NodeId, labels: &[String], default: Option<usize>, fixed: Option<usize>) -> Result<usize, RunError> {
        if let Some(f) = fixed {
            return Ok(f.min(labels.len().saturating_sub(1)));
        }
        let sel = self.read_input(id, 0, None)?.ok_or_else(|| RunError::Unwired(id, "Case Structure".into(), "?".into()))?;
        let enum_items = match self.types.input(self.vi, id, 0) {
            Some(DataType::Enum(items)) => Some(items),
            _ => None,
        };
        let (key, num): (String, Option<i64>) = match (&sel, &enum_items) {
            (Value::Bool(b), _) => ((if *b { "True" } else { "False" }).into(), None),
            (Value::Int(i), Some(items)) => (items.get(*i as usize).cloned().unwrap_or_default(), Some(*i)),
            (Value::Int(i), None) => (i.to_string(), Some(*i)),
            (Value::Dbl(d), _) => ((d.round() as i64).to_string(), Some(d.round() as i64)),
            (Value::Str(s), _) => (s.clone(), None),
            _ => (String::new(), None),
        };
        let hit = labels.iter().position(|l| label_matches(l, &key, num));
        Ok(hit.or(default).unwrap_or(0))
    }

    fn fire_case(&mut self, id: NodeId, labels: &[String], default: Option<usize>, outs: &[DataType], fixed: Option<usize>) -> Result<(), RunError> {
        let frame = self.case_frame(id, labels, default, fixed)?;
        self.eval_scope(Some(id), Some(frame), &mut HashSet::new())?;
        for (k, ty) in outs.iter().enumerate() {
            let v = self.read_input(id, NodeKind::tunnel_in_idx(k, frame), Some(ty))?.unwrap_or_else(|| Value::default_for(ty));
            self.env.insert((id, k), v);
        }
        Ok(())
    }

    fn fire_event(&mut self, id: NodeId, frames: &[EventFrame], outs: &[DataType]) -> Result<(), RunError> {
        let timeout = self.read_input(id, 0, Some(&DataType::I32))?.and_then(|v| v.as_i64()).unwrap_or(-1);
        let start = Instant::now();
        let chosen: Option<(usize, Option<Value>)> = loop {
            self.check_abort()?;
            let mut hit = None;
            for (f, fr) in frames.iter().enumerate() {
                if let Some(src) = fr.source {
                    let cur = self.live_value(src);
                    let last = self.event_last.entry((id, src)).or_insert_with(|| cur.clone());
                    if *last != cur {
                        *last = cur.clone();
                        hit = Some((f, Some(cur)));
                        break;
                    }
                }
            }
            if hit.is_some() {
                break hit;
            }
            if timeout >= 0 && start.elapsed() >= Duration::from_millis(timeout as u64) {
                break frames.iter().position(|f| f.source.is_none()).map(|f| (f, None));
            }
            std::thread::sleep(Duration::from_millis(4));
        };
        let out_base = MAX_FRAMES;
        match chosen {
            Some((f, data)) => {
                if let Some(v) = data {
                    self.env.insert((id, f), v);
                }
                self.eval_scope(Some(id), Some(f), &mut HashSet::new())?;
                for (k, ty) in outs.iter().enumerate() {
                    let v = self.read_input(id, NodeKind::tunnel_in_idx(k, f), Some(ty))?.unwrap_or_else(|| Value::default_for(ty));
                    self.env.insert((id, out_base + k), v);
                }
            }
            None => {
                for (k, ty) in outs.iter().enumerate() {
                    self.env.insert((id, out_base + k), Value::default_for(ty));
                }
            }
        }
        Ok(())
    }

    fn next_rand(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn compute(&mut self, id: NodeId, kind: &NodeKind, own: Option<&Value>, a: &[Option<Value>]) -> Result<Vec<Value>, RunError> {
        let outs = self.types.outs(id).to_vec();
        let out0 = outs.first().cloned().unwrap_or(DataType::DBL);
        let req = |i: usize| a.get(i).and_then(|v| v.as_ref()).ok_or_else(|| RunError::Internal("missing input".into()));
        let opt = |i: usize| a.get(i).and_then(|v| v.as_ref());
        let f0 = |i: usize| req(i).map(|v| v.as_f64().unwrap_or(0.0));
        let bin = |op| ops::arith(op, req(0)?, req(1)?, &out0, id);
        Ok(match kind {
            NodeKind::Control { ty, .. } => {
                let live = self.ctl.live_controls.lock().ok().and_then(|m| m.get(&id).cloned());
                vec![live.or_else(|| own.cloned()).unwrap_or(Value::default_for(ty))]
            }
            NodeKind::Constant(ty) => vec![own.cloned().unwrap_or_else(|| Value::default_for(ty))],
            NodeKind::Indicator { .. } | NodeKind::Loop { .. } | NodeKind::Case { .. } | NodeKind::Sequence { .. } | NodeKind::Event { .. } | NodeKind::Feedback { .. } => vec![],
            NodeKind::Local { target, ty, write } => {
                let Some(t) = target else { return Ok(if *write { vec![] } else { vec![Value::default_for(ty)] }) };
                let tnode = self.vi.node(*t).ok_or_else(|| RunError::Internal("local variable target missing".into()))?;
                if *write {
                    let v = req(0)?.clone();
                    match tnode.kind {
                        NodeKind::Control { .. } => {
                            if let Ok(mut m) = self.ctl.live_controls.lock() {
                                m.insert(*t, v.clone());
                            }
                            (self.ctl.on_control_write)(*t, &v);
                        }
                        _ => {
                            (self.ctl.on_indicator)(*t, &v);
                            self.indicators.insert(*t, v);
                        }
                    }
                    vec![]
                } else {
                    let v = match tnode.kind {
                        NodeKind::Control { .. } => self.live_value(*t),
                        _ => self.indicators.get(t).cloned().or_else(|| tnode.value.clone()).unwrap_or_else(|| Value::default_for(ty)),
                    };
                    vec![v.coerce(ty).unwrap_or(v)]
                }
            }
            NodeKind::Global { name, ty, write } => {
                let mut g = self.ctl.globals.lock().map_err(|_| RunError::Internal("globals poisoned".into()))?;
                if *write {
                    g.insert(name.clone(), req(0)?.clone());
                    vec![]
                } else {
                    vec![g.get(name).and_then(|v| v.coerce(ty)).unwrap_or_else(|| Value::default_for(ty))]
                }
            }
            NodeKind::Decoration { .. } => vec![],
            NodeKind::Property { target, prop, write } => {
                let Some(t) = target else { return Ok(if *write { vec![] } else { vec![Value::default_for(&prop.ty())] }) };
                if *write {
                    let v = req(0)?.clone();
                    if let Ok(mut m) = self.ctl.properties.lock() {
                        m.insert((*t, *prop), v.clone());
                    }
                    (self.ctl.on_property)(*t, *prop, &v);
                    vec![]
                } else {
                    let written = self.ctl.properties.lock().ok().and_then(|m| m.get(&(*t, *prop)).cloned());
                    let tn = self.vi.node(*t);
                    let v = written.unwrap_or_else(|| match (prop, tn) {
                        (Prop::Visible, Some(n)) => Value::Bool(n.visible),
                        (Prop::Disabled, Some(n)) => Value::Bool(n.disabled),
                        (Prop::Caption, Some(n)) => Value::Str(n.caption.clone().unwrap_or_else(|| n.kind.title())),
                        _ => Value::default_for(&prop.ty()),
                    });
                    vec![v]
                }
            }
            NodeKind::BuildWaveform => {
                let t0 = opt(0).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let dt = opt(1).and_then(|v| v.as_f64()).unwrap_or(1.0);
                vec![Value::Cluster(vec![Value::Dbl(t0), Value::Dbl(dt), req(2)?.clone()])]
            }
            NodeKind::GetWaveform => match req(0)? {
                Value::Cluster(v) if v.len() == 3 => v.clone(),
                _ => vec![],
            },
            NodeKind::Add => vec![bin(BinOp::Add)?],
            NodeKind::Subtract => vec![bin(BinOp::Sub)?],
            NodeKind::Multiply => vec![bin(BinOp::Mul)?],
            NodeKind::Divide => vec![bin(BinOp::Div)?],
            NodeKind::Increment => vec![ops::unary(Unary::Inc, req(0)?, &out0)?],
            NodeKind::Decrement => vec![ops::unary(Unary::Dec, req(0)?, &out0)?],
            NodeKind::Negate => vec![ops::unary(Unary::Neg, req(0)?, &out0)?],
            NodeKind::Absolute => vec![ops::unary(Unary::Abs, req(0)?, &out0)?],
            NodeKind::SquareRoot => vec![ops::unary(Unary::Sqrt, req(0)?, &out0)?],
            NodeKind::Convert(_) => vec![req(0)?.coerce(&out0).ok_or_else(|| RunError::Internal("cannot convert".into()))?],
            NodeKind::And => vec![ops::logic(Logic::And, req(0)?, req(1)?)?],
            NodeKind::Or => vec![ops::logic(Logic::Or, req(0)?, req(1)?)?],
            NodeKind::Xor => vec![ops::logic(Logic::Xor, req(0)?, req(1)?)?],
            NodeKind::Not => vec![ops::not(req(0)?)?],
            NodeKind::Greater => vec![ops::compare(Cmp::Gt, req(0)?, req(1)?)?],
            NodeKind::Less => vec![ops::compare(Cmp::Lt, req(0)?, req(1)?)?],
            NodeKind::GreaterEq => vec![ops::compare(Cmp::Ge, req(0)?, req(1)?)?],
            NodeKind::LessEq => vec![ops::compare(Cmp::Le, req(0)?, req(1)?)?],
            NodeKind::Equal => vec![ops::compare(Cmp::Eq, req(0)?, req(1)?)?],
            NodeKind::NotEq => vec![ops::compare(Cmp::Ne, req(0)?, req(1)?)?],
            NodeKind::Select => {
                let chosen = if matches!(req(0)?, Value::Bool(true)) { req(1)? } else { req(2)? };
                vec![chosen.coerce(&out0).unwrap_or_else(|| chosen.clone())]
            }
            NodeKind::Concatenate => match (req(0)?, req(1)?) {
                (Value::Str(x), Value::Str(y)) => vec![Value::Str(format!("{x}{y}"))],
                _ => vec![],
            },
            NodeKind::NumToString => vec![Value::Str(match req(0)? {
                Value::Int(i) => i.to_string(),
                v => v.as_f64().unwrap_or(0.0).to_string(),
            })],
            NodeKind::Sine => vec![Value::Dbl(f0(0)?.sin())],
            NodeKind::Random => vec![Value::Dbl(self.next_rand())],
            NodeKind::WaitMs => {
                let mut left = req(0)?.as_i64().unwrap_or(0).max(0) as u64;
                while left > 0 {
                    self.check_abort()?;
                    let step = left.min(10);
                    std::thread::sleep(Duration::from_millis(step));
                    left -= step;
                }
                vec![]
            }
            NodeKind::BuildArray => {
                let base = opt(0).cloned().and_then(|v| v.coerce(&out0)).unwrap_or_else(|| Value::default_for(&out0));
                vec![ops::array_append(base, req(1)?, &out0)]
            }
            NodeKind::ArraySize => match req(0)? {
                Value::Array(arr) if arr.dims.len() == 1 => vec![Value::Int(arr.dims[0] as i64)],
                Value::Array(arr) => vec![Value::array1(arr.dims.iter().map(|d| Value::Int(*d as i64)).collect())],
                _ => vec![],
            },
            NodeKind::IndexArray => {
                let idxs: Vec<i64> = [opt(1), opt(2)].into_iter().flatten().map(|v| v.as_i64().unwrap_or(-1)).collect();
                vec![ops::index_array(req(0)?, &idxs, &out0)]
            }
            NodeKind::InitializeArray => {
                let size = opt(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                vec![ops::initialize_array(req(0)?, size, &out0)]
            }
            NodeKind::Transpose => vec![ops::transpose(req(0)?)],
            NodeKind::Bundle { n } => {
                let mut cluster = opt(0).cloned().and_then(|v| v.coerce(&out0)).unwrap_or_else(|| Value::default_for(&out0));
                if let (Value::Cluster(vals), DataType::Cluster(fields)) = (&mut cluster, &out0) {
                    for i in 0..*n {
                        if let (Some(v), Some(f)) = (opt(1 + i), fields.get(i)) {
                            vals[i] = v.coerce(&f.ty).unwrap_or_else(|| v.clone());
                        }
                    }
                }
                vec![cluster]
            }
            NodeKind::Unbundle { n } => match req(0)? {
                Value::Cluster(vals) => (0..*n).map(|i| vals.get(i).cloned().unwrap_or_else(|| Value::default_for(outs.get(i).unwrap_or(&DataType::DBL)))).collect(),
                _ => vec![],
            },
            NodeKind::BundleByName { names } => {
                let mut cluster = req(0)?.clone();
                if let (Value::Cluster(vals), DataType::Cluster(fields)) = (&mut cluster, &out0) {
                    for (i, name) in names.iter().enumerate() {
                        if let (Some(v), Some(p)) = (opt(1 + i), fields.iter().position(|f| &f.name == name)) {
                            vals[p] = v.coerce(&fields[p].ty).unwrap_or_else(|| v.clone());
                        }
                    }
                }
                vec![cluster]
            }
            NodeKind::UnbundleByName { names } => {
                let src_ty = self.types.input(self.vi, id, 0);
                match (req(0)?, src_ty) {
                    (Value::Cluster(vals), Some(DataType::Cluster(fields))) => names
                        .iter()
                        .enumerate()
                        .map(|(i, n)| fields.iter().position(|f| &f.name == n).and_then(|p| vals.get(p).cloned()).unwrap_or_else(|| Value::default_for(outs.get(i).unwrap_or(&DataType::DBL))))
                        .collect(),
                    _ => vec![],
                }
            }
            NodeKind::ErrorFromCode => {
                let code = req(0)?.as_i64().unwrap_or(0);
                let source = match opt(1) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                vec![Value::Cluster(vec![Value::Bool(code != 0), Value::Int(code), Value::Str(source)])]
            }
            NodeKind::MergeErrors => {
                let none = Value::default_for(&DataType::error_cluster());
                let is_err = |v: &Value| matches!(v, Value::Cluster(f) if matches!(f.first(), Some(Value::Bool(true))));
                let (x, y) = (opt(0).cloned().unwrap_or_else(|| none.clone()), opt(1).cloned().unwrap_or_else(|| none.clone()));
                vec![if is_err(&x) || !is_err(&y) { x } else { y }]
            }
        })
    }
}

/// Does a Case frame label match the selector? Supports exact text, integers, `a..b` ranges and lists.
fn label_matches(label: &str, key: &str, num: Option<i64>) -> bool {
    let label = label.trim();
    if label == key {
        return true;
    }
    let Some(n) = num else { return false };
    label.split(',').any(|part| {
        let part = part.trim();
        if let Some((lo, hi)) = part.split_once("..") {
            let lo = lo.trim().parse::<i64>().unwrap_or(i64::MIN);
            let hi = hi.trim().parse::<i64>().unwrap_or(i64::MAX);
            (lo..=hi).contains(&n)
        } else {
            part.parse::<i64>() == Ok(n)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Command, EditError, Editor};
    use crate::types::{ArrayVal, Field, NumKind};
    use crate::typing::{coercion_dots, resolve};

    fn mk(ed: &mut Editor, kind: NodeKind, pos: [f32; 2]) -> NodeId {
        ed.apply(Command::AddNode { kind, pos, widget: None }).unwrap().unwrap()
    }
    fn konst(ed: &mut Editor, ty: DataType, v: Value) -> NodeId {
        let id = mk(ed, NodeKind::Constant(ty), [-900.0, -900.0]);
        ed.apply(Command::SetValue { id, value: v }).unwrap();
        id
    }
    fn wire(ed: &mut Editor, from: (NodeId, usize), to: (NodeId, usize)) {
        ed.apply(Command::Connect { from, to }).unwrap();
    }
    fn ind(ed: &mut Editor, ty: DataType) -> NodeId {
        mk(ed, NodeKind::Indicator { name: "out".into(), ty }, [-900.0, 0.0])
    }
    fn ctl(name: &str, ty: DataType) -> NodeKind {
        NodeKind::Control { name: name.into(), ty }
    }
    fn new_ed() -> Editor {
        Editor::new(Vi::new("t"))
    }
    fn result_of(ed: &Editor, id: NodeId) -> Value {
        run(&ed.vi).unwrap().indicators.into_iter().find(|(i, _)| *i == id).unwrap().1
    }
    fn k(kind: NumKind) -> DataType {
        DataType::Num(kind)
    }
    fn arr(v: &[f64]) -> Value {
        Value::from_f64_vec(v)
    }

    #[test]
    fn adds_and_undo_and_json() {
        let mut ed = new_ed();
        let a = konst(&mut ed, DataType::DBL, Value::Dbl(2.0));
        let b = konst(&mut ed, DataType::DBL, Value::Dbl(3.5));
        let add = mk(&mut ed, NodeKind::Add, [0.0; 2]);
        let out = ind(&mut ed, DataType::DBL);
        wire(&mut ed, (a, 0), (add, 0));
        wire(&mut ed, (b, 0), (add, 1));
        wire(&mut ed, (add, 0), (out, 0));
        assert_eq!(result_of(&ed, out), Value::Dbl(5.5));
        assert_eq!(Vi::from_json(&ed.vi.to_json()).unwrap().nodes.len(), ed.vi.nodes.len());
        let n = ed.vi.nodes.len();
        mk(&mut ed, NodeKind::Sine, [0.0; 2]);
        ed.undo().unwrap();
        assert_eq!(ed.vi.nodes.len(), n);
    }

    #[test]
    fn integer_arithmetic_wraps_and_mixing_promotes() {
        let mut ed = new_ed();
        let a = konst(&mut ed, DataType::U8, Value::Int(200));
        let b = konst(&mut ed, DataType::U8, Value::Int(100));
        let add = mk(&mut ed, NodeKind::Add, [0.0; 2]);
        let out = ind(&mut ed, DataType::U8);
        wire(&mut ed, (a, 0), (add, 0));
        wire(&mut ed, (b, 0), (add, 1));
        wire(&mut ed, (add, 0), (out, 0));
        assert_eq!(result_of(&ed, out), Value::Int(44)); // 300 wraps in U8

        let i = konst(&mut ed, DataType::I32, Value::Int(3));
        let d = konst(&mut ed, DataType::DBL, Value::Dbl(0.5));
        let add2 = mk(&mut ed, NodeKind::Add, [0.0; 2]);
        let out2 = ind(&mut ed, DataType::DBL);
        wire(&mut ed, (i, 0), (add2, 0));
        wire(&mut ed, (d, 0), (add2, 1));
        wire(&mut ed, (add2, 0), (out2, 0));
        assert_eq!(result_of(&ed, out2), Value::Dbl(3.5));
        let tm = resolve(&ed.vi);
        assert_eq!(tm.out(add2, 0), Some(&DataType::DBL));
        assert!(coercion_dots(&ed.vi, &tm).contains(&(add2, 0)));
        assert!(!coercion_dots(&ed.vi, &tm).contains(&(add2, 1)));
    }

    #[test]
    fn float_into_narrow_indicator_saturates() {
        let mut ed = new_ed();
        let a = konst(&mut ed, DataType::DBL, Value::Dbl(300.7));
        let conv = mk(&mut ed, NodeKind::Convert(NumKind::U8), [0.0; 2]);
        let out = ind(&mut ed, DataType::U8);
        wire(&mut ed, (a, 0), (conv, 0));
        wire(&mut ed, (conv, 0), (out, 0));
        assert_eq!(result_of(&ed, out), Value::Int(255));
    }

    #[test]
    fn array_scalar_broadcast_and_array_array() {
        let mut ed = new_ed();
        let a = konst(&mut ed, DataType::arr_dbl(), arr(&[1.0, 2.0, 3.0]));
        let s = konst(&mut ed, DataType::DBL, Value::Dbl(2.0));
        let mul = mk(&mut ed, NodeKind::Multiply, [0.0; 2]);
        let out = ind(&mut ed, DataType::arr_dbl());
        wire(&mut ed, (a, 0), (mul, 0));
        wire(&mut ed, (s, 0), (mul, 1));
        wire(&mut ed, (mul, 0), (out, 0));
        assert_eq!(result_of(&ed, out), arr(&[2.0, 4.0, 6.0]));
        // array + array, truncating to the shorter length
        let b = konst(&mut ed, DataType::arr_dbl(), arr(&[10.0, 20.0]));
        let add = mk(&mut ed, NodeKind::Add, [0.0; 2]);
        let out2 = ind(&mut ed, DataType::arr_dbl());
        wire(&mut ed, (a, 0), (add, 0));
        wire(&mut ed, (b, 0), (add, 1));
        wire(&mut ed, (add, 0), (out2, 0));
        assert_eq!(result_of(&ed, out2), arr(&[11.0, 22.0]));
    }

    #[test]
    fn rejects_bad_types() {
        let mut ed = new_ed();
        let s = konst(&mut ed, DataType::Str, Value::Str("x".into()));
        let add = mk(&mut ed, NodeKind::Add, [0.0; 2]);
        assert!(matches!(ed.apply(Command::Connect { from: (s, 0), to: (add, 0) }), Err(EditError::TypeMismatch(..))));
        // array can't feed a scalar-only terminal
        let a = konst(&mut ed, DataType::arr_dbl(), arr(&[1.0]));
        let sine = mk(&mut ed, NodeKind::Sine, [0.0; 2]);
        assert!(matches!(ed.apply(Command::Connect { from: (a, 0), to: (sine, 0) }), Err(EditError::TypeMismatch(..))));
        // string array vs numeric operand mismatch
        let b = konst(&mut ed, DataType::Bool, Value::Bool(true));
        let and = mk(&mut ed, NodeKind::And, [0.0; 2]);
        ed.apply(Command::Connect { from: (b, 0), to: (and, 0) }).unwrap();
        assert!(ed.apply(Command::Connect { from: (a, 0), to: (and, 1) }).is_err());
    }

    #[test]
    fn clusters_bundle_unbundle_by_name() {
        let mut ed = new_ed();
        let cty = DataType::Cluster(vec![
            Field { name: "temp".into(), ty: DataType::DBL },
            Field { name: "count".into(), ty: DataType::U8 },
            Field { name: "label".into(), ty: DataType::Str },
        ]);
        let c = konst(&mut ed, cty.clone(), Value::Cluster(vec![Value::Dbl(21.5), Value::Int(7), Value::Str("probe".into())]));
        let un = mk(&mut ed, NodeKind::UnbundleByName { names: vec!["label".into(), "temp".into()] }, [0.0; 2]);
        wire(&mut ed, (c, 0), (un, 0));
        let o1 = ind(&mut ed, DataType::Str);
        let o2 = ind(&mut ed, DataType::DBL);
        wire(&mut ed, (un, 0), (o1, 0));
        wire(&mut ed, (un, 1), (o2, 0));
        // bundle by name: replace count
        let nine = konst(&mut ed, DataType::U8, Value::Int(9));
        let bn = mk(&mut ed, NodeKind::BundleByName { names: vec!["count".into()] }, [0.0; 2]);
        wire(&mut ed, (c, 0), (bn, 0));
        wire(&mut ed, (nine, 0), (bn, 1));
        let o3 = ind(&mut ed, cty);
        wire(&mut ed, (bn, 0), (o3, 0));
        let r = run(&ed.vi).unwrap();
        let get = |id| r.indicators.iter().find(|(i, _)| *i == id).unwrap().1.clone();
        assert_eq!(get(o1), Value::Str("probe".into()));
        assert_eq!(get(o2), Value::Dbl(21.5));
        assert_eq!(get(o3), Value::Cluster(vec![Value::Dbl(21.5), Value::Int(9), Value::Str("probe".into())]));
        // a wrong element type is rejected
        let s = konst(&mut ed, DataType::Str, Value::Str("no".into()));
        assert!(matches!(ed.apply(Command::Connect { from: (s, 0), to: (bn, 1) }), Err(EditError::TypeMismatch(..))));
    }

    #[test]
    fn bundle_builds_cluster_and_unbundle_autosizes() {
        let mut ed = new_ed();
        let x = konst(&mut ed, DataType::DBL, Value::Dbl(1.5));
        let y = konst(&mut ed, DataType::Str, Value::Str("hi".into()));
        let b = mk(&mut ed, NodeKind::Bundle { n: 2 }, [0.0; 2]);
        wire(&mut ed, (x, 0), (b, 1));
        wire(&mut ed, (y, 0), (b, 2));
        let u = mk(&mut ed, NodeKind::Unbundle { n: 0 }, [0.0; 2]);
        wire(&mut ed, (b, 0), (u, 0));
        assert!(matches!(ed.vi.node(u).unwrap().kind, NodeKind::Unbundle { n: 2 }));
        let o = ind(&mut ed, DataType::Str);
        wire(&mut ed, (u, 1), (o, 0));
        assert_eq!(result_of(&ed, o), Value::Str("hi".into()));
    }

    #[test]
    fn two_d_arrays_build_index_transpose() {
        let mut ed = new_ed();
        let row1 = konst(&mut ed, DataType::arr_dbl(), arr(&[1.0, 2.0, 3.0]));
        let row2 = konst(&mut ed, DataType::arr_dbl(), arr(&[4.0, 5.0, 6.0]));
        let b1 = mk(&mut ed, NodeKind::BuildArray, [-900.0, 100.0]);
        wire(&mut ed, (row1, 0), (b1, 1));
        let b2 = mk(&mut ed, NodeKind::BuildArray, [-900.0, 200.0]);
        wire(&mut ed, (b1, 0), (b2, 0));
        wire(&mut ed, (row2, 0), (b2, 1));
        assert_eq!(resolve(&ed.vi).out(b2, 0), Some(&DataType::array(DataType::DBL, 2)));
        let tr = mk(&mut ed, NodeKind::Transpose, [-900.0, 300.0]);
        wire(&mut ed, (b2, 0), (tr, 0));
        let ix = mk(&mut ed, NodeKind::IndexArray, [-900.0, 400.0]);
        wire(&mut ed, (tr, 0), (ix, 0));
        let i0 = konst(&mut ed, DataType::I32, Value::Int(2));
        let i1 = konst(&mut ed, DataType::I32, Value::Int(1));
        wire(&mut ed, (i0, 0), (ix, 1));
        wire(&mut ed, (i1, 0), (ix, 2));
        let o = ind(&mut ed, DataType::DBL);
        wire(&mut ed, (ix, 0), (o, 0));
        // transposed [[1,4],[2,5],[3,6]][2][1] == 6
        assert_eq!(result_of(&ed, o), Value::Dbl(6.0));
        // indexing with only one index returns a row
        let mut ed2 = ed;
        ed2.apply(Command::Disconnect { to: (ix, 2) }).unwrap();
        assert_eq!(resolve(&ed2.vi).out(ix, 0), Some(&DataType::arr_dbl()));
    }

    #[test]
    fn enum_compare_select_and_errors() {
        let mut ed = new_ed();
        let en = DataType::Enum(vec!["Celsius".into(), "Kelvin".into()]);
        let m = konst(&mut ed, en, Value::Int(1));
        let one = konst(&mut ed, DataType::I32, Value::Int(1));
        let eq = mk(&mut ed, NodeKind::Equal, [0.0; 2]);
        wire(&mut ed, (m, 0), (eq, 0));
        wire(&mut ed, (one, 0), (eq, 1));
        let t = konst(&mut ed, DataType::Str, Value::Str("K".into()));
        let f = konst(&mut ed, DataType::Str, Value::Str("C".into()));
        let sel = mk(&mut ed, NodeKind::Select, [0.0; 2]);
        wire(&mut ed, (eq, 0), (sel, 0));
        wire(&mut ed, (t, 0), (sel, 1));
        wire(&mut ed, (f, 0), (sel, 2));
        let o = ind(&mut ed, DataType::Str);
        wire(&mut ed, (sel, 0), (o, 0));
        assert_eq!(result_of(&ed, o), Value::Str("K".into()));
        // error cluster
        let code = konst(&mut ed, DataType::I32, Value::Int(5000));
        let ef = mk(&mut ed, NodeKind::ErrorFromCode, [0.0; 2]);
        wire(&mut ed, (code, 0), (ef, 0));
        let eo = ind(&mut ed, DataType::error_cluster());
        wire(&mut ed, (ef, 0), (eo, 0));
        assert_eq!(result_of(&ed, eo), Value::Cluster(vec![Value::Bool(true), Value::Int(5000), Value::Str(String::new())]));
    }

    #[test]
    fn shift_register_takes_type_from_init_wire() {
        let mut ed = new_ed();
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![DataType::DBL] }, [0.0, 0.0]);
        let init = konst(&mut ed, DataType::U8, Value::Int(250));
        wire(&mut ed, (init, 0), (l, 1));
        assert!(matches!(&ed.vi.node(l).unwrap().kind, NodeKind::Loop { regs, .. } if regs[0] == DataType::U8));
        let n = konst(&mut ed, DataType::I32, Value::Int(10));
        wire(&mut ed, (n, 0), (l, 0));
        let inc = mk(&mut ed, NodeKind::Increment, [100.0, 60.0]);
        wire(&mut ed, (l, 1), (inc, 0));
        wire(&mut ed, (inc, 0), (l, 2));
        let o = ind(&mut ed, DataType::U8);
        wire(&mut ed, (l, 2), (o, 0));
        assert_eq!(result_of(&ed, o), Value::Int(4)); // 250 + 10 wraps in U8
    }

    #[test]
    fn set_type_converts_value_and_prunes_wires() {
        let mut ed = new_ed();
        let c = mk(&mut ed, ctl("x", DataType::DBL), [0.0; 2]);
        let o = ind(&mut ed, DataType::DBL);
        wire(&mut ed, (c, 0), (o, 0));
        ed.apply(Command::SetValue { id: c, value: Value::Dbl(12.6) }).unwrap();
        ed.apply(Command::SetType { id: c, ty: DataType::I32 }).unwrap();
        assert_eq!(ed.vi.node(c).unwrap().value, Some(Value::Int(13)));
        assert_eq!(ed.vi.wires.len(), 1); // I32 still feeds a DBL indicator
        ed.apply(Command::SetType { id: c, ty: DataType::Str }).unwrap();
        assert!(ed.vi.wires.is_empty()); // string can't feed DBL: wire pruned
    }

    #[test]
    fn for_loop_shift_register_sums_iterations() {
        let mut ed = new_ed();
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![DataType::DBL] }, [0.0, 0.0]);
        let n = konst(&mut ed, DataType::I32, Value::Int(5));
        let add = mk(&mut ed, NodeKind::Add, [100.0, 60.0]);
        let out = ind(&mut ed, DataType::DBL);
        assert_eq!(ed.vi.node(add).unwrap().parent, Some(l));
        wire(&mut ed, (n, 0), (l, 0));
        wire(&mut ed, (l, 1), (add, 0));
        wire(&mut ed, (l, 0), (add, 1));
        wire(&mut ed, (add, 0), (l, 2));
        wire(&mut ed, (l, 2), (out, 0));
        assert_eq!(result_of(&ed, out), Value::Dbl(10.0));
    }

    #[test]
    fn while_loop_with_stop_condition_and_tunnels() {
        let mut ed = new_ed();
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
        let c = konst(&mut ed, DataType::DBL, Value::Dbl(2.5));
        let gt = mk(&mut ed, NodeKind::Greater, [100.0, 60.0]);
        let out = ind(&mut ed, DataType::I32);
        wire(&mut ed, (l, 0), (gt, 0));
        wire(&mut ed, (c, 0), (gt, 1));
        wire(&mut ed, (gt, 0), (l, 0));
        let inc = mk(&mut ed, NodeKind::Increment, [100.0, 130.0]);
        wire(&mut ed, (l, 0), (inc, 0));
        wire(&mut ed, (inc, 0), (out, 0));
        assert_eq!(result_of(&ed, out), Value::Int(4)); // last i is 3, incremented
    }

    #[test]
    fn build_array_in_loop_and_auto_typing() {
        let mut ed = new_ed();
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![DataType::arr_dbl()] }, [0.0, 0.0]);
        let n = konst(&mut ed, DataType::I32, Value::Int(4));
        let ba = mk(&mut ed, NodeKind::BuildArray, [100.0, 60.0]);
        let out = ind(&mut ed, DataType::arr_dbl());
        wire(&mut ed, (n, 0), (l, 0));
        wire(&mut ed, (l, 1), (ba, 0));
        wire(&mut ed, (l, 0), (ba, 1));
        wire(&mut ed, (ba, 0), (l, 2));
        wire(&mut ed, (l, 2), (out, 0));
        assert_eq!(result_of(&ed, out), arr(&[0.0, 1.0, 2.0, 3.0]));
    }

    #[test]
    fn inner_terminal_cannot_feed_outside() {
        let mut ed = new_ed();
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [0.0, 0.0]);
        let out = ind(&mut ed, DataType::DBL);
        assert_eq!(ed.apply(Command::Connect { from: (l, 0), to: (out, 0) }), Err(EditError::Scope));
    }

    #[test]
    fn abort_stops_infinite_loop() {
        let mut ed = new_ed();
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
        let f = konst(&mut ed, DataType::Bool, Value::Bool(false));
        wire(&mut ed, (f, 0), (l, 0));
        let ctl = RunControl::headless();
        ctl.abort.store(true, Ordering::Relaxed);
        assert!(matches!(run_with(&ed.vi, &ctl), Err(RunError::Aborted)));
    }

    #[test]
    fn value_coercion_rules() {
        assert_eq!(Value::Dbl(-200.4).coerce(&k(NumKind::I8)), Some(Value::Int(-128)));
        assert_eq!(Value::Dbl(0.1).coerce(&k(NumKind::Sgl)), Some(Value::Dbl(0.1f32 as f64)));
        let a = Value::Array(ArrayVal { dims: vec![2], data: vec![Value::Int(1), Value::Int(2)] });
        assert_eq!(a.coerce(&DataType::arr_dbl()), Some(arr(&[1.0, 2.0])));
        assert!(Value::Str("x".into()).coerce(&DataType::DBL).is_none());
        assert!(DataType::I32.can_feed(&DataType::DBL));
        assert!(!DataType::DBL.can_feed(&DataType::Bool));
    }
}
