use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    Dbl,
    Int,
    Bool,
    Str,
}

impl DataType {
    /// Whether a wire of type `self` may feed a terminal of type `to` (with coercion).
    pub fn can_feed(self, to: DataType) -> bool {
        self == to || (self == DataType::Int && to == DataType::Dbl)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Dbl(f64),
    Int(i64),
    Bool(bool),
    Str(String),
}

impl Value {
    pub fn data_type(&self) -> DataType {
        match self {
            Value::Dbl(_) => DataType::Dbl,
            Value::Int(_) => DataType::Int,
            Value::Bool(_) => DataType::Bool,
            Value::Str(_) => DataType::Str,
        }
    }
    pub fn default_for(t: DataType) -> Value {
        match t {
            DataType::Dbl => Value::Dbl(0.0),
            DataType::Int => Value::Int(0),
            DataType::Bool => Value::Bool(false),
            DataType::Str => Value::Str(String::new()),
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Dbl(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
    pub fn coerce(&self, to: DataType) -> Option<Value> {
        match (self, to) {
            (v, t) if v.data_type() == t => Some(v.clone()),
            (Value::Int(i), DataType::Dbl) => Some(Value::Dbl(*i as f64)),
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
        }
    }
}
