//! The LabCraft VI DSL: a compact text format that agents can author directly.
//!
//! The DSL is the primary AI surface. It compiles to the canonical JSON IR
//! (`ViDocument`) and is intentionally boring: declarations, pipelines, and the
//! small set of structure forms that map one-to-one onto LabVIEW concepts.
//!
//! ```text
//! vi "Over Temperature" {
//!   description "Raises an alarm above the limit."
//!
//!   panel {
//!     control temp "Temperature" @(40, 60) { type: dbl, default: 25.0, unit: "C" }
//!     indicator alarm "Alarm" @(40, 150) { type: bool }
//!   }
//!
//!   diagram {
//!     let limit = 30.0
//!     temp -> greater(limit) -> alarm
//!   }
//! }
//! ```

pub mod ast;
pub mod build;
pub mod emit;
pub mod lexer;
pub mod parser;

use std::fmt;

pub use ast::Program;
pub use build::{build, refine_types, BuildOutcome};
pub use emit::emit;
pub use parser::parse;

/// A parse or build failure with a source location. Diagnostics are written for
/// agents: short, specific, and paired with a suggested repair when possible.
#[derive(Debug, Clone, PartialEq)]
pub struct DslError {
    pub message: String,
    pub line: usize,
    pub col: usize,
    pub hint: Option<String>,
}

impl DslError {
    pub fn new(message: impl Into<String>, line: usize, col: usize) -> Self {
        Self {
            message: message.into(),
            line,
            col,
            hint: None,
        }
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn render(&self) -> String {
        match &self.hint {
            Some(h) => format!("{}:{}: {}\n  hint: {h}", self.line, self.col, self.message),
            None => format!("{}:{}: {}", self.line, self.col, self.message),
        }
    }
}

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.hint {
            Some(h) => write!(
                f,
                "{}:{}: {} (hint: {h})",
                self.line, self.col, self.message
            ),
            None => write!(f, "{}:{}: {}", self.line, self.col, self.message),
        }
    }
}

impl std::error::Error for DslError {}
