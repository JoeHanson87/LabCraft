use crate::{diagram, panel, theme};
use eframe::egui::{self, Pos2, RichText};
use labcraft_core::{run_with, Command, DataType, Editor, Field, LoopKind, NodeId, NodeKind, NumKind, RunControl, RunError, Value, Vi, Widget};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

const CHART_HISTORY: usize = 2000;

#[derive(PartialEq, Clone, Copy)]
pub enum View {
    FrontPanel,
    BlockDiagram,
}

enum Msg {
    Indicator(NodeId, Value),
    ControlWrite(NodeId, Value),
    Done(Result<(), RunError>),
}

struct Running {
    rx: Receiver<Msg>,
    abort: Arc<AtomicBool>,
}

pub struct LabCraft {
    pub ed: Editor,
    pub view: View,
    pub selected: Option<NodeId>,
    pub wiring: Option<(NodeId, usize)>,
    pub continuous: bool,
    pub status: String,
    pub pending: Vec<Command>,
    running: Option<Running>,
    live: Arc<Mutex<HashMap<NodeId, Value>>>,
    live_pushed: HashMap<NodeId, Value>,
    globals: Arc<Mutex<HashMap<String, Value>>>,
    spawn_count: usize,
    pub editing_const: Option<NodeId>,
    pub wire_menu: Option<((NodeId, usize), Pos2)>,
}

impl LabCraft {
    pub fn new() -> Self {
        let mut app = LabCraft {
            ed: Editor::new(Vi::new("Untitled 1.vi")),
            view: View::FrontPanel,
            selected: None,
            wiring: None,
            continuous: false,
            status: "Ready".into(),
            pending: vec![],
            running: None,
            live: Arc::new(Mutex::new(HashMap::new())),
            live_pushed: HashMap::new(),
            globals: Arc::new(Mutex::new(HashMap::new())),
            spawn_count: 0,
            editing_const: None,
            wire_menu: None,
        };
        app.load_example(Example::Loops);
        app
    }

    pub fn load_example(&mut self, ex: Example) {
        let mut ed = Editor::new(Vi::new(match ex {
            Example::Loops => "Loops and Graphs.vi",
            Example::Types => "Arrays and Clusters.vi",
            Example::Structures => "Structures.vi",
        }));
        match ex {
            Example::Loops => demo_loops(&mut ed),
            Example::Types => demo_types(&mut ed),
            Example::Structures => demo_structures(&mut ed),
        }
        self.ed = ed;
        self.selected = None;
        self.status = "Example loaded".into();
    }

    pub fn spawn(&mut self, kind: NodeKind, widget: Option<Widget>) {
        let i = self.spawn_count as f32;
        self.spawn_count += 1;
        let pos = [160.0 + (i % 6.0) * 28.0, 120.0 + (i % 6.0) * 28.0];
        self.pending.push(Command::AddNode { kind, pos, widget });
    }

    fn name_for(&self, base: &str) -> String {
        let n = self.ed.vi.nodes.iter().filter(|n| n.kind.title().starts_with(base)).count();
        if n == 0 { base.into() } else { format!("{base} {}", n + 1) }
    }

    fn start_run(&mut self, reset_charts: bool) {
        if reset_charts {
            for n in self.ed.vi.nodes.iter_mut().filter(|n| n.widget == Widget::Chart) {
                n.value = Some(Value::default_for(&DataType::arr_dbl()));
            }
        }
        let vi = self.ed.vi.clone();
        let (tx, rx) = channel();
        let abort = Arc::new(AtomicBool::new(false));
        let txi = tx.clone();
        let txc = tx.clone();
        // Start every run from the controls' current panel values.
        self.live.lock().unwrap().clear();
        self.live_pushed.clear();
        let ctl = RunControl {
            abort: abort.clone(),
            live_controls: self.live.clone(),
            globals: self.globals.clone(),
            on_indicator: Box::new(move |id, v| {
                let _ = txi.send(Msg::Indicator(id, v.clone()));
            }),
            on_control_write: Box::new(move |id, v| {
                let _ = txc.send(Msg::ControlWrite(id, v.clone()));
            }),
        };
        std::thread::spawn(move || {
            let r = run_with(&vi, &ctl).map(|_| ());
            let _ = tx.send(Msg::Done(r));
        });
        self.running = Some(Running { rx, abort });
        self.status = "Running…".into();
    }

    fn abort(&mut self) {
        self.continuous = false;
        if let Some(r) = &self.running {
            r.abort.store(true, Ordering::Relaxed);
        }
    }

    fn pump(&mut self) {
        let msgs: Vec<Msg> = self.running.as_ref().map(|r| r.rx.try_iter().collect()).unwrap_or_default();
        for m in msgs {
            match m {
                Msg::Indicator(id, v) => {
                    if let Some(n) = self.ed.vi.node_mut(id) {
                        match (n.widget, &v, &mut n.value) {
                            (Widget::Chart, Value::Dbl(x), Some(Value::Array(h))) => {
                                h.data.push(Value::Dbl(*x));
                                if h.data.len() > CHART_HISTORY {
                                    h.data.remove(0);
                                }
                                h.dims = vec![h.data.len()];
                            }
                            _ => n.value = Some(v),
                        }
                    }
                }
                Msg::ControlWrite(id, v) => {
                    if let Some(n) = self.ed.vi.node_mut(id) {
                        n.value = Some(v.clone());
                    }
                    self.live_pushed.insert(id, v);
                }
                Msg::Done(res) => {
                    self.running = None;
                    match res {
                        Ok(()) if self.continuous => self.start_run(false),
                        Ok(()) => self.status = "Run complete".into(),
                        Err(RunError::Aborted) => self.status = "Aborted".into(),
                        Err(e) => {
                            self.continuous = false;
                            self.status = format!("Broken: {e}");
                        }
                    }
                    break;
                }
            }
        }
        // Publish control values the user changed so a running VI sees them (Stop buttons, events).
        let mut live = self.live.lock().unwrap();
        for n in self.ed.vi.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Control { .. })) {
            if let Some(v) = &n.value {
                if self.live_pushed.get(&n.id) != Some(v) {
                    live.insert(n.id, v.clone());
                    self.live_pushed.insert(n.id, v.clone());
                }
            }
        }
    }

    fn flush(&mut self) {
        for c in std::mem::take(&mut self.pending) {
            let removing = if let Command::RemoveNode(id) = &c { Some(*id) } else { None };
            match self.ed.apply(c) {
                Ok(_) => {
                    if removing.is_some() && removing == self.selected {
                        self.selected = None;
                    }
                }
                Err(e) => self.status = format!("Edit rejected: {e}"),
            }
        }
    }

    fn section(ui: &mut egui::Ui, title: &str, color: egui::Color32, body: impl FnOnce(&mut egui::Ui)) {
        egui::CollapsingHeader::new(RichText::new(title).strong().color(color))
            .default_open(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                body(ui);
            });
    }

    fn item(ui: &mut egui::Ui, label: &str) -> bool {
        ui.add_sized([ui.available_width(), 22.0], egui::Button::new(label).frame(false).fill(egui::Color32::TRANSPARENT))
            .on_hover_text(format!("Add {label}"))
            .clicked()
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.view == View::FrontPanel { "Controls" } else { "Functions" });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.view == View::FrontPanel {
                for (title, ty, items) in panel_palette() {
                    Self::section(ui, title, theme::type_color(&ty), |ui| {
                        for it in items {
                            if Self::item(ui, it.label) {
                                let name = self.name_for(it.name);
                                let kind = if it.ctl { NodeKind::Control { name, ty: it.ty } } else { NodeKind::Indicator { name, ty: it.ty } };
                                self.spawn(kind, Some(it.widget));
                            }
                        }
                    });
                }
            } else {
                for (title, ty, items) in diagram_palette() {
                    Self::section(ui, title, theme::type_color(&ty), |ui| {
                        for (label, kind) in items {
                            if Self::item(ui, &label) {
                                self.spawn(kind, None);
                            }
                        }
                    });
                }
            }
        });
    }
}

#[derive(Clone, Copy)]
pub enum Example {
    Loops,
    Types,
    Structures,
}

struct PanelItem {
    label: &'static str,
    name: &'static str,
    ty: DataType,
    ctl: bool,
    widget: Widget,
}

fn pi(label: &'static str, name: &'static str, ty: DataType, ctl: bool, widget: Widget) -> PanelItem {
    PanelItem { label, name, ty, ctl, widget }
}

fn sensor_type() -> DataType {
    DataType::Cluster(vec![
        Field { name: "x".into(), ty: DataType::DBL },
        Field { name: "flag".into(), ty: DataType::Bool },
        Field { name: "text".into(), ty: DataType::Str },
    ])
}

fn enum_type() -> DataType {
    DataType::Enum(vec!["Item 0".into(), "Item 1".into(), "Item 2".into()])
}

fn panel_palette() -> Vec<(&'static str, DataType, Vec<PanelItem>)> {
    use Widget::*;
    let d = || DataType::DBL;
    vec![
        ("Numeric", d(), vec![
            pi("Numeric Control", "Numeric", d(), true, NumCtl),
            pi("Knob", "Knob", d(), true, Knob),
            pi("Horizontal Slide", "Slide", d(), true, Slide),
            pi("Numeric Indicator", "Numeric Indicator", d(), false, NumInd),
            pi("Gauge", "Gauge", d(), false, Gauge),
            pi("Thermometer", "Thermometer", d(), false, Thermometer),
            pi("Tank", "Tank", d(), false, Tank),
        ]),
        ("Boolean", DataType::Bool, vec![
            pi("Toggle Switch", "Toggle", DataType::Bool, true, Toggle),
            pi("Push Button", "Button", DataType::Bool, true, PushButton),
            pi("Round LED", "LED", DataType::Bool, false, Led),
        ]),
        ("String", DataType::Str, vec![
            pi("String Control", "String", DataType::Str, true, StrCtl),
            pi("String Indicator", "String Indicator", DataType::Str, false, StrInd),
        ]),
        ("Enum", DataType::Enum(vec![]), vec![
            pi("Enum Control", "Enum", enum_type(), true, EnumCtl),
            pi("Enum Indicator", "Enum Indicator", enum_type(), false, EnumInd),
        ]),
        ("Array", DataType::array(DataType::DBL, 1), vec![
            pi("Numeric Array Control", "Array", DataType::arr_dbl(), true, ArrayCtl),
            pi("Numeric Array Indicator", "Array Indicator", DataType::arr_dbl(), false, ArrayInd),
            pi("2D Numeric Array Control", "2D Array", DataType::array(DataType::DBL, 2), true, ArrayCtl),
            pi("2D Numeric Array Indicator", "2D Array Indicator", DataType::array(DataType::DBL, 2), false, ArrayInd),
            pi("Boolean Array Control", "Boolean Array", DataType::array(DataType::Bool, 1), true, ArrayCtl),
            pi("String Array Control", "String Array", DataType::array(DataType::Str, 1), true, ArrayCtl),
        ]),
        ("Cluster", DataType::Cluster(vec![]), vec![
            pi("Cluster Control", "Cluster", sensor_type(), true, ClusterCtl),
            pi("Cluster Indicator", "Cluster Indicator", sensor_type(), false, ClusterInd),
            pi("Error In", "error in", DataType::error_cluster(), true, ClusterCtl),
            pi("Error Out", "error out", DataType::error_cluster(), false, ClusterInd),
        ]),
        ("Graph", DataType::arr_dbl(), vec![
            pi("Waveform Chart", "Waveform Chart", d(), false, Chart),
            pi("Waveform Graph", "Waveform Graph", DataType::arr_dbl(), false, Graph),
        ]),
    ]
}

fn diagram_palette() -> Vec<(&'static str, DataType, Vec<(String, NodeKind)>)> {
    let l = |s: &str, k: NodeKind| (s.to_string(), k);
    let conv = NumKind::ALL.iter().map(|k| (format!("To {}", k.label()), NodeKind::Convert(*k))).collect();
    vec![
        ("Structures", DataType::I32, vec![
            l("While Loop", NodeKind::Loop { kind: LoopKind::While, regs: vec![] }),
            l("For Loop", NodeKind::Loop { kind: LoopKind::For, regs: vec![] }),
            l("Case Structure", NodeKind::Case { labels: vec!["False".into(), "True".into()], default: None, outs: vec![], fixed: None }),
            l("Flat Sequence", NodeKind::Sequence { frames: 2 }),
            l("Event Structure", NodeKind::Event { frames: vec![], outs: vec![] }),
            l("Diagram Disable Structure", NodeKind::Case { labels: vec!["Disabled".into(), "Enabled".into()], default: None, outs: vec![], fixed: Some(1) }),
        ]),
        ("Variables", DataType::Cluster(vec![]), vec![
            l("Local Variable", NodeKind::Local { target: None, ty: DataType::DBL, write: false }),
            l("Global Variable", NodeKind::Global { name: "Global 1".into(), ty: DataType::DBL, write: false }),
            l("Feedback Node", NodeKind::Feedback { ty: DataType::DBL }),
        ]),
        ("Numeric", DataType::DBL, vec![
            l("Add", NodeKind::Add), l("Subtract", NodeKind::Subtract), l("Multiply", NodeKind::Multiply), l("Divide", NodeKind::Divide),
            l("Increment", NodeKind::Increment), l("Decrement", NodeKind::Decrement), l("Negate", NodeKind::Negate),
            l("Absolute Value", NodeKind::Absolute), l("Square Root", NodeKind::SquareRoot),
            l("Sine", NodeKind::Sine), l("Random Number", NodeKind::Random),
        ]),
        ("Constants", DataType::Cluster(vec![]), vec![
            l("Numeric Constant", NodeKind::Constant(DataType::DBL)),
            l("Integer Constant", NodeKind::Constant(DataType::I32)),
            l("Boolean Constant", NodeKind::Constant(DataType::Bool)),
            l("String Constant", NodeKind::Constant(DataType::Str)),
            l("Enum Constant", NodeKind::Constant(enum_type())),
            l("Array Constant", NodeKind::Constant(DataType::arr_dbl())),
            l("Cluster Constant", NodeKind::Constant(sensor_type())),
        ]),
        ("Conversion", DataType::I32, conv),
        ("Boolean", DataType::Bool, vec![l("And", NodeKind::And), l("Or", NodeKind::Or), l("Exclusive Or", NodeKind::Xor), l("Not", NodeKind::Not)]),
        ("Comparison", DataType::Bool, vec![
            l("Greater?", NodeKind::Greater), l("Less?", NodeKind::Less), l("Greater Or Equal?", NodeKind::GreaterEq),
            l("Less Or Equal?", NodeKind::LessEq), l("Equal?", NodeKind::Equal), l("Not Equal?", NodeKind::NotEq), l("Select", NodeKind::Select),
        ]),
        ("String", DataType::Str, vec![l("Concatenate Strings", NodeKind::Concatenate), l("Number To String", NodeKind::NumToString)]),
        ("Array", DataType::arr_dbl(), vec![
            l("Build Array", NodeKind::BuildArray), l("Array Size", NodeKind::ArraySize), l("Index Array", NodeKind::IndexArray),
            l("Initialize Array", NodeKind::InitializeArray), l("Transpose 2D Array", NodeKind::Transpose),
        ]),
        ("Cluster", DataType::Cluster(vec![]), vec![
            l("Bundle", NodeKind::Bundle { n: 2 }), l("Unbundle", NodeKind::Unbundle { n: 2 }),
            l("Bundle By Name", NodeKind::BundleByName { names: vec![String::new()] }),
            l("Unbundle By Name", NodeKind::UnbundleByName { names: vec![String::new()] }),
        ]),
        ("Error Handling", DataType::error_cluster(), vec![l("Error From Code", NodeKind::ErrorFromCode), l("Merge Errors", NodeKind::MergeErrors)]),
        ("Timing", DataType::I32, vec![l("Wait (ms)", NodeKind::WaitMs)]),
    ]
}

fn add(ed: &mut Editor, kind: NodeKind, pos: [f32; 2], widget: Option<Widget>) -> NodeId {
    ed.apply(Command::AddNode { kind, pos, widget }).unwrap().unwrap()
}

fn konst(ed: &mut Editor, ty: DataType, v: Value, pos: [f32; 2]) -> NodeId {
    let id = add(ed, NodeKind::Constant(ty), pos, None);
    ed.apply(Command::SetValue { id, value: v }).unwrap();
    id
}

fn wire(ed: &mut Editor, from: (NodeId, usize), to: (NodeId, usize)) {
    ed.apply(Command::Connect { from, to }).unwrap();
}

fn set(ed: &mut Editor, id: NodeId, v: Value) {
    ed.apply(Command::SetValue { id, value: v }).unwrap();
}

/// M2 demo: a For loop builds a sine array for a graph; a While loop streams a chart until Stop.
fn demo_loops(ed: &mut Editor) {
    let ctl = |name: &str, ty| NodeKind::Control { name: name.into(), ty };
    let ind = |name: &str, ty| NodeKind::Indicator { name: name.into(), ty };
    let fl = add(ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![DataType::arr_dbl()] }, [230.0, 30.0], None);
    let wl = add(ed, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [230.0, 300.0], None);
    for id in [fl, wl] {
        ed.apply(Command::Resize { id, size: [560.0, 230.0] }).unwrap();
    }
    let n = konst(ed, DataType::I32, Value::Int(64), [80.0, 40.0]);
    let mul = add(ed, NodeKind::Multiply, [290.0, 60.0], None);
    let k = konst(ed, DataType::DBL, Value::Dbl(0.2), [290.0, 150.0]);
    let sin = add(ed, NodeKind::Sine, [430.0, 60.0], None);
    let ba = add(ed, NodeKind::BuildArray, [580.0, 60.0], None);
    let graph = add(ed, ind("Sine Array", DataType::arr_dbl()), [800.0, 60.0], Some(Widget::Graph));
    let stop = add(ed, ctl("Stop", DataType::Bool), [60.0, 380.0], Some(Widget::PushButton));
    let mul2 = add(ed, NodeKind::Multiply, [290.0, 340.0], None);
    let k2 = konst(ed, DataType::DBL, Value::Dbl(0.3), [290.0, 430.0]);
    let sin2 = add(ed, NodeKind::Sine, [430.0, 340.0], None);
    let ms = konst(ed, DataType::I32, Value::Int(40), [430.0, 440.0]);
    let wait = add(ed, NodeKind::WaitMs, [580.0, 440.0], None);
    let chart = add(ed, ind("Sine Chart", DataType::DBL), [800.0, 340.0], Some(Widget::Chart));
    let gauge = add(ed, ind("Level", DataType::DBL), [800.0, 200.0], Some(Widget::Gauge));
    let amp = add(ed, ctl("Amplitude", DataType::DBL), [60.0, 200.0], Some(Widget::Knob));
    let scale = add(ed, NodeKind::Multiply, [580.0, 340.0], None);
    wire(ed, (n, 0), (fl, 0));
    wire(ed, (fl, 0), (mul, 0));
    wire(ed, (k, 0), (mul, 1));
    wire(ed, (mul, 0), (sin, 0));
    wire(ed, (fl, 1), (ba, 0));
    wire(ed, (sin, 0), (ba, 1));
    wire(ed, (ba, 0), (fl, 2));
    wire(ed, (fl, 2), (graph, 0));
    wire(ed, (stop, 0), (wl, 0));
    wire(ed, (wl, 0), (mul2, 0));
    wire(ed, (k2, 0), (mul2, 1));
    wire(ed, (mul2, 0), (sin2, 0));
    wire(ed, (sin2, 0), (scale, 0));
    wire(ed, (amp, 0), (scale, 1));
    wire(ed, (scale, 0), (chart, 0));
    wire(ed, (scale, 0), (gauge, 0));
    wire(ed, (ms, 0), (wait, 0));
    for (id, range) in [(gauge, [-10.0, 10.0]), (amp, [0.0, 10.0])] {
        ed.apply(Command::SetRange { id, range }).unwrap();
    }
    set(ed, amp, Value::Dbl(5.0));
}

/// M4 demo: Case structure selected by an enum, auto-indexing, a Feedback Node, and an Event
/// structure inside a While loop that counts button presses and shows the count via a Local Variable.
fn demo_structures(ed: &mut Editor) {
    let ctl = |name: &str, ty| NodeKind::Control { name: name.into(), ty };
    let ind = |name: &str, ty| NodeKind::Indicator { name: name.into(), ty };
    let view = |ed: &mut Editor, id, frame| ed.apply(Command::SetFrameView { id, frame }).unwrap();

    // --- Case structure chosen by an enum ---
    let mode_ty = DataType::Enum(vec!["Square".into(), "Double".into(), "Negate".into()]);
    let x = add(ed, ctl("x", DataType::DBL), [20.0, 20.0], None);
    let mode = add(ed, ctl("Operation", mode_ty), [20.0, 100.0], None);
    set(ed, x, Value::Dbl(7.0));
    set(ed, mode, Value::Int(1));
    let case = add(ed, NodeKind::Case { labels: vec!["False".into(), "True".into()], default: None, outs: vec![], fixed: None }, [170.0, 20.0], None);
    ed.apply(Command::Resize { id: case, size: [330.0, 210.0] }).unwrap();
    wire(ed, (mode, 0), (case, 0)); // frames take the enum's item names
    ed.apply(Command::AddTunnel { id: case, ty: DataType::DBL }).unwrap();
    view(ed, case, 0);
    let sq = add(ed, NodeKind::Multiply, [200.0, 70.0], None);
    wire(ed, (x, 0), (sq, 0));
    wire(ed, (x, 0), (sq, 1));
    wire(ed, (sq, 0), (case, NodeKind::tunnel_in_idx(0, 0)));
    view(ed, case, 1);
    let dbl = add(ed, NodeKind::Multiply, [200.0, 70.0], None);
    let two = konst(ed, DataType::DBL, Value::Dbl(2.0), [330.0, 70.0]);
    wire(ed, (x, 0), (dbl, 0));
    wire(ed, (two, 0), (dbl, 1));
    wire(ed, (dbl, 0), (case, NodeKind::tunnel_in_idx(0, 1)));
    view(ed, case, 2);
    let neg = add(ed, NodeKind::Negate, [200.0, 70.0], None);
    wire(ed, (x, 0), (neg, 0));
    wire(ed, (neg, 0), (case, NodeKind::tunnel_in_idx(0, 2)));
    view(ed, case, 1);
    let result = add(ed, ind("Result", DataType::DBL), [540.0, 90.0], None);
    wire(ed, (case, 0), (result, 0));

    // --- Auto-indexing: the For loop count comes from the array, results are gathered ---
    let values = add(ed, ctl("Values", DataType::arr_dbl()), [20.0, 260.0], None);
    set(ed, values, Value::from_f64_vec(&[1.0, 2.0, 3.0, 4.0, 5.0]));
    let f1 = add(ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [170.0, 260.0], None);
    ed.apply(Command::Resize { id: f1, size: [330.0, 190.0] }).unwrap();
    let m10 = add(ed, NodeKind::Multiply, [200.0, 310.0], None);
    let ten = konst(ed, DataType::DBL, Value::Dbl(10.0), [340.0, 310.0], );
    let times = add(ed, ind("Times Ten", DataType::arr_dbl()), [540.0, 310.0], None);
    wire(ed, (values, 0), (m10, 0));
    wire(ed, (ten, 0), (m10, 1));
    wire(ed, (m10, 0), (times, 0));

    // --- Feedback Node: running sum, gathered into a graph ---
    let n8 = konst(ed, DataType::I32, Value::Int(8), [600.0, 420.0]);
    let f2 = add(ed, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [740.0, 380.0], None);
    ed.apply(Command::Resize { id: f2, size: [340.0, 190.0] }).unwrap();
    let fb = add(ed, NodeKind::Feedback { ty: DataType::DBL }, [770.0, 430.0], None);
    let acc = add(ed, NodeKind::Add, [920.0, 430.0], None);
    let sums = add(ed, ind("Running Sum", DataType::arr_dbl()), [880.0, 600.0], Some(Widget::Graph));
    wire(ed, (n8, 0), (f2, 0));
    wire(ed, (acc, 0), (fb, 0));
    wire(ed, (fb, 0), (acc, 0));
    wire(ed, (f2, 0), (acc, 1));
    wire(ed, (acc, 0), (sums, 0));

    // --- Event structure in a While loop: count button presses ---
    let add_btn = add(ed, ctl("Add", DataType::Bool), [600.0, 20.0], Some(Widget::PushButton));
    let stop = add(ed, ctl("Stop", DataType::Bool), [600.0, 120.0], Some(Widget::PushButton));
    let zero = konst(ed, DataType::DBL, Value::Dbl(0.0), [600.0, 230.0]);
    let timeout = konst(ed, DataType::I32, Value::Int(100), [600.0, 300.0]);
    let wl = add(ed, NodeKind::Loop { kind: LoopKind::While, regs: vec![DataType::DBL] }, [740.0, 20.0], None);
    ed.apply(Command::Resize { id: wl, size: [340.0, 340.0] }).unwrap();
    let ev = add(ed, NodeKind::Event { frames: vec![], outs: vec![] }, [770.0, 50.0], None);
    ed.apply(Command::Resize { id: ev, size: [260.0, 170.0] }).unwrap();
    ed.apply(Command::Drop(ev)).unwrap();
    ed.apply(Command::AddEventFrame { id: ev, source: Some(add_btn) }).unwrap();
    ed.apply(Command::AddEventFrame { id: ev, source: None }).unwrap();
    ed.apply(Command::AddTunnel { id: ev, ty: DataType::DBL }).unwrap();
    view(ed, ev, 0);
    let inc = add(ed, NodeKind::Increment, [800.0, 100.0], None);
    wire(ed, (wl, 1), (inc, 0));
    wire(ed, (inc, 0), (ev, NodeKind::tunnel_in_idx(0, 0)));
    view(ed, ev, 1);
    let keep = add(ed, NodeKind::Convert(NumKind::Dbl), [800.0, 100.0], None);
    wire(ed, (wl, 1), (keep, 0));
    wire(ed, (keep, 0), (ev, NodeKind::tunnel_in_idx(0, 1)));
    view(ed, ev, 0);
    let count = add(ed, ind("Count", DataType::DBL), [600.0, 330.0], None);
    let show = add(ed, NodeKind::Local { target: None, ty: DataType::DBL, write: true }, [800.0, 260.0], None);
    ed.apply(Command::SetLocalTarget { id: show, target: Some(count) }).unwrap();
    wire(ed, (zero, 0), (wl, 1));
    wire(ed, (timeout, 0), (ev, 0));
    wire(ed, (ev, labcraft_core::MAX_FRAMES), (wl, 2)); // tunnel -> next count
    wire(ed, (ev, labcraft_core::MAX_FRAMES), (show, 0)); // tunnel -> Local Variable (write Count)
    wire(ed, (stop, 0), (wl, 0));
}

/// M3 demo: typed numerics, array/cluster polymorphism, enums, Select, Bundle/Unbundle.
fn demo_types(ed: &mut Editor) {
    let ctl = |name: &str, ty| NodeKind::Control { name: name.into(), ty };
    let ind = |name: &str, ty| NodeKind::Indicator { name: name.into(), ty };
    let sensor = DataType::Cluster(vec![
        Field { name: "temp".into(), ty: DataType::DBL },
        Field { name: "count".into(), ty: DataType::U8 },
        Field { name: "label".into(), ty: DataType::Str },
    ]);
    let units = DataType::Enum(vec!["Celsius".into(), "Fahrenheit".into()]);

    let readings = add(ed, ctl("Readings", DataType::arr_dbl()), [20.0, 20.0], None);
    let gain = add(ed, ctl("Gain", DataType::DBL), [20.0, 110.0], Some(Widget::Knob));
    let sens = add(ed, ctl("Sensor", sensor.clone()), [20.0, 300.0], None);
    let mode = add(ed, ctl("Units", units), [20.0, 480.0], None);
    set(ed, readings, Value::from_f64_vec(&[1.0, 2.0, 3.0, 4.0]));
    set(ed, gain, Value::Dbl(2.5));
    set(ed, sens, Value::Cluster(vec![Value::Dbl(21.5), Value::Int(200), Value::Str("probe".into())]));
    set(ed, mode, Value::Int(1));

    let mul = add(ed, NodeKind::Multiply, [190.0, 40.0], None);
    wire(ed, (readings, 0), (mul, 0));
    wire(ed, (gain, 0), (mul, 1));
    let scaled = add(ed, ind("Scaled", DataType::arr_dbl()), [700.0, 40.0], None);
    let scaled_g = add(ed, ind("Scaled Graph", DataType::arr_dbl()), [700.0, 130.0], Some(Widget::Graph));
    wire(ed, (mul, 0), (scaled, 0));
    wire(ed, (mul, 0), (scaled_g, 0));

    let un = add(ed, NodeKind::UnbundleByName { names: vec!["temp".into(), "count".into(), "label".into()] }, [190.0, 300.0], None);
    wire(ed, (sens, 0), (un, 0));
    let k18 = konst(ed, DataType::DBL, Value::Dbl(1.8), [360.0, 390.0]);
    let mulf = add(ed, NodeKind::Multiply, [360.0, 300.0], None);
    wire(ed, (un, 0), (mulf, 0));
    wire(ed, (k18, 0), (mulf, 1));
    let k32 = konst(ed, DataType::DBL, Value::Dbl(32.0), [530.0, 390.0]);
    let addf = add(ed, NodeKind::Add, [530.0, 300.0], None);
    wire(ed, (mulf, 0), (addf, 0));
    wire(ed, (k32, 0), (addf, 1));

    let one = konst(ed, DataType::I32, Value::Int(1), [190.0, 570.0]);
    let eq = add(ed, NodeKind::Equal, [190.0, 480.0], None);
    wire(ed, (mode, 0), (eq, 0));
    wire(ed, (one, 0), (eq, 1));
    let sel = add(ed, NodeKind::Select, [700.0, 300.0], None);
    wire(ed, (eq, 0), (sel, 0));
    wire(ed, (addf, 0), (sel, 1));
    wire(ed, (un, 0), (sel, 2));
    let disp = add(ed, ind("Display Temp", DataType::DBL), [870.0, 300.0], Some(Widget::Thermometer));
    ed.apply(Command::SetRange { id: disp, range: [-40.0, 120.0] }).unwrap();
    wire(ed, (sel, 0), (disp, 0));

    let k100 = konst(ed, DataType::U8, Value::Int(100), [360.0, 570.0]);
    let add8 = add(ed, NodeKind::Add, [360.0, 480.0], None);
    wire(ed, (un, 1), (add8, 0));
    wire(ed, (k100, 0), (add8, 1));
    let wrapped = add(ed, ind("Wrapped Count", DataType::U8), [530.0, 480.0], None);
    wire(ed, (add8, 0), (wrapped, 0));

    let bundle = add(ed, NodeKind::Bundle { n: 3 }, [700.0, 440.0], None);
    wire(ed, (sel, 0), (bundle, 1));
    wire(ed, (add8, 0), (bundle, 2));
    wire(ed, (un, 2), (bundle, 3));
    let result_ty = DataType::Cluster(vec![
        Field { name: "temp".into(), ty: DataType::DBL },
        Field { name: "count".into(), ty: DataType::U8 },
        Field { name: "label".into(), ty: DataType::Str },
    ]);
    let result = add(ed, ind("Result", result_ty), [870.0, 440.0], None);
    wire(ed, (bundle, 0), (result, 0));
}

impl eframe::App for LabCraft {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump();
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::E)) {
            self.view = if self.view == View::FrontPanel { View::BlockDiagram } else { View::FrontPanel };
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::R)) && self.running.is_none() {
            self.start_run(true);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z)) {
            let _ = self.ed.redo();
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            let _ = self.ed.undo();
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("LabCRAFT").strong().color(theme::ACCENT));
                ui.separator();
                ui.selectable_value(&mut self.view, View::FrontPanel, "Front Panel");
                ui.selectable_value(&mut self.view, View::BlockDiagram, "Block Diagram");
                ui.separator();
                let idle = self.running.is_none();
                if ui.add_enabled(idle, egui::Button::new("▶ Run")).on_hover_text("Ctrl/Cmd+R").clicked() {
                    self.start_run(true);
                }
                if ui.checkbox(&mut self.continuous, "⟳ Run Continuously").changed() && self.continuous && idle {
                    self.start_run(true);
                }
                if ui.add_enabled(!idle, egui::Button::new("■ Abort")).clicked() {
                    self.abort();
                }
                ui.separator();
                if ui.button("↶ Undo").clicked() {
                    let _ = self.ed.undo();
                }
                if ui.button("↷ Redo").clicked() {
                    let _ = self.ed.redo();
                }
                ui.separator();
                if ui.button("Save").clicked() {
                    let path = format!("{}.json", self.ed.vi.name);
                    self.status = match std::fs::write(&path, self.ed.vi.to_json()) {
                        Ok(_) => format!("Saved {path}"),
                        Err(e) => format!("Save failed: {e}"),
                    };
                }
                ui.menu_button("Examples", |ui| {
                    if ui.button("Loops and Graphs").clicked() {
                        self.load_example(Example::Loops);
                        ui.close();
                    }
                    if ui.button("Arrays and Clusters").clicked() {
                        self.load_example(Example::Types);
                        ui.close();
                    }
                    if ui.button("Structures").clicked() {
                        self.load_example(Example::Structures);
                        ui.close();
                    }
                });
                if ui.button("Open").clicked() {
                    let path = format!("{}.json", self.ed.vi.name);
                    match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| Vi::from_json(&s).map_err(|e| e.to_string())) {
                        Ok(vi) => {
                            self.ed = Editor::new(vi);
                            self.status = format!("Opened {path}");
                        }
                        Err(e) => self.status = format!("Open failed: {e}"),
                    }
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.ed.vi.name).color(theme::TEXT_DIM));
                ui.separator();
                ui.label(&self.status);
            });
        });

        egui::Panel::left("palette").default_size(190.0).show(ui, |ui| self.palette(ui));

        egui::CentralPanel::default().show(ui, |ui| match self.view {
            View::FrontPanel => panel::show(self, ui),
            View::BlockDiagram => diagram::show(self, ui),
        });

        self.flush();
        if self.running.is_some() {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(ed: &Editor, title: &str) -> NodeId {
        ed.vi.nodes.iter().find(|n| n.kind.title() == title).unwrap().id
    }

    #[test]
    fn loops_demo_runs_headless() {
        let mut ed = Editor::new(Vi::new("t"));
        demo_loops(&mut ed);
        let stop = find(&ed, "Stop");
        set(&mut ed, stop, Value::Bool(true));
        let res = labcraft_core::run(&ed.vi).unwrap();
        let graph = find(&ed, "Sine Array");
        match res.indicators.iter().find(|(id, _)| *id == graph).map(|(_, v)| v.clone()) {
            Some(Value::Array(a)) => {
                assert_eq!(a.data.len(), 64);
                assert!((a.data[5].as_f64().unwrap() - 1.0f64.sin()).abs() < 1e-9);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn types_demo_runs_headless() {
        let mut ed = Editor::new(Vi::new("t"));
        demo_types(&mut ed);
        let res = labcraft_core::run(&ed.vi).unwrap();
        let get = |t: &str| res.indicators.iter().find(|(id, _)| *id == find(&ed, t)).unwrap().1.clone();
        assert_eq!(get("Wrapped Count"), Value::Int(44)); // U8: 200 + 100 wraps
        assert_eq!(get("Scaled"), Value::from_f64_vec(&[2.5, 5.0, 7.5, 10.0]));
        match get("Display Temp") {
            Value::Dbl(v) => assert!((v - 70.7).abs() < 1e-9),
            o => panic!("{o:?}"),
        }
        assert_eq!(get("Result"), Value::Cluster(vec![Value::Dbl(70.7), Value::Int(44), Value::Str("probe".into())]));
    }

    #[test]
    fn structures_demo_runs_headless() {
        let mut ed = Editor::new(Vi::new("t"));
        demo_structures(&mut ed);
        let stop = find(&ed, "Stop");
        set(&mut ed, stop, Value::Bool(true));
        let res = labcraft_core::run(&ed.vi).unwrap();
        let get = |t: &str| res.indicators.iter().find(|(id, _)| *id == find(&ed, t)).unwrap().1.clone();
        assert_eq!(get("Result"), Value::Dbl(14.0)); // "Double" frame: 7 * 2
        assert_eq!(get("Times Ten"), Value::from_f64_vec(&[10.0, 20.0, 30.0, 40.0, 50.0]));
        assert_eq!(get("Running Sum"), Value::from_f64_vec(&[0.0, 1.0, 3.0, 6.0, 10.0, 15.0, 21.0, 28.0]));
        assert_eq!(get("Count"), Value::Dbl(0.0)); // the event timed out; nobody pressed Add
    }

    #[test]
    fn palettes_build_valid_nodes() {
        // every palette entry must be addable to a VI without error
        let mut ed = Editor::new(Vi::new("t"));
        for (_, _, items) in diagram_palette() {
            for (_, kind) in items {
                add(&mut ed, kind, [0.0, 0.0], None);
            }
        }
        for (_, _, items) in panel_palette() {
            for it in items {
                let kind = if it.ctl { NodeKind::Control { name: it.name.into(), ty: it.ty.clone() } } else { NodeKind::Indicator { name: it.name.into(), ty: it.ty.clone() } };
                assert!(it.widget.fits(&it.ty, it.ctl), "{} does not fit", it.label);
                add(&mut ed, kind, [0.0, 0.0], Some(it.widget));
            }
        }
    }

    #[test]
    fn examples_render_in_both_views_without_panicking() {
        for ex in [Example::Loops, Example::Types, Example::Structures] {
            for view in [View::FrontPanel, View::BlockDiagram] {
                let mut app = LabCraft::new();
                app.load_example(ex);
                app.view = view;
                let ctx = egui::Context::default();
                crate::theme::apply(&ctx);
                let multi: Vec<(NodeId, usize)> = app.ed.vi.nodes.iter().filter(|n| n.kind.frame_count() > 1).map(|n| (n.id, n.kind.frame_count())).collect();
                for round in 0..4 {
                    // show every frame of every multi-frame structure
                    for (id, count) in &multi {
                        app.ed.apply(Command::SetFrameView { id: *id, frame: round % count }).unwrap();
                    }
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                        ..Default::default()
                    };
                    let mut out = ctx.run_ui(input, |ui| match view {
                        View::FrontPanel => crate::panel::show(&mut app, ui),
                        View::BlockDiagram => crate::diagram::show(&mut app, ui),
                    });
                    out.textures_delta.clear();
                    app.flush();
                }
                assert!(app.status != "never");
            }
        }
    }
}
