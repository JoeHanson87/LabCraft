//! LabCRAFT core: the headless VI model, command API and dataflow evaluator.
//! The GUI and (later) the agent interface are both clients of this crate.

pub mod eval;
pub mod model;
pub mod types;

pub use eval::{run, RunError, RunResult};
pub use model::{Command, Editor, EditError, Node, NodeId, NodeKind, Terminal, Vi, Wire};
pub use types::{DataType, Value};
