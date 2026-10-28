use crate::types::{DataType, Value};
use serde::{Deserialize, Serialize};

pub type NodeId = u64;

pub const NODE_W: f32 = 120.0;
/// Loop border thickness used by terminal layout.
pub const LOOP_BORDER: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopKind {
    While,
    For,
}

/// How a control or indicator is rendered on the front panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Widget {
    NumCtl,
    Knob,
    Slide,
    NumInd,
    Gauge,
    Thermometer,
    Tank,
    Toggle,
    PushButton,
    Led,
    StrCtl,
    StrInd,
    Chart,
    Graph,
}

impl Widget {
    pub fn default_for(kind: &NodeKind) -> Widget {
        match kind {
            NodeKind::Control { ty, .. } => match ty {
                DataType::Bool => Widget::Toggle,
                DataType::Str => Widget::StrCtl,
                _ => Widget::NumCtl,
            },
            NodeKind::Indicator { ty, .. } => match ty {
                DataType::Bool => Widget::Led,
                DataType::Str => Widget::StrInd,
                DataType::ArrDbl => Widget::Graph,
                _ => Widget::NumInd,
            },
            _ => Widget::NumCtl,
        }
    }
    pub fn panel_size(self) -> [f32; 2] {
        match self {
            Widget::NumCtl | Widget::NumInd => [170.0, 48.0],
            Widget::Knob => [130.0, 150.0],
            Widget::Slide => [250.0, 66.0],
            Widget::Gauge => [190.0, 150.0],
            Widget::Thermometer => [100.0, 180.0],
            Widget::Tank => [110.0, 180.0],
            Widget::Toggle => [100.0, 70.0],
            Widget::PushButton => [120.0, 68.0],
            Widget::Led => [80.0, 80.0],
            Widget::StrCtl | Widget::StrInd => [210.0, 52.0],
            Widget::Chart | Widget::Graph => [360.0, 220.0],
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Widget::NumCtl => "Numeric Control",
            Widget::Knob => "Knob",
            Widget::Slide => "Horizontal Slide",
            Widget::NumInd => "Numeric Indicator",
            Widget::Gauge => "Gauge",
            Widget::Thermometer => "Thermometer",
            Widget::Tank => "Tank",
            Widget::Toggle => "Toggle Switch",
            Widget::PushButton => "Push Button",
            Widget::Led => "Round LED",
            Widget::StrCtl => "String Control",
            Widget::StrInd => "String Indicator",
            Widget::Chart => "Waveform Chart",
            Widget::Graph => "Waveform Graph",
        }
    }
    pub fn is_numeric_scaled(self) -> bool {
        matches!(self, Widget::Knob | Widget::Slide | Widget::Gauge | Widget::Thermometer | Widget::Tank)
    }
}

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
    Sine,
    Random,
    WaitMs,
    BuildArray,
    ArraySize,
    IndexArray,
    /// A While or For loop structure with shift registers of the given types.
    Loop { kind: LoopKind, regs: Vec<DataType> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Terminal {
    pub name: String,
    pub ty: DataType,
    /// Terminal sits inside a structure's border (not on the outer side).
    pub inner: bool,
    /// Unwired optional inputs take the type's default value.
    pub optional: bool,
}

impl Terminal {
    fn new(name: &str, ty: DataType) -> Self {
        Terminal { name: name.into(), ty, inner: false, optional: false }
    }
    fn inner(mut self) -> Self {
        self.inner = true;
        self
    }
    fn opt(mut self) -> Self {
        self.optional = true;
        self
    }
}

impl NodeKind {
    pub fn title(&self) -> String {
        match self {
            NodeKind::Control { name, .. } | NodeKind::Indicator { name, .. } => name.clone(),
            NodeKind::Constant(_) => "Constant".into(),
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
            NodeKind::Sine => "Sine".into(),
            NodeKind::Random => "Random Number".into(),
            NodeKind::WaitMs => "Wait (ms)".into(),
            NodeKind::BuildArray => "Build Array".into(),
            NodeKind::ArraySize => "Array Size".into(),
            NodeKind::IndexArray => "Index Array".into(),
            NodeKind::Loop { kind: LoopKind::While, .. } => "While Loop".into(),
            NodeKind::Loop { kind: LoopKind::For, .. } => "For Loop".into(),
        }
    }

    /// Input terminals. For loops: `[N|stop, init0, next0, init1, next1, ...]`.
    pub fn inputs(&self) -> Vec<Terminal> {
        use DataType::*;
        let t = Terminal::new;
        match self {
            NodeKind::Control { .. } | NodeKind::Constant(_) | NodeKind::Random => vec![],
            NodeKind::Indicator { ty, .. } => vec![t("in", *ty)],
            NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => {
                vec![t("x", Dbl), t("y", Dbl)]
            }
            NodeKind::And | NodeKind::Or => vec![t("x", Bool), t("y", Bool)],
            NodeKind::Not => vec![t("x", Bool)],
            NodeKind::Greater | NodeKind::Less | NodeKind::Equal => vec![t("x", Dbl), t("y", Dbl)],
            NodeKind::Concatenate => vec![t("a", Str), t("b", Str)],
            NodeKind::Sine => vec![t("x", Dbl)],
            NodeKind::WaitMs => vec![t("ms", Int)],
            NodeKind::BuildArray => vec![t("array", ArrDbl).opt(), t("element", Dbl)],
            NodeKind::ArraySize => vec![t("array", ArrDbl)],
            NodeKind::IndexArray => vec![t("array", ArrDbl), t("index", Int)],
            NodeKind::Loop { kind, regs } => {
                let mut v = vec![match kind {
                    LoopKind::For => t("N", Int),
                    LoopKind::While => t("stop", Bool).inner(),
                }];
                for ty in regs {
                    v.push(t("init", *ty).opt());
                    v.push(t("next", *ty).inner().opt());
                }
                v
            }
        }
    }

    /// Output terminals. For loops: `[i, cur0, final0, cur1, final1, ...]`.
    pub fn outputs(&self) -> Vec<Terminal> {
        use DataType::*;
        let t = Terminal::new;
        match self {
            NodeKind::Control { ty, .. } => vec![t("out", *ty)],
            NodeKind::Indicator { .. } | NodeKind::WaitMs => vec![],
            NodeKind::Constant(v) => vec![t("out", v.data_type())],
            NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => {
                vec![t("result", Dbl)]
            }
            NodeKind::And | NodeKind::Or | NodeKind::Not => vec![t("result", Bool)],
            NodeKind::Greater | NodeKind::Less | NodeKind::Equal => vec![t("result", Bool)],
            NodeKind::Concatenate => vec![t("result", Str)],
            NodeKind::Sine => vec![t("sin", Dbl)],
            NodeKind::Random => vec![t("rand", Dbl)],
            NodeKind::BuildArray => vec![t("result", ArrDbl)],
            NodeKind::ArraySize => vec![t("size", Int)],
            NodeKind::IndexArray => vec![t("element", Dbl)],
            NodeKind::Loop { regs, .. } => {
                let mut v = vec![t("i", Int).inner()];
                for ty in regs {
                    v.push(t("cur", *ty).inner());
                    v.push(t("out", *ty));
                }
                v
            }
        }
    }

    pub fn is_panel_item(&self) -> bool {
        matches!(self, NodeKind::Control { .. } | NodeKind::Indicator { .. })
    }

    pub fn is_loop(&self) -> bool {
        matches!(self, NodeKind::Loop { .. })
    }

    pub fn default_size(&self) -> [f32; 2] {
        if self.is_loop() {
            return [300.0, 200.0];
        }
        let rows = self.inputs().len().max(self.outputs().len()).max(1) as f32;
        let h = if matches!(self, NodeKind::Constant(_)) { 62.0 } else { 28.0 + rows * 22.0 };
        [NODE_W, h]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Top-left position on the block diagram.
    pub pos: [f32; 2],
    /// Diagram size (meaningful for structures; fixed for other nodes).
    pub size: [f32; 2],
    /// Top-left position on the front panel (controls/indicators only).
    pub panel_pos: [f32; 2],
    pub widget: Widget,
    /// Scale range for knobs, gauges, slides, tanks and thermometers.
    pub range: [f64; 2],
    /// Enclosing structure, if this node sits inside one.
    pub parent: Option<NodeId>,
    /// Current value for controls (user input), indicators (last result) and constants.
    pub value: Option<Value>,
}

impl Node {
    pub fn loop_reg_y(&self, k: usize) -> f32 {
        self.pos[1] + 52.0 + 30.0 * k as f32
    }

    /// Absolute diagram position of a terminal.
    pub fn terminal_pos(&self, idx: usize, output: bool) -> [f32; 2] {
        let [x, y] = self.pos;
        let [w, h] = self.size;
        if let NodeKind::Loop { kind, .. } = &self.kind {
            let b = LOOP_BORDER;
            return if output {
                match idx {
                    0 => [x + b + 8.0, y + h - b - 12.0],
                    n if n % 2 == 1 => [x + b, self.loop_reg_y((n - 1) / 2)],
                    n => [x + w, self.loop_reg_y((n - 2) / 2)],
                }
            } else {
                match idx {
                    0 => match kind {
                        LoopKind::For => [x, y + 28.0],
                        LoopKind::While => [x + w - b - 8.0, y + h - b - 12.0],
                    },
                    n if n % 2 == 1 => [x, self.loop_reg_y((n - 1) / 2)],
                    n => [x + w - b, self.loop_reg_y((n - 2) / 2)],
                }
            };
        }
        let count = if output { self.kind.outputs().len() } else { self.kind.inputs().len() }.max(1) as f32;
        let ty = 28.0 + (idx as f32 + 0.5) * (h - 28.0) / count;
        [x + if output { w } else { 0.0 }, y + ty]
    }
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

    /// True if `id` is nested (at any depth) inside `ancestor`.
    pub fn is_descendant(&self, id: NodeId, ancestor: NodeId) -> bool {
        let mut cur = self.node(id).and_then(|n| n.parent);
        let mut guard = 0;
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            cur = self.node(p).and_then(|n| n.parent);
            guard += 1;
            if guard > 1000 {
                break;
            }
        }
        false
    }

    pub fn depth(&self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = self.node(id).and_then(|n| n.parent);
        while let Some(p) = cur {
            d += 1;
            cur = self.node(p).and_then(|n| n.parent);
            if d > 1000 {
                break;
            }
        }
        d
    }

    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes.iter().filter(|n| self.is_descendant(n.id, id)).map(|n| n.id).collect()
    }

    /// Innermost loop containing `point`, ignoring `exclude` and everything inside it.
    pub fn container_at(&self, point: [f32; 2], exclude: Option<NodeId>) -> Option<NodeId> {
        self.nodes
            .iter()
            .filter(|n| n.kind.is_loop())
            .filter(|n| Some(n.id) != exclude && !exclude.is_some_and(|e| self.is_descendant(n.id, e)))
            .filter(|n| {
                point[0] >= n.pos[0] && point[0] <= n.pos[0] + n.size[0] && point[1] >= n.pos[1] && point[1] <= n.pos[1] + n.size[1]
            })
            .min_by(|a, b| (a.size[0] * a.size[1]).total_cmp(&(b.size[0] * b.size[1])))
            .map(|n| n.id)
    }

    fn free_panel_spot(&self, size: [f32; 2]) -> [f32; 2] {
        let used: Vec<(NodeId, [f32; 2], [f32; 2])> = self
            .nodes
            .iter()
            .filter(|n| n.kind.is_panel_item())
            .map(|n| (n.id, n.panel_pos, n.widget.panel_size()))
            .collect();
        let mut y = 20.0;
        while y < 4000.0 {
            let mut x = 20.0;
            while x < 700.0 {
                let free = used.iter().all(|(_, p, s)| {
                    x + size[0] + 12.0 <= p[0] || p[0] + s[0] + 12.0 <= x || y + size[1] + 12.0 <= p[1] || p[1] + s[1] + 12.0 <= y
                });
                if free {
                    return [x, y];
                }
                x += 20.0;
            }
            y += 20.0;
        }
        [20.0, 20.0]
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
    #[error("a structure's inner terminal can only feed nodes inside that structure")]
    Scope,
    #[error("node {0} is not a loop")]
    NotALoop(NodeId),
    #[error("loop has no shift registers")]
    NoShiftRegister,
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
}

/// Every edit, from the GUI or an agent, goes through a `Command`.
#[derive(Clone, Debug)]
pub enum Command {
    AddNode { kind: NodeKind, pos: [f32; 2], widget: Option<Widget> },
    RemoveNode(NodeId),
    MoveNode { id: NodeId, pos: [f32; 2] },
    MovePanelItem { id: NodeId, pos: [f32; 2] },
    Resize { id: NodeId, size: [f32; 2] },
    Connect { from: (NodeId, usize), to: (NodeId, usize) },
    Disconnect { to: (NodeId, usize) },
    SetValue { id: NodeId, value: Value },
    SetWidget { id: NodeId, widget: Widget },
    SetRange { id: NodeId, range: [f64; 2] },
    SetParent { id: NodeId, parent: Option<NodeId> },
    AddShiftRegister { id: NodeId, ty: DataType },
    RemoveShiftRegister { id: NodeId },
    /// Re-evaluates which structure a node sits in after a drag.
    Drop(NodeId),
    /// Ends a drag gesture so the next move starts a new undo step.
    EndGesture,
}

/// Owns a VI plus undo/redo history.
#[derive(Default)]
pub struct Editor {
    pub vi: Vi,
    undo: Vec<Vi>,
    redo: Vec<Vi>,
    last_key: Option<(u8, NodeId)>,
}

impl Editor {
    pub fn new(vi: Vi) -> Self {
        Editor { vi, ..Default::default() }
    }

    /// Apply a command. Returns the id of a newly created node, if any.
    /// Value changes are runtime state and are not undoable; consecutive moves of
    /// the same node coalesce into one undo step.
    pub fn apply(&mut self, cmd: Command) -> Result<Option<NodeId>, EditError> {
        if matches!(cmd, Command::EndGesture) {
            self.last_key = None;
            return Ok(None);
        }
        let key = match &cmd {
            Command::MoveNode { id, .. } => Some((0, *id)),
            Command::MovePanelItem { id, .. } => Some((1, *id)),
            Command::Resize { id, .. } => Some((2, *id)),
            _ => None,
        };
        let record = !matches!(cmd, Command::SetValue { .. }) && !(key.is_some() && key == self.last_key);
        let mut next = self.vi.clone();
        let created = Self::exec(&mut next, cmd)?;
        if record {
            self.undo.push(std::mem::replace(&mut self.vi, next));
            self.redo.clear();
        } else {
            self.vi = next;
        }
        if !matches!(key, None) || record {
            self.last_key = key;
        }
        Ok(created)
    }

    pub fn undo(&mut self) -> Result<(), EditError> {
        let prev = self.undo.pop().ok_or(EditError::NothingToUndo)?;
        self.redo.push(std::mem::replace(&mut self.vi, prev));
        self.last_key = None;
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), EditError> {
        let next = self.redo.pop().ok_or(EditError::NothingToRedo)?;
        self.undo.push(std::mem::replace(&mut self.vi, next));
        self.last_key = None;
        Ok(())
    }

    fn exec(vi: &mut Vi, cmd: Command) -> Result<Option<NodeId>, EditError> {
        match cmd {
            Command::AddNode { kind, pos, widget } => {
                let id = vi.next_id;
                vi.next_id += 1;
                let widget = widget.unwrap_or_else(|| Widget::default_for(&kind));
                let value = match &kind {
                    NodeKind::Control { ty, .. } | NodeKind::Indicator { ty, .. } => {
                        if widget == Widget::Chart { Some(Value::ArrDbl(vec![])) } else { Some(Value::default_for(*ty)) }
                    }
                    NodeKind::Constant(v) => Some(v.clone()),
                    _ => None,
                };
                let size = kind.default_size();
                let panel_pos = if kind.is_panel_item() { vi.free_panel_spot(widget.panel_size()) } else { [0.0, 0.0] };
                let parent = if kind.is_loop() { None } else { vi.container_at([pos[0] + size[0] / 2.0, pos[1] + size[1] / 2.0], None) };
                vi.nodes.push(Node { id, kind, pos, size, panel_pos, widget, range: [0.0, 10.0], parent, value });
                Ok(Some(id))
            }
            Command::RemoveNode(id) => {
                let parent = vi.node(id).ok_or(EditError::NoNode(id))?.parent;
                for n in vi.nodes.iter_mut().filter(|n| n.parent == Some(id)) {
                    n.parent = parent;
                }
                vi.nodes.retain(|n| n.id != id);
                vi.wires.retain(|w| w.from.0 != id && w.to.0 != id);
                Ok(None)
            }
            Command::MoveNode { id, pos } => {
                let old = vi.node(id).ok_or(EditError::NoNode(id))?.pos;
                let (dx, dy) = (pos[0] - old[0], pos[1] - old[1]);
                for d in vi.descendants(id) {
                    if let Some(n) = vi.node_mut(d) {
                        n.pos[0] += dx;
                        n.pos[1] += dy;
                    }
                }
                vi.node_mut(id).unwrap().pos = pos;
                Ok(None)
            }
            Command::MovePanelItem { id, pos } => {
                vi.node_mut(id).ok_or(EditError::NoNode(id))?.panel_pos = pos;
                Ok(None)
            }
            Command::Resize { id, size } => {
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                if let NodeKind::Loop { regs, .. } = &n.kind {
                    let min_h = 110.0 + 30.0 * regs.len() as f32;
                    n.size = [size[0].max(180.0), size[1].max(min_h)];
                }
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
                if st.inner && !(vi.is_descendant(to.0, from.0) || (to.0 == from.0 && dt.inner)) {
                    return Err(EditError::Scope);
                }
                if !st.inner && !dt.inner && (from.0 == to.0 || reaches(vi, to.0, from.0)) {
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
                vi.node_mut(id).ok_or(EditError::NoNode(id))?.value = Some(value);
                Ok(None)
            }
            Command::SetWidget { id, widget } => {
                vi.node_mut(id).ok_or(EditError::NoNode(id))?.widget = widget;
                Ok(None)
            }
            Command::SetRange { id, range } => {
                vi.node_mut(id).ok_or(EditError::NoNode(id))?.range = range;
                Ok(None)
            }
            Command::SetParent { id, parent } => {
                vi.node(id).ok_or(EditError::NoNode(id))?;
                if let Some(p) = parent {
                    if !vi.node(p).ok_or(EditError::NoNode(p))?.kind.is_loop() {
                        return Err(EditError::NotALoop(p));
                    }
                    if p == id || vi.is_descendant(p, id) {
                        return Err(EditError::Cycle);
                    }
                }
                vi.node_mut(id).unwrap().parent = parent;
                Ok(None)
            }
            Command::AddShiftRegister { id, ty } => {
                match &mut vi.node_mut(id).ok_or(EditError::NoNode(id))?.kind {
                    NodeKind::Loop { regs, .. } => regs.push(ty),
                    _ => return Err(EditError::NotALoop(id)),
                }
                let n = vi.node_mut(id).unwrap();
                let min_h = 110.0 + 30.0 * regs_len(&n.kind) as f32;
                n.size[1] = n.size[1].max(min_h);
                Ok(None)
            }
            Command::RemoveShiftRegister { id } => {
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                let remaining = match &mut n.kind {
                    NodeKind::Loop { regs, .. } => {
                        regs.pop().ok_or(EditError::NoShiftRegister)?;
                        regs.len()
                    }
                    _ => return Err(EditError::NotALoop(id)),
                };
                // Terminals at or beyond these indices no longer exist.
                let (min_in, min_out) = (1 + 2 * remaining, 1 + 2 * remaining);
                vi.wires.retain(|w| !(w.to.0 == id && w.to.1 >= min_in) && !(w.from.0 == id && w.from.1 >= min_out));
                Ok(None)
            }
            Command::Drop(id) => {
                let n = vi.node(id).ok_or(EditError::NoNode(id))?;
                let center = [n.pos[0] + n.size[0] / 2.0, n.pos[1] + n.size[1] / 2.0];
                let parent = vi.container_at(center, Some(id));
                vi.node_mut(id).unwrap().parent = parent;
                Ok(None)
            }
            Command::EndGesture => Ok(None),
        }
    }
}

fn regs_len(k: &NodeKind) -> usize {
    match k {
        NodeKind::Loop { regs, .. } => regs.len(),
        _ => 0,
    }
}

/// True if data flows from `start` to `target` through existing wires
/// (wires into a structure's inner terminals don't carry data within an iteration).
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
        for w in vi.wires.iter().filter(|w| w.from.0 == n) {
            let inner = vi.node(w.to.0).and_then(|d| d.kind.inputs().get(w.to.1).map(|t| t.inner)).unwrap_or(false);
            if !inner {
                stack.push(w.to.0);
            }
        }
    }
    false
}
