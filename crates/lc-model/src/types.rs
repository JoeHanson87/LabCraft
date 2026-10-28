use serde::{Deserialize, Serialize};

/// Numeric representation. Widths are tracked in the graph type, while runtime
/// values normalize to `i64` / `u64` / `f64` (see [`crate::value::NumericValue`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NumericType {
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
    Ext,
    Csg,
    Cdb,
}

impl NumericType {
    pub fn is_float(self) -> bool {
        matches!(self, Self::Sgl | Self::Dbl | Self::Ext)
    }
    pub fn is_complex(self) -> bool {
        matches!(self, Self::Csg | Self::Cdb)
    }
    pub fn is_signed(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::Sgl | Self::Dbl | Self::Ext
        )
    }
    pub fn rank(self) -> u8 {
        match self {
            Self::I8 => 1,
            Self::U8 => 2,
            Self::I16 => 3,
            Self::U16 => 4,
            Self::I32 => 5,
            Self::U32 => 6,
            Self::I64 => 7,
            Self::U64 => 8,
            Self::Sgl => 9,
            Self::Dbl => 10,
            Self::Ext => 11,
            Self::Csg => 12,
            Self::Cdb => 13,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::I8 => "I8",
            Self::I16 => "I16",
            Self::I32 => "I32",
            Self::I64 => "I64",
            Self::U8 => "U8",
            Self::U16 => "U16",
            Self::U32 => "U32",
            Self::U64 => "U64",
            Self::Sgl => "SGL",
            Self::Dbl => "DBL",
            Self::Ext => "EXT",
            Self::Csg => "CSG",
            Self::Cdb => "CDB",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClusterField {
    pub name: String,
    pub ty: Type,
}

/// The LabCraft type system.
///
/// Arrays are n-dimensional and flattened row-major. LabVIEW has no jagged
/// arrays, so an array's element is never itself an array of unknown rank.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Type {
    Void,
    Bool,
    Numeric { numeric: NumericType },
    String,
    Path,
    Timestamp,
    Array { element: Box<Type>, dims: usize },
    Cluster { fields: Vec<ClusterField> },
    Enum { values: Vec<String> },
    Variant,
    Error,
    Waveform,
    Refnum { class: String },
    Any,
}

impl Type {
    pub const BOOL: Type = Type::Bool;
    pub const STRING: Type = Type::String;
    pub const ERROR: Type = Type::Error;

    pub fn dbl() -> Self {
        Type::Numeric {
            numeric: NumericType::Dbl,
        }
    }
    pub fn i32() -> Self {
        Type::Numeric {
            numeric: NumericType::I32,
        }
    }
    pub fn numeric(n: NumericType) -> Self {
        Type::Numeric { numeric: n }
    }
    pub fn array_of(element: Type) -> Self {
        Type::Array {
            element: Box::new(element),
            dims: 1,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Numeric { .. })
    }
    pub fn is_void(&self) -> bool {
        matches!(self, Type::Void)
    }
    pub fn as_numeric(&self) -> Option<NumericType> {
        match self {
            Type::Numeric { numeric } => Some(*numeric),
            _ => None,
        }
    }
    pub fn element(&self) -> Option<&Type> {
        match self {
            Type::Array { element, .. } => Some(element),
            _ => None,
        }
    }
    pub fn dims(&self) -> usize {
        match self {
            Type::Array { dims, .. } => *dims,
            _ => 0,
        }
    }

    /// Short human/agent readable name, used in diagnostics and pin maps.
    pub fn display(&self) -> String {
        match self {
            Type::Void => "void".into(),
            Type::Bool => "bool".into(),
            Type::Numeric { numeric } => numeric.name().to_string(),
            Type::String => "string".into(),
            Type::Path => "path".into(),
            Type::Timestamp => "timestamp".into(),
            Type::Array { element, dims } => {
                format!("{}{}", element.display(), "[]".repeat(*dims))
            }
            Type::Cluster { fields } => {
                let inner = fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, f.ty.display()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("cluster{{{inner}}}")
            }
            Type::Enum { values } => format!("enum{{{}}}", values.join("|")),
            Type::Variant => "variant".into(),
            Type::Error => "error cluster".into(),
            Type::Waveform => "waveform".into(),
            Type::Refnum { class } => format!("refnum<{class}>"),
            Type::Any => "any".into(),
        }
    }

    /// Is a value of `other` acceptable where `self` is expected without an
    /// explicit conversion node? Numeric widening and any->T are allowed; the
    /// runtime inserts a coercion step with a visible coercion dot.
    pub fn accepts(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Any, _) | (_, Type::Any) => true,
            (Type::Void, Type::Void) => true,
            (Type::Numeric { .. }, Type::Numeric { .. }) => true,
            (Type::Bool, Type::Bool) => true,
            (Type::String, Type::String) => true,
            (Type::Path, Type::Path) => true,
            (Type::Timestamp, Type::Timestamp) => true,
            (Type::Error, Type::Error) => true,
            (Type::Waveform, Type::Waveform) => true,
            (Type::Variant, _) | (_, Type::Variant) => true,
            (Type::Enum { values: a }, Type::Enum { values: b }) => a == b,
            (Type::Enum { .. }, Type::Numeric { .. }) => true,
            (
                Type::Array {
                    element: a,
                    dims: da,
                },
                Type::Array {
                    element: b,
                    dims: db,
                },
            ) => da == db && a.accepts(b),
            (Type::Cluster { fields: a }, Type::Cluster { fields: b }) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.ty.accepts(&y.ty))
            }
            (Type::Refnum { class: a }, Type::Refnum { class: b }) => a == b,
            _ => false,
        }
    }

    /// Numeric promotion used by arithmetic nodes, matching LabVIEW's general
    /// rule: the wider representation wins, floats beat integers.
    pub fn promote_numeric(a: NumericType, b: NumericType) -> NumericType {
        if a.is_complex() || b.is_complex() {
            if a == NumericType::Cdb || b == NumericType::Cdb {
                NumericType::Cdb
            } else {
                NumericType::Csg
            }
        } else if a.is_float() || b.is_float() {
            use NumericType::*;
            let f = |n: NumericType| match n {
                Sgl => 9,
                Dbl => 10,
                Ext => 11,
                _ => 0,
            };
            match f(a).max(f(b)) {
                9 => Sgl,
                10 => Dbl,
                _ => Ext,
            }
        } else if a == NumericType::U64 || b == NumericType::U64 {
            NumericType::U64
        } else if a == NumericType::I64 || b == NumericType::I64 {
            NumericType::I64
        } else if a.is_signed() || b.is_signed() {
            NumericType::I32
        } else {
            NumericType::U32
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConnectorPin {
    pub name: String,
    pub ty: Type,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Input,
    Output,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConnectorPane {
    pub inputs: Vec<ConnectorPin>,
    pub outputs: Vec<ConnectorPin>,
}
