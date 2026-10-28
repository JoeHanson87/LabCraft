use crate::{diagram, panel, theme};
use eframe::egui::{self, RichText};
use labcraft_core::{run, Command, DataType, Editor, NodeId, NodeKind, Value, Vi};

#[derive(PartialEq, Clone, Copy)]
pub enum View {
    FrontPanel,
    BlockDiagram,
}

pub struct LabCraft {
    pub ed: Editor,
    pub view: View,
    pub selected: Option<NodeId>,
    pub wiring: Option<(NodeId, usize)>,
    pub continuous: bool,
    pub status: String,
    pub pending: Vec<Command>,
    spawn_count: usize,
}

impl LabCraft {
    pub fn new() -> Self {
        let mut app = LabCraft {
            ed: Editor::new(Vi::new("Untitled 1.vi")),
            view: View::BlockDiagram,
            selected: None,
            wiring: None,
            continuous: false,
            status: "Ready".into(),
            pending: vec![],
            spawn_count: 0,
        };
        app.demo();
        app
    }

    /// Starter diagram: x + y -> Sum.
    fn demo(&mut self) {
        let mut add = |ed: &mut Editor, kind, pos| {
            ed.apply(Command::AddNode { kind, pos }).unwrap().unwrap()
        };
        let ed = &mut self.ed;
        let x = add(ed, NodeKind::Control { name: "x".into(), ty: DataType::Dbl }, [60.0, 60.0]);
        let y = add(ed, NodeKind::Control { name: "y".into(), ty: DataType::Dbl }, [60.0, 170.0]);
        let a = add(ed, NodeKind::Add, [280.0, 100.0]);
        let o = add(ed, NodeKind::Indicator { name: "Sum".into(), ty: DataType::Dbl }, [500.0, 100.0]);
        for c in [
            Command::Connect { from: (x, 0), to: (a, 0) },
            Command::Connect { from: (y, 0), to: (a, 1) },
            Command::Connect { from: (a, 0), to: (o, 0) },
            Command::SetValue { id: x, value: Value::Dbl(2.0) },
            Command::SetValue { id: y, value: Value::Dbl(3.0) },
        ] {
            ed.apply(c).unwrap();
        }
    }

    pub fn spawn(&mut self, kind: NodeKind) {
        let i = self.spawn_count as f32;
        self.spawn_count += 1;
        let pos = [200.0 + (i % 5.0) * 30.0, 220.0 + (i % 5.0) * 30.0];
        self.pending.push(Command::AddNode { kind, pos });
    }

    fn name_for(&self, base: &str) -> String {
        let n = self.ed.vi.nodes.iter().filter(|n| n.kind.title().starts_with(base)).count();
        if n == 0 { base.into() } else { format!("{base} {}", n + 1) }
    }

    fn run_vi(&mut self) {
        match run(&self.ed.vi) {
            Ok(res) => {
                for (id, v) in res.indicators {
                    if let Some(n) = self.ed.vi.node_mut(id) {
                        n.value = Some(v);
                    }
                }
                self.status = "Run OK".into();
            }
            Err(e) => {
                self.continuous = false;
                self.status = format!("Broken: {e}");
            }
        }
    }

    fn flush(&mut self) {
        for c in std::mem::take(&mut self.pending) {
            let removing = if let Command::RemoveNode(id) = &c { Some(*id) } else { None };
            match self.ed.apply(c) {
                Ok(_) => {
                    if removing == self.selected {
                        self.selected = None;
                    }
                }
                Err(e) => self.status = format!("Edit rejected: {e}"),
            }
        }
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.view == View::FrontPanel { "Controls" } else { "Functions" });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.view == View::FrontPanel {
                for (label, ty, base) in [
                    ("Numeric", DataType::Dbl, "Numeric"),
                    ("Boolean", DataType::Bool, "Boolean"),
                    ("String", DataType::Str, "String"),
                ] {
                    ui.colored_label(theme::type_color(ty), RichText::new(label).strong());
                    ui.horizontal(|ui| {
                        if ui.button("Control").clicked() {
                            let name = self.name_for(base);
                            self.spawn(NodeKind::Control { name, ty });
                        }
                        if ui.button("Indicator").clicked() {
                            let name = self.name_for(&format!("{base} Out"));
                            self.spawn(NodeKind::Indicator { name, ty });
                        }
                    });
                }
            } else {
                let groups: [(&str, DataType, Vec<NodeKind>); 4] = [
                    ("Numeric", DataType::Dbl, vec![NodeKind::Add, NodeKind::Subtract, NodeKind::Multiply, NodeKind::Divide, NodeKind::Constant(Value::Dbl(0.0))]),
                    ("Boolean", DataType::Bool, vec![NodeKind::And, NodeKind::Or, NodeKind::Not, NodeKind::Constant(Value::Bool(true))]),
                    ("Comparison", DataType::Bool, vec![NodeKind::Greater, NodeKind::Less, NodeKind::Equal]),
                    ("String", DataType::Str, vec![NodeKind::Concatenate, NodeKind::Constant(Value::Str("text".into()))]),
                ];
                for (g, ty, kinds) in groups {
                    ui.colored_label(theme::type_color(ty), RichText::new(g).strong());
                    for k in kinds {
                        let label = match &k {
                            NodeKind::Constant(v) => format!("Constant ({:?})", v.data_type()),
                            k => k.title(),
                        };
                        if ui.button(label).clicked() {
                            self.spawn(k);
                        }
                    }
                }
            }
        });
    }
}

impl eframe::App for LabCraft {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::E)) {
            self.view = if self.view == View::FrontPanel { View::BlockDiagram } else { View::FrontPanel };
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::R)) {
            self.run_vi();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            let _ = self.ed.undo();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z)) {
            let _ = self.ed.redo();
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("LabCRAFT").strong().color(theme::ACCENT));
                ui.separator();
                ui.selectable_value(&mut self.view, View::FrontPanel, "Front Panel");
                ui.selectable_value(&mut self.view, View::BlockDiagram, "Block Diagram");
                ui.separator();
                if ui.button("▶ Run").on_hover_text("Ctrl/Cmd+R").clicked() {
                    self.run_vi();
                }
                ui.checkbox(&mut self.continuous, "⟳ Run Continuously");
                if ui.button("■ Abort").clicked() {
                    self.continuous = false;
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

        egui::Panel::left("palette").default_size(170.0).show(ui, |ui| self.palette(ui));

        egui::CentralPanel::default().show(ui, |ui| match self.view {
            View::FrontPanel => panel::show(self, ui),
            View::BlockDiagram => diagram::show(self, ui),
        });

        self.flush();
        if self.continuous {
            self.run_vi();
            ctx.request_repaint();
        }
    }
}
