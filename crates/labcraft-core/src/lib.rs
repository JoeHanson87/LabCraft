//! LabCRAFT core: the headless VI model, command API and dataflow evaluator.
//! The GUI and (later) the agent interface are both clients of this crate.

pub mod eval;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod panel_tests;
pub mod model;
pub mod ops;
pub mod panel;
pub mod typing;
pub mod types;

pub use eval::{run, run_with, RunControl, RunError, RunResult};
pub use model::{chart_default, Command, EditError, Editor, EventFrame, LoopKind, Node, NodeId, NodeKind, Spec, Terminal, Vi, Widget, Wire, LOOP_BORDER, MAX_FRAMES, NODE_W};
pub use panel::{arrange_rects, Arrange, DecoKind, GraphConfig, Prop};
pub use types::{ArrayVal, DataType, Field, NumKind, Value};
pub use typing::{coercion_dots, resolve, TypeMap};
