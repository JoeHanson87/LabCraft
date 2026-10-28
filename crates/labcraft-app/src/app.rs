use crate::{diagram, panel, theme};
use eframe::egui::{self, RichText};
use labcraft_core::{run_with, Command, DataType, Editor, LoopKind, NodeId, NodeKind, RunControl, RunError, Value, Vi, Widget};
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
    spawn_count: usize,
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
            spawn_count: 0,
        };
        app.demo();
        app
    }

    /// Starter VI: a For loop builds a sine array for a graph, and a While loop streams a sine chart until Stop.
    fn demo(&mut self) {
        use DataType::*;
        let ed = &mut self.ed;
        let mut add = |kind: NodeKind, pos: [f32; 2], widget: Option<Widget>| {
            ed.apply(Command::AddNode { kind, pos, widget }).unwrap().unwrap()
        };
        let fl = add(NodeKind::Loop { kind: LoopKind::For, regs: vec![ArrDbl] }, [230.0, 30.0], None);
        let wl = add(NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [230.0, 300.0], None);
        let n = add(NodeKind::Constant(Value::Int(64)), [80.0, 40.0], None);
        let mul = add(NodeKind::Multiply, [290.0, 60.0], None);
        let k = add(NodeKind::Constant(Value::Dbl(0.2)), [290.0, 150.0], None);
        let sin = add(NodeKind::Sine, [430.0, 60.0], None);
        let ba = add(NodeKind::BuildArray, [580.0, 60.0], None);
        let graph = add(NodeKind::Indicator { name: "Sine Array".into(), ty: ArrDbl }, [800.0, 60.0], Some(Widget::Graph));
        let stop = add(NodeKind::Control { name: "Stop".into(), ty: Bool }, [60.0, 380.0], Some(Widget::PushButton));
        let mul2 = add(NodeKind::Multiply, [290.0, 340.0], None);
        let k2 = add(NodeKind::Constant(Value::Dbl(0.3)), [290.0, 430.0], None);
        let sin2 = add(NodeKind::Sine, [430.0, 340.0], None);
        let ms = add(NodeKind::Constant(Value::Int(40)), [430.0, 440.0], None);
        let wait = add(NodeKind::WaitMs, [580.0, 440.0], None);
        let chart = add(NodeKind::Indicator { name: "Sine Chart".into(), ty: Dbl }, [800.0, 340.0], Some(Widget::Chart));
        let gauge = add(NodeKind::Indicator { name: "Level".into(), ty: Dbl }, [800.0, 200.0], Some(Widget::Gauge));
        let amp = add(NodeKind::Control { name: "Amplitude".into(), ty: Dbl }, [60.0, 200.0], Some(Widget::Knob));
        let scale = add(NodeKind::Multiply, [580.0, 340.0], None);
        let _ = (n, k, k2, ms);
        for (id, size) in [(fl, [560.0, 230.0]), (wl, [560.0, 230.0])] {
            ed.apply(Command::Resize { id, size }).unwrap();
        }
        let ids: Vec<NodeId> = ed.vi.nodes.iter().filter(|n| !n.kind.is_loop()).map(|n| n.id).collect();
        for id in ids {
            ed.apply(Command::Drop(id)).unwrap();
        }
        let w = |ed: &mut Editor, from, to| ed.apply(Command::Connect { from, to }).unwrap();
        w(ed, (n, 0), (fl, 0));
        w(ed, (fl, 0), (mul, 0));
        w(ed, (k, 0), (mul, 1));
        w(ed, (mul, 0), (sin, 0));
        w(ed, (fl, 1), (ba, 0));
        w(ed, (sin, 0), (ba, 1));
        w(ed, (ba, 0), (fl, 2));
        w(ed, (fl, 2), (graph, 0));
        w(ed, (stop, 0), (wl, 0));
        w(ed, (wl, 0), (mul2, 0));
        w(ed, (k2, 0), (mul2, 1));
        w(ed, (mul2, 0), (sin2, 0));
        w(ed, (sin2, 0), (scale, 0));
        w(ed, (amp, 0), (scale, 1));
        w(ed, (scale, 0), (chart, 0));
        w(ed, (scale, 0), (gauge, 0));
        w(ed, (ms, 0), (wait, 0));
        for (id, range) in [(gauge, [-10.0, 10.0]), (amp, [0.0, 10.0])] {
            ed.apply(Command::SetRange { id, range }).unwrap();
        }
        ed.apply(Command::SetValue { id: amp, value: Value::Dbl(5.0) }).unwrap();
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
                n.value = Some(Value::ArrDbl(vec![]));
            }
        }
        let vi = self.ed.vi.clone();
        let (tx, rx) = channel();
        let abort = Arc::new(AtomicBool::new(false));
        let txi = tx.clone();
        let ctl = RunControl {
            abort: abort.clone(),
            live_controls: self.live.clone(),
            on_indicator: Box::new(move |id, v| {
                let _ = txi.send(Msg::Indicator(id, v.clone()));
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
        {
            let mut live = self.live.lock().unwrap();
            for n in self.ed.vi.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Control { .. })) {
                if let Some(v) = &n.value {
                    live.insert(n.id, v.clone());
                }
            }
        }
        let Some(r) = &self.running else { return };
        let msgs: Vec<Msg> = r.rx.try_iter().collect();
        for m in msgs {
            match m {
                Msg::Indicator(id, v) => {
                    if let Some(n) = self.ed.vi.node_mut(id) {
                        match (n.widget, &v, &mut n.value) {
                            (Widget::Chart, Value::Dbl(x), Some(Value::ArrDbl(h))) => {
                                h.push(*x);
                                if h.len() > CHART_HISTORY {
                                    h.remove(0);
                                }
                            }
                            _ => n.value = Some(v),
                        }
                    }
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
        use DataType::*;
        ui.heading(if self.view == View::FrontPanel { "Controls" } else { "Functions" });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.view == View::FrontPanel {
                let groups: [(&str, DataType, Vec<(Widget, DataType, bool)>); 4] = [
                    ("Numeric", Dbl, vec![
                        (Widget::NumCtl, Dbl, true), (Widget::Knob, Dbl, true), (Widget::Slide, Dbl, true),
                        (Widget::NumInd, Dbl, false), (Widget::Gauge, Dbl, false), (Widget::Thermometer, Dbl, false), (Widget::Tank, Dbl, false),
                    ]),
                    ("Boolean", Bool, vec![(Widget::Toggle, Bool, true), (Widget::PushButton, Bool, true), (Widget::Led, Bool, false)]),
                    ("String", Str, vec![(Widget::StrCtl, Str, true), (Widget::StrInd, Str, false)]),
                    ("Graph", ArrDbl, vec![(Widget::Chart, Dbl, false), (Widget::Graph, ArrDbl, false)]),
                ];
                for (title, ty, items) in groups {
                    Self::section(ui, title, theme::type_color(ty), |ui| {
                        for (w, ty, is_ctl) in items {
                            if Self::item(ui, w.label()) {
                                let name = self.name_for(w.label());
                                let kind = if is_ctl { NodeKind::Control { name, ty } } else { NodeKind::Indicator { name, ty } };
                                self.spawn(kind, Some(w));
                            }
                        }
                    });
                }
            } else {
                let c = |v| NodeKind::Constant(v);
                let groups: Vec<(&str, DataType, Vec<(&str, NodeKind)>)> = vec![
                    ("Structures", theme::ACCENT_TYPE, vec![
                        ("While Loop", NodeKind::Loop { kind: LoopKind::While, regs: vec![] }),
                        ("For Loop", NodeKind::Loop { kind: LoopKind::For, regs: vec![] }),
                    ]),
                    ("Numeric", Dbl, vec![
                        ("Add", NodeKind::Add), ("Subtract", NodeKind::Subtract), ("Multiply", NodeKind::Multiply), ("Divide", NodeKind::Divide),
                        ("Sine", NodeKind::Sine), ("Random Number", NodeKind::Random),
                        ("Numeric Constant", c(Value::Dbl(0.0))), ("Integer Constant", c(Value::Int(0))),
                    ]),
                    ("Boolean", Bool, vec![("And", NodeKind::And), ("Or", NodeKind::Or), ("Not", NodeKind::Not), ("Boolean Constant", c(Value::Bool(true)))]),
                    ("Comparison", Bool, vec![("Greater?", NodeKind::Greater), ("Less?", NodeKind::Less), ("Equal?", NodeKind::Equal)]),
                    ("String", Str, vec![("Concatenate Strings", NodeKind::Concatenate), ("String Constant", c(Value::Str("text".into())))]),
                    ("Array", ArrDbl, vec![("Build Array", NodeKind::BuildArray), ("Array Size", NodeKind::ArraySize), ("Index Array", NodeKind::IndexArray)]),
                    ("Timing", Int, vec![("Wait (ms)", NodeKind::WaitMs)]),
                ];
                for (title, ty, items) in groups {
                    Self::section(ui, title, theme::type_color(ty), |ui| {
                        for (label, kind) in items {
                            if Self::item(ui, label) {
                                self.spawn(kind, None);
                            }
                        }
                    });
                }
            }
        });
    }
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

    #[test]
    fn demo_vi_runs_headless() {
        let mut app = LabCraft::new();
        let stop = app.ed.vi.nodes.iter().find(|n| n.kind.title() == "Stop").unwrap().id;
        app.ed.apply(Command::SetValue { id: stop, value: Value::Bool(true) }).unwrap();
        let res = labcraft_core::run(&app.ed.vi).unwrap();
        let graph = app.ed.vi.nodes.iter().find(|n| n.kind.title() == "Sine Array").unwrap().id;
        let arr = res.indicators.iter().find(|(id, _)| *id == graph).map(|(_, v)| v.clone());
        match arr {
            Some(Value::ArrDbl(v)) => {
                assert_eq!(v.len(), 64);
                assert!((v[5] - 1.0f64.sin()).abs() < 1e-9);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
