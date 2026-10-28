//! The LabCraft visual editor.
//!
//! The editor is a thin client over the same document IR and runtime as the CLI
//! and MCP server: it never invents its own graph semantics. The first version
//! focuses on the two views that matter — the front panel and the block
//! diagram — with a palette, inspector, live diagnostics and a run button.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eframe::egui;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use lc_model::document::{kinds, Control, ControlKind, Node, Scope, SourceRef, ViDocument};
use lc_model::{Severity, Type, Value};
use lc_nodes::Registry;
use lc_theme::{Rgba, Theme};

pub fn run(path: Option<PathBuf>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0])
            .with_title("LabCraft"),
        ..Default::default()
    };
    eframe::run_native(
        "LabCraft",
        options,
        Box::new(move |cc| Ok(Box::new(LabCraftApp::new(cc, path)))),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Panel,
    Diagram,
}

#[derive(Clone, PartialEq)]
enum Selection {
    Control(String),
    Node(String),
}

#[derive(Clone, Copy, PartialEq)]
enum PaletteTab {
    Controls,
    Nodes,
}

struct LabCraftApp {
    doc: Option<ViDocument>,
    path: Option<PathBuf>,
    theme: Theme,
    dark: bool,
    view: View,
    selection: Option<Selection>,
    diagnostics: Vec<lc_model::Diagnostic>,
    last_run: Option<lc_runtime::RunOutput>,
    status: String,
    palette_tab: PaletteTab,
    palette_search: String,
    pan: Vec2,
    zoom: f32,
    show_open: bool,
    show_save: bool,
    path_input: String,
    node_drag: bool,
}

impl LabCraftApp {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let theme = Theme::dark();
        apply_visuals(&cc.egui_ctx, &theme);
        let mut app = Self {
            doc: None,
            path: None,
            theme,
            dark: true,
            view: View::Panel,
            selection: None,
            diagnostics: Vec::new(),
            last_run: None,
            status: "Ready".into(),
            palette_tab: PaletteTab::Controls,
            palette_search: String::new(),
            pan: Vec2::new(24.0, 24.0),
            zoom: 1.0,
            show_open: false,
            show_save: false,
            path_input: String::new(),
            node_drag: false,
        };
        match path {
            Some(path) => match ViDocument::load(&path) {
                Ok(doc) => {
                    app.status = format!("Opened {}", path.display());
                    app.path = Some(path);
                    app.doc = Some(doc);
                }
                Err(err) => {
                    app.status = format!("Could not open {}: {err}", path.display());
                    app.doc = Some(ViDocument::new("Untitled"));
                }
            },
            None => app.doc = Some(example_doc()),
        }
        app.refresh_diagnostics();
        app
    }

    fn doc(&self) -> &ViDocument {
        self.doc.as_ref().expect("editor always has a document")
    }

    fn doc_mut(&mut self) -> &mut ViDocument {
        self.doc.as_mut().expect("editor always has a document")
    }

    fn refresh_diagnostics(&mut self) {
        let report = lc_runtime::compile(self.doc());
        self.diagnostics = report.diagnostics;
    }

    fn color(&self, token: &str) -> Color32 {
        color(self.theme.color(token))
    }

    fn run(&mut self) {
        let doc = self.doc().clone();
        let opts = lc_runtime::RunOptions {
            inputs: lc_runtime::default_inputs(&doc),
            ..lc_runtime::RunOptions::defaults()
        };
        match lc_runtime::run(&doc, &opts) {
            Ok(out) => {
                self.status = match &out.error {
                    Some(err) => format!("Runtime error: {err}"),
                    None => format!("Ran in {:.2} ms", out.duration_ms),
                };
                self.last_run = Some(out);
            }
            Err(lc_runtime::RunError::Compile(diags)) => {
                self.status = format!("{} compile error(s)", diags.len());
                self.last_run = None;
                self.diagnostics = diags;
            }
        }
    }

    fn new_doc(&mut self) {
        self.doc = Some(ViDocument::new("Untitled"));
        self.path = None;
        self.selection = None;
        self.last_run = None;
        self.status = "New VI".into();
        self.refresh_diagnostics();
    }

    fn save(&mut self, path: PathBuf) {
        if let Some(doc) = &self.doc {
            match doc.save(&path) {
                Ok(()) => {
                    self.status = format!("Saved {}", path.display());
                    self.path = Some(path);
                }
                Err(err) => self.status = format!("Save failed: {err}"),
            }
        }
    }

    fn open(&mut self, path: PathBuf) {
        match ViDocument::load(&path) {
            Ok(doc) => {
                self.status = format!("Opened {}", path.display());
                self.path = Some(path);
                self.doc = Some(doc);
                self.selection = None;
                self.last_run = None;
                self.refresh_diagnostics();
            }
            Err(err) => self.status = format!("Open failed: {err}"),
        }
    }

    fn add_node(&mut self, kind: &str) {
        let key = unique_key(self.doc(), &kind.replace('.', "_"));
        let node = Node::new(kind, key.clone());
        self.doc_mut().diagram.root.nodes.push(node);
        lc_layout::layout_vi(self.doc_mut());
        self.selection = Some(Selection::Node(key));
        self.view = View::Diagram;
        self.refresh_diagnostics();
        self.status = format!("Added node `{kind}`");
    }

    fn add_control(&mut self, kind: ControlKind, indicator: bool) {
        let key = unique_key(self.doc(), widget_key(kind, indicator));
        let label = widget_label(kind, indicator);
        let mut control = Control::new(key.clone(), kind, label);
        control.indicator = indicator;
        control.ty = kind.default_type();
        control.default = Value::default_for(&control.ty);
        control.bounds.w = 180.0;
        control.bounds.h = 44.0;
        self.doc_mut().panel.controls.push(control);
        let node_kind = if indicator {
            kinds::INDICATOR
        } else {
            kinds::CONTROL
        };
        let mut node = Node::new(node_kind, key.clone());
        node.props.insert("control".into(), serde_json::json!(key));
        self.doc_mut().diagram.root.nodes.push(node);
        lc_layout::layout_vi(self.doc_mut());
        self.selection = Some(Selection::Control(key));
        self.view = View::Panel;
        self.refresh_diagnostics();
        self.status = format!("Added {}", widget_label(kind, indicator));
    }

    fn delete_selection(&mut self) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        match selection {
            Selection::Control(key) => {
                self.doc_mut().panel.controls.retain(|c| c.key != key);
                remove_terminals(&mut self.doc_mut().diagram.root, &key);
                self.status = format!("Removed control `{key}`");
            }
            Selection::Node(key) => {
                if let Some(node) = self.doc().node_by_key(&key) {
                    let id = node.id.clone();
                    remove_node(&mut self.doc_mut().diagram.root, &id);
                    self.status = format!("Removed node `{key}`");
                }
            }
        }
        self.selection = None;
        lc_layout::layout_vi(self.doc_mut());
        self.refresh_diagnostics();
    }

    // ---- Panels ---------------------------------------------------------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::default()
                    .fill(self.color("bg.panel"))
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("LabCraft")
                            .color(self.color("accent"))
                            .strong()
                            .size(17.0),
                    );
                    ui.add_space(6.0);
                    let file = self
                        .path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Untitled".into());
                    ui.label(
                        egui::RichText::new(file)
                            .color(self.color("text.secondary"))
                            .size(12.0),
                    );
                    ui.add_space(12.0);
                    if ui.button("New").clicked() {
                        self.new_doc();
                    }
                    if ui.button("Open").clicked() {
                        self.path_input = self
                            .path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        self.show_open = true;
                    }
                    if ui.button("Save").clicked() {
                        match &self.path {
                            Some(path) => self.save(path.clone()),
                            None => {
                                self.path_input = "untitled.lcvi.json".into();
                                self.show_save = true;
                            }
                        }
                    }

                    ui.separator();
                    if ui
                        .selectable_label(self.view == View::Panel, "Front Panel")
                        .clicked()
                    {
                        self.view = View::Panel;
                    }
                    if ui
                        .selectable_label(self.view == View::Diagram, "Block Diagram")
                        .clicked()
                    {
                        self.view = View::Diagram;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(if self.dark { "Light" } else { "Dark" })
                            .clicked()
                        {
                            self.dark = !self.dark;
                            self.theme = if self.dark {
                                Theme::dark()
                            } else {
                                Theme::light()
                            };
                            apply_visuals(&ctx, &self.theme);
                        }
                        if ui
                            .button(egui::RichText::new("▶ Run").color(Color32::BLACK).strong())
                            .clicked()
                        {
                            self.run();
                        }
                        if ui.button("−").clicked() {
                            self.zoom = (self.zoom - 0.1).clamp(0.35, 2.5);
                        }
                        ui.label(format!("{:.0}%", self.zoom * 100.0));
                        if ui.button("+").clicked() {
                            self.zoom = (self.zoom + 0.1).clamp(0.35, 2.5);
                        }
                    });
                });
            });
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("palette")
            .resizable(true)
            .default_size(232.0)
            .frame(
                egui::Frame::default()
                    .fill(self.color("bg.panel"))
                    .inner_margin(egui::Margin::symmetric(12, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.palette_tab == PaletteTab::Controls, "Controls")
                        .clicked()
                    {
                        self.palette_tab = PaletteTab::Controls;
                    }
                    if ui
                        .selectable_label(self.palette_tab == PaletteTab::Nodes, "Nodes")
                        .clicked()
                    {
                        self.palette_tab = PaletteTab::Nodes;
                    }
                });
                ui.add_space(6.0);
                match self.palette_tab {
                    PaletteTab::Controls => self.control_palette(ui),
                    PaletteTab::Nodes => self.node_palette(ui),
                }
            });
    }

    fn control_palette(&mut self, ui: &mut egui::Ui) {
        let items: &[(&str, ControlKind, bool)] = &[
            ("Numeric control", ControlKind::Numeric, false),
            ("Slider", ControlKind::Slider, false),
            ("Toggle switch", ControlKind::Toggle, false),
            ("Push button", ControlKind::Button, false),
            ("String control", ControlKind::String, false),
            ("Path control", ControlKind::Path, false),
            ("Numeric indicator", ControlKind::Numeric, true),
            ("LED", ControlKind::Led, true),
            ("Chart", ControlKind::Chart, true),
            ("Graph", ControlKind::Graph, true),
            ("String indicator", ControlKind::String, true),
            ("Table", ControlKind::Table, true),
            ("Enum", ControlKind::Enum, false),
            ("Array", ControlKind::Array, false),
            ("Cluster", ControlKind::Cluster, false),
        ];
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (label, kind, indicator) in items {
                if ui
                    .add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::new(*label)
                            .fill(self.color("bg.elevated"))
                            .stroke(Stroke::new(1.0, self.color("border.subtle"))),
                    )
                    .clicked()
                {
                    self.add_control(*kind, *indicator);
                }
            }
        });
    }

    fn node_palette(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::TextEdit::singleline(&mut self.palette_search)
                .hint_text("Search nodes…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        let infos = if self.palette_search.trim().is_empty() {
            Registry::global().infos()
        } else {
            Registry::global().search(&self.palette_search)
        };
        let mut clicked: Option<String> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for info in infos.iter().take(120) {
                let response = ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::Button::new(
                        egui::RichText::new(format!("{}  ·  {}", info.name, info.id))
                            .color(self.color("text.primary"))
                            .size(12.0),
                    )
                    .fill(self.color("bg.elevated"))
                    .stroke(Stroke::NONE),
                );
                if response.on_hover_text(&info.description).clicked() {
                    clicked = Some(info.id.clone());
                }
            }
        });
        if let Some(kind) = clicked {
            self.add_node(&kind);
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(286.0)
            .frame(
                egui::Frame::default()
                    .fill(self.color("bg.panel"))
                    .inner_margin(egui::Margin::symmetric(12, 12)),
            )
            .show(ui, |ui| {
                ui.heading(egui::RichText::new("Inspector").size(15.0));
                ui.add_space(4.0);
                match self.selection.clone() {
                    None => {
                        ui.label(
                            egui::RichText::new("Select a control or node to inspect it.")
                                .color(self.color("text.secondary")),
                        );
                    }
                    Some(Selection::Control(key)) => self.inspect_control(ui, &key),
                    Some(Selection::Node(key)) => self.inspect_node(ui, &key),
                }
                ui.add_space(12.0);
                ui.separator();
                ui.label(
                    egui::RichText::new("Diagnostics")
                        .strong()
                        .color(self.color("text.secondary")),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        if self.diagnostics.is_empty() {
                            ui.label(
                                egui::RichText::new("No issues.")
                                    .color(self.color("text.muted"))
                                    .size(12.0),
                            );
                        }
                        for d in &self.diagnostics {
                            let color = match d.severity {
                                Severity::Error => self.color("error"),
                                Severity::Warning => self.color("warn"),
                                Severity::Info => self.color("text.secondary"),
                            };
                            ui.label(
                                egui::RichText::new(format!("{}: {}", d.code, d.message))
                                    .color(color)
                                    .size(11.5),
                            );
                        }
                    });
            });
    }

    fn inspect_control(&mut self, ui: &mut egui::Ui, key: &str) {
        let Some(control) = self.doc().control(key).cloned() else {
            ui.label("Missing control.");
            return;
        };
        ui.label(
            egui::RichText::new(&control.label)
                .size(16.0)
                .color(self.color("text.primary")),
        );
        ui.label(
            egui::RichText::new(format!(
                "{} · {} · {}",
                control.key,
                if control.is_indicator() {
                    "indicator"
                } else {
                    "control"
                },
                control.ty.display()
            ))
            .color(self.color("text.secondary"))
            .size(11.5),
        );
        ui.add_space(8.0);

        let mut label = control.label.clone();
        ui.label("Label");
        if ui.text_edit_singleline(&mut label).changed() {
            if let Some(c) = self.doc_mut().control_mut(key) {
                c.label = label;
            }
        }

        let mut default_text = value_edit_text(&control.default);
        ui.label("Default value");
        if ui.text_edit_singleline(&mut default_text).lost_focus() {
            if let Ok(value) = parse_value_text(&default_text, &control.ty) {
                if let Some(c) = self.doc_mut().control_mut(key) {
                    c.default = value;
                }
                self.refresh_diagnostics();
            }
        }

        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!(
                "Bounds  {:.0}, {:.0}",
                control.bounds.x, control.bounds.y
            ))
            .color(self.color("text.muted"))
            .size(11.0),
        );
        if let Some(unit) = &control.style.unit {
            ui.label(
                egui::RichText::new(format!("Unit  {unit}"))
                    .color(self.color("text.muted"))
                    .size(11.0),
            );
        }
        if let Some(out) = &self.last_run {
            if let Some(value) = out.indicators.get(key) {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("Last run: {}", value.display()))
                        .color(self.color("run"))
                        .strong(),
                );
            }
        }
        ui.add_space(10.0);
        if ui
            .add(
                egui::Button::new(egui::RichText::new("Delete control").color(self.color("error")))
                    .fill(self.color("bg.elevated")),
            )
            .clicked()
        {
            self.delete_selection();
        }
    }

    fn inspect_node(&mut self, ui: &mut egui::Ui, key: &str) {
        let Some(node) = self.doc().node_by_key(key).cloned() else {
            ui.label("Missing node.");
            return;
        };
        ui.label(
            egui::RichText::new(&node.key)
                .size(16.0)
                .color(self.color("text.primary")),
        );
        ui.label(
            egui::RichText::new(&node.kind)
                .color(self.color("accent"))
                .size(12.0),
        );
        ui.label(
            egui::RichText::new(format!("id {}", node.id))
                .color(self.color("text.muted"))
                .size(10.0),
        );
        ui.add_space(8.0);

        if node.kind == kinds::CONSTANT {
            if let Some(value) = node.props.get("value").cloned() {
                let mut text = value.to_string();
                ui.label("Constant value");
                if ui.text_edit_singleline(&mut text).lost_focus() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(n) = self.doc_mut().node_by_key_mut(key) {
                            n.props.insert("value".into(), json);
                        }
                        self.refresh_diagnostics();
                    }
                }
            }
        }

        ui.add_space(6.0);
        if !node.inputs.is_empty() {
            ui.label(
                egui::RichText::new("Inputs")
                    .strong()
                    .color(self.color("text.secondary"))
                    .size(12.0),
            );
            for (port, src) in &node.inputs {
                ui.label(
                    egui::RichText::new(format!("{port} ← {}", source_label(src)))
                        .color(self.color("text.muted"))
                        .size(11.0),
                );
            }
        }
        if !node.tunnels.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Tunnels")
                    .strong()
                    .color(self.color("text.secondary"))
                    .size(12.0),
            );
            for tunnel in &node.tunnels {
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {} · {:?}",
                        tunnel.name,
                        tunnel.ty.display(),
                        tunnel.direction
                    ))
                    .color(self.color("text.muted"))
                    .size(11.0),
                );
            }
        }
        if !node.props.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Properties")
                    .strong()
                    .color(self.color("text.secondary"))
                    .size(12.0),
            );
            for (k, v) in &node.props {
                ui.label(
                    egui::RichText::new(format!("{k}: {v}"))
                        .color(self.color("text.muted"))
                        .size(11.0),
                );
            }
        }
        ui.add_space(10.0);
        if ui
            .add(
                egui::Button::new(egui::RichText::new("Delete node").color(self.color("error")))
                    .fill(self.color("bg.elevated")),
            )
            .clicked()
        {
            self.delete_selection();
        }
    }

    // ---- Canvas ---------------------------------------------------------

    fn canvas(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default_margins()
            .frame(
                egui::Frame::default()
                    .fill(self.color("bg.canvas"))
                    .inner_margin(egui::Margin::same(0)),
            )
            .show(ui, |ui| {
                let available = ui.available_size();
                let (response, mut painter) =
                    ui.allocate_painter(available, Sense::click_and_drag());
                let origin = response.rect.min + self.pan;
                let clip = response.rect;
                painter.set_clip_rect(clip);

                if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                    self.delete_selection();
                }

                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > 0.01 {
                    self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.35, 2.5);
                }

                self.node_drag = false;
                match self.view {
                    View::Panel => self.draw_panel(ui, &painter, origin),
                    View::Diagram => self.draw_diagram(ui, &painter, origin),
                }

                if response.dragged() && !self.node_drag {
                    self.pan += response.drag_delta();
                }

                if self.doc().panel.controls.is_empty() && self.view == View::Panel {
                    empty_hint(
                        &painter,
                        clip.center(),
                        "No controls yet",
                        "Add one from the Controls palette on the left.",
                        self.color("text.muted"),
                    );
                }
            });
    }

    fn draw_panel(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, origin: Pos2) {
        let title_pos = origin + Vec2::new(4.0, 4.0);
        painter.text(
            title_pos,
            egui::Align2::LEFT_TOP,
            "Front Panel",
            FontId::proportional(18.0),
            self.color("text.secondary"),
        );
        let controls = self.doc().panel.controls.clone();
        let indicators = self
            .last_run
            .as_ref()
            .map(|r| r.indicators.clone())
            .unwrap_or_default();
        let mut clicked: Option<String> = None;
        for control in &controls {
            let min = origin + Vec2::new(control.bounds.x, control.bounds.y) * self.zoom;
            let size = Vec2::new(control.bounds.w, control.bounds.h) * self.zoom;
            let rect = Rect::from_min_size(min, size.max(Vec2::new(90.0, 30.0)));
            let id = ui.scope_id().with(("control", &control.key));
            let response = ui.interact(rect, id, Sense::click());
            if response.clicked() {
                clicked = Some(control.key.clone());
            }
            let selected = self.selection == Some(Selection::Control(control.key.clone()));
            let border = if selected {
                self.color("selection.border")
            } else {
                self.color("border.subtle")
            };
            painter.rect_filled(rect, egui::CornerRadius::same(7), self.color("bg.elevated"));
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(7),
                Stroke::new(if selected { 1.6 } else { 1.0 }, border),
                StrokeKind::Inside,
            );
            painter.text(
                rect.min + Vec2::new(10.0, 6.0),
                egui::Align2::LEFT_TOP,
                &control.label,
                FontId::proportional(11.5),
                self.color("text.secondary"),
            );
            let value = indicators
                .get(&control.key)
                .cloned()
                .unwrap_or_else(|| control.default.clone());
            let shown = if control.is_indicator() {
                value.display()
            } else {
                format!(
                    "{}{}",
                    value.display(),
                    control
                        .style
                        .unit
                        .as_ref()
                        .map(|u| format!(" {u}"))
                        .unwrap_or_default()
                )
            };
            painter.text(
                rect.min + Vec2::new(10.0, rect.height() * 0.62),
                egui::Align2::LEFT_CENTER,
                shown,
                FontId::proportional((15.0 * self.zoom).clamp(10.0, 22.0)),
                if control.is_indicator() {
                    self.color("run")
                } else {
                    self.color("text.primary")
                },
            );
            if control.is_indicator() {
                let dot = Pos2::new(rect.max.x - 12.0, rect.min.y + 12.0);
                painter.circle_filled(dot, 4.0, self.color("run"));
            }
        }
        if let Some(key) = clicked {
            self.selection = Some(Selection::Control(key));
        }
    }

    fn draw_diagram(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, origin: Pos2) {
        let rect = painter.clip_rect();
        // Dot grid.
        let step = 24.0 * self.zoom;
        let dot = self.color("grid.dot");
        let mut x = rect.left() - (rect.left() - origin.x).rem_euclid(step);
        while x < rect.right() {
            let mut y = rect.top() - (rect.top() - origin.y).rem_euclid(step);
            while y < rect.bottom() {
                painter.circle_filled(Pos2::new(x, y), 1.0, dot);
                y += step;
            }
            x += step;
        }

        // Wire colors from the compiled graph, so numeric/boolean/string wires
        // are visibly different.
        let report = lc_runtime::compile(self.doc());
        let mut wire_colors: BTreeMap<(String, String), Color32> = BTreeMap::new();
        if let Some(compiled) = &report.compiled {
            for (node, ports) in &compiled.node_types {
                for (port, ty) in ports {
                    wire_colors.insert(
                        (node.clone(), port.clone()),
                        color(self.theme.wire_color(ty)),
                    );
                }
            }
        }

        let scope = self.doc().diagram.root.clone();
        self.draw_wires(painter, &scope, origin, &wire_colors);
        self.draw_scope(ui, painter, &scope, origin);
    }

    fn draw_scope(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        scope: &Scope,
        origin: Pos2,
    ) {
        let reg = Registry::global();
        let nodes = scope.nodes.clone();
        let mut clicked: Option<String> = None;
        let mut drag: Option<(String, Vec2)> = None;

        for node in &nodes {
            let geo = lc_layout::geometry(node, reg);
            let min = origin + Vec2::new(geo.rect.x, geo.rect.y) * self.zoom;
            let size = Vec2::new(geo.rect.w, geo.rect.h) * self.zoom;
            let rect = Rect::from_min_size(min, size.max(Vec2::new(80.0, 40.0)));
            let id = ui.scope_id().with(("node", &node.id));
            let response = ui.interact(rect, id, Sense::click_and_drag());
            if response.clicked() {
                clicked = Some(node.key.clone());
            }
            if response.dragged_by(egui::PointerButton::Primary) {
                self.node_drag = true;
                drag = Some((node.key.clone(), response.drag_delta() / self.zoom));
            }

            let selected = self.selection == Some(Selection::Node(node.key.clone()));
            let is_struct = lc_model::document::is_structure(&node.kind);
            let fill = if is_struct {
                self.color("bg.overlay")
            } else {
                self.color("bg.elevated")
            };
            painter.rect_filled(rect, egui::CornerRadius::same(6), fill);
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(6),
                Stroke::new(
                    if selected { 1.8 } else { 1.0 },
                    if selected {
                        self.color("selection.border")
                    } else {
                        self.color("border.strong")
                    },
                ),
                StrokeKind::Inside,
            );
            let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 22.0 * self.zoom));
            painter.rect_filled(header, egui::CornerRadius::same(6), self.color("bg.panel"));
            painter.text(
                rect.min + Vec2::new(8.0, 4.0),
                egui::Align2::LEFT_TOP,
                &node.key,
                FontId::proportional((11.5 * self.zoom).clamp(8.0, 15.0)),
                self.color("text.primary"),
            );
            painter.text(
                rect.min + Vec2::new(8.0, 24.0 * self.zoom),
                egui::Align2::LEFT_TOP,
                node.kind.trim_start_matches("structure."),
                FontId::proportional((9.5 * self.zoom).clamp(7.0, 12.0)),
                self.color("text.muted"),
            );

            // Pins.
            for (name, p) in &geo.inputs {
                let pos = origin + Vec2::new(p.x, p.y) * self.zoom;
                painter.circle_filled(pos, (3.2 * self.zoom).clamp(2.0, 5.0), self.color("accent"));
                if self.zoom > 0.7 && !name.starts_with("t:") {
                    painter.text(
                        pos + Vec2::new(6.0, -1.0),
                        egui::Align2::LEFT_BOTTOM,
                        name,
                        FontId::proportional((8.5 * self.zoom).clamp(7.0, 11.0)),
                        self.color("text.muted"),
                    );
                }
            }
            for (name, p) in &geo.outputs {
                let pos = origin + Vec2::new(p.x, p.y) * self.zoom;
                painter.circle_filled(pos, (3.2 * self.zoom).clamp(2.0, 5.0), self.color("run"));
                if self.zoom > 0.7 && !name.starts_with("t:") {
                    painter.text(
                        pos + Vec2::new(-6.0, -1.0),
                        egui::Align2::RIGHT_BOTTOM,
                        name,
                        FontId::proportional((8.5 * self.zoom).clamp(7.0, 11.0)),
                        self.color("text.muted"),
                    );
                }
            }
        }

        if let Some((key, delta)) = drag {
            if let Some(node) = self.doc_mut().node_by_key_mut(&key) {
                node.pos.x += delta.x;
                node.pos.y += delta.y;
            }
        }
        if let Some(key) = clicked {
            self.selection = Some(Selection::Node(key));
        }

        for node in &nodes {
            for frame in &node.frames {
                self.draw_wires(painter, &frame.scope, origin, &BTreeMap::new());
                self.draw_scope(ui, painter, &frame.scope.clone(), origin);
            }
        }
    }

    fn draw_wires(
        &self,
        painter: &egui::Painter,
        scope: &Scope,
        origin: Pos2,
        colors: &BTreeMap<(String, String), Color32>,
    ) {
        let reg = Registry::global();
        for (from, to, from_port, _) in lc_layout::wires(scope, reg) {
            let points: Vec<Pos2> = lc_layout::route_wire(from, to)
                .into_iter()
                .map(|p| Pos2::new(origin.x + p.x * self.zoom, origin.y + p.y * self.zoom))
                .collect();
            if points.len() < 2 {
                continue;
            }
            let mut color = self.color("accent");
            // Wires() does not carry the source node id; infer by matching the
            // source point against the scope's nodes.
            for node in &scope.nodes {
                let geo = lc_layout::geometry(node, reg);
                if geo.output(&from_port) == Some(from) {
                    if let Some(c) = colors.get(&(node.id.clone(), from_port.clone())) {
                        color = *c;
                    }
                }
            }
            painter.add(egui::Shape::line(points, Stroke::new(1.6, color)));
        }
    }

    // ---- Modals / status ------------------------------------------------

    fn modals(&mut self, ctx: &egui::Context) {
        if self.show_open {
            let mut open = self.show_open;
            egui::Window::new("Open VI")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Path");
                    ui.text_edit_singleline(&mut self.path_input);
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            let path = PathBuf::from(self.path_input.clone());
                            self.open(path);
                            self.show_open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_open = false;
                        }
                    });
                });
            self.show_open = open;
        }
        if self.show_save {
            let mut open = self.show_save;
            egui::Window::new("Save VI")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Path");
                    ui.text_edit_singleline(&mut self.path_input);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let path = PathBuf::from(self.path_input.clone());
                            self.save(path);
                            self.show_save = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_save = false;
                        }
                    });
                });
            self.show_save = open;
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::default()
                    .fill(self.color("bg.panel"))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&self.status)
                            .color(self.color("text.secondary"))
                            .size(11.5),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let errors = self
                            .diagnostics
                            .iter()
                            .filter(|d| d.severity == Severity::Error)
                            .count();
                        let warnings = self
                            .diagnostics
                            .iter()
                            .filter(|d| d.severity == Severity::Warning)
                            .count();
                        ui.label(
                            egui::RichText::new(format!("{errors} errors · {warnings} warnings"))
                                .color(if errors > 0 {
                                    self.color("error")
                                } else {
                                    self.color("text.muted")
                                })
                                .size(11.0),
                        );
                        if let Some(run) = &self.last_run {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} indicator(s) · {:.1} ms",
                                    run.indicators.len(),
                                    run.duration_ms
                                ))
                                .color(self.color("text.muted"))
                                .size(11.0),
                            );
                        }
                    });
                });
            });
    }
}

impl eframe::App for LabCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.top_bar(ui);
        self.status_bar(ui);
        self.palette(ui);
        self.inspector(ui);
        self.canvas(ui);
        self.modals(&ctx);
    }
}

// ---- Drawing and model helpers -----------------------------------------

fn apply_visuals(ctx: &egui::Context, theme: &Theme) {
    let dark = theme.is_dark();
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let token = |name: &str| color(theme.color(name));
    visuals.panel_fill = token("bg.panel");
    visuals.window_fill = token("bg.panel");
    visuals.extreme_bg_color = token("bg.input");
    visuals.faint_bg_color = token("bg.elevated");
    visuals.override_text_color = Some(token("text.primary"));
    visuals.selection.bg_fill = token("accent.soft");
    visuals.selection.stroke = Stroke::new(1.0, token("accent"));
    visuals.widgets.noninteractive.bg_fill = token("bg.panel");
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, token("text.secondary"));
    visuals.widgets.inactive.bg_fill = token("bg.elevated");
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, token("text.primary"));
    visuals.widgets.hovered.bg_fill = token("bg.overlay");
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, token("accent.hover"));
    visuals.widgets.active.bg_fill = token("accent.soft");
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, token("accent"));
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
    });
}

fn color(rgba: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba.r, rgba.g, rgba.b, rgba.a)
}

fn empty_hint(painter: &egui::Painter, center: Pos2, title: &str, subtitle: &str, color: Color32) {
    painter.text(
        center - Vec2::new(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        title,
        FontId::proportional(18.0),
        color,
    );
    painter.text(
        center + Vec2::new(0.0, 16.0),
        egui::Align2::CENTER_CENTER,
        subtitle,
        FontId::proportional(12.0),
        color,
    );
}

fn example_doc() -> ViDocument {
    let dsl = r#"vi "Over Temperature" {
  description "Raises an alarm above the limit."
  panel {
    control temp "Temperature" @(40, 60) { type: dbl, default: 35.0, unit: "C" }
    control limit "Limit" @(40, 130) { type: dbl, default: 30.0, unit: "C" }
    indicator alarm "Alarm" @(40, 200) { type: bool }
  }
  diagram {
    temp -> greater(limit) -> alarm
  }
}"#;
    match lc_dsl::build(dsl) {
        Ok(outcome) => outcome.doc,
        Err(_) => ViDocument::new("Untitled"),
    }
}

fn unique_key(doc: &ViDocument, base: &str) -> String {
    let base = sanitize(base);
    if !key_taken(doc, &base) {
        return base;
    }
    let mut i = 2;
    loop {
        let key = format!("{base}_{i}");
        if !key_taken(doc, &key) {
            return key;
        }
        i += 1;
    }
}

fn key_taken(doc: &ViDocument, key: &str) -> bool {
    doc.node_by_key(key).is_some() || doc.control(key).is_some()
}

fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('n');
    }
    if s.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        s.insert(0, 'n');
    }
    s
}

fn widget_key(kind: ControlKind, indicator: bool) -> &'static str {
    match (kind, indicator) {
        (ControlKind::Numeric, false) => "numeric",
        (ControlKind::Numeric, true) => "numeric_indicator",
        (ControlKind::Slider, _) => "slider",
        (ControlKind::Toggle, _) => "toggle",
        (ControlKind::Led, _) => "led",
        (ControlKind::String, false) => "string",
        (ControlKind::String, true) => "string_indicator",
        (ControlKind::Path, _) => "path",
        (ControlKind::Button, _) => "button",
        (ControlKind::Chart, _) => "chart",
        (ControlKind::Graph, _) => "graph",
        (ControlKind::Enum, _) => "enum",
        (ControlKind::Table, _) => "table",
        (ControlKind::Cluster, _) => "cluster",
        (ControlKind::Array, _) => "array",
    }
}

fn widget_label(kind: ControlKind, indicator: bool) -> String {
    match (kind, indicator) {
        (ControlKind::Numeric, false) => "Numeric".into(),
        (ControlKind::Numeric, true) => "Numeric Indicator".into(),
        (ControlKind::String, false) => "String".into(),
        (ControlKind::String, true) => "String Indicator".into(),
        (ControlKind::Led, _) => "LED".into(),
        (ControlKind::Slider, _) => "Slider".into(),
        (ControlKind::Toggle, _) => "Toggle".into(),
        (ControlKind::Path, _) => "Path".into(),
        (ControlKind::Button, _) => "Button".into(),
        (ControlKind::Chart, _) => "Chart".into(),
        (ControlKind::Graph, _) => "Graph".into(),
        (ControlKind::Enum, _) => "Enum".into(),
        (ControlKind::Table, _) => "Table".into(),
        (ControlKind::Cluster, _) => "Cluster".into(),
        (ControlKind::Array, _) => "Array".into(),
    }
}

fn source_label(src: &SourceRef) -> String {
    match src {
        SourceRef::Node { node, port } => format!("{node}.{port}"),
        SourceRef::Tunnel { structure, name } => format!("{structure}.{name}"),
        SourceRef::ShiftRegister { structure, name } => format!("{structure}.sr:{name}"),
        SourceRef::Iteration { structure } => format!("{structure}.i"),
        SourceRef::EventData { structure, field } => match field {
            Some(field) => format!("{structure}.{field}"),
            None => format!("{structure}.eventData"),
        },
        SourceRef::Default { ty } => format!("default {}", ty.display()),
    }
}

fn value_edit_text(value: &Value) -> String {
    match value {
        Value::Str(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Void => String::new(),
        other => serde_json::to_string(other)
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|_| other.display()),
    }
}

fn parse_value_text(text: &str, ty: &Type) -> Result<Value, String> {
    if let Type::String = ty {
        return Ok(Value::string(text));
    }
    if let Type::Bool = ty {
        return text
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|e| e.to_string());
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        if let Ok(value) = serde_json::from_value::<Value>(json) {
            return value.coerce_to(ty);
        }
    }
    Err(format!("`{text}` is not a {}", ty.display()))
}

fn remove_terminals(scope: &mut Scope, key: &str) {
    scope
        .nodes
        .retain(|n| n.props.get("control").and_then(|v| v.as_str()) != Some(key));
    for node in &mut scope.nodes {
        for frame in &mut node.frames {
            remove_terminals(&mut frame.scope, key);
        }
    }
}

fn remove_node(scope: &mut Scope, id: &str) {
    scope.nodes.retain(|n| n.id != id);
    for node in &mut scope.nodes {
        for (port, src) in node.inputs.clone() {
            if src.source_node() == Some(id) {
                node.inputs.remove(&port);
            }
        }
        for frame in &mut node.frames {
            remove_node(&mut frame.scope, id);
        }
    }
}
