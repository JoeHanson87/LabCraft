use serde::{Deserialize, Serialize};

/// Numeric representations. Integers are stored as `i64` (U64 is limited to the i64 range).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NumKind {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Sgl,
    Dbl,
}

impl NumKind {
    pub const ALL: [NumKind; 10] = [
        NumKind::Dbl, NumKind::Sgl, NumKind::I8, NumKind::I16, NumKind::I32, NumKind::I64,
        NumKind::U8, NumKind::U16, NumKind::U32, NumKind::U64,
    ];

    pub fn is_float(self) -> bool {
        matches!(self, NumKind::Sgl | NumKind::Dbl)
    }
    pub fn is_signed(self) -> bool {
        matches!(self, NumKind::I8 | NumKind::I16 | NumKind::I32 | NumKind::I64) || self.is_float()
    }
    pub fn bits(self) -> u32 {
        match self {
            NumKind::I8 | NumKind::U8 => 8,
            NumKind::I16 | NumKind::U16 => 16,
            NumKind::I32 | NumKind::U32 | NumKind::Sgl => 32,
            _ => 64,
        }
    }
    pub fn int_range(self) -> (i64, i64) {
        match self {
            NumKind::I8 => (i8::MIN as i64, i8::MAX as i64),
            NumKind::I16 => (i16::MIN as i64, i16::MAX as i64),
            NumKind::I32 => (i32::MIN as i64, i32::MAX as i64),
            NumKind::I64 => (i64::MIN, i64::MAX),
            NumKind::U8 => (0, u8::MAX as i64),
            NumKind::U16 => (0, u16::MAX as i64),
            NumKind::U32 => (0, u32::MAX as i64),
            _ => (0, i64::MAX),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            NumKind::I8 => "I8",
            NumKind::I16 => "I16",
            NumKind::I32 => "I32",
            NumKind::I64 => "I64",
            NumKind::U8 => "U8",
            NumKind::U16 => "U16",
            NumKind::U32 => "U32",
            NumKind::U64 => "U64",
            NumKind::Sgl => "SGL",
            NumKind::Dbl => "DBL",
        }
    }
    /// Two's-complement wrap-around, as LabVIEW integer arithmetic does.
    pub fn wrap(self, v: i128) -> i64 {
        match self {
            NumKind::I8 => v as i8 as i64,
            NumKind::I16 => v as i16 as i64,
            NumKind::I32 => v as i32 as i64,
            NumKind::I64 => v as i64,
            NumKind::U8 => v as u8 as i64,
            NumKind::U16 => v as u16 as i64,
            NumKind::U32 => v as u32 as i64,
            _ => (v as u64 & i64::MAX as u64) as i64,
        }
    }
    pub fn from_f64(self, x: f64) -> Value {
        match self {
            NumKind::Dbl => Value::Dbl(x),
            NumKind::Sgl => Value::Dbl(x as f32 as f64),
            _ => {
                let (lo, hi) = self.int_range();
                Value::Int(if x.is_nan() { 0 } else { x.round().clamp(lo as f64, hi as f64) as i64 })
            }
        }
    }
    pub fn from_i64(self, v: i64) -> Value {
        match self {
            NumKind::Dbl => Value::Dbl(v as f64),
            NumKind::Sgl => Value::Dbl(v as f32 as f64),
            _ => {
                let (lo, hi) = self.int_range();
                Value::Int(v.clamp(lo, hi))
            }
        }
    }
    /// The representation arithmetic produces when mixing two kinds.
    pub fn join(a: NumKind, b: NumKind) -> NumKind {
        if a == b {
            return a;
        }
        match (a.is_float(), b.is_float()) {
            (true, true) => NumKind::Dbl,
            (true, false) => a,
            (false, true) => b,
            _ if a.bits() != b.bits() => if a.bits() > b.bits() { a } else { b },
            _ => if a.is_signed() { a } else { b },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: DataType,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    Num(NumKind),
    Bool,
    Str,
    /// Enumerated type; values are stored as the item index.
    Enum(Vec<String>),
    /// N-dimensional array of the element type.
    Array(Box<DataType>, usize),
    Cluster(Vec<Field>),
}

impl DataType {
    pub const DBL: DataType = DataType::Num(NumKind::Dbl);
    pub const I32: DataType = DataType::Num(NumKind::I32);
    pub const U8: DataType = DataType::Num(NumKind::U8);

    pub fn array(elem: DataType, dims: usize) -> DataType {
        DataType::Array(Box::new(elem), dims)
    }
    pub fn arr_dbl() -> DataType {
        DataType::array(DataType::DBL, 1)
    }
    pub fn field(name: &str, ty: DataType) -> Field {
        Field { name: name.into(), ty }
    }
    /// The standard LabVIEW error cluster: status, code, source.
    pub fn error_cluster() -> DataType {
        DataType::Cluster(vec![
            Self::field("status", DataType::Bool),
            Self::field("code", DataType::I32),
            Self::field("source", DataType::Str),
        ])
    }
    pub fn is_error_cluster(&self) -> bool {
        self.fieldwise_same(&Self::error_cluster())
    }
    fn fieldwise_same(&self, other: &DataType) -> bool {
        match (self, other) {
            (DataType::Cluster(a), DataType::Cluster(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.name == y.name && x.ty == y.ty),
            _ => false,
        }
    }
    pub fn is_numeric_scalar(&self) -> bool {
        matches!(self, DataType::Num(_))
    }
    pub fn is_scalar_numlike(&self) -> bool {
        matches!(self, DataType::Num(_) | DataType::Enum(_))
    }
    /// Numeric scalar, or an array/cluster built from numerics.
    pub fn is_numlike(&self) -> bool {
        match self {
            DataType::Num(_) | DataType::Enum(_) => true,
            DataType::Array(e, _) => e.is_numlike(),
            DataType::Cluster(f) => !f.is_empty() && f.iter().all(|f| f.ty.is_numlike()),
            _ => false,
        }
    }
    pub fn is_boollike(&self) -> bool {
        match self {
            DataType::Bool => true,
            DataType::Array(e, _) => e.is_boollike(),
            DataType::Cluster(f) => !f.is_empty() && f.iter().all(|f| f.ty.is_boollike()),
            _ => false,
        }
    }
    /// Innermost element type of nested arrays; other types return themselves.
    pub fn base(&self) -> &DataType {
        match self {
            DataType::Array(e, _) => e.base(),
            t => t,
        }
    }
    /// Whether a wire of this type may feed a terminal of type `to` (possibly with a coercion dot).
    pub fn can_feed(&self, to: &DataType) -> bool {
        match (self, to) {
            (a, b) if a == b => true,
            (DataType::Num(_) | DataType::Enum(_), DataType::Num(_)) => true,
            (DataType::Array(a, n), DataType::Array(b, m)) => n == m && a.can_feed(b),
            (DataType::Cluster(a), DataType::Cluster(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.ty.can_feed(&y.ty)),
            _ => false,
        }
    }
    pub fn label(&self) -> String {
        match self {
            DataType::Num(k) => k.label().into(),
            DataType::Bool => "Boolean".into(),
            DataType::Str => "String".into(),
            DataType::Enum(_) => "Enum".into(),
            DataType::Array(e, n) => format!("{n}D Array of {}", e.label()),
            DataType::Cluster(f) if self.is_error_cluster() => format!("Error Cluster ({} fields)", f.len()),
            DataType::Cluster(f) => format!("Cluster ({} elements)", f.len()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArrayVal {
    pub dims: Vec<usize>,
    /// Row-major elements.
    pub data: Vec<Value>,
}

impl ArrayVal {
    pub fn strides(dims: &[usize]) -> Vec<usize> {
        let mut s = vec![1; dims.len()];
        for i in (0..dims.len().saturating_sub(1)).rev() {
            s[i] = s[i + 1] * dims[i + 1];
        }
        s
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Dbl(f64),
    /// Integers of every width, and enum item indices.
    Int(i64),
    Bool(bool),
    Str(String),
    Array(ArrayVal),
    Cluster(Vec<Value>),
}

impl Value {
    pub fn default_for(t: &DataType) -> Value {
        match t {
            DataType::Num(k) => if k.is_float() { Value::Dbl(0.0) } else { Value::Int(0) },
            DataType::Bool => Value::Bool(false),
            DataType::Str => Value::Str(String::new()),
            DataType::Enum(_) => Value::Int(0),
            DataType::Array(_, n) => Value::Array(ArrayVal { dims: vec![0; *n], data: vec![] }),
            DataType::Cluster(f) => Value::Cluster(f.iter().map(|f| Value::default_for(&f.ty)).collect()),
        }
    }
    pub fn array1(data: Vec<Value>) -> Value {
        Value::Array(ArrayVal { dims: vec![data.len()], data })
    }
    pub fn from_f64_vec(v: &[f64]) -> Value {
        Value::array1(v.iter().map(|x| Value::Dbl(*x)).collect())
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Dbl(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Dbl(v) => Some(v.round() as i64),
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
    /// Flattened numeric elements, for plotting.
    pub fn to_f64_vec(&self) -> Vec<f64> {
        match self {
            Value::Array(a) => a.data.iter().filter_map(|v| v.as_f64()).collect(),
            _ => vec![],
        }
    }
    pub fn coerce(&self, to: &DataType) -> Option<Value> {
        match (self, to) {
            (Value::Bool(_), DataType::Bool) | (Value::Str(_), DataType::Str) => Some(self.clone()),
            (Value::Int(i), DataType::Num(k)) => Some(k.from_i64(*i)),
            (Value::Dbl(d), DataType::Num(k)) => Some(k.from_f64(*d)),
            (Value::Int(i), DataType::Enum(items)) => Some(Value::Int((*i).clamp(0, items.len().saturating_sub(1) as i64))),
            (Value::Array(a), DataType::Array(..)) if a.data.is_empty() => Some(Value::default_for(to)),
            (Value::Array(a), DataType::Array(e, n)) if a.dims.len() == *n => {
                let data = a.data.iter().map(|v| v.coerce(e)).collect::<Option<Vec<_>>>()?;
                Some(Value::Array(ArrayVal { dims: a.dims.clone(), data }))
            }
            (Value::Cluster(v), DataType::Cluster(f)) if v.len() == f.len() => {
                Some(Value::Cluster(v.iter().zip(f).map(|(v, f)| v.coerce(&f.ty)).collect::<Option<Vec<_>>>()?))
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Dbl(v) => write!(f, "{v}"),
            Value::Int(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{}", if *v { "TRUE" } else { "FALSE" }),
            Value::Str(v) => write!(f, "{v}"),
            Value::Array(a) => {
                write!(f, "[")?;
                for (i, v) in a.data.iter().take(8).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                if a.data.len() > 8 {
                    write!(f, ", …")?;
                }
                write!(f, "]")
            }
            Value::Cluster(v) => {
                write!(f, "{{")?;
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, "}}")
            }
        }
    }
}
