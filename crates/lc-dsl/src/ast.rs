//! Syntax tree for the VI DSL. Kept public so tools can parse without building,
//! and so the parser tests can assert on structure.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum LitValue {
    Num { value: f64, is_int: bool },
    Str(String),
    Bool(bool),
    Ident(String),
    List(Vec<LitValue>),
}

impl LitValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            LitValue::Num { value, .. } => Some(*value),
            LitValue::Ident(s) => s.parse().ok(),
            LitValue::Str(s) => s.parse().ok(),
            LitValue::Bool(b) => Some(*b as i64 as f64),
            LitValue::List(_) => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            LitValue::Str(s) | LitValue::Ident(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            LitValue::Bool(b) => Some(*b),
            LitValue::Ident(s) if s == "true" => Some(true),
            LitValue::Ident(s) if s == "false" => Some(false),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[LitValue]> {
        match self {
            LitValue::List(items) => Some(items),
            _ => None,
        }
    }
}

impl fmt::Display for LitValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LitValue::Num { value, is_int } => {
                if *is_int {
                    write!(f, "{}", *value as i64)
                } else {
                    write!(f, "{value}")
                }
            }
            LitValue::Str(s) => write!(f, "\"{}\"", escape(s)),
            LitValue::Bool(b) => write!(f, "{b}"),
            LitValue::Ident(s) => write!(f, "{s}"),
            LitValue::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
        }
    }
}

pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub name: String,
    pub description: String,
    pub panel: Vec<PanelDecl>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelDecl {
    /// `true` for an indicator, `false` for a control.
    pub indicator: bool,
    pub key: String,
    pub label: String,
    pub pos: Option<(f32, f32)>,
    pub props: BTreeMap<String, LitValue>,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: usize,
    pub col: usize,
}

impl Stmt {
    pub fn new(kind: StmtKind, line: usize, col: usize) -> Self {
        Self { kind, line, col }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let {
        name: String,
        value: Expr,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
    },
    Case {
        selector: Expr,
        frames: Vec<Frame>,
    },
    Event {
        timeout: Option<Expr>,
        frames: Vec<Frame>,
    },
    Sequence {
        frames: Vec<Vec<Stmt>>,
    },
    /// Declare a value flowing out of the enclosing structure.
    Tunnel {
        name: String,
        value: Expr,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub label: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub name: String,
    pub args: Vec<Arg>,
    /// `true` when written with parentheses (`random()`), false for a bare
    /// node name (`temp -> not -> alarm`).
    pub parens: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(LitValue),
    Ident(String),
    Call {
        name: String,
        args: Vec<Arg>,
    },
    Array(Vec<Expr>),
    Cluster(Vec<(String, Expr)>),
    Pipe {
        start: Box<Expr>,
        steps: Vec<Step>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
    /// Structures can be used as expressions, which lets a VI bind a loop's
    /// output tunnels: `let totals = while ... { ... }`.
    Structure(Box<Stmt>),
}

impl Expr {
    pub fn ident(name: impl Into<String>) -> Self {
        Expr::Ident(name.into())
    }
}
