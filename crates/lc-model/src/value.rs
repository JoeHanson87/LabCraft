use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::types::{NumericType, Type};

/// Runtime numeric value. Widths live in the graph type; runtime numbers are
/// normalized to signed, unsigned or floating point so arithmetic is simple and
/// deterministic. Conversions (including wrap/clamp rules) happen at coercion
/// boundaries and when a value is written back into a typed terminal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum NumericValue {
    Int(i64),
    UInt(u64),
    Float(f64),
    Complex { re: f64, im: f64 },
}

impl NumericValue {
    pub fn zero() -> Self {
        Self::Int(0)
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Int(v) => *v as f64,
            Self::UInt(v) => *v as f64,
            Self::Float(v) => *v,
            Self::Complex { re, .. } => *re,
        }
    }
    pub fn as_i64(&self) -> i64 {
        match self {
            Self::Int(v) => *v,
            Self::UInt(v) => (*v).min(i64::MAX as u64) as i64,
            Self::Float(v) => v.round() as i64,
            Self::Complex { re, .. } => re.round() as i64,
        }
    }
    pub fn as_u64(&self) -> u64 {
        match self {
            Self::Int(v) => (*v).max(0) as u64,
            Self::UInt(v) => *v,
            Self::Float(v) => v.max(0.0).round() as u64,
            Self::Complex { re, .. } => re.max(0.0).round() as u64,
        }
    }
    pub fn is_complex(&self) -> bool {
        matches!(self, Self::Complex { .. })
    }

    /// Convert to a concrete numeric representation, saturating rather than
    /// panicking (integers wrap like LabVIEW for in-range casts, saturate on
    /// overflow).
    pub fn cast(&self, ty: NumericType) -> NumericValue {
        use NumericType::*;
        let float = |v: f64| match ty {
            Sgl => NumericValue::Float((v as f32) as f64),
            _ => NumericValue::Float(v),
        };
        match ty {
            Csg | Cdb => match self {
                NumericValue::Complex { re, im } => NumericValue::Complex { re: *re, im: *im },
                _ => NumericValue::Complex {
                    re: self.as_f64(),
                    im: 0.0,
                },
            },
            Sgl | Dbl | Ext => match self {
                NumericValue::Complex { re, .. } => float(*re),
                _ => float(self.as_f64()),
            },
            U8 => NumericValue::UInt(self.as_u64().min(u8::MAX as u64)),
            U16 => NumericValue::UInt(self.as_u64().min(u16::MAX as u64)),
            U32 => NumericValue::UInt(self.as_u64().min(u32::MAX as u64)),
            U64 => NumericValue::UInt(self.as_u64()),
            I8 => NumericValue::Int(self.as_i64().clamp(i8::MIN as i64, i8::MAX as i64)),
            I16 => NumericValue::Int(self.as_i64().clamp(i16::MIN as i64, i16::MAX as i64)),
            I32 => NumericValue::Int(self.as_i64().clamp(i32::MIN as i64, i32::MAX as i64)),
            I64 => NumericValue::Int(self.as_i64()),
        }
    }

    pub fn from_type(ty: NumericType, v: f64) -> Self {
        match ty {
            NumericType::Csg | NumericType::Cdb => NumericValue::Complex { re: v, im: 0.0 },
            NumericType::I8
            | NumericType::I16
            | NumericType::I32
            | NumericType::I64
            | NumericType::U8
            | NumericType::U16
            | NumericType::U32
            | NumericType::U64 => NumericValue::Int(v.round() as i64),
            _ => NumericValue::Float(v),
        }
    }
}

/// LabVIEW-style timestamp: seconds and fractions since 1904-01-01 UTC.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct Timestamp {
    pub seconds_since_1904: i64,
    pub fraction: u64,
}

impl Timestamp {
    pub fn now() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        // 1904-01-01 -> 1970-01-01 is 2082844800 seconds.
        Self {
            seconds_since_1904: now.as_secs() as i64 + 2_082_844_800,
            fraction: now.subsec_nanos() as u64 * 4_294_967_296 / 1_000_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ArrayValue {
    pub dims: Vec<usize>,
    pub data: Vec<Value>,
}

impl ArrayValue {
    pub fn new(dims: Vec<usize>, data: Vec<Value>) -> Self {
        Self { dims, data }
    }
    pub fn from_vec(data: Vec<Value>) -> Self {
        let n = data.len();
        Self {
            dims: vec![n],
            data,
        }
    }
    /// Total number of elements described by the declared dimensions.
    pub fn len(&self) -> usize {
        if self.dims.is_empty() {
            0
        } else {
            self.dims.iter().product()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.data.get(index)
    }
    /// Row-major stride for dimension `d`.
    pub fn stride(&self, d: usize) -> usize {
        self.dims[d + 1..].iter().product()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClusterValue {
    pub fields: Vec<Value>,
}

impl ClusterValue {
    pub fn new(fields: Vec<Value>) -> Self {
        Self { fields }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorCluster {
    pub status: bool,
    pub code: i64,
    pub source: String,
}

impl ErrorCluster {
    pub fn ok() -> Self {
        Self {
            status: false,
            code: 0,
            source: String::new(),
        }
    }
    pub fn err(code: i64, source: impl Into<String>) -> Self {
        Self {
            status: true,
            code,
            source: source.into(),
        }
    }
    pub fn message(&self) -> String {
        if self.status {
            format!("Error {} in {}", self.code, self.source)
        } else {
            "No error".into()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Waveform {
    pub t0: Timestamp,
    pub dt: f64,
    pub y: Vec<f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VariantValue {
    pub value: Box<Value>,
}

/// A runtime value. JSON representation is untagged so agents can write `1.5`,
/// `"text"`, `true`, `[1,2,3]` naturally. Arrays and clusters use explicit
/// wrapper objects because both would otherwise serialize as JSON arrays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Num(NumericValue),
    Str(String),
    Array(ArrayValue),
    Cluster(ClusterValue),
    Timestamp(Timestamp),
    Error(ErrorCluster),
    Waveform(Waveform),
    Variant(VariantValue),
    Void,
}

impl Value {
    pub fn num(v: f64) -> Self {
        Self::Num(NumericValue::Float(v))
    }
    pub fn int(v: i64) -> Self {
        Self::Num(NumericValue::Int(v))
    }
    pub fn string(v: impl Into<String>) -> Self {
        Self::Str(v.into())
    }
    pub fn void() -> Self {
        Self::Void
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            Value::Num(n) => n.as_f64() != 0.0,
            Value::Str(s) => !s.is_empty(),
            _ => false,
        }
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Num(n) => n.as_f64(),
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }
    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Num(n) => n.as_i64(),
            Value::Bool(b) => *b as i64,
            Value::Str(s) => s.trim().parse().unwrap_or(0),
            _ => 0,
        }
    }
    pub fn as_u64(&self) -> u64 {
        match self {
            Value::Num(n) => n.as_u64(),
            _ => self.as_i64().max(0) as u64,
        }
    }
    pub fn as_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Num(n) => {
                let f = n.as_f64();
                if f.fract() == 0.0 {
                    format!("{}", n.as_i64())
                } else {
                    format!("{f}")
                }
            }
            Value::Bool(b) => {
                if *b {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                }
            }
            Value::Error(e) => e.message(),
            other => serde_json::to_string(other).unwrap_or_default(),
        }
    }
    pub fn as_array(&self) -> Option<&ArrayValue> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_cluster(&self) -> Option<&ClusterValue> {
        match self {
            Value::Cluster(c) => Some(c),
            _ => None,
        }
    }
    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(e) if e.status)
    }

    /// Best-effort inferred type, used for defaults and diagnostics.
    pub fn type_of(&self) -> Type {
        match self {
            Value::Bool(_) => Type::Bool,
            Value::Num(NumericValue::Int(_)) => Type::numeric(NumericType::I64),
            Value::Num(NumericValue::UInt(_)) => Type::numeric(NumericType::U64),
            Value::Num(NumericValue::Float(_)) => Type::dbl(),
            Value::Num(NumericValue::Complex { .. }) => Type::numeric(NumericType::Cdb),
            Value::Str(_) => Type::String,
            Value::Array(a) => Type::Array {
                element: Box::new(a.data.first().map(Value::type_of).unwrap_or(Type::Void)),
                dims: a.dims.len().max(1),
            },
            Value::Cluster(c) => Type::Cluster {
                fields: c
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, v)| crate::ClusterField {
                        name: format!("{i}"),
                        ty: v.type_of(),
                    })
                    .collect(),
            },
            Value::Timestamp(_) => Type::Timestamp,
            Value::Error(_) => Type::Error,
            Value::Waveform(_) => Type::Waveform,
            Value::Variant(_) => Type::Variant,
            Value::Void => Type::Void,
        }
    }

    pub fn default_for(ty: &Type) -> Value {
        match ty {
            Type::Void => Value::Void,
            Type::Bool => Value::Bool(false),
            Type::Numeric { numeric } => Value::Num(NumericValue::from_type(*numeric, 0.0)),
            Type::String => Value::Str(String::new()),
            Type::Path => Value::Str(String::new()),
            Type::Timestamp => Value::Timestamp(Timestamp::default()),
            Type::Array { element: _, dims } => Value::Array(ArrayValue {
                dims: vec![0; *dims],
                data: Vec::new(),
            }),
            Type::Cluster { fields } => Value::Cluster(ClusterValue {
                fields: fields.iter().map(|f| Value::default_for(&f.ty)).collect(),
            }),
            Type::Enum { values: _ } => Value::Num(NumericValue::UInt(0)),
            Type::Variant => Value::Variant(VariantValue {
                value: Box::new(Value::Void),
            }),
            Type::Error => Value::Error(ErrorCluster::ok()),
            Type::Waveform => Value::Waveform(Waveform {
                t0: Timestamp::default(),
                dt: 1.0,
                y: Vec::new(),
                attributes: BTreeMap::new(),
            }),
            Type::Refnum { class } => Value::Str(format!("<{class} refnum>")),
            Type::Any => Value::Void,
        }
    }

    /// Coerce to the expected type. Numeric conversions always succeed; other
    /// mismatches return an explanation used in diagnostics.
    pub fn coerce_to(&self, ty: &Type) -> Result<Value, String> {
        if matches!(ty, Type::Any | Type::Void) {
            return Ok(self.clone());
        }
        match (self, ty) {
            (_, Type::Variant) => Ok(self.clone()),
            (Value::Variant(v), t) => v.value.coerce_to(t),
            (Value::Num(n), Type::Numeric { numeric }) => Ok(Value::Num(n.cast(*numeric))),
            (Value::Bool(b), Type::Numeric { numeric }) => Ok(Value::Num(NumericValue::from_type(
                *numeric,
                *b as i64 as f64,
            ))),
            (Value::Num(n), Type::Bool) => Ok(Value::Bool(n.as_f64() != 0.0)),
            (Value::Str(s), Type::Numeric { numeric }) => Ok(Value::Num(NumericValue::from_type(
                *numeric,
                s.trim().parse().unwrap_or(0.0),
            ))),
            (Value::Num(n), Type::String) => Ok(Value::Str(n.as_f64().to_string())),
            (Value::Bool(b), Type::String) => Ok(Value::Str(b.to_string())),
            (Value::Str(s), Type::String | Type::Path) => Ok(Value::Str(s.clone())),
            (Value::Array(a), Type::Array { element, dims }) => {
                if a.dims.len() != *dims {
                    return Err(format!(
                        "array rank mismatch: have {} dimension(s), need {dims}",
                        a.dims.len()
                    ));
                }
                let data = a
                    .data
                    .iter()
                    .map(|v| v.coerce_to(element))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Array(ArrayValue::new(a.dims.clone(), data)))
            }
            (Value::Cluster(c), Type::Cluster { fields }) if c.fields.len() == fields.len() => {
                let mut out = Vec::with_capacity(fields.len());
                for (v, f) in c.fields.iter().zip(fields) {
                    out.push(v.coerce_to(&f.ty)?);
                }
                Ok(Value::Cluster(ClusterValue { fields: out }))
            }
            (Value::Error(_), Type::Error) => Ok(self.clone()),
            (Value::Timestamp(_), Type::Timestamp) => Ok(self.clone()),
            (Value::Waveform(_), Type::Waveform) => Ok(self.clone()),
            (a, b) if a.type_of() == *b => Ok(self.clone()),
            _ => Err(format!(
                "cannot use {} where {} is expected",
                self.type_of().display(),
                ty.display()
            )),
        }
    }

    /// Compact single-line representation for UI terminals and logs.
    pub fn display(&self) -> String {
        match self {
            Value::Void => "<void>".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(NumericValue::Int(v)) => v.to_string(),
            Value::Num(NumericValue::UInt(v)) => v.to_string(),
            Value::Num(NumericValue::Float(v)) => {
                if v.fract() == 0.0 {
                    format!("{v:.0}")
                } else {
                    format!("{v}")
                }
            }
            Value::Num(NumericValue::Complex { re, im }) => format!("{re}+{im}i"),
            Value::Str(s) => s.clone(),
            Value::Array(a) => format!("[{}]", a.data.len()),
            Value::Cluster(c) => format!("cluster({})", c.fields.len()),
            Value::Timestamp(t) => format!("t={}", t.seconds_since_1904),
            Value::Error(e) => e.message(),
            Value::Waveform(w) => format!("waveform({})", w.y.len()),
            Value::Variant(v) => v.value.display(),
        }
    }
}
