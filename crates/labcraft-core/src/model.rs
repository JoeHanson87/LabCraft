use crate::typing;
use crate::types::{DataType, NumKind, Value};
use serde::{Deserialize, Serialize};

pub type NodeId = u64;

pub const NODE_W: f32 = 120.0;
/// Loop border thickness used by terminal layout.
pub const LOOP_BORDER: f32 = 14.0;
/// Upper bound on frames in a Case/Event structure; per-frame tunnel terminals are laid out in this stride.
pub const MAX_FRAMES: usize = 16;

/// One frame of an Event structure: a control whose value change triggers it, or the timeout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventFrame {
    pub label: String,
    /// The control whose Value Change fires this frame; `None` means the Timeout frame.
    pub source: Option<NodeId>,
    pub ty: DataType,
}

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
    EnumCtl,
    EnumInd,
    ArrayCtl,
    ArrayInd,
    ClusterCtl,
    ClusterInd,
}

fn field_height(ty: &DataType) -> f32 {
    match ty {
        DataType::Cluster(f) => 30.0 + f.iter().map(|f| field_height(&f.ty)).sum::<f32>(),
        DataType::Array(..) => 104.0,
        _ => 28.0,
    }
}

impl Widget {
    pub fn default_for(kind: &NodeKind) -> Widget {
        let (ty, ctl) = match kind {
            NodeKind::Control { ty, .. } => (ty, true),
            NodeKind::Indicator { ty, .. } => (ty, false),
            _ => return Widget::NumCtl,
        };
        match (ty, ctl) {
            (DataType::Bool, true) => Widget::Toggle,
            (DataType::Bool, false) => Widget::Led,
            (DataType::Str, true) => Widget::StrCtl,
            (DataType::Str, false) => Widget::StrInd,
            (DataType::Enum(_), true) => Widget::EnumCtl,
            (DataType::Enum(_), false) => Widget::EnumInd,
            (DataType::Array(..), true) => Widget::ArrayCtl,
            (DataType::Array(..), false) => Widget::ArrayInd,
            (DataType::Cluster(_), true) => Widget::ClusterCtl,
            (DataType::Cluster(_), false) => Widget::ClusterInd,
            (_, true) => Widget::NumCtl,
            (_, false) => Widget::NumInd,
        }
    }

    /// Whether this widget can display a value of type `ty`.
    pub fn fits(self, ty: &DataType, is_ctl: bool) -> bool {
        let n = |w: Widget| w == self;
        match ty {
            DataType::Num(_) => {
                if is_ctl { n(Widget::NumCtl) || n(Widget::Knob) || n(Widget::Slide) } else { n(Widget::NumInd) || n(Widget::Gauge) || n(Widget::Thermometer) || n(Widget::Tank) || n(Widget::Chart) }
            }
            DataType::Bool => if is_ctl { n(Widget::Toggle) || n(Widget::PushButton) } else { n(Widget::Led) },
            DataType::Str => if is_ctl { n(Widget::StrCtl) } else { n(Widget::StrInd) },
            DataType::Enum(_) => if is_ctl { n(Widget::EnumCtl) } else { n(Widget::EnumInd) },
            DataType::Array(e, 1) if matches!(**e, DataType::Num(_)) && !is_ctl => n(Widget::Graph) || n(Widget::ArrayInd),
            DataType::Array(..) => if is_ctl { n(Widget::ArrayCtl) } else { n(Widget::ArrayInd) },
            DataType::Cluster(_) => if is_ctl { n(Widget::ClusterCtl) } else { n(Widget::ClusterInd) },
        }
    }

    pub fn panel_size(self, ty: &DataType) -> [f32; 2] {
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
            Widget::EnumCtl | Widget::EnumInd => [170.0, 48.0],
            Widget::Chart | Widget::Graph => [360.0, 220.0],
            Widget::ArrayCtl | Widget::ArrayInd => {
                let dims = if let DataType::Array(_, n) = ty { *n } else { 1 };
                [320.0, if dims > 1 { 168.0 } else { 118.0 }]
            }
            Widget::ClusterCtl | Widget::ClusterInd => [250.0, 22.0 + field_height(ty)],
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
            Widget::EnumCtl => "Enum Control",
            Widget::EnumInd => "Enum Indicator",
            Widget::ArrayCtl => "Array Control",
            Widget::ArrayInd => "Array Indicator",
            Widget::ClusterCtl => "Cluster Control",
            Widget::ClusterInd => "Cluster Indicator",
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
    /// A diagram constant of the given type; its value lives on the node.
    Constant(DataType),
    Add,
    Subtract,
    Multiply,
    Divide,
    Increment,
    Decrement,
    Negate,
    Absolute,
    SquareRoot,
    Convert(NumKind),
    And,
    Or,
    Xor,
    Not,
    Greater,
    Less,
    GreaterEq,
    LessEq,
    Equal,
    NotEq,
    Select,
    Concatenate,
    NumToString,
    Sine,
    Random,
    WaitMs,
    BuildArray,
    ArraySize,
    IndexArray,
    InitializeArray,
    Transpose,
    Bundle { n: usize },
    Unbundle { n: usize },
    BundleByName { names: Vec<String> },
    UnbundleByName { names: Vec<String> },
    ErrorFromCode,
    MergeErrors,
    /// A While or For loop structure with shift registers of the given types.
    Loop { kind: LoopKind, regs: Vec<DataType> },
    /// Case structure (or Diagram Disable structure when `fixed` is set). Output tunnels have
    /// explicit per-frame terminals because only one frame runs.
    Case { labels: Vec<String>, default: Option<usize>, outs: Vec<DataType>, fixed: Option<usize> },
    /// Flat sequence: frames run left to right.
    Sequence { frames: usize },
    Event { frames: Vec<EventFrame>, outs: Vec<DataType> },
    /// Holds a value from the previous loop iteration.
    Feedback { ty: DataType },
    /// Reads or writes a front-panel control/indicator by reference.
    Local { target: Option<NodeId>, ty: DataType, write: bool },
    /// Named variable shared across the whole VI (and across runs).
    Global { name: String, ty: DataType, write: bool },
}

/// What types a terminal accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spec {
    /// Exactly `Terminal::ty`, with numeric coercion.
    Exact,
    /// Numeric scalar, or array/cluster of numerics (polymorphic).
    NumLike,
    /// Boolean scalar, or array/cluster of booleans (polymorphic).
    BoolLike,
    Any,
    AnyArray,
    AnyCluster,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Terminal {
    pub name: String,
    /// Declared type; the default shown for polymorphic terminals before they are wired.
    pub ty: DataType,
    pub spec: Spec,
    /// Terminal sits inside a structure's border (not on the outer side).
    pub inner: bool,
    /// Unwired optional inputs are allowed.
    pub optional: bool,
}

impl Terminal {
    fn new(name: &str, ty: DataType) -> Self {
        Terminal { name: name.into(), ty, spec: Spec::Exact, inner: false, optional: false }
    }
    fn spec(mut self, s: Spec) -> Self {
        self.spec = s;
        self
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
            NodeKind::Increment => "Increment".into(),
            NodeKind::Decrement => "Decrement".into(),
            NodeKind::Negate => "Negate".into(),
            NodeKind::Absolute => "Absolute Value".into(),
            NodeKind::SquareRoot => "Square Root".into(),
            NodeKind::Convert(k) => format!("To {}", k.label()),
            NodeKind::And => "And".into(),
            NodeKind::Or => "Or".into(),
            NodeKind::Xor => "Exclusive Or".into(),
            NodeKind::Not => "Not".into(),
            NodeKind::Greater => "Greater?".into(),
            NodeKind::Less => "Less?".into(),
            NodeKind::GreaterEq => "Greater Or Equal?".into(),
            NodeKind::LessEq => "Less Or Equal?".into(),
            NodeKind::Equal => "Equal?".into(),
            NodeKind::NotEq => "Not Equal?".into(),
            NodeKind::Select => "Select".into(),
            NodeKind::Concatenate => "Concatenate Strings".into(),
            NodeKind::NumToString => "Number To String".into(),
            NodeKind::Sine => "Sine".into(),
            NodeKind::Random => "Random Number".into(),
            NodeKind::WaitMs => "Wait (ms)".into(),
            NodeKind::BuildArray => "Build Array".into(),
            NodeKind::ArraySize => "Array Size".into(),
            NodeKind::IndexArray => "Index Array".into(),
            NodeKind::InitializeArray => "Initialize Array".into(),
            NodeKind::Transpose => "Transpose 2D Array".into(),
            NodeKind::Bundle { .. } => "Bundle".into(),
            NodeKind::Unbundle { .. } => "Unbundle".into(),
            NodeKind::BundleByName { .. } => "Bundle By Name".into(),
            NodeKind::UnbundleByName { .. } => "Unbundle By Name".into(),
            NodeKind::ErrorFromCode => "Error From Code".into(),
            NodeKind::MergeErrors => "Merge Errors".into(),
            NodeKind::Case { fixed: Some(_), .. } => "Diagram Disable Structure".into(),
            NodeKind::Case { .. } => "Case Structure".into(),
            NodeKind::Sequence { .. } => "Flat Sequence".into(),
            NodeKind::Event { .. } => "Event Structure".into(),
            NodeKind::Feedback { .. } => "Feedback Node".into(),
            NodeKind::Local { write: true, .. } => "Local Variable (write)".into(),
            NodeKind::Local { .. } => "Local Variable".into(),
            NodeKind::Global { name, write, .. } => format!("{name}{}", if *write { " (write)" } else { "" }),
            NodeKind::Loop { kind: LoopKind::While, .. } => "While Loop".into(),
            NodeKind::Loop { kind: LoopKind::For, .. } => "For Loop".into(),
        }
    }

    /// Input terminals. For loops: `[N|stop, init0, next0, init1, next1, ...]`.
    pub fn inputs(&self) -> Vec<Terminal> {
        use DataType as T;
        use Spec::*;
        let t = Terminal::new;
        let num2 = || vec![t("x", T::DBL).spec(NumLike), t("y", T::DBL).spec(NumLike)];
        let num1 = || vec![t("x", T::DBL).spec(NumLike)];
        match self {
            NodeKind::Control { .. } | NodeKind::Constant(_) | NodeKind::Random => vec![],
            NodeKind::Indicator { ty, .. } => vec![t("in", ty.clone()).opt()],
            NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => num2(),
            NodeKind::Greater | NodeKind::Less | NodeKind::GreaterEq | NodeKind::LessEq => num2(),
            NodeKind::Increment | NodeKind::Decrement | NodeKind::Negate | NodeKind::Absolute | NodeKind::SquareRoot | NodeKind::Convert(_) => num1(),
            NodeKind::And | NodeKind::Or | NodeKind::Xor => vec![t("x", T::Bool).spec(BoolLike), t("y", T::Bool).spec(BoolLike)],
            NodeKind::Not => vec![t("x", T::Bool).spec(BoolLike)],
            NodeKind::Equal | NodeKind::NotEq => vec![t("x", T::DBL).spec(Any), t("y", T::DBL).spec(Any)],
            NodeKind::Select => vec![t("s", T::Bool), t("true", T::DBL).spec(Any), t("false", T::DBL).spec(Any)],
            NodeKind::Concatenate => vec![t("a", T::Str), t("b", T::Str)],
            NodeKind::NumToString => vec![t("number", T::DBL)],
            NodeKind::Sine => vec![t("x", T::DBL)],
            NodeKind::WaitMs => vec![t("ms", T::I32)],
            NodeKind::BuildArray => vec![t("array", T::arr_dbl()).spec(AnyArray).opt(), t("element", T::DBL).spec(Any)],
            NodeKind::ArraySize | NodeKind::Transpose => vec![t("array", T::arr_dbl()).spec(AnyArray)],
            NodeKind::IndexArray => vec![t("array", T::arr_dbl()).spec(AnyArray), t("index", T::I32), t("index 2", T::I32).opt()],
            NodeKind::InitializeArray => vec![t("element", T::DBL).spec(Any), t("size", T::I32).opt()],
            NodeKind::Bundle { n } => {
                let mut v = vec![t("cluster", T::Cluster(vec![])).spec(AnyCluster).opt()];
                v.extend((0..*n).map(|i| t(&format!("e{i}"), T::DBL).spec(Any).opt()));
                v
            }
            NodeKind::Unbundle { .. } => vec![t("cluster", T::Cluster(vec![])).spec(AnyCluster)],
            NodeKind::BundleByName { names } => {
                let mut v = vec![t("cluster", T::Cluster(vec![])).spec(AnyCluster)];
                v.extend(names.iter().map(|n| t(if n.is_empty() { "?" } else { n }, T::DBL).spec(Any).opt()));
                v
            }
            NodeKind::UnbundleByName { .. } => vec![t("cluster", T::Cluster(vec![])).spec(AnyCluster)],
            NodeKind::ErrorFromCode => vec![t("code", T::I32), t("source", T::Str).opt()],
            NodeKind::MergeErrors => vec![t("error 1", T::error_cluster()).opt(), t("error 2", T::error_cluster()).opt()],
            NodeKind::Case { outs, fixed, labels, .. } => {
                let mut v = vec![t("?", T::DBL).spec(Any).opt()];
                let _ = fixed;
                for (k, ty) in outs.iter().enumerate() {
                    for f in 0..MAX_FRAMES {
                        let name = format!("tunnel {k}: {}", labels.get(f).map(String::as_str).unwrap_or("-"));
                        v.push(t(&name, ty.clone()).inner().opt());
                    }
                }
                v
            }
            NodeKind::Event { outs, .. } => {
                let mut v = vec![t("timeout", T::I32).opt()];
                for (k, ty) in outs.iter().enumerate() {
                    for _ in 0..MAX_FRAMES {
                        v.push(t(&format!("tunnel {k}"), ty.clone()).inner().opt());
                    }
                }
                v
            }
            NodeKind::Sequence { .. } => vec![],
            NodeKind::Feedback { ty } => vec![t("in", ty.clone()), t("init", ty.clone()).opt()],
            NodeKind::Local { ty, write: true, .. } | NodeKind::Global { ty, write: true, .. } => vec![t("in", ty.clone())],
            NodeKind::Local { .. } | NodeKind::Global { .. } => vec![],
            NodeKind::Loop { kind, regs } => {
                let mut v = vec![match kind {
                    LoopKind::For => t("N", T::I32),
                    LoopKind::While => t("stop", T::Bool).inner(),
                }];
                for ty in regs {
                    v.push(t("init", ty.clone()).opt());
                    v.push(t("next", ty.clone()).inner().opt());
                }
                v
            }
        }
    }

    /// Declared output terminals (their real types are resolved by `typing`).
    /// For loops: `[i, cur0, final0, cur1, final1, ...]`.
    pub fn outputs(&self) -> Vec<Terminal> {
        use DataType as T;
        let t = Terminal::new;
        match self {
            NodeKind::Control { ty, .. } => vec![t("out", ty.clone())],
            NodeKind::Indicator { .. } | NodeKind::WaitMs => vec![],
            NodeKind::Constant(ty) => vec![t("out", ty.clone())],
            NodeKind::Add | NodeKind::Subtract | NodeKind::Multiply | NodeKind::Divide => vec![t("result", T::DBL)],
            NodeKind::Increment | NodeKind::Decrement | NodeKind::Negate | NodeKind::Absolute | NodeKind::SquareRoot | NodeKind::Convert(_) => vec![t("result", T::DBL)],
            NodeKind::And | NodeKind::Or | NodeKind::Xor | NodeKind::Not => vec![t("result", T::Bool)],
            NodeKind::Greater | NodeKind::Less | NodeKind::GreaterEq | NodeKind::LessEq | NodeKind::Equal | NodeKind::NotEq => vec![t("result", T::Bool)],
            NodeKind::Select => vec![t("result", T::DBL)],
            NodeKind::Concatenate | NodeKind::NumToString => vec![t("result", T::Str)],
            NodeKind::Sine => vec![t("sin", T::DBL)],
            NodeKind::Random => vec![t("rand", T::DBL)],
            NodeKind::BuildArray | NodeKind::Transpose | NodeKind::InitializeArray => vec![t("result", T::arr_dbl())],
            NodeKind::ArraySize => vec![t("size", T::I32)],
            NodeKind::IndexArray => vec![t("element", T::DBL)],
            NodeKind::Bundle { .. } | NodeKind::BundleByName { .. } => vec![t("cluster", T::Cluster(vec![]))],
            NodeKind::Unbundle { n } => (0..*n).map(|i| t(&format!("{i}"), T::DBL)).collect(),
            NodeKind::UnbundleByName { names } => names.iter().map(|n| t(if n.is_empty() { "?" } else { n }, T::DBL)).collect(),
            NodeKind::ErrorFromCode | NodeKind::MergeErrors => vec![t("error out", T::error_cluster())],
            NodeKind::Case { outs, .. } => outs.iter().map(|ty| t("out", ty.clone())).collect(),
            NodeKind::Event { frames, outs } => {
                let mut v: Vec<Terminal> = (0..MAX_FRAMES)
                    .map(|f| match frames.get(f) {
                        Some(fr) if fr.source.is_some() => t("NewVal", fr.ty.clone()).inner(),
                        _ => t("-", T::Bool).inner(),
                    })
                    .collect();
                v.extend(outs.iter().map(|ty| t("out", ty.clone())));
                v
            }
            NodeKind::Sequence { .. } => vec![],
            NodeKind::Feedback { ty } => vec![t("out", ty.clone())],
            NodeKind::Local { ty, write: false, .. } | NodeKind::Global { ty, write: false, .. } => vec![t("value", ty.clone())],
            NodeKind::Local { .. } | NodeKind::Global { .. } => vec![],
            NodeKind::Loop { regs, .. } => {
                let mut v = vec![t("i", T::I32).inner()];
                for ty in regs {
                    v.push(t("cur", ty.clone()).inner());
                    v.push(t("out", ty.clone()));
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

    /// Loops, Case, Sequence and Event structures contain other nodes.
    pub fn is_structure(&self) -> bool {
        matches!(self, NodeKind::Loop { .. } | NodeKind::Case { .. } | NodeKind::Sequence { .. } | NodeKind::Event { .. })
    }

    /// Structures where only one frame runs (so data leaving needs an explicit tunnel).
    pub fn is_frame_gated(&self) -> bool {
        matches!(self, NodeKind::Case { .. } | NodeKind::Event { .. })
    }

    pub fn frame_count(&self) -> usize {
        match self {
            NodeKind::Case { labels, .. } => labels.len(),
            NodeKind::Sequence { frames } => *frames,
            NodeKind::Event { frames, .. } => frames.len(),
            _ => 1,
        }
    }

    pub fn tunnel_count(&self) -> usize {
        match self {
            NodeKind::Case { outs, .. } | NodeKind::Event { outs, .. } => outs.len(),
            _ => 0,
        }
    }

    /// Index of the inner input terminal for tunnel `k` in `frame`.
    pub fn tunnel_in_idx(k: usize, frame: usize) -> usize {
        1 + k * MAX_FRAMES + frame
    }

    /// Index of the outer output terminal for tunnel `k`.
    pub fn tunnel_out_idx(&self, k: usize) -> usize {
        if matches!(self, NodeKind::Event { .. }) { MAX_FRAMES + k } else { k }
    }

    /// Whether a terminal is currently shown (per-frame terminals only show for the visible frame).
    pub fn terminal_visible(&self, idx: usize, output: bool, frame_view: usize) -> bool {
        match self {
            NodeKind::Case { fixed, labels, .. } => {
                if output {
                    true
                } else if idx == 0 {
                    fixed.is_none()
                } else {
                    let f = (idx - 1) % MAX_FRAMES;
                    f == frame_view && f < labels.len()
                }
            }
            NodeKind::Event { frames, .. } => {
                if output {
                    if idx < MAX_FRAMES { idx == frame_view && idx < frames.len() && frames[idx].source.is_some() } else { true }
                } else if idx == 0 {
                    true
                } else {
                    (idx - 1) % MAX_FRAMES == frame_view
                }
            }
            _ => true,
        }
    }

    pub fn default_size(&self) -> [f32; 2] {
        match self {
            NodeKind::Loop { .. } | NodeKind::Case { .. } | NodeKind::Event { .. } => return [300.0, 200.0],
            NodeKind::Sequence { frames } => return [60.0 + 200.0 * *frames as f32, 200.0],
            _ => {}
        }
        let rows = self.inputs().len().max(self.outputs().len()).max(1) as f32;
        let h = match self {
            NodeKind::Constant(DataType::Num(_) | DataType::Bool | DataType::Str | DataType::Enum(_)) => 62.0,
            NodeKind::Constant(_) => 62.0,
            _ => 28.0 + rows * 22.0,
        };
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
    /// Which frame of the parent structure this node is in.
    #[serde(default)]
    pub frame: usize,
    /// Frame currently displayed (multi-frame structures only).
    #[serde(default)]
    pub frame_view: usize,
    /// Current value for controls (user input), indicators (last result) and constants.
    pub value: Option<Value>,
}

impl Node {
    pub fn data_type(&self) -> Option<&DataType> {
        match &self.kind {
            NodeKind::Control { ty, .. } | NodeKind::Indicator { ty, .. } | NodeKind::Constant(ty) => Some(ty),
            _ => None,
        }
    }

    pub fn panel_size(&self) -> [f32; 2] {
        self.widget.panel_size(self.data_type().unwrap_or(&DataType::DBL))
    }

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
        let b = LOOP_BORDER;
        match &self.kind {
            NodeKind::Case { .. } => {
                return if output {
                    [x + w, self.loop_reg_y(idx)]
                } else if idx == 0 {
                    [x, y + h / 2.0]
                } else {
                    [x + w - b, self.loop_reg_y((idx - 1) / MAX_FRAMES)]
                };
            }
            NodeKind::Event { .. } => {
                return if output {
                    if idx < MAX_FRAMES { [x + b + 8.0, y + 44.0] } else { [x + w, self.loop_reg_y(idx - MAX_FRAMES)] }
                } else if idx == 0 {
                    [x, y + 28.0]
                } else {
                    [x + w - b, self.loop_reg_y((idx - 1) / MAX_FRAMES)]
                };
            }
            NodeKind::Sequence { .. } => return [x, y],
            _ => {}
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
    /// For wires crossing a loop border: gather per-iteration values into an array (leaving) or
    /// hand out one element per iteration (entering).
    #[serde(default)]
    pub auto_index: bool,
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

    /// Structures enclosing `id`, innermost first.
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![];
        let mut cur = self.node(id).and_then(|n| n.parent);
        while let Some(p) = cur {
            out.push(p);
            cur = self.node(p).and_then(|n| n.parent);
            if out.len() > 1000 {
                break;
            }
        }
        out
    }

    /// `(structure, frame)` pairs from the outermost structure down to the one directly holding `id`.
    pub fn frames_chain(&self, id: NodeId) -> Vec<(NodeId, usize)> {
        let mut chain = vec![];
        let mut cur = id;
        while let Some(n) = self.node(cur) {
            match n.parent {
                Some(p) => {
                    chain.push((p, n.frame));
                    cur = p;
                }
                None => break,
            }
            if chain.len() > 1000 {
                break;
            }
        }
        chain.reverse();
        chain
    }

    /// Frame of `structure` that contains `point`.
    pub fn frame_at(&self, structure: NodeId, point: [f32; 2]) -> usize {
        let Some(st) = self.node(structure) else { return 0 };
        let count = st.kind.frame_count().max(1);
        match &st.kind {
            NodeKind::Sequence { frames } => {
                let fw = st.size[0] / *frames as f32;
                (((point[0] - st.pos[0]) / fw).floor().max(0.0) as usize).min(frames - 1)
            }
            NodeKind::Case { .. } | NodeKind::Event { .. } => st.frame_view.min(count - 1),
            _ => 0,
        }
    }

    /// False when the node lives in a frame of a Case/Event structure that is not displayed.
    pub fn node_visible(&self, id: NodeId) -> bool {
        self.frames_chain(id).iter().all(|(p, f)| match self.node(*p) {
            Some(st) if st.kind.is_frame_gated() => st.frame_view.min(st.kind.frame_count().max(1) - 1) == *f,
            _ => true,
        })
    }

    /// Innermost structure containing `point`, ignoring `exclude` and everything inside it.
    pub fn container_at(&self, point: [f32; 2], exclude: Option<NodeId>) -> Option<NodeId> {
        self.nodes
            .iter()
            .filter(|n| n.kind.is_structure())
            .filter(|n| Some(n.id) != exclude && !exclude.is_some_and(|e| self.is_descendant(n.id, e)))
            .filter(|n| self.node_visible(n.id))
            .filter(|n| {
                point[0] >= n.pos[0] && point[0] <= n.pos[0] + n.size[0] && point[1] >= n.pos[1] && point[1] <= n.pos[1] + n.size[1]
            })
            .min_by(|a, b| (a.size[0] * a.size[1]).total_cmp(&(b.size[0] * b.size[1])).then(self.depth(b.id).cmp(&self.depth(a.id))))
            .map(|n| n.id)
    }

    /// Reject wires between nodes in different frames of the same structure, and wires leaving a
    /// Case/Event frame without a tunnel.
    pub fn check_frames(&self, from: NodeId, to: NodeId) -> Result<(), EditError> {
        let (a, b) = (self.frames_chain(from), self.frames_chain(to));
        for (x, y) in a.iter().zip(b.iter()) {
            if x.0 != y.0 {
                break;
            }
            if x.1 != y.1 {
                let kind = &self.node(x.0).ok_or(EditError::NoNode(x.0))?.kind;
                let ok = matches!(kind, NodeKind::Sequence { .. }) && x.1 < y.1;
                return if ok { Ok(()) } else { Err(EditError::Scope) };
            }
        }
        let dest_anc = self.ancestors(to);
        for s in self.ancestors(from) {
            let gated = self.node(s).is_some_and(|n| n.kind.is_frame_gated());
            if gated && s != to && !dest_anc.contains(&s) {
                return Err(EditError::NeedsTunnel(s));
            }
        }
        Ok(())
    }

    fn free_panel_spot(&self, size: [f32; 2]) -> [f32; 2] {
        let used: Vec<(NodeId, [f32; 2], [f32; 2])> = self
            .nodes
            .iter()
            .filter(|n| n.kind.is_panel_item())
            .map(|n| (n.id, n.panel_pos, n.panel_size()))
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
    #[error("type mismatch: {} cannot feed {}", .0.label(), .1.label())]
    TypeMismatch(DataType, DataType),
    #[error("node {0} cannot be edited this way")]
    WrongNodeKind(NodeId),
    #[error("cluster has no element named '{0}'")]
    NoSuchField(String),
    #[error("connection would create a cycle")]
    Cycle,
    #[error("a structure's inner terminal can only feed nodes inside that structure")]
    Scope,
    #[error("data leaving structure {0} must go through an output tunnel (right-click the border to add one)")]
    NeedsTunnel(NodeId),
    #[error("cannot remove the last frame")]
    LastFrame,
    #[error("no more frames can be added")]
    TooManyFrames,
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
    /// Change the data type of a control, indicator or constant.
    SetType { id: NodeId, ty: DataType },
    /// Resize Bundle / Unbundle / (By Name) nodes.
    SetElementCount { id: NodeId, n: usize },
    /// Choose the element names of the By Name nodes.
    SetNames { id: NodeId, names: Vec<String> },
    AddShiftRegister { id: NodeId, ty: DataType },
    RemoveShiftRegister { id: NodeId },
    /// Switch which frame of a Case/Event structure is displayed (not undoable).
    SetFrameView { id: NodeId, frame: usize },
    /// Insert a frame after `after` in a Case or Flat Sequence structure.
    AddFrame { id: NodeId, after: usize },
    RemoveFrame { id: NodeId, frame: usize },
    SetFrameLabel { id: NodeId, frame: usize, label: String },
    SetDefaultFrame { id: NodeId, frame: Option<usize> },
    /// Choose which frame of a Diagram Disable structure is enabled.
    SetEnabledFrame { id: NodeId, frame: usize },
    /// Add an output tunnel (Case/Event).
    AddTunnel { id: NodeId, ty: DataType },
    RemoveTunnel { id: NodeId },
    /// Add an Event frame for a control's Value Change, or a Timeout frame when `source` is None.
    AddEventFrame { id: NodeId, source: Option<NodeId> },
    SetLocalTarget { id: NodeId, target: Option<NodeId> },
    SetWrite { id: NodeId, write: bool },
    SetGlobalName { id: NodeId, name: String },
    SetWireIndexing { to: (NodeId, usize), enabled: bool },
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
            Command::SetType { id, .. } => Some((3, *id)),
            Command::Resize { id, .. } => Some((2, *id)),
            _ => None,
        };
        let record = !matches!(cmd, Command::SetValue { .. } | Command::SetFrameView { .. }) && !(key.is_some() && key == self.last_key);
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
                        if widget == Widget::Chart { Some(Value::default_for(&DataType::arr_dbl())) } else { Some(Value::default_for(ty)) }
                    }
                    NodeKind::Constant(ty) => Some(Value::default_for(ty)),
                    _ => None,
                };
                let size = kind.default_size();
                let panel_pos = if kind.is_panel_item() { vi.free_panel_spot(widget.panel_size(match &kind { NodeKind::Control { ty, .. } | NodeKind::Indicator { ty, .. } => ty, _ => &DataType::DBL })) } else { [0.0, 0.0] };
                let center = [pos[0] + size[0] / 2.0, pos[1] + size[1] / 2.0];
                let parent = if kind.is_structure() { None } else { vi.container_at(center, None) };
                let frame = parent.map(|p| vi.frame_at(p, center)).unwrap_or(0);
                vi.nodes.push(Node { id, kind, pos, size, panel_pos, widget, range: [0.0, 10.0], parent, frame, frame_view: 0, value });
                Ok(Some(id))
            }
            Command::RemoveNode(id) => {
                let removed = vi.node(id).ok_or(EditError::NoNode(id))?;
                let (parent, slot) = (removed.parent, removed.frame);
                for n in vi.nodes.iter_mut().filter(|n| n.parent == Some(id)) {
                    n.parent = parent;
                    n.frame = slot;
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
                let rows = match &n.kind {
                    NodeKind::Loop { regs, .. } => Some(regs.len()),
                    NodeKind::Case { outs, .. } | NodeKind::Event { outs, .. } => Some(outs.len()),
                    NodeKind::Sequence { frames } => {
                        n.size[1] = size[1].max(120.0);
                        n.size[0] = 60.0 + 200.0 * *frames as f32;
                        None
                    }
                    _ => None,
                };
                if let Some(r) = rows {
                    n.size = [size[0].max(220.0), size[1].max(110.0 + 30.0 * r as f32)];
                }
                Ok(None)
            }
            Command::Connect { from, to } => {
                let src = vi.node(from.0).ok_or(EditError::NoNode(from.0))?;
                let dst = vi.node(to.0).ok_or(EditError::NoNode(to.0))?;
                let st = src.kind.outputs().get(from.1).cloned().ok_or(EditError::NoTerminal(from.0, from.1))?;
                let dt = dst.kind.inputs().get(to.1).cloned().ok_or(EditError::NoTerminal(to.0, to.1))?;
                let dst_kind = dst.kind.clone();
                if st.inner && !(vi.is_descendant(to.0, from.0) || (to.0 == from.0 && dt.inner)) {
                    return Err(EditError::Scope);
                }
                // A Case/Event tunnel input accepts data only from nodes in the matching frame.
                if dt.inner && dst_kind.is_frame_gated() && to.1 >= 1 && from.0 != to.0 {
                    let f = (to.1 - 1) % MAX_FRAMES;
                    let in_frame = vi.is_descendant(from.0, to.0) && vi.frames_chain(from.0).iter().find(|(p, _)| *p == to.0).map(|x| x.1) == Some(f);
                    if !in_frame {
                        return Err(EditError::Scope);
                    }
                }
                vi.check_frames(from.0, to.0)?;
                // A structure's outer input cannot be fed from inside the structure itself.
                if !dt.inner && dst_kind.is_structure() && vi.is_descendant(from.0, to.0) {
                    return Err(EditError::Scope);
                }
                let delay = matches!(dst_kind, NodeKind::Feedback { .. }) && to.1 == 0;
                if !st.inner && !dt.inner && !delay && (from.0 == to.0 || reaches(vi, to.0, from.0)) {
                    return Err(EditError::Cycle);
                }
                let raw = typing::out_type(vi, from);
                // Wiring an untouched shift register's init terminal gives it the source's type.
                if let (NodeKind::Loop { .. }, true) = (&dst_kind, to.1 >= 1 && to.1 % 2 == 1) {
                    let k = (to.1 - 1) / 2;
                    let untouched = !vi.wires.iter().any(|w| (w.to.0 == to.0 && w.to.1 == to.1 + 1) || (w.from.0 == to.0 && (w.from.1 == to.1 + 1 || w.from.1 == to.1 + 2)));
                    if let (true, Some(ty)) = (untouched, raw.clone()) {
                        if let NodeKind::Loop { regs, .. } = &mut vi.node_mut(to.0).unwrap().kind {
                            regs[k] = ty;
                        }
                    }
                }
                // Likewise for an untouched Case/Event output tunnel.
                if dt.inner && dst_kind.is_frame_gated() && to.1 >= 1 {
                    let k = (to.1 - 1) / MAX_FRAMES;
                    let (base, out_i) = (NodeKind::tunnel_in_idx(k, 0), dst_kind.tunnel_out_idx(k));
                    let untouched = !vi.wires.iter().any(|w| (w.to.0 == to.0 && w.to.1 >= base && w.to.1 < base + MAX_FRAMES) || (w.from.0 == to.0 && w.from.1 == out_i));
                    if let (true, Some(ty)) = (untouched, raw.clone()) {
                        if let NodeKind::Case { outs, .. } | NodeKind::Event { outs, .. } = &mut vi.node_mut(to.0).unwrap().kind {
                            outs[k] = ty;
                        }
                    }
                }
                // Feedback nodes take the type of what is first wired to them.
                if matches!(dst_kind, NodeKind::Feedback { .. }) {
                    let untouched = !vi.wires.iter().any(|w| (w.to.0 == to.0) || (w.from.0 == to.0));
                    if let (true, Some(ty)) = (untouched, raw.clone()) {
                        vi.node_mut(to.0).unwrap().kind = NodeKind::Feedback { ty };
                    }
                }
                let default_auto = raw.as_ref().map(|t| typing::default_auto_index(vi, from.0, to.0, t)).unwrap_or(false);
                let (l, e) = typing::crossing_loops(vi, from.0, to.0);
                let crosses = !l.is_empty() || !e.is_empty();
                // If the default indexing mode does not type-check, try the other one.
                let auto = match typing::check_connect(vi, from, to, default_auto) {
                    Ok(()) => default_auto,
                    Err(err) if crosses && typing::check_connect(vi, from, to, !default_auto).is_ok() => {
                        let _ = err;
                        !default_auto
                    }
                    Err(err) => return Err(err),
                };
                vi.wires.retain(|w| w.to != to);
                vi.wires.push(Wire { from, to, auto_index: auto });
                if to.1 == 0 {
                    if let Some(ty) = &raw {
                        // Bundle/Unbundle size themselves to the cluster that is wired in.
                        if let DataType::Cluster(fields) = ty {
                            match &mut vi.node_mut(to.0).unwrap().kind {
                                NodeKind::Unbundle { n } | NodeKind::Bundle { n } => *n = fields.len(),
                                _ => {}
                            }
                            refresh_size(vi, to.0);
                        }
                        // A Case selector relabels its frames to match the wired type (when still empty).
                        sync_case_labels(vi, to.0, ty);
                    }
                }
                prune_invalid_wires(vi);
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
                    if !vi.node(p).ok_or(EditError::NoNode(p))?.kind.is_structure() {
                        return Err(EditError::NotALoop(p));
                    }
                    if p == id || vi.is_descendant(p, id) {
                        return Err(EditError::Cycle);
                    }
                }
                let frame = parent.map(|p| {
                    let c = vi.node(id).map(|n| [n.pos[0] + n.size[0] / 2.0, n.pos[1] + n.size[1] / 2.0]).unwrap_or([0.0, 0.0]);
                    vi.frame_at(p, c)
                });
                let n = vi.node_mut(id).unwrap();
                n.parent = parent;
                n.frame = frame.unwrap_or(0);
                Ok(None)
            }
            Command::SetType { id, ty } => {
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                let is_ctl = matches!(n.kind, NodeKind::Control { .. });
                match &mut n.kind {
                    NodeKind::Control { ty: t, .. } | NodeKind::Indicator { ty: t, .. } | NodeKind::Constant(t) => *t = ty.clone(),
                    NodeKind::Feedback { ty: t } | NodeKind::Global { ty: t, .. } => {
                        *t = ty.clone();
                        prune_invalid_wires(vi);
                        return Ok(None);
                    }
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                for l in vi.nodes.iter_mut() {
                    if let NodeKind::Local { target: Some(t), ty: lt, .. } = &mut l.kind {
                        if *t == id {
                            *lt = ty.clone();
                        }
                    }
                }
                let n = vi.node_mut(id).unwrap();
                if n.widget != Widget::Chart {
                    n.value = Some(n.value.as_ref().and_then(|v| v.coerce(&ty)).unwrap_or_else(|| Value::default_for(&ty)));
                    if !n.widget.fits(&ty, is_ctl) && !matches!(n.kind, NodeKind::Constant(_)) {
                        n.widget = Widget::default_for(&n.kind);
                    }
                }
                refresh_size(vi, id);
                prune_invalid_wires(vi);
                Ok(None)
            }
            Command::SetElementCount { id, n } => {
                let node = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                match &mut node.kind {
                    NodeKind::Bundle { n: c } | NodeKind::Unbundle { n: c } => *c = n,
                    NodeKind::BundleByName { names } | NodeKind::UnbundleByName { names } => names.resize(n, String::new()),
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                let is_bundle = matches!(node.kind, NodeKind::Bundle { .. } | NodeKind::BundleByName { .. });
                refresh_size(vi, id);
                vi.wires.retain(|w| !(w.to.0 == id && is_bundle && w.to.1 > n) && !(w.from.0 == id && !is_bundle && w.from.1 >= n));
                Ok(None)
            }
            Command::SetNames { id, names } => {
                let node = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                match &mut node.kind {
                    NodeKind::BundleByName { names: n } | NodeKind::UnbundleByName { names: n } => *n = names,
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                refresh_size(vi, id);
                prune_invalid_wires(vi);
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
            Command::SetFrameView { id, frame } => {
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                n.frame_view = frame.min(n.kind.frame_count().max(1) - 1);
                Ok(None)
            }
            Command::AddFrame { id, after } => {
                let st = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                let pos = (after + 1).min(st.kind.frame_count());
                match &mut st.kind {
                    NodeKind::Case { labels, default, fixed: None, .. } => {
                        if labels.len() >= MAX_FRAMES {
                            return Err(EditError::TooManyFrames);
                        }
                        labels.insert(pos, format!("Case {}", labels.len()));
                        if let Some(d) = default {
                            if *d >= pos {
                                *d += 1;
                            }
                        }
                    }
                    NodeKind::Sequence { frames } => {
                        *frames += 1;
                        st.size[0] += 200.0;
                    }
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                st.frame_view = pos;
                let is_seq = matches!(st.kind, NodeKind::Sequence { .. });
                let kids: Vec<NodeId> = vi.nodes.iter().filter(|n| n.parent == Some(id) && n.frame >= pos).map(|n| n.id).collect();
                for k in kids {
                    vi.node_mut(k).unwrap().frame += 1;
                    if is_seq {
                        let p = vi.node(k).unwrap().pos;
                        Self::exec(vi, Command::MoveNode { id: k, pos: [p[0] + 200.0, p[1]] })?;
                    }
                }
                remap_frame_wires(vi, id, pos, true);
                Ok(None)
            }
            Command::RemoveFrame { id, frame } => {
                let st = vi.node(id).ok_or(EditError::NoNode(id))?;
                if !matches!(st.kind, NodeKind::Case { .. } | NodeKind::Sequence { .. } | NodeKind::Event { .. }) {
                    return Err(EditError::WrongNodeKind(id));
                }
                if st.kind.frame_count() <= 1 {
                    return Err(EditError::LastFrame);
                }
                if matches!(st.kind, NodeKind::Case { fixed: Some(_), .. }) || frame >= st.kind.frame_count() {
                    return Err(EditError::WrongNodeKind(id));
                }
                let doomed: Vec<NodeId> = vi.nodes.iter().filter(|n| n.parent == Some(id) && n.frame == frame).map(|n| n.id).collect();
                for d in doomed {
                    remove_subtree(vi, d);
                }
                let is_seq = matches!(vi.node(id).unwrap().kind, NodeKind::Sequence { .. });
                let later: Vec<NodeId> = vi.nodes.iter().filter(|n| n.parent == Some(id) && n.frame > frame).map(|n| n.id).collect();
                for k in later {
                    vi.node_mut(k).unwrap().frame -= 1;
                    if is_seq {
                        let p = vi.node(k).unwrap().pos;
                        Self::exec(vi, Command::MoveNode { id: k, pos: [p[0] - 200.0, p[1]] })?;
                    }
                }
                let st = vi.node_mut(id).unwrap();
                match &mut st.kind {
                    NodeKind::Case { labels, default, .. } => {
                        labels.remove(frame);
                        *default = match *default {
                            Some(d) if d == frame => None,
                            Some(d) if d > frame => Some(d - 1),
                            d => d,
                        };
                    }
                    NodeKind::Sequence { frames } => {
                        *frames -= 1;
                        st.size[0] -= 200.0;
                    }
                    NodeKind::Event { frames, .. } => {
                        frames.remove(frame);
                    }
                    _ => {}
                }
                st.frame_view = st.frame_view.min(st.kind.frame_count() - 1);
                remap_frame_wires(vi, id, frame, false);
                Ok(None)
            }
            Command::SetFrameLabel { id, frame, label } => {
                match &mut vi.node_mut(id).ok_or(EditError::NoNode(id))?.kind {
                    NodeKind::Case { labels, .. } => *labels.get_mut(frame).ok_or(EditError::NoNode(id))? = label,
                    NodeKind::Event { frames, .. } => frames.get_mut(frame).ok_or(EditError::NoNode(id))?.label = label,
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                Ok(None)
            }
            Command::SetDefaultFrame { id, frame } => {
                match &mut vi.node_mut(id).ok_or(EditError::NoNode(id))?.kind {
                    NodeKind::Case { default, labels, .. } => *default = frame.filter(|f| *f < labels.len()),
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                Ok(None)
            }
            Command::SetEnabledFrame { id, frame } => {
                match &mut vi.node_mut(id).ok_or(EditError::NoNode(id))?.kind {
                    NodeKind::Case { fixed: Some(f), labels, .. } => *f = frame.min(labels.len() - 1),
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                Ok(None)
            }
            Command::AddTunnel { id, ty } => {
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                match &mut n.kind {
                    NodeKind::Case { outs, .. } | NodeKind::Event { outs, .. } => outs.push(ty),
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                n.size[1] = n.size[1].max(110.0 + 30.0 * n.kind.tunnel_count() as f32);
                Ok(None)
            }
            Command::RemoveTunnel { id } => {
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                let k = match &mut n.kind {
                    NodeKind::Case { outs, .. } | NodeKind::Event { outs, .. } if !outs.is_empty() => {
                        outs.pop();
                        outs.len()
                    }
                    NodeKind::Case { .. } | NodeKind::Event { .. } => return Ok(None),
                    _ => return Err(EditError::WrongNodeKind(id)),
                };
                let out_i = vi.node(id).unwrap().kind.tunnel_out_idx(k);
                let base = NodeKind::tunnel_in_idx(k, 0);
                vi.wires.retain(|w| !(w.to.0 == id && w.to.1 >= base && w.to.1 < base + MAX_FRAMES) && !(w.from.0 == id && w.from.1 == out_i));
                Ok(None)
            }
            Command::AddEventFrame { id, source } => {
                let frame = match source {
                    Some(c) => {
                        let (name, ty) = match &vi.node(c).ok_or(EditError::NoNode(c))?.kind {
                            NodeKind::Control { name, ty } => (name.clone(), ty.clone()),
                            _ => return Err(EditError::WrongNodeKind(c)),
                        };
                        EventFrame { label: format!("{name}: Value Change"), source: Some(c), ty }
                    }
                    None => EventFrame { label: "Timeout".into(), source: None, ty: DataType::Bool },
                };
                let n = vi.node_mut(id).ok_or(EditError::NoNode(id))?;
                match &mut n.kind {
                    NodeKind::Event { frames, .. } => {
                        if frames.len() >= MAX_FRAMES || (source.is_none() && frames.iter().any(|f| f.source.is_none())) {
                            return Err(EditError::TooManyFrames);
                        }
                        frames.push(frame);
                        n.frame_view = frames.len() - 1;
                    }
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                Ok(None)
            }
            Command::SetLocalTarget { id, target } => {
                let new_ty = match target {
                    Some(t) => Some(vi.node(t).and_then(|n| n.data_type().filter(|_| n.kind.is_panel_item())).cloned().ok_or(EditError::WrongNodeKind(t))?),
                    None => None,
                };
                match &mut vi.node_mut(id).ok_or(EditError::NoNode(id))?.kind {
                    NodeKind::Local { target: t, ty, .. } => {
                        *t = target;
                        if let Some(nt) = new_ty {
                            *ty = nt;
                        }
                    }
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                prune_invalid_wires(vi);
                Ok(None)
            }
            Command::SetWrite { id, write: w } => {
                match &mut vi.node_mut(id).ok_or(EditError::NoNode(id))?.kind {
                    NodeKind::Local { write, .. } | NodeKind::Global { write, .. } => *write = w,
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                vi.wires.retain(|x| x.from.0 != id && x.to.0 != id);
                Ok(None)
            }
            Command::SetGlobalName { id, name: new } => {
                match &mut vi.node_mut(id).ok_or(EditError::NoNode(id))?.kind {
                    NodeKind::Global { name, .. } => *name = new,
                    _ => return Err(EditError::WrongNodeKind(id)),
                }
                Ok(None)
            }
            Command::SetWireIndexing { to, enabled } => {
                if let Some(i) = vi.wires.iter().position(|w| w.to == to) {
                    let w = vi.wires[i];
                    let crosses = !typing::crossing_loops(vi, w.from.0, w.to.0).0.is_empty() || !typing::crossing_loops(vi, w.from.0, w.to.0).1.is_empty();
                    vi.wires[i].auto_index = enabled && crosses;
                    prune_invalid_wires(vi);
                }
                Ok(None)
            }
            Command::Drop(id) => {
                let n = vi.node(id).ok_or(EditError::NoNode(id))?;
                let center = [n.pos[0] + n.size[0] / 2.0, n.pos[1] + n.size[1] / 2.0];
                let parent = vi.container_at(center, Some(id));
                let frame = parent.map(|p| vi.frame_at(p, center)).unwrap_or(0);
                let n = vi.node_mut(id).unwrap();
                n.parent = parent;
                n.frame = frame;
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
            let dest = vi.node(w.to.0);
            let inner = dest.and_then(|d| d.kind.inputs().get(w.to.1).map(|t| t.inner)).unwrap_or(false);
            let delay = matches!(dest.map(|d| &d.kind), Some(NodeKind::Feedback { .. })) && w.to.1 == 0;
            if !inner && !delay {
                stack.push(w.to.0);
            }
        }
    }
    false
}

fn refresh_size(vi: &mut Vi, id: NodeId) {
    if let Some(n) = vi.node_mut(id) {
        if !n.kind.is_loop() {
            n.size = n.kind.default_size();
        }
    }
}

/// Drops wires that no longer type-check after an upstream type change.
fn prune_invalid_wires(vi: &mut Vi) {
    loop {
        let bad = vi.wires.iter().position(|w| typing::check_connect(vi, w.from, w.to, w.auto_index).is_err());
        match bad {
            Some(i) => {
                vi.wires.remove(i);
            }
            None => break,
        }
    }
}

fn remove_subtree(vi: &mut Vi, id: NodeId) {
    let mut doomed = vi.descendants(id);
    doomed.push(id);
    vi.nodes.retain(|n| !doomed.contains(&n.id));
    vi.wires.retain(|w| !doomed.contains(&w.from.0) && !doomed.contains(&w.to.0));
    for n in vi.nodes.iter_mut() {
        if let NodeKind::Local { target: Some(t), .. } = &mut n.kind {
            if doomed.contains(t) {
                n.kind = NodeKind::Local { target: None, ty: DataType::DBL, write: matches!(n.kind, NodeKind::Local { write: true, .. }) };
            }
        }
    }
}

/// After inserting (`insert`) or removing a frame at `f`, renumber the per-frame tunnel and event terminals.
fn remap_frame_wires(vi: &mut Vi, id: NodeId, f: usize, insert: bool) {
    let is_event = matches!(vi.node(id).map(|n| &n.kind), Some(NodeKind::Event { .. }));
    let mut keep = vec![];
    for mut w in vi.wires.drain(..) {
        if w.to.0 == id && w.to.1 >= 1 {
            let (k, ff) = ((w.to.1 - 1) / MAX_FRAMES, (w.to.1 - 1) % MAX_FRAMES);
            if insert && ff >= f {
                w.to.1 = NodeKind::tunnel_in_idx(k, ff + 1);
            } else if !insert && ff == f {
                continue;
            } else if !insert && ff > f {
                w.to.1 = NodeKind::tunnel_in_idx(k, ff - 1);
            }
        }
        if is_event && w.from.0 == id && w.from.1 < MAX_FRAMES {
            if insert && w.from.1 >= f {
                w.from.1 += 1;
            } else if !insert && w.from.1 == f {
                continue;
            } else if !insert && w.from.1 > f {
                w.from.1 -= 1;
            }
        }
        keep.push(w);
    }
    vi.wires = keep;
}

/// Relabel an empty Case structure to match the type wired into its selector.
fn sync_case_labels(vi: &mut Vi, id: NodeId, ty: &DataType) {
    let has_children = vi.nodes.iter().any(|n| n.parent == Some(id));
    let Some(NodeKind::Case { fixed: None, .. }) = vi.node(id).map(|n| n.kind.clone()) else { return };
    if has_children {
        return;
    }
    let (labels, default): (Vec<String>, Option<usize>) = match ty {
        DataType::Bool => (vec!["False".into(), "True".into()], None),
        DataType::Enum(items) if !items.is_empty() => (items.clone(), None),
        DataType::Num(_) => (vec!["0".into(), "1".into()], Some(1)),
        DataType::Str => (vec!["A".into(), "B".into()], Some(1)),
        _ => return,
    };
    let n = vi.node_mut(id).unwrap();
    if let NodeKind::Case { labels: l, default: d, .. } = &mut n.kind {
        *l = labels;
        *d = default;
    }
    n.frame_view = n.frame_view.min(n.kind.frame_count() - 1);
    let count = n.kind.frame_count();
    vi.wires.retain(|w| !(w.to.0 == id && w.to.1 >= 1 && (w.to.1 - 1) % MAX_FRAMES >= count));
}
