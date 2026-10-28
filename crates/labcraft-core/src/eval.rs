use crate::model::{LoopKind, NodeId, NodeKind, Vi};
use crate::types::{DataType, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    pub on_indicator: Box<dyn Fn(NodeId, &Value) + Send + Sync>,
}

impl RunControl {
    pub fn headless() -> Self {
        RunControl {
            abort: Arc::new(AtomicBool::new(false)),
            live_controls: Arc::new(Mutex::new(HashMap::new())),
            on_indicator: Box::new(|_, _| {}),
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

/// Execute a VI in dataflow order, including loops and shift registers.
pub fn run_with(vi: &Vi, ctl: &RunControl) -> Result<RunResult, RunError> {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    let mut cx = Ctx { vi, ctl, env: HashMap::new(), indicators: HashMap::new(), rng: seed };
    cx.eval_scope(None)?;
    let mut indicators: Vec<_> = cx.indicators.into_iter().collect();
    indicators.sort_by_key(|(id, _)| *id);
    Ok(RunResult { outputs: cx.env, indicators })
}

struct Ctx<'a> {
    vi: &'a Vi,
    ctl: &'a RunControl,
    env: HashMap<(NodeId, usize), Value>,
    indicators: HashMap<NodeId, Value>,
    rng: u64,
}

impl Ctx<'_> {
    fn check_abort(&self) -> Result<(), RunError> {
        if self.ctl.abort.load(Ordering::Relaxed) { Err(RunError::Aborted) } else { Ok(()) }
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

    fn eval_scope(&mut self, scope: Option<NodeId>) -> Result<(), RunError> {
        let mut remaining: Vec<NodeId> = self.vi.nodes.iter().filter(|n| n.parent == scope).map(|n| n.id).collect();
        let mut done: HashSet<NodeId> = HashSet::new();
        while !remaining.is_empty() {
            self.check_abort()?;
            let before = remaining.len();
            let mut still = vec![];
            for id in remaining {
                if self.deps(id, scope).iter().any(|d| !done.contains(d)) {
                    still.push(id);
                    continue;
                }
                self.fire(id)?;
                done.insert(id);
            }
            if still.len() == before {
                return Err(RunError::Cycle);
            }
            remaining = still;
        }
        Ok(())
    }

    fn read_input(&self, id: NodeId, idx: usize, ty: DataType) -> Result<Option<Value>, RunError> {
        let Some(w) = self.vi.wires.iter().find(|w| w.to == (id, idx)) else { return Ok(None) };
        let src = self.vi.node(w.from.0).ok_or_else(|| RunError::Internal("dangling wire".into()))?;
        let raw = match &src.kind {
            NodeKind::Control { ty: cty, .. } => {
                let live = self.ctl.live_controls.lock().ok().and_then(|m| m.get(&src.id).cloned());
                live.or_else(|| src.value.clone()).unwrap_or(Value::default_for(*cty))
            }
            _ => self.env.get(&w.from).cloned().ok_or_else(|| RunError::Internal(format!("node {} not ready", w.from.0)))?,
        };
        raw.coerce(ty).map(Some).ok_or_else(|| RunError::Internal("type mismatch on wire".into()))
    }

    fn fire(&mut self, id: NodeId) -> Result<(), RunError> {
        let node = self.vi.node(id).ok_or_else(|| RunError::Internal("missing node".into()))?;
        let kind = node.kind.clone();
        if let NodeKind::Loop { kind: lk, regs } = &kind {
            return self.fire_loop(id, *lk, regs);
        }
        let mut args = Vec::new();
        for (i, t) in kind.inputs().iter().enumerate() {
            match self.read_input(id, i, t.ty)? {
                Some(v) => args.push(v),
                None if t.optional => args.push(Value::default_for(t.ty)),
                None => return Err(RunError::Unwired(id, kind.title(), t.name.clone())),
            }
        }
        if matches!(kind, NodeKind::Indicator { .. }) {
            (self.ctl.on_indicator)(id, &args[0]);
            self.indicators.insert(id, args[0].clone());
            return Ok(());
        }
        let own = node.value.clone();
        for (i, v) in self.compute(id, &kind, own.as_ref(), &args)?.into_iter().enumerate() {
            self.env.insert((id, i), v);
        }
        Ok(())
    }

    fn fire_loop(&mut self, id: NodeId, kind: LoopKind, regs: &[DataType]) -> Result<(), RunError> {
        let count = match kind {
            LoopKind::For => match self.read_input(id, 0, DataType::Int)? {
                Some(Value::Int(n)) => n,
                _ => return Err(RunError::Unwired(id, "For Loop".into(), "N".into())),
            },
            LoopKind::While => i64::MAX,
        };
        let mut state = Vec::with_capacity(regs.len());
        for (k, ty) in regs.iter().enumerate() {
            state.push(self.read_input(id, 1 + 2 * k, *ty)?.unwrap_or(Value::default_for(*ty)));
        }
        let mut i = 0i64;
        while i < count {
            self.check_abort()?;
            if i >= ITERATION_LIMIT {
                return Err(RunError::IterationLimit(id));
            }
            self.env.insert((id, 0), Value::Int(i));
            for (k, s) in state.iter().enumerate() {
                self.env.insert((id, 1 + 2 * k), s.clone());
            }
            self.eval_scope(Some(id))?;
            for (k, ty) in regs.iter().enumerate() {
                if let Some(v) = self.read_input(id, 2 + 2 * k, *ty)? {
                    state[k] = v;
                }
            }
            i += 1;
            if kind == LoopKind::While {
                match self.read_input(id, 0, DataType::Bool)? {
                    Some(Value::Bool(true)) => break,
                    Some(_) => {}
                    None => return Err(RunError::Unwired(id, "While Loop".into(), "stop".into())),
                }
            }
        }
        for (k, s) in state.into_iter().enumerate() {
            self.env.insert((id, 2 + 2 * k), s);
        }
        Ok(())
    }

    fn next_rand(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn compute(&mut self, id: NodeId, kind: &NodeKind, own: Option<&Value>, a: &[Value]) -> Result<Vec<Value>, RunError> {
        let f = |i: usize| a[i].as_f64().unwrap_or(0.0);
        let b = |i: usize| matches!(a[i], Value::Bool(true));
        Ok(match kind {
            NodeKind::Control { ty, .. } => {
                let live = self.ctl.live_controls.lock().ok().and_then(|m| m.get(&id).cloned());
                vec![live.or_else(|| own.cloned()).unwrap_or(Value::default_for(*ty))]
            }
            NodeKind::Constant(v) => vec![own.cloned().unwrap_or_else(|| v.clone())],
            NodeKind::Indicator { .. } | NodeKind::Loop { .. } => vec![],
            NodeKind::Add => vec![Value::Dbl(f(0) + f(1))],
            NodeKind::Subtract => vec![Value::Dbl(f(0) - f(1))],
            NodeKind::Multiply => vec![Value::Dbl(f(0) * f(1))],
            NodeKind::Divide => {
                if f(1) == 0.0 {
                    return Err(RunError::DivideByZero(id));
                }
                vec![Value::Dbl(f(0) / f(1))]
            }
            NodeKind::And => vec![Value::Bool(b(0) && b(1))],
            NodeKind::Or => vec![Value::Bool(b(0) || b(1))],
            NodeKind::Not => vec![Value::Bool(!b(0))],
            NodeKind::Greater => vec![Value::Bool(f(0) > f(1))],
            NodeKind::Less => vec![Value::Bool(f(0) < f(1))],
            NodeKind::Equal => vec![Value::Bool(f(0) == f(1))],
            NodeKind::Concatenate => match (&a[0], &a[1]) {
                (Value::Str(x), Value::Str(y)) => vec![Value::Str(format!("{x}{y}"))],
                _ => vec![],
            },
            NodeKind::Sine => vec![Value::Dbl(f(0).sin())],
            NodeKind::Random => vec![Value::Dbl(self.next_rand())],
            NodeKind::WaitMs => {
                let mut left = match a[0] { Value::Int(n) => n.max(0) as u64, _ => 0 };
                while left > 0 {
                    self.check_abort()?;
                    let step = left.min(10);
                    std::thread::sleep(std::time::Duration::from_millis(step));
                    left -= step;
                }
                vec![]
            }
            NodeKind::BuildArray => match &a[0] {
                Value::ArrDbl(v) => {
                    let mut v = v.clone();
                    v.push(f(1));
                    vec![Value::ArrDbl(v)]
                }
                _ => vec![],
            },
            NodeKind::ArraySize => match &a[0] {
                Value::ArrDbl(v) => vec![Value::Int(v.len() as i64)],
                _ => vec![],
            },
            NodeKind::IndexArray => match (&a[0], &a[1]) {
                (Value::ArrDbl(v), Value::Int(i)) => {
                    vec![Value::Dbl(usize::try_from(*i).ok().and_then(|i| v.get(i)).copied().unwrap_or(0.0))]
                }
                _ => vec![],
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Command, EditError, Editor};

    fn mk(ed: &mut Editor, kind: NodeKind, pos: [f32; 2]) -> NodeId {
        ed.apply(Command::AddNode { kind, pos, widget: None }).unwrap().unwrap()
    }
    fn wire(ed: &mut Editor, from: (NodeId, usize), to: (NodeId, usize)) {
        ed.apply(Command::Connect { from, to }).unwrap();
    }
    fn ctl(name: &str, ty: DataType) -> NodeKind {
        NodeKind::Control { name: name.into(), ty }
    }
    fn ind(name: &str, ty: DataType) -> NodeKind {
        NodeKind::Indicator { name: name.into(), ty }
    }

    fn add_graph() -> (Editor, NodeId) {
        let mut ed = Editor::new(Vi::new("t"));
        let a = mk(&mut ed, ctl("a", DataType::Dbl), [0.0; 2]);
        let b = mk(&mut ed, ctl("b", DataType::Dbl), [0.0; 2]);
        let add = mk(&mut ed, NodeKind::Add, [0.0; 2]);
        let out = mk(&mut ed, ind("sum", DataType::Dbl), [0.0; 2]);
        wire(&mut ed, (a, 0), (add, 0));
        wire(&mut ed, (b, 0), (add, 1));
        wire(&mut ed, (add, 0), (out, 0));
        ed.apply(Command::SetValue { id: a, value: Value::Dbl(2.0) }).unwrap();
        ed.apply(Command::SetValue { id: b, value: Value::Dbl(3.5) }).unwrap();
        (ed, out)
    }

    #[test]
    fn adds() {
        let (ed, out) = add_graph();
        assert_eq!(run(&ed.vi).unwrap().indicators, vec![(out, Value::Dbl(5.5))]);
    }

    #[test]
    fn type_mismatch_and_undo() {
        let (mut ed, _) = add_graph();
        let s = mk(&mut ed, NodeKind::Constant(Value::Str("x".into())), [0.0; 2]);
        assert!(matches!(ed.apply(Command::Connect { from: (s, 0), to: (3, 0) }), Err(EditError::TypeMismatch(..))));
        let n = ed.vi.nodes.len();
        ed.undo().unwrap();
        assert_eq!(ed.vi.nodes.len(), n - 1);
    }

    #[test]
    fn json_roundtrip() {
        let (ed, _) = add_graph();
        let vi = Vi::from_json(&ed.vi.to_json()).unwrap();
        assert_eq!(vi.nodes.len(), ed.vi.nodes.len());
    }

    #[test]
    fn for_loop_shift_register_sums_iterations() {
        let mut ed = Editor::new(Vi::new("t"));
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![DataType::Dbl] }, [0.0, 0.0]);
        let n = mk(&mut ed, NodeKind::Constant(Value::Int(5)), [-300.0, 0.0]);
        let add = mk(&mut ed, NodeKind::Add, [100.0, 60.0]);
        let out = mk(&mut ed, ind("sum", DataType::Dbl), [-300.0, 200.0]);
        assert_eq!(ed.vi.node(add).unwrap().parent, Some(l));
        wire(&mut ed, (n, 0), (l, 0));
        wire(&mut ed, (l, 1), (add, 0)); // sr value
        wire(&mut ed, (l, 0), (add, 1)); // i
        wire(&mut ed, (add, 0), (l, 2)); // next
        wire(&mut ed, (l, 2), (out, 0)); // final
        assert_eq!(run(&ed.vi).unwrap().indicators, vec![(out, Value::Dbl(10.0))]);
    }

    #[test]
    fn while_loop_with_stop_condition_and_tunnels() {
        let mut ed = Editor::new(Vi::new("t"));
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
        let two_and_half = mk(&mut ed, NodeKind::Constant(Value::Dbl(2.5)), [-300.0, 0.0]);
        let gt = mk(&mut ed, NodeKind::Greater, [100.0, 60.0]);
        let out = mk(&mut ed, ind("last i", DataType::Dbl), [-300.0, 200.0]);
        wire(&mut ed, (l, 0), (gt, 0));
        wire(&mut ed, (two_and_half, 0), (gt, 1));
        wire(&mut ed, (gt, 0), (l, 0));
        // output tunnel: last iteration's value of a node inside the loop
        let sine = mk(&mut ed, NodeKind::Add, [100.0, 130.0]);
        wire(&mut ed, (l, 0), (sine, 0));
        wire(&mut ed, (l, 0), (sine, 1));
        wire(&mut ed, (sine, 0), (out, 0));
        assert_eq!(run(&ed.vi).unwrap().indicators, vec![(out, Value::Dbl(6.0))]);
    }

    #[test]
    fn build_array_in_loop() {
        let mut ed = Editor::new(Vi::new("t"));
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![DataType::ArrDbl] }, [0.0, 0.0]);
        let n = mk(&mut ed, NodeKind::Constant(Value::Int(4)), [-300.0, 0.0]);
        let ba = mk(&mut ed, NodeKind::BuildArray, [100.0, 60.0]);
        let out = mk(&mut ed, ind("arr", DataType::ArrDbl), [-300.0, 200.0]);
        wire(&mut ed, (n, 0), (l, 0));
        wire(&mut ed, (l, 1), (ba, 0));
        wire(&mut ed, (l, 0), (ba, 1));
        wire(&mut ed, (ba, 0), (l, 2));
        wire(&mut ed, (l, 2), (out, 0));
        assert_eq!(run(&ed.vi).unwrap().indicators, vec![(out, Value::ArrDbl(vec![0.0, 1.0, 2.0, 3.0]))]);
    }

    #[test]
    fn inner_terminal_cannot_feed_outside() {
        let mut ed = Editor::new(Vi::new("t"));
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [0.0, 0.0]);
        let out = mk(&mut ed, ind("x", DataType::Dbl), [-300.0, 0.0]);
        assert_eq!(ed.apply(Command::Connect { from: (l, 0), to: (out, 0) }), Err(EditError::Scope));
    }

    #[test]
    fn abort_stops_infinite_loop() {
        let mut ed = Editor::new(Vi::new("t"));
        let l = mk(&mut ed, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
        let f = mk(&mut ed, NodeKind::Constant(Value::Bool(false)), [50.0, 60.0]);
        wire(&mut ed, (f, 0), (l, 0));
        let ctl = RunControl::headless();
        ctl.abort.store(true, Ordering::Relaxed);
        assert!(matches!(run_with(&ed.vi, &ctl), Err(RunError::Aborted)));
    }
}
