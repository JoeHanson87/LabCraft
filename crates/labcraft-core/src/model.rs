use crate::types::{DataType, Value};
use serde::{Deserialize, Serialize};

pub type NodeId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    /// Front-panel control; its terminal on the diagram is a data source.
    Control { name: String, ty: DataType },
    /// Front-panel indicator; its terminal on the diagram is a data sink.
    Indicator { name: String, ty: DataType },
    Constant(Value),
    Add,
    Subtract,
    Multiply,
    Divide,
    And,
    Or,
    Not,
    Greater,
    Less,
    Equal,
    Concatenate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Terminal {
    pub name: &'static str,
    pub ty: DataType,
}

impl NodeKind {
    pub fn title(&self) -> String {
        match self {
            NodeKind::Control { name, .. } | NodeKind::Indicator { name, .. } => name.clone(),
            NodeKind::Constant(v) => v.to_string(),
            NodeKind::Add => "Add".into(),
            NodeKind::Subtract => "Subtract".into(),
            NodeKind::Multiply => "Multiply".into(),
            NodeKind::Divide => "Divide".into(),
            NodeKind::And => "And".into(),
            NodeKind::Or => "Or".into(),
            NodeKind::Not => "Not".into(),
            NodeKind::Greater => "Greater?".into(),
            NodeKind::Less => "Less?".into(),
            NodeKind::Equal => "Equal?".into(),
            NodeKind::Concatenate => "Concatenate Strings".into(),
        }
    }

    pub fn inputs(&self) -> Vec<Terminal> {
        use DataType::*;
        let t = |name, ty| Terminal { name, ty };
        match self {
            NodeKind::Control { .. } | NodeKind::Constant(_) => vec![],
            NodeKind::Indicator { ty, .. } => vec![t("in", *ty)],
            NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => {
                vec![t("x", Dbl), t("y", Dbl)]
            }
            NodeKind::And | NodeKind::Or => vec![t("x", Bool), t("y", Bool)],
            NodeKind::Not => vec![t("x", Bool)],
            NodeKind::Greater | NodeKind::Less | NodeKind::Equal => vec![t("x", Dbl), t("y", Dbl)],
            NodeKind::Concatenate => vec![t("a", Str), t("b", Str)],
        }
    }

    pub fn outputs(&self) -> Vec<Terminal> {
        use DataType::*;
        let t = |name, ty| Terminal { name, ty };
        match self {
            NodeKind::Control { ty, .. } => vec![t("out", *ty)],
            NodeKind::Indicator { .. } => vec![],
            NodeKind::Constant(v) => vec![t("out", v.data_type())],
            NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => {
                vec![t("result", Dbl)]
            }
            NodeKind::And | NodeKind::Or | NodeKind::Not => vec![t("result", Bool)],
            NodeKind::Greater | NodeKind::Less | NodeKind::Equal => vec![t("result", Bool)],
            NodeKind::Concatenate => vec![t("result", Str)],
        }
    }

    pub fn is_panel_item(&self) -> bool {
        matches!(self, NodeKind::Control { .. } | NodeKind::Indicator { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Position on the block diagram.
    pub pos: [f32; 2],
    /// Position on the front panel (controls/indicators only).
    pub panel_pos: [f32; 2],
    /// Current value for controls (user input) and indicators (last result).
    pub value: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire {
    pub from: (NodeId, usize),
    pub to: (NodeId, usize),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Vi {
    pub name: String,
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    next_id: NodeId,
}

impl Vi {
    pub fn new(name: impl Into<String>) -> Self {
        Vi { name: name.into(), next_id: 1, ..Default::default() }
    }
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("VI serializes")
    }
    pub fn from_json(s: &str) -> Result<Vi, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EditError {
    #[error("no such node {0}")]
    NoNode(NodeId),
    #[error("node {0} has no such terminal {1}")]
    NoTerminal(NodeId, usize),
    #[error("type mismatch: {0:?} cannot feed {1:?}")]
    TypeMismatch(DataType, DataType),
    #[error("connection would create a cycle")]
    Cycle,
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
}

/// Every edit, from the GUI or an agent, goes through a `Command`.
#[derive(Clone, Debug)]
pub enum Command {
    AddNode { kind: NodeKind, pos: [f32; 2] },
    RemoveNode(NodeId),
    MoveNode { id: NodeId, pos: [f32; 2] },
    MovePanelItem { id: NodeId, pos: [f32; 2] },
    Connect { from: (NodeId, usize), to: (NodeId, usize) },
    Disconnect { to: (NodeId, usize) },
    SetValue { id: NodeId, value: Value },
}

/// Owns a VI plus undo/redo history.
#[derive(Default)]
pub struct Editor {
    pub vi: Vi,
    undo: Vec<Vi>,
    redo: Vec<Vi>,
}

impl Editor {
    pub fn new(vi: Vi) -> Self {
        Editor { vi, undo: vec![], redo: vec![] }
    }

    /// Apply a command. Returns the id of a newly created node, if any.
    pub fn apply(&mut self, cmd: Command) -> Result<Option<NodeId>, EditError> {
        let snapshot = self.vi.clone();
        let mut next = snapshot.clone();
        let created = Self::exec(&mut next, cmd)?;
        self.undo.push(snapshot);
        self.redo.clear();
        self.vi = next;
        Ok(created)
    }

    pub fn undo(&mut self) -> Result<(), EditError> {
        let prev = self.undo.pop().ok_or(EditError::NothingToUndo)?;
        self.redo.push(std::mem::replace(&mut self.vi, prev));
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), EditError> {
        let next = self.redo.pop().ok_or(EditError::NothingToRedo)?;
        self.undo.push(std::mem::replace(&mut self.vi, next));
        Ok(())
    }

    fn exec(vi: &mut Vi, cmd: Command) -> Result<Option<NodeId>, EditError> {
        match cmd {
            Command::AddNode { kind, pos } => {
                let id = vi.next_id;
                vi.next_id += 1;
                let value = match &kind {
                    NodeKind::Control { ty, .. } | NodeKind::Indicator { ty, .. } => {
                        Some(Value::default_for(*ty))
                    }
                    NodeKind::Constant(v) => Some(v.clone()),
                    _ => None,
                };
                let n = vi.nodes.iter().filter(|n| n.kind.is_panel_item()).count() as f32;
                vi.nodes.push(Node {
                    id,
                    kind,
                    pos,
                    panel_pos: [40.0 + (n % 3.0) * 200.0, 40.0 + (n / 3.0).floor() * 90.0],
                    value,
                });
                Ok(Some(id))
            }
            Command::RemoveNode(id) => {
                vi.node(id).ok_or(EditError::NoNode(id))?;
                vi.nodes.retain(|n| n.id != id);
                vi.wires.retain(|w| w.from.0 != id && w.to.0 != id);
                Ok(None)
            }
            Command::MoveNode { id, pos } => {
                vi.node_mut(id).ok_or(EditError::NoNode(id))?.pos = pos;
                Ok(None)
            }
            Command::MovePanelItem { id, pos } => {
                vi.node_mut(id).ok_or(EditError::NoNode(id))?.panel_pos = pos;
                Ok(None)
            }
            Command::Connect { from, to } => {
                let src = vi.node(from.0).ok_or(EditError::NoNode(from.0))?;
                let dst = vi.node(to.0).ok_or(EditError::NoNode(to.0))?;
                let st = src.kind.outputs().get(from.1).cloned().ok_or(EditError::NoTerminal(from.0, from.1))?;
                let dt = dst.kind.inputs().get(to.1).cloned().ok_or(EditError::NoTerminal(to.0, to.1))?;
                if !st.ty.can_feed(dt.ty) {
                    return Err(EditError::TypeMismatch(st.ty, dt.ty));
                }
                if from.0 == to.0 || reaches(vi, to.0, from.0) {
                    return Err(EditError::Cycle);
                }
                vi.wires.retain(|w| w.to != to);
                vi.wires.push(Wire { from, to });
                Ok(None)
            }
            Command::Disconnect { to } => {
                vi.wires.retain(|w| w.to != to);
                Ok(None)
            }
            Command::SetValue { id, value } => {
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                n.value = Some(value);
                Ok(None)
            }
        }
    }
}

/// True if data flows from `start` to `target` through existing wires.
fn reaches(vi: &Vi, start: NodeId, target: NodeId) -> bool {
    let mut stack = vec![start];
    let mut seen = vec![];
    while let Some(n) = stack.pop() {
        if n == target {
            return true;
        }
        if seen.contains(&n) {
            continue;
        }
        seen.push(n);
        stack.extend(vi.wires.iter().filter(|w| w.from.0 == n).map(|w| w.to.0));
    }
    false
}
