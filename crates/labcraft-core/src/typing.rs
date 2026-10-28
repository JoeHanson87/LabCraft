//! Type inference for polymorphic nodes: resolves the real type of every output
//! terminal from the wires feeding it, validates connections, and finds coercion dots.

use crate::model::{EditError, NodeId, NodeKind, Spec, Vi};
use crate::types::{DataType, Field, NumKind};
use std::collections::{HashMap, HashSet};

pub struct TypeMap {
    outs: HashMap<NodeId, Vec<DataType>>,
}

impl TypeMap {
    pub fn out(&self, id: NodeId, idx: usize) -> Option<&DataType> {
        self.outs.get(&id).and_then(|v| v.get(idx))
    }
    pub fn outs(&self, id: NodeId) -> &[DataType] {
        self.outs.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }
    /// Type arriving at an input terminal (the wire's source type), if wired.
    pub fn input(&self, vi: &Vi, id: NodeId, idx: usize) -> Option<&DataType> {
        let w = vi.wires.iter().find(|w| w.to == (id, idx))?;
        self.out(w.from.0, w.from.1)
    }
}

pub fn resolve(vi: &Vi) -> TypeMap {
    let mut r = Resolver { vi, memo: HashMap::new(), stack: HashSet::new() };
    for n in &vi.nodes {
        r.node_outs(n.id);
    }
    TypeMap { outs: r.memo }
}

pub fn out_type(vi: &Vi, from: (NodeId, usize)) -> Option<DataType> {
    resolve(vi).out(from.0, from.1).cloned()
}

struct Resolver<'a> {
    vi: &'a Vi,
    memo: HashMap<NodeId, Vec<DataType>>,
    stack: HashSet<NodeId>,
}

impl Resolver<'_> {
    fn node_outs(&mut self, id: NodeId) -> Vec<DataType> {
        if let Some(v) = self.memo.get(&id) {
            return v.clone();
        }
        let Some(node) = self.vi.node(id) else { return vec![] };
        let declared: Vec<DataType> = node.kind.outputs().into_iter().map(|t| t.ty).collect();
        if !self.stack.insert(id) {
            return declared;
        }
        let outs = match &node.kind {
            NodeKind::Control { .. } | NodeKind::Constant(_) | NodeKind::Loop { .. } | NodeKind::Indicator { .. } => declared,
            kind => {
                let n_in = kind.inputs().len();
                let ins: Vec<Option<DataType>> = (0..n_in)
                    .map(|i| {
                        let w = self.vi.wires.iter().find(|w| w.to == (id, i)).copied()?;
                        self.node_outs(w.from.0).get(w.from.1).cloned()
                    })
                    .collect();
                result_types(kind, &ins)
            }
        };
        self.stack.remove(&id);
        self.memo.insert(id, outs.clone());
        outs
    }
}

fn map_leaf(ty: &DataType, f: &dyn Fn(&DataType) -> DataType) -> DataType {
    match ty {
        DataType::Array(e, n) => DataType::Array(Box::new(map_leaf(e, f)), *n),
        DataType::Cluster(fs) => DataType::Cluster(fs.iter().map(|x| Field { name: x.name.clone(), ty: map_leaf(&x.ty, f) }).collect()),
        t => f(t),
    }
}

/// Enums behave as U16 in arithmetic.
fn numify(ty: &DataType) -> DataType {
    map_leaf(ty, &|t| if let DataType::Enum(_) = t { DataType::Num(NumKind::U16) } else { t.clone() })
}

fn floatify(ty: &DataType) -> DataType {
    map_leaf(ty, &|t| match t {
        DataType::Num(k) if k.is_float() => t.clone(),
        _ => DataType::DBL,
    })
}

fn cmp_shape(ty: &DataType) -> DataType {
    match ty {
        DataType::Array(e, n) => DataType::Array(Box::new(cmp_shape(e)), *n),
        _ => DataType::Bool,
    }
}

fn leaf_join_num(a: &DataType, b: &DataType) -> Option<DataType> {
    let k = |t: &DataType| match t {
        DataType::Num(k) => Some(*k),
        DataType::Enum(_) => Some(NumKind::U16),
        _ => None,
    };
    Some(DataType::Num(NumKind::join(k(a)?, k(b)?)))
}

fn leaf_join_bool(a: &DataType, b: &DataType) -> Option<DataType> {
    (*a == DataType::Bool && *b == DataType::Bool).then_some(DataType::Bool)
}

fn is_leaf(t: &DataType) -> bool {
    !matches!(t, DataType::Array(..) | DataType::Cluster(_))
}

/// Shape-combining join used by polymorphic arithmetic, logic and comparisons.
fn join(a: &DataType, b: &DataType, leaf: &dyn Fn(&DataType, &DataType) -> Option<DataType>) -> Option<DataType> {
    match (a, b) {
        (x, y) if is_leaf(x) && is_leaf(y) => leaf(x, y),
        (DataType::Array(x, n), DataType::Array(y, m)) if n == m => Some(DataType::Array(Box::new(join(x, y, leaf)?), *n)),
        (DataType::Array(x, n), y) if !matches!(y, DataType::Array(..)) => Some(DataType::Array(Box::new(join(x, y, leaf)?), *n)),
        (x, DataType::Array(y, n)) if !matches!(x, DataType::Array(..)) => Some(DataType::Array(Box::new(join(x, y, leaf)?), *n)),
        (DataType::Cluster(f), DataType::Cluster(g)) if f.len() == g.len() => {
            let fields = f.iter().zip(g).map(|(x, y)| Some(Field { name: x.name.clone(), ty: join(&x.ty, &y.ty, leaf)? })).collect::<Option<Vec<_>>>()?;
            Some(DataType::Cluster(fields))
        }
        (DataType::Cluster(f), y) if is_leaf(y) => {
            Some(DataType::Cluster(f.iter().map(|x| Some(Field { name: x.name.clone(), ty: join(&x.ty, y, leaf)? })).collect::<Option<Vec<_>>>()?))
        }
        (x, DataType::Cluster(g)) if is_leaf(x) => {
            Some(DataType::Cluster(g.iter().map(|y| Some(Field { name: y.name.clone(), ty: join(x, &y.ty, leaf)? })).collect::<Option<Vec<_>>>()?))
        }
        _ => None,
    }
}

pub fn join_num(a: &DataType, b: &DataType) -> Option<DataType> {
    join(a, b, &leaf_join_num)
}

pub fn join_bool(a: &DataType, b: &DataType) -> Option<DataType> {
    join(a, b, &leaf_join_bool)
}

fn field_named<'a>(ty: &'a Option<DataType>, name: &str) -> Option<&'a DataType> {
    match ty {
        Some(DataType::Cluster(f)) => f.iter().find(|f| f.name == name).map(|f| &f.ty),
        _ => None,
    }
}

/// Resolved output types of `kind`, given the types arriving at each input (None = unwired).
pub fn result_types(kind: &NodeKind, ins: &[Option<DataType>]) -> Vec<DataType> {
    let d = |i: usize| ins.get(i).cloned().flatten();
    let dbl = || DataType::DBL;
    match kind {
        NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => {
            vec![match (d(0), d(1)) {
                (Some(a), Some(b)) => join_num(&a, &b).unwrap_or_else(|| numify(&a)),
                (Some(a), None) | (None, Some(a)) => numify(&a),
                _ => dbl(),
            }]
        }
        NodeKind::Increment | NodeKind::Decrement | NodeKind::Negate | NodeKind::Absolute => vec![d(0).map(|t| numify(&t)).unwrap_or_else(dbl)],
        NodeKind::SquareRoot => vec![d(0).map(|t| floatify(&t)).unwrap_or_else(dbl)],
        NodeKind::Convert(k) => {
            let k = *k;
            vec![d(0).map(|t| map_leaf(&numify(&t), &|_| DataType::Num(k))).unwrap_or(DataType::Num(k))]
        }
        NodeKind::And | NodeKind::Or | NodeKind::Xor => vec![match (d(0), d(1)) {
            (Some(a), Some(b)) => join_bool(&a, &b).unwrap_or(a),
            (Some(a), None) | (None, Some(a)) => a,
            _ => DataType::Bool,
        }],
        NodeKind::Not => vec![d(0).unwrap_or(DataType::Bool)],
        NodeKind::Greater | NodeKind::Less | NodeKind::GreaterEq | NodeKind::LessEq => {
            vec![match (d(0), d(1)) {
                (Some(a), Some(b)) => cmp_shape(&join_num(&a, &b).unwrap_or(a)),
                (Some(a), None) | (None, Some(a)) => cmp_shape(&a),
                _ => DataType::Bool,
            }]
        }
        NodeKind::Equal | NodeKind::NotEq => {
            let (a, b) = (d(0), d(1));
            vec![match (a, b) {
                (Some(a @ DataType::Array(..)), _) | (_, Some(a @ DataType::Array(..))) => cmp_shape(&a),
                _ => DataType::Bool,
            }]
        }
        NodeKind::Select => vec![d(1).or(d(2)).unwrap_or_else(dbl)],
        NodeKind::Concatenate | NodeKind::NumToString => vec![DataType::Str],
        NodeKind::Sine | NodeKind::Random => vec![dbl()],
        NodeKind::WaitMs => vec![],
        NodeKind::BuildArray => vec![match (d(0), d(1)) {
            (Some(a @ DataType::Array(..)), _) => a,
            (_, Some(DataType::Array(e, m))) => DataType::Array(e, m + 1),
            (_, Some(e)) => DataType::array(e, 1),
            _ => DataType::arr_dbl(),
        }],
        NodeKind::ArraySize => vec![match d(0) {
            Some(DataType::Array(_, n)) if n > 1 => DataType::array(DataType::I32, 1),
            _ => DataType::I32,
        }],
        NodeKind::IndexArray => {
            let k = ins.iter().skip(1).filter(|t| t.is_some()).count().max(1);
            vec![match d(0) {
                Some(DataType::Array(e, n)) => if k >= n { *e } else { DataType::Array(e, n - k) },
                _ => dbl(),
            }]
        }
        NodeKind::InitializeArray => vec![match d(0) {
            Some(DataType::Array(e, m)) => DataType::Array(e, m + 1),
            Some(e) => DataType::array(e, 1),
            None => DataType::arr_dbl(),
        }],
        NodeKind::Transpose => vec![d(0).unwrap_or_else(|| DataType::array(DataType::DBL, 2))],
        NodeKind::Bundle { n } => vec![match d(0) {
            Some(c @ DataType::Cluster(_)) => c,
            _ => DataType::Cluster((0..*n).map(|i| Field { name: format!("e{i}"), ty: d(1 + i).unwrap_or_else(dbl) }).collect()),
        }],
        NodeKind::BundleByName { .. } => vec![d(0).unwrap_or(DataType::Cluster(vec![]))],
        NodeKind::Unbundle { n } => (0..*n)
            .map(|i| match d(0) {
                Some(DataType::Cluster(f)) => f.get(i).map(|f| f.ty.clone()).unwrap_or_else(dbl),
                _ => dbl(),
            })
            .collect(),
        NodeKind::UnbundleByName { names } => names.iter().map(|n| field_named(&d(0), n).cloned().unwrap_or_else(dbl)).collect(),
        NodeKind::ErrorFromCode | NodeKind::MergeErrors => vec![DataType::error_cluster()],
        _ => kind.outputs().into_iter().map(|t| t.ty).collect(),
    }
}

fn mismatch(a: &DataType, b: &DataType) -> EditError {
    EditError::TypeMismatch(a.clone(), b.clone())
}

/// Validate wiring `from` -> `to` against the current diagram.
pub fn check_connect(vi: &Vi, from: (NodeId, usize), to: (NodeId, usize)) -> Result<(), EditError> {
    let tm = resolve(vi);
    let src = tm.out(from.0, from.1).cloned().ok_or(EditError::NoTerminal(from.0, from.1))?;
    let dst = vi.node(to.0).ok_or(EditError::NoNode(to.0))?;
    let terms = dst.kind.inputs();
    let term = terms.get(to.1).ok_or(EditError::NoTerminal(to.0, to.1))?;
    // The other operand of a binary node, if it is wired from somewhere other than this wire.
    let other = |idx: usize| -> Option<DataType> { tm.input(vi, to.0, idx).cloned() };
    match term.spec {
        Spec::Exact => {
            if !src.can_feed(&term.ty) {
                return Err(mismatch(&src, &term.ty));
            }
        }
        Spec::NumLike => {
            if !src.is_numlike() {
                return Err(mismatch(&src, &DataType::DBL));
            }
            let partner = if to.1 == 0 { 1 } else { 0 };
            if terms.len() == 2 {
                if let Some(o) = other(partner) {
                    if join_num(&src, &o).is_none() {
                        return Err(mismatch(&src, &o));
                    }
                }
            }
        }
        Spec::BoolLike => {
            if !src.is_boollike() {
                return Err(mismatch(&src, &DataType::Bool));
            }
            let partner = if to.1 == 0 { 1 } else { 0 };
            if terms.len() == 2 {
                if let Some(o) = other(partner) {
                    if join_bool(&src, &o).is_none() {
                        return Err(mismatch(&src, &o));
                    }
                }
            }
        }
        Spec::AnyArray => {
            if !matches!(src, DataType::Array(..)) {
                return Err(mismatch(&src, &DataType::arr_dbl()));
            }
        }
        Spec::AnyCluster => {
            if !matches!(src, DataType::Cluster(_)) {
                return Err(mismatch(&src, &DataType::Cluster(vec![])));
            }
        }
        Spec::Any => match &dst.kind {
            NodeKind::Equal | NodeKind::NotEq | NodeKind::Select => {
                let partner = match (&dst.kind, to.1) {
                    (NodeKind::Select, 1) => 2,
                    (NodeKind::Select, _) => 1,
                    (_, 0) => 1,
                    _ => 0,
                };
                if let Some(o) = other(partner) {
                    if !src.can_feed(&o) && !o.can_feed(&src) {
                        return Err(mismatch(&src, &o));
                    }
                }
            }
            NodeKind::BuildArray => {
                if let Some(DataType::Array(e, n)) = other(0) {
                    let ok = if n == 1 { src.can_feed(&e) } else { src.can_feed(&DataType::Array(e.clone(), n - 1)) };
                    if !ok {
                        return Err(mismatch(&src, &e));
                    }
                }
            }
            NodeKind::Bundle { .. } => {
                if let Some(DataType::Cluster(f)) = other(0) {
                    if let Some(field) = f.get(to.1 - 1) {
                        if !src.can_feed(&field.ty) {
                            return Err(mismatch(&src, &field.ty));
                        }
                    }
                }
            }
            NodeKind::BundleByName { names } => {
                if let Some(DataType::Cluster(f)) = other(0) {
                    let name = names.get(to.1 - 1).cloned().unwrap_or_default();
                    match f.iter().find(|x| x.name == name) {
                        Some(field) if src.can_feed(&field.ty) => {}
                        Some(field) => return Err(mismatch(&src, &field.ty)),
                        None if name.is_empty() => {}
                        None => return Err(EditError::NoSuchField(name)),
                    }
                }
            }
            _ => {}
        },
    }
    Ok(())
}

fn num_kind_of(t: &DataType) -> Option<NumKind> {
    match t {
        DataType::Num(k) => Some(*k),
        DataType::Array(e, _) => num_kind_of(e),
        _ => None,
    }
}

/// Input terminals where a coercion dot should be drawn (numeric representation changes).
pub fn coercion_dots(vi: &Vi, tm: &TypeMap) -> HashSet<(NodeId, usize)> {
    let mut dots = HashSet::new();
    for w in &vi.wires {
        let Some(node) = vi.node(w.to.0) else { continue };
        let Some(src) = tm.out(w.from.0, w.from.1) else { continue };
        let terms = node.kind.inputs();
        let Some(term) = terms.get(w.to.1) else { continue };
        let expected: Option<NumKind> = match term.spec {
            Spec::Exact => num_kind_of(&term.ty),
            Spec::NumLike => match &node.kind {
                NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => tm.out(w.to.0, 0).and_then(num_kind_of),
                NodeKind::Greater | NodeKind::Less | NodeKind::GreaterEq | NodeKind::LessEq => {
                    let (a, b) = (tm.input(vi, w.to.0, 0), tm.input(vi, w.to.0, 1));
                    match (a, b) {
                        (Some(a), Some(b)) => join_num(a, b).and_then(|t| num_kind_of(&t)),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        };
        if let (Some(e), Some(s)) = (expected, num_kind_of(src)) {
            if e != s {
                dots.insert((w.to.0, w.to.1));
            }
        }
    }
    dots
}
