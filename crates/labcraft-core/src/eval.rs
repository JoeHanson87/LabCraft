use crate::model::{NodeId, NodeKind, Vi};
use crate::types::Value;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RunError {
    #[error("node {0} ({1}) has an unwired input '{2}'")]
    Unwired(NodeId, String, &'static str),
    #[error("division by zero at node {0}")]
    DivideByZero(NodeId),
    #[error("diagram contains a cycle")]
    Cycle,
}

pub struct RunResult {
    /// Output value of every node terminal, keyed by (node, output index).
    pub outputs: HashMap<(NodeId, usize), Value>,
    /// Indicator name -> value.
    pub indicators: Vec<(NodeId, Value)>,
}

/// Evaluate a feed-forward diagram in dataflow order.
pub fn run(vi: &Vi) -> Result<RunResult, RunError> {
    let mut done: HashMap<NodeId, Vec<Value>> = HashMap::new();
    let mut remaining: Vec<NodeId> = vi.nodes.iter().map(|n| n.id).collect();
    let mut indicators = vec![];

    while !remaining.is_empty() {
        let before = remaining.len();
        let mut still = vec![];
        for id in remaining {
            let node = vi.node(id).unwrap();
            let ins = node.kind.inputs();
            let mut args = Vec::with_capacity(ins.len());
            let mut ready = true;
            for (i, term) in ins.iter().enumerate() {
                match vi.wires.iter().find(|w| w.to == (id, i)) {
                    None => return Err(RunError::Unwired(id, node.kind.title(), term.name)),
                    Some(w) => match done.get(&w.from.0) {
                        Some(o) => args.push(o[w.from.1].coerce(term.ty).unwrap()),
                        None => {
                            ready = false;
                            break;
                        }
                    },
                }
            }
            if !ready {
                still.push(id);
                continue;
            }
            let out = fire(id, &node.kind, node.value.as_ref(), &args)?;
            if matches!(node.kind, NodeKind::Indicator { .. }) {
                indicators.push((id, args[0].clone()));
            }
            done.insert(id, out);
        }
        if still.len() == before {
            return Err(RunError::Cycle);
        }
        remaining = still;
    }

    let mut outputs = HashMap::new();
    for (id, vals) in done {
        for (i, v) in vals.into_iter().enumerate() {
            outputs.insert((id, i), v);
        }
    }
    Ok(RunResult { outputs, indicators })
}

fn fire(id: NodeId, kind: &NodeKind, value: Option<&Value>, a: &[Value]) -> Result<Vec<Value>, RunError> {
    let f = |i: usize| a[i].as_f64().unwrap();
    let b = |i: usize| matches!(a[i], Value::Bool(true));
    Ok(match kind {
        NodeKind::Control { ty, .. } => vec![value.cloned().unwrap_or(Value::default_for(*ty))],
        NodeKind::Constant(v) => vec![v.clone()],
        NodeKind::Indicator { .. } => vec![],
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
            _ => unreachable!(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Command, Editor, EditError};
    use crate::types::DataType;

    fn add_graph() -> (Editor, NodeId) {
        let mut ed = Editor::new(Vi::new("t"));
        let mk = |ed: &mut Editor, kind| ed.apply(Command::AddNode { kind, pos: [0.0; 2] }).unwrap().unwrap();
        let a = mk(&mut ed, NodeKind::Control { name: "a".into(), ty: DataType::Dbl });
        let b = mk(&mut ed, NodeKind::Control { name: "b".into(), ty: DataType::Dbl });
        let add = mk(&mut ed, NodeKind::Add);
        let out = mk(&mut ed, NodeKind::Indicator { name: "sum".into(), ty: DataType::Dbl });
        ed.apply(Command::Connect { from: (a, 0), to: (add, 0) }).unwrap();
        ed.apply(Command::Connect { from: (b, 0), to: (add, 1) }).unwrap();
        ed.apply(Command::Connect { from: (add, 0), to: (out, 0) }).unwrap();
        ed.apply(Command::SetValue { id: a, value: Value::Dbl(2.0) }).unwrap();
        ed.apply(Command::SetValue { id: b, value: Value::Dbl(3.5) }).unwrap();
        (ed, out)
    }

    #[test]
    fn adds() {
        let (ed, out) = add_graph();
        let r = run(&ed.vi).unwrap();
        assert_eq!(r.indicators, vec![(out, Value::Dbl(5.5))]);
    }

    #[test]
    fn type_mismatch_and_undo() {
        let (mut ed, _) = add_graph();
        let s = ed.apply(Command::AddNode { kind: NodeKind::Constant(Value::Str("x".into())), pos: [0.0; 2] }).unwrap().unwrap();
        let add = 3;
        assert!(matches!(ed.apply(Command::Connect { from: (s, 0), to: (add, 0) }), Err(EditError::TypeMismatch(..))));
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
}
