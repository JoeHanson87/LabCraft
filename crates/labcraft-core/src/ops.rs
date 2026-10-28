//! Polymorphic value operations: arithmetic and logic that broadcast over arrays and clusters,
//! plus array helpers.

use crate::eval::RunError;
use crate::model::NodeId;
use crate::types::{ArrayVal, DataType, NumKind, Value};

#[derive(Clone, Copy, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug)]
pub enum Cmp {
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
}

#[derive(Clone, Copy, Debug)]
pub enum Logic {
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug)]
pub enum Unary {
    Inc,
    Dec,
    Neg,
    Abs,
    Sqrt,
}

fn bad(what: &str) -> RunError {
    RunError::Internal(format!("operand type mismatch ({what})"))
}

/// The type of the elements inside `out` (array element, or the i-th cluster field).
fn sub_out(out: &DataType, i: usize) -> DataType {
    match out {
        DataType::Array(e, _) => (**e).clone(),
        DataType::Cluster(f) => f.get(i).map(|f| f.ty.clone()).unwrap_or(DataType::DBL),
        t => t.clone(),
    }
}

type Leaf<'a> = &'a mut dyn FnMut(&Value, &Value, &DataType) -> Result<Value, RunError>;

/// Apply `leaf` to matching scalar pairs, recursing through arrays and clusters.
pub fn broadcast(a: &Value, b: &Value, out: &DataType, leaf: Leaf) -> Result<Value, RunError> {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            let elem = sub_out(out, 0);
            if x.dims == y.dims {
                let data = x.data.iter().zip(&y.data).map(|(p, q)| broadcast(p, q, &elem, leaf)).collect::<Result<Vec<_>, _>>()?;
                return Ok(Value::Array(ArrayVal { dims: x.dims.clone(), data }));
            }
            let nd = x.dims.len().min(y.dims.len());
            let dims: Vec<usize> = (0..nd).map(|i| x.dims[i].min(y.dims[i])).collect();
            let total: usize = dims.iter().product();
            let (sd, sx, sy) = (ArrayVal::strides(&dims), ArrayVal::strides(&x.dims), ArrayVal::strides(&y.dims));
            let mut data = Vec::with_capacity(total);
            for flat in 0..total {
                let (mut ox, mut oy, mut rest) = (0, 0, flat);
                for k in 0..nd {
                    let idx = rest / sd[k];
                    rest %= sd[k];
                    ox += idx * sx[k];
                    oy += idx * sy[k];
                }
                data.push(broadcast(&x.data[ox], &y.data[oy], &elem, leaf)?);
            }
            Ok(Value::Array(ArrayVal { dims, data }))
        }
        (Value::Array(x), _) => {
            let elem = sub_out(out, 0);
            let data = x.data.iter().map(|p| broadcast(p, b, &elem, leaf)).collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(ArrayVal { dims: x.dims.clone(), data }))
        }
        (_, Value::Array(y)) => {
            let elem = sub_out(out, 0);
            let data = y.data.iter().map(|q| broadcast(a, q, &elem, leaf)).collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(ArrayVal { dims: y.dims.clone(), data }))
        }
        (Value::Cluster(x), Value::Cluster(y)) => {
            let v = x.iter().zip(y).enumerate().map(|(i, (p, q))| broadcast(p, q, &sub_out(out, i), leaf)).collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Cluster(v))
        }
        (Value::Cluster(x), _) => {
            Ok(Value::Cluster(x.iter().enumerate().map(|(i, p)| broadcast(p, b, &sub_out(out, i), leaf)).collect::<Result<Vec<_>, _>>()?))
        }
        (_, Value::Cluster(y)) => {
            Ok(Value::Cluster(y.iter().enumerate().map(|(i, q)| broadcast(a, q, &sub_out(out, i), leaf)).collect::<Result<Vec<_>, _>>()?))
        }
        _ => leaf(a, b, out),
    }
}

pub fn map1(a: &Value, out: &DataType, f: &mut dyn FnMut(&Value, &DataType) -> Result<Value, RunError>) -> Result<Value, RunError> {
    match a {
        Value::Array(x) => {
            let elem = sub_out(out, 0);
            let data = x.data.iter().map(|p| map1(p, &elem, f)).collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(ArrayVal { dims: x.dims.clone(), data }))
        }
        Value::Cluster(x) => Ok(Value::Cluster(x.iter().enumerate().map(|(i, p)| map1(p, &sub_out(out, i), f)).collect::<Result<Vec<_>, _>>()?)),
        _ => f(a, out),
    }
}

pub fn arith(op: BinOp, a: &Value, b: &Value, out: &DataType, id: NodeId) -> Result<Value, RunError> {
    broadcast(a, b, out, &mut |x, y, out| {
        let kind = if let DataType::Num(k) = out { *k } else { NumKind::Dbl };
        if kind.is_float() {
            let (x, y) = (x.as_f64().ok_or_else(|| bad("arith"))?, y.as_f64().ok_or_else(|| bad("arith"))?);
            return Ok(kind.from_f64(match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => x / y,
            }));
        }
        let (x, y) = (x.as_i64().ok_or_else(|| bad("arith"))? as i128, y.as_i64().ok_or_else(|| bad("arith"))? as i128);
        let r = match op {
            BinOp::Add => x + y,
            BinOp::Sub => x - y,
            BinOp::Mul => x * y,
            BinOp::Div => {
                if y == 0 {
                    return Err(RunError::DivideByZero(id));
                }
                x / y
            }
        };
        Ok(Value::Int(kind.wrap(r)))
    })
}

pub fn unary(op: Unary, a: &Value, out: &DataType) -> Result<Value, RunError> {
    map1(a, out, &mut |x, out| {
        let kind = if let DataType::Num(k) = out { *k } else { NumKind::Dbl };
        if kind.is_float() {
            let x = x.as_f64().ok_or_else(|| bad("unary"))?;
            return Ok(kind.from_f64(match op {
                Unary::Inc => x + 1.0,
                Unary::Dec => x - 1.0,
                Unary::Neg => -x,
                Unary::Abs => x.abs(),
                Unary::Sqrt => x.sqrt(),
            }));
        }
        let x = x.as_i64().ok_or_else(|| bad("unary"))? as i128;
        Ok(Value::Int(kind.wrap(match op {
            Unary::Inc => x + 1,
            Unary::Dec => x - 1,
            Unary::Neg => -x,
            Unary::Abs => x.abs(),
            Unary::Sqrt => (x as f64).sqrt() as i128,
        })))
    })
}

pub fn logic(op: Logic, a: &Value, b: &Value) -> Result<Value, RunError> {
    broadcast(a, b, &DataType::Bool, &mut |x, y, _| match (x, y) {
        (Value::Bool(p), Value::Bool(q)) => Ok(Value::Bool(match op {
            Logic::And => *p && *q,
            Logic::Or => *p || *q,
            Logic::Xor => p != q,
        })),
        _ => Err(bad("logic")),
    })
}

pub fn not(a: &Value) -> Result<Value, RunError> {
    map1(a, &DataType::Bool, &mut |x, _| match x {
        Value::Bool(p) => Ok(Value::Bool(!p)),
        _ => Err(bad("not")),
    })
}

fn scalar_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => a.as_f64()?.partial_cmp(&b.as_f64()?),
    }
}

pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => x.dims == y.dims && x.data.iter().zip(&y.data).all(|(p, q)| values_equal(p, q)),
        (Value::Cluster(x), Value::Cluster(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q)),
        _ => scalar_cmp(a, b) == Some(std::cmp::Ordering::Equal),
    }
}

pub fn compare(op: Cmp, a: &Value, b: &Value) -> Result<Value, RunError> {
    if matches!((a, b), (Value::Cluster(_), Value::Cluster(_))) {
        return match op {
            Cmp::Eq => Ok(Value::Bool(values_equal(a, b))),
            Cmp::Ne => Ok(Value::Bool(!values_equal(a, b))),
            _ => Err(bad("cluster ordering")),
        };
    }
    broadcast(a, b, &DataType::Bool, &mut |x, y, _| {
        use std::cmp::Ordering::*;
        let o = scalar_cmp(x, y);
        Ok(Value::Bool(match op {
            Cmp::Gt => o == Some(Greater),
            Cmp::Lt => o == Some(Less),
            Cmp::Ge => matches!(o, Some(Greater | Equal)),
            Cmp::Le => matches!(o, Some(Less | Equal)),
            Cmp::Eq => o == Some(Equal),
            Cmp::Ne => o != Some(Equal),
        }))
    })
}

fn array_parts(out: &DataType) -> (DataType, usize) {
    match out {
        DataType::Array(e, n) => ((**e).clone(), *n),
        _ => (DataType::DBL, 1),
    }
}

/// Append `elem` (a scalar, or a row for N-D arrays) to `arr`.
pub fn array_append(arr: Value, elem: &Value, out: &DataType) -> Value {
    let (e, n) = array_parts(out);
    let Value::Array(mut a) = arr else { return Value::default_for(out) };
    if n == 1 {
        a.data.push(elem.coerce(&e).unwrap_or_else(|| Value::default_for(&e)));
        a.dims = vec![a.data.len()];
    } else if let Some(Value::Array(row)) = elem.coerce(&DataType::Array(Box::new(e), n - 1)) {
        if a.dims.first() == Some(&0) || a.data.is_empty() {
            a.dims = std::iter::once(1).chain(row.dims.iter().copied()).collect();
            a.data = row.data;
        } else if a.dims[1..] == row.dims[..] {
            a.dims[0] += 1;
            a.data.extend(row.data);
        }
    }
    Value::Array(a)
}

pub fn index_array(arr: &Value, idxs: &[i64], out: &DataType) -> Value {
    let Value::Array(a) = arr else { return Value::default_for(out) };
    let strides = ArrayVal::strides(&a.dims);
    let mut off = 0;
    for (k, i) in idxs.iter().enumerate() {
        if k >= a.dims.len() || *i < 0 || *i as usize >= a.dims[k] {
            return Value::default_for(out);
        }
        off += *i as usize * strides[k];
    }
    if idxs.len() >= a.dims.len() {
        return a.data.get(off).cloned().unwrap_or_else(|| Value::default_for(out));
    }
    let dims = a.dims[idxs.len()..].to_vec();
    let len: usize = dims.iter().product();
    Value::Array(ArrayVal { dims, data: a.data[off..off + len].to_vec() })
}

pub fn initialize_array(elem: &Value, size: usize, out: &DataType) -> Value {
    let (e, n) = array_parts(out);
    if n == 1 {
        let v = elem.coerce(&e).unwrap_or_else(|| Value::default_for(&e));
        return Value::Array(ArrayVal { dims: vec![size], data: vec![v; size] });
    }
    match elem.coerce(&DataType::Array(Box::new(e), n - 1)) {
        Some(Value::Array(row)) => {
            let mut data = Vec::with_capacity(row.data.len() * size);
            for _ in 0..size {
                data.extend(row.data.iter().cloned());
            }
            Value::Array(ArrayVal { dims: std::iter::once(size).chain(row.dims).collect(), data })
        }
        _ => Value::default_for(out),
    }
}

pub fn transpose(arr: &Value) -> Value {
    match arr {
        Value::Array(a) if a.dims.len() == 2 => {
            let (r, c) = (a.dims[0], a.dims[1]);
            let mut data = Vec::with_capacity(a.data.len());
            for j in 0..c {
                for i in 0..r {
                    data.push(a.data[i * c + j].clone());
                }
            }
            Value::Array(ArrayVal { dims: vec![c, r], data })
        }
        v => v.clone(),
    }
}
