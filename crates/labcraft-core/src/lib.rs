//! LabCRAFT core: the headless VI model, command API and dataflow evaluator.
//! The GUI and (later) the agent interface are both clients of this crate.

pub mod eval;
pub mod model;
pub mod ops;
pub mod typing;
pub mod types;

pub use eval::{run, run_with, RunControl, RunError, RunResult};
pub use model::{Command, EditError, Editor, LoopKind, Node, NodeId, NodeKind, Spec, Terminal, Vi, Widget, Wire, LOOP_BORDER, NODE_W};
pub use types::{ArrayVal, DataType, Field, NumKind, Value};
pub use typing::{coercion_dots, resolve, TypeMap};
