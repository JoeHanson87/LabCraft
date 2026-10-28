use crate::app::LabCraft;
use crate::{theme, valedit};
use eframe::egui::{self, epaint::CubicBezierShape, epaint::Shape, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use labcraft_core::typing::{crossing_loops, wire_type};
use labcraft_core::{coercion_dots, resolve, Command, DataType, LoopKind, Node, NodeId, NodeKind, Spec, TypeMap, Value, Vi, LOOP_BORDER};
use std::collections::HashSet;

const TERM_R: f32 = 5.0;
const UNKNOWN: Color32 = Color32::from_rgb(140, 146, 160);

fn v2(p: [f32; 2]) -> Vec2 {
    Vec2::new(p[0], p[1])
}

fn term_screen(origin: Pos2, n: &Node, idx: usize, output: bool) -> Pos2 {
    origin + v2(n.terminal_pos(idx, output))
}

struct Frame<'a> {
    origin: Pos2,
    tm: &'a TypeMap,
    dots: &'a HashSet<(NodeId, usize)>,
    hit_input: Option<(NodeId, usize)>,
}

pub fn show(app: &mut LabCraft, ui: &mut egui::Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click());
    let rect = resp.rect;
    let origin = rect.min;
    painter.rect_filled(rect, 0.0, theme::BG_DIAGRAM);
    let mut x = rect.left();
    while x < rect.right() {
        let mut y = rect.top();
        while y < rect.bottom() {
            painter.circle_filled(Pos2::new(x, y), 0.8, theme::GRID);
            y += 24.0;
        }
        x += 24.0;
    }
    if resp.clicked() {
        app.selected = None;
    }

    let nodes: Vec<Node> = app.ed.vi.nodes.clone();
    let tm = resolve(&app.ed.vi);
    let dots = coercion_dots(&app.ed.vi, &tm);
    let pointer = ui.input(|i| i.pointer.interact_pos());

    let visible: HashSet<NodeId> = nodes.iter().filter(|n| app.ed.vi.node_visible(n.id)).map(|n| n.id).collect();
    let mut hit_input: Option<(NodeId, usize)> = None;
    if let Some(p) = pointer {
        for n in nodes.iter().filter(|n| visible.contains(&n.id)) {
            for i in 0..n.kind.inputs().len() {
                if n.kind.terminal_visible(i, false, n.frame_view) && term_screen(origin, n, i, false).distance(p) < 10.0 {
                    hit_input = Some((n.id, i));
                }
            }
        }
    }
    let fr = Frame { origin, tm: &tm, dots: &dots, hit_input };

    let mut structures: Vec<&Node> = nodes.iter().filter(|n| n.kind.is_structure() && visible.contains(&n.id)).collect();
    structures.sort_by_key(|n| app.ed.vi.depth(n.id));
    for n in structures {
        draw_structure(app, ui, &painter, &fr, n);
    }

    let mut wire_paths: Vec<((NodeId, usize), Vec<Pos2>)> = vec![];
    for w in &app.ed.vi.wires {
        let (Some(a), Some(b)) = (app.ed.vi.node(w.from.0), app.ed.vi.node(w.to.0)) else { continue };
        if !visible.contains(&a.id) || !visible.contains(&b.id) {
            continue;
        }
        if !a.kind.terminal_visible(w.from.1, true, a.frame_view) || !b.kind.terminal_visible(w.to.1, false, b.frame_view) {
            continue;
        }
        let Some(raw) = tm.out(w.from.0, w.from.1) else { continue };
        let ty = wire_type(&app.ed.vi, w.from.0, w.to.0, w.auto_index, raw);
        let (p0, p1) = (term_screen(origin, a, w.from.1, true), term_screen(origin, b, w.to.1, false));
        let path = draw_wire(&painter, p0, p1, &ty, 1.0);
        draw_tunnel_markers(&painter, &app.ed.vi, origin, w.from.0, w.to.0, w.auto_index, &path, &ty);
        wire_paths.push((w.to, path));
    }
    if resp.secondary_clicked() {
        if let Some(p) = pointer {
            let near = wire_paths.iter().filter_map(|(to, path)| {
                let d = path.windows(2).map(|s| dist_to_segment(p, s[0], s[1])).fold(f32::MAX, f32::min);
                (d < 7.0).then_some((*to, d))
            });
            app.wire_menu = near.min_by(|a, b| a.1.total_cmp(&b.1)).map(|(to, _)| (to, p));
        }
    }

    for n in nodes.iter().filter(|n| !n.kind.is_structure() && visible.contains(&n.id)) {
        let r = Rect::from_min_size(origin + v2(n.pos), v2(n.size));
        let sel = app.selected == Some(n.id);
        let body = ui.interact(r, ui.scope_id().with(("node", n.id)), Sense::click_and_drag());
        handle_drag(app, n, &body);
        body.context_menu(|ui| node_menu(app, ui, n, &tm));
        painter.rect_filled(r, 4.0, theme::NODE_FILL);
        painter.rect_stroke(r, 4.0, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { theme::ACCENT } else { theme::NODE_STROKE }), StrokeKind::Inside);
        let accent = tm.outs(n.id).first().map(theme::type_color).or_else(|| n.kind.inputs().first().map(|t| theme::type_color(&t.ty))).unwrap_or(theme::ACCENT);
        painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(4.0, r.height())), 2.0, accent);
        painter.text(r.min + Vec2::new(r.width() / 2.0, 14.0), Align2::CENTER_CENTER, node_title(&app.ed.vi, n), FontId::proportional(12.5), Color32::from_gray(225));

        if let NodeKind::Constant(ty) = &n.kind {
            constant_body(app, ui, &painter, n, ty, r);
        }
        draw_terminals(app, ui, &painter, &fr, n, false);
    }

    if let (Some((id, i)), Some(p)) = (app.wiring, pointer) {
        if let (Some(n), Some(ty)) = (app.ed.vi.node(id), tm.out(id, i)) {
            let _ = draw_wire(&painter, term_screen(origin, n, i, true), p, ty, 0.6);
        }
    }

    constant_window(app, ui.ctx());
    wire_menu(app, ui.ctx());

    if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
        if let Some(id) = app.selected {
            app.pending.push(Command::RemoveNode(id));
        }
    }
}

fn constant_body(app: &mut LabCraft, ui: &mut egui::Ui, painter: &egui::Painter, n: &Node, ty: &DataType, r: Rect) {
    let col = theme::type_color(ty);
    let area = Rect::from_center_size(r.center() + Vec2::new(0.0, 10.0), Vec2::new(r.width() - 26.0, 24.0));
    match ty {
        DataType::Array(..) | DataType::Cluster(_) => {
            let v = n.value.clone().unwrap_or(Value::default_for(ty));
            let resp = ui.interact(area, ui.scope_id().with(("constbtn", n.id)), Sense::click());
            painter.rect_filled(area, 3.0, if resp.hovered() { Color32::from_gray(60) } else { Color32::from_gray(40) });
            painter.rect_stroke(area, 3.0, Stroke::new(1.0, col), StrokeKind::Inside);
            let summary = match &v {
                Value::Array(a) => format!("{} ✎", a.dims.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("×")),
                Value::Cluster(c) => format!("{{{}}} ✎", c.len()),
                _ => String::new(),
            };
            painter.text(area.center(), Align2::CENTER_CENTER, summary, FontId::monospace(11.0), col);
            if resp.clicked() {
                app.editing_const = Some(n.id);
            }
        }
        _ => {
            let mut value = n.value.clone().unwrap_or(Value::default_for(ty));
            let before = value.clone();
            ui.scope_builder(egui::UiBuilder::new().max_rect(area).id_salt(("const", n.id)), |ui| {
                valedit::edit_value(ui, ty, &mut value, true);
            });
            if value != before {
                app.pending.push(Command::SetValue { id: n.id, value });
            }
        }
    }
}

fn constant_window(app: &mut LabCraft, ctx: &egui::Context) {
    let Some(id) = app.editing_const else { return };
    let Some(n) = app.ed.vi.node(id) else {
        app.editing_const = None;
        return;
    };
    let NodeKind::Constant(ty) = &n.kind else { return };
    let ty = ty.clone();
    let mut value = n.value.clone().unwrap_or(Value::default_for(&ty));
    let mut open = true;
    let mut changed = false;
    egui::Window::new(format!("Constant — {}", ty.label())).open(&mut open).resizable(true).show(ctx, |ui| {
        changed = valedit::edit_value(ui, &ty, &mut value, true);
    });
    if changed {
        app.pending.push(Command::SetValue { id, value });
    }
    if !open {
        app.editing_const = None;
    }
}

fn node_menu(app: &mut LabCraft, ui: &mut egui::Ui, n: &Node, tm: &TypeMap) {
    if let NodeKind::Constant(ty) = &n.kind {
        let mut t = ty.clone();
        ui.menu_button(format!("Data type: {}", ty.label()), |ui| {
            if valedit::type_menu(ui, &mut t) {
                app.pending.push(Command::SetType { id: n.id, ty: t.clone() });
            }
        });
    }
    let count = match &n.kind {
        NodeKind::Bundle { n } | NodeKind::Unbundle { n } => Some(*n),
        NodeKind::BundleByName { names } | NodeKind::UnbundleByName { names } => Some(names.len()),
        _ => None,
    };
    if let Some(c) = count {
        if ui.button("Add element").clicked() {
            app.pending.push(Command::SetElementCount { id: n.id, n: c + 1 });
            ui.close();
        }
        if ui.add_enabled(c > 0, egui::Button::new("Remove last element")).clicked() {
            app.pending.push(Command::SetElementCount { id: n.id, n: c - 1 });
            ui.close();
        }
    }
    if let NodeKind::BundleByName { names } | NodeKind::UnbundleByName { names } = &n.kind {
        let fields: Vec<String> = match tm.input(&app.ed.vi, n.id, 0) {
            Some(DataType::Cluster(f)) => f.iter().map(|f| f.name.clone()).collect(),
            _ => vec![],
        };
        for (i, current) in names.iter().enumerate() {
            ui.menu_button(format!("Element {}: {}", i + 1, if current.is_empty() { "(choose)" } else { current }), |ui| {
                if fields.is_empty() {
                    ui.label("Wire a cluster into the node first");
                }
                for f in &fields {
                    if ui.selectable_label(f == current, f).clicked() {
                        let mut new = names.clone();
                        new[i] = f.clone();
                        app.pending.push(Command::SetNames { id: n.id, names: new });
                        ui.close();
                    }
                }
            });
        }
    }
    match &n.kind {
        NodeKind::Local { target, write, .. } => {
            let current = target.and_then(|t| app.ed.vi.node(t)).map(|t| t.kind.title()).unwrap_or_else(|| "(choose)".into());
            ui.menu_button(format!("Variable: {current}"), |ui| {
                let items: Vec<(NodeId, String)> = app.ed.vi.nodes.iter().filter(|x| x.kind.is_panel_item()).map(|x| (x.id, x.kind.title())).collect();
                if items.is_empty() {
                    ui.label("Add a control or indicator first");
                }
                for (id, name) in items {
                    if ui.selectable_label(*target == Some(id), name).clicked() {
                        app.pending.push(Command::SetLocalTarget { id: n.id, target: Some(id) });
                        ui.close();
                    }
                }
            });
            if ui.button(if *write { "Change to Read" } else { "Change to Write" }).clicked() {
                app.pending.push(Command::SetWrite { id: n.id, write: !*write });
                ui.close();
            }
        }
        NodeKind::Global { name, ty, write } => {
            let mut new_name = name.clone();
            ui.horizontal(|ui| {
                ui.label("Name");
                if ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(110.0)).changed() {
                    app.pending.push(Command::SetGlobalName { id: n.id, name: new_name.clone() });
                }
            });
            let mut t = ty.clone();
            ui.menu_button(format!("Data type: {}", ty.label()), |ui| {
                if valedit::type_menu(ui, &mut t) {
                    app.pending.push(Command::SetType { id: n.id, ty: t.clone() });
                }
            });
            if ui.button(if *write { "Change to Read" } else { "Change to Write" }).clicked() {
                app.pending.push(Command::SetWrite { id: n.id, write: !*write });
                ui.close();
            }
        }
        NodeKind::Feedback { ty } => {
            let mut t = ty.clone();
            ui.menu_button(format!("Data type: {}", ty.label()), |ui| {
                if valedit::type_menu(ui, &mut t) {
                    app.pending.push(Command::SetType { id: n.id, ty: t.clone() });
                }
            });
        }
        _ => {}
    }
    if ui.button("Delete").clicked() {
        app.pending.push(Command::RemoveNode(n.id));
        ui.close();
    }
}

fn node_title(vi: &Vi, n: &Node) -> String {
    match &n.kind {
        NodeKind::Local { target, write, .. } => {
            let name = target.and_then(|t| vi.node(t)).map(|t| t.kind.title()).unwrap_or_else(|| "?".into());
            format!("{name}{}", if *write { " ▸" } else { "" })
        }
        k => k.title(),
    }
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    let t = if len2 == 0.0 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

/// Popup for a right-clicked wire: toggle auto-indexing or delete.
fn wire_menu(app: &mut LabCraft, ctx: &egui::Context) {
    let Some((to, pos)) = app.wire_menu else { return };
    let Some(w) = app.ed.vi.wires.iter().find(|w| w.to == to).copied() else {
        app.wire_menu = None;
        return;
    };
    let (l, e) = crossing_loops(&app.ed.vi, w.from.0, w.to.0);
    let crosses = !l.is_empty() || !e.is_empty();
    let area = egui::Area::new(egui::Id::unique("wire_menu")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            if crosses {
                let label = if w.auto_index { "Disable Indexing" } else { "Enable Indexing" };
                if ui.button(label).clicked() {
                    app.pending.push(Command::SetWireIndexing { to, enabled: !w.auto_index });
                    app.wire_menu = None;
                }
            }
            if ui.button("Delete Wire").clicked() {
                app.pending.push(Command::Disconnect { to });
                app.wire_menu = None;
            }
        });
    });
    if area.response.clicked_elsewhere() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.wire_menu = None;
    }
}

fn handle_drag(app: &mut LabCraft, n: &Node, resp: &Response) {
    if resp.clicked() || resp.drag_started() {
        app.selected = Some(n.id);
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        app.pending.push(Command::MoveNode { id: n.id, pos: [n.pos[0] + d.x, n.pos[1] + d.y] });
    }
    if resp.drag_stopped() {
        app.pending.push(Command::Drop(n.id));
        app.pending.push(Command::EndGesture);
    }
}

fn draw_terminals(app: &mut LabCraft, ui: &mut egui::Ui, painter: &egui::Painter, fr: &Frame, n: &Node, square: bool) {
    let origin = fr.origin;
    let mark = |p: Pos2, col: Color32| {
        if square {
            let r = Rect::from_center_size(p, Vec2::splat(TERM_R * 2.0));
            painter.rect_filled(r, 1.0, col);
            painter.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_black_alpha(160)), StrokeKind::Inside);
        } else {
            painter.circle_filled(p, TERM_R, col);
        }
    };
    for (i, t) in n.kind.inputs().iter().enumerate() {
        if !n.kind.terminal_visible(i, false, n.frame_view) {
            continue;
        }
        let p = term_screen(origin, n, i, false);
        let col = match fr.tm.input(&app.ed.vi, n.id, i) {
            Some(ty) => theme::type_color(&ty),
            None if t.spec == Spec::Exact => theme::type_color(&t.ty),
            None => UNKNOWN,
        };
        mark(p, col);
        if fr.dots.contains(&(n.id, i)) {
            painter.circle_filled(p - Vec2::new(9.0, 0.0), 3.5, Color32::from_rgb(255, 120, 20));
        }
        if fr.hit_input == Some((n.id, i)) && app.wiring.is_some() {
            painter.circle_stroke(p, TERM_R + 4.0, Stroke::new(1.5, theme::ACCENT));
        }
        if !square {
            painter.text(p + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, &t.name, FontId::proportional(10.0), theme::TEXT_DIM);
        }
        let tr = ui.interact(Rect::from_center_size(p, Vec2::splat(14.0)), ui.scope_id().with(("in", n.id, i)), Sense::click());
        if tr.secondary_clicked() {
            app.pending.push(Command::Disconnect { to: (n.id, i) });
        }
    }
    for (i, t) in n.kind.outputs().iter().enumerate() {
        if !n.kind.terminal_visible(i, true, n.frame_view) {
            continue;
        }
        let p = term_screen(origin, n, i, true);
        let col = fr.tm.out(n.id, i).map(theme::type_color).unwrap_or_else(|| theme::type_color(&t.ty));
        mark(p, col);
        if !square {
            painter.text(p - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, &t.name, FontId::proportional(10.0), theme::TEXT_DIM);
        }
        let tr = ui.interact(Rect::from_center_size(p, Vec2::splat(14.0)), ui.scope_id().with(("out", n.id, i)), Sense::drag());
        if tr.drag_started() {
            app.wiring = Some((n.id, i));
        }
        if tr.drag_stopped() {
            if let (Some(from), Some(to)) = (app.wiring.take(), fr.hit_input) {
                app.pending.push(Command::Connect { from, to });
            }
        }
    }
}

fn structure_accent(kind: &NodeKind) -> Color32 {
    match kind {
        NodeKind::Loop { kind: LoopKind::While, .. } => Color32::from_rgb(0, 190, 170),
        NodeKind::Loop { kind: LoopKind::For, .. } => Color32::from_rgb(90, 140, 255),
        NodeKind::Case { fixed: Some(_), .. } => Color32::from_rgb(150, 150, 165),
        NodeKind::Case { .. } => Color32::from_rgb(255, 170, 60),
        NodeKind::Sequence { .. } => Color32::from_rgb(185, 150, 235),
        _ => Color32::from_rgb(150, 215, 90),
    }
}

fn tunnel_types(kind: &NodeKind) -> Vec<DataType> {
    match kind {
        NodeKind::Case { outs, .. } | NodeKind::Event { outs, .. } => outs.clone(),
        _ => vec![],
    }
}

fn tunnel_menu(app: &mut LabCraft, ui: &mut egui::Ui, n: &Node) {
    ui.menu_button("Add Output Tunnel", |ui| {
        let opts = [
            ("Numeric", DataType::DBL),
            ("Integer", DataType::I32),
            ("Boolean", DataType::Bool),
            ("String", DataType::Str),
            ("Array", DataType::arr_dbl()),
            ("Error Cluster", DataType::error_cluster()),
        ];
        for (label, ty) in opts {
            if ui.button(label).clicked() {
                app.pending.push(Command::AddTunnel { id: n.id, ty });
                ui.close();
            }
        }
    });
    if ui.add_enabled(n.kind.tunnel_count() > 0, egui::Button::new("Remove Last Tunnel")).clicked() {
        app.pending.push(Command::RemoveTunnel { id: n.id });
        ui.close();
    }
}

fn structure_menu(app: &mut LabCraft, ui: &mut egui::Ui, n: &Node) {
    ui.label(egui::RichText::new(n.kind.title()).strong());
    let count = n.kind.frame_count();
    let cur = n.frame_view.min(count.saturating_sub(1));
    match &n.kind {
        NodeKind::Loop { regs, .. } => {
            ui.menu_button("Add Shift Register", |ui| {
                let opts = [
                    ("Numeric", DataType::DBL),
                    ("Integer", DataType::I32),
                    ("Boolean", DataType::Bool),
                    ("String", DataType::Str),
                    ("Array", DataType::arr_dbl()),
                ];
                for (label, ty) in opts {
                    if ui.button(label).clicked() {
                        app.pending.push(Command::AddShiftRegister { id: n.id, ty });
                        ui.close();
                    }
                }
            });
            if ui.add_enabled(!regs.is_empty(), egui::Button::new("Remove Last Shift Register")).clicked() {
                app.pending.push(Command::RemoveShiftRegister { id: n.id });
                ui.close();
            }
        }
        NodeKind::Case { labels, default, fixed, .. } => {
            if let Some(enabled) = fixed {
                if ui.add_enabled(*enabled != cur, egui::Button::new("Enable This Frame")).clicked() {
                    app.pending.push(Command::SetEnabledFrame { id: n.id, frame: cur });
                    ui.close();
                }
            } else {
                let mut text = labels.get(cur).cloned().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("Frame label");
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(110.0)).changed() {
                        app.pending.push(Command::SetFrameLabel { id: n.id, frame: cur, label: text.clone() });
                    }
                });
                if ui.button("Add Frame After").clicked() {
                    app.pending.push(Command::AddFrame { id: n.id, after: cur });
                    ui.close();
                }
                if ui.add_enabled(count > 1, egui::Button::new("Remove This Frame")).clicked() {
                    app.pending.push(Command::RemoveFrame { id: n.id, frame: cur });
                    ui.close();
                }
                let is_default = *default == Some(cur);
                if ui.button(if is_default { "Clear Default" } else { "Make This Frame Default" }).clicked() {
                    app.pending.push(Command::SetDefaultFrame { id: n.id, frame: if is_default { None } else { Some(cur) } });
                    ui.close();
                }
            }
            tunnel_menu(app, ui, n);
        }
        NodeKind::Sequence { frames } => {
            ui.menu_button("Add Frame", |ui| {
                for f in 0..*frames {
                    if ui.button(format!("After frame {f}")).clicked() {
                        app.pending.push(Command::AddFrame { id: n.id, after: f });
                        ui.close();
                    }
                }
            });
            ui.menu_button("Remove Frame", |ui| {
                for f in 0..*frames {
                    if ui.add_enabled(*frames > 1, egui::Button::new(format!("Frame {f}"))).clicked() {
                        app.pending.push(Command::RemoveFrame { id: n.id, frame: f });
                        ui.close();
                    }
                }
            });
        }
        NodeKind::Event { frames, .. } => {
            ui.menu_button("Add Event: Value Change", |ui| {
                let ctls: Vec<(NodeId, String)> = app.ed.vi.nodes.iter().filter(|x| matches!(x.kind, NodeKind::Control { .. })).map(|x| (x.id, x.kind.title())).collect();
                if ctls.is_empty() {
                    ui.label("Add a control first");
                }
                for (id, name) in ctls {
                    if ui.button(name).clicked() {
                        app.pending.push(Command::AddEventFrame { id: n.id, source: Some(id) });
                        ui.close();
                    }
                }
            });
            if ui.add_enabled(!frames.iter().any(|f| f.source.is_none()), egui::Button::new("Add Timeout Frame")).clicked() {
                app.pending.push(Command::AddEventFrame { id: n.id, source: None });
                ui.close();
            }
            if ui.add_enabled(count > 1, egui::Button::new("Remove This Frame")).clicked() {
                app.pending.push(Command::RemoveFrame { id: n.id, frame: cur });
                ui.close();
            }
            tunnel_menu(app, ui, n);
        }
        _ => {}
    }
    ui.separator();
    if ui.button("Delete").clicked() {
        app.pending.push(Command::RemoveNode(n.id));
        ui.close();
    }
}

fn frame_selector(app: &mut LabCraft, ui: &mut egui::Ui, painter: &egui::Painter, n: &Node, r: Rect, accent: Color32) {
    let count = n.kind.frame_count();
    let center = Pos2::new(r.center().x, r.top() + 11.0);
    if count == 0 {
        painter.text(center, Align2::CENTER_CENTER, "no events — right-click to add", FontId::proportional(11.0), theme::TEXT_DIM);
        return;
    }
    let cur = n.frame_view.min(count - 1);
    let label = match &n.kind {
        NodeKind::Case { labels, default, fixed, .. } => {
            let mut t = labels.get(cur).cloned().unwrap_or_default();
            if *default == Some(cur) {
                t.push_str(" ★");
            }
            if *fixed == Some(cur) {
                t.push_str(" (enabled)");
            }
            t
        }
        NodeKind::Event { frames, .. } => frames[cur].label.clone(),
        _ => String::new(),
    };
    painter.text(center, Align2::CENTER_CENTER, format!("{label}   {}/{count}", cur + 1), FontId::proportional(11.5), Color32::from_gray(235));
    for (dx, dir, glyph) in [(-100.0f32, -1i64, "◀"), (100.0, 1, "▶")] {
        let ar = Rect::from_center_size(center + Vec2::new(dx, 0.0), Vec2::new(20.0, 18.0));
        let resp = ui.interact(ar, ui.scope_id().with(("framearrow", n.id, dir)), Sense::click());
        painter.text(ar.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(12.0), if resp.hovered() { Color32::WHITE } else { accent });
        let target = cur as i64 + dir;
        if resp.clicked() && target >= 0 && (target as usize) < count {
            app.pending.push(Command::SetFrameView { id: n.id, frame: target as usize });
        }
    }
}

fn draw_structure(app: &mut LabCraft, ui: &mut egui::Ui, painter: &egui::Painter, fr: &Frame, n: &Node) {
    let origin = fr.origin;
    let r = Rect::from_min_size(origin + v2(n.pos), v2(n.size));
    let sel = app.selected == Some(n.id);
    let accent = structure_accent(&n.kind);
    let b = LOOP_BORDER;
    let top = 22.0;
    let border_col = accent.linear_multiply(if sel { 0.9 } else { 0.55 });
    painter.rect_filled(r, 3.0, accent.linear_multiply(0.05));
    let bars = [
        Rect::from_min_size(r.min, Vec2::new(r.width(), top)),
        Rect::from_min_max(Pos2::new(r.left(), r.bottom() - b), r.max),
        Rect::from_min_max(r.min, Pos2::new(r.left() + b, r.bottom())),
        Rect::from_min_max(Pos2::new(r.right() - b, r.top()), r.max),
    ];
    for (i, bar) in bars.iter().enumerate() {
        painter.rect_filled(*bar, 0.0, border_col.linear_multiply(0.45));
        let resp = ui.interact(*bar, ui.scope_id().with(("loopbar", n.id, i)), Sense::click_and_drag());
        handle_drag(app, n, &resp);
        resp.context_menu(|ui| structure_menu(app, ui, n));
    }
    painter.rect_stroke(r, 3.0, Stroke::new(if sel { 2.5 } else { 1.5 }, border_col), StrokeKind::Inside);
    let inner = Rect::from_min_max(Pos2::new(r.left() + b, r.top() + top), Pos2::new(r.right() - b, r.bottom() - b));
    painter.rect_stroke(inner, 0.0, Stroke::new(1.0, border_col.linear_multiply(0.6)), StrokeKind::Inside);

    match &n.kind {
        NodeKind::Loop { kind, regs } => {
            painter.text(r.min + Vec2::new(10.0, top / 2.0), Align2::LEFT_CENTER, n.kind.title(), FontId::proportional(11.5), accent);
            for (k, ty) in regs.iter().enumerate() {
                let y = origin.y + n.loop_reg_y(k);
                let col = theme::type_color(ty);
                for x in [r.left(), r.right() - b] {
                    let br = Rect::from_min_size(Pos2::new(x, y - 11.0), Vec2::new(b, 22.0));
                    painter.rect_filled(br, 2.0, theme::NODE_FILL);
                    painter.rect_stroke(br, 2.0, Stroke::new(1.5, col), StrokeKind::Inside);
                }
                painter.text(Pos2::new(r.left() + b / 2.0, y), Align2::CENTER_CENTER, "▸", FontId::proportional(9.0), col);
                painter.text(Pos2::new(r.right() - b / 2.0, y), Align2::CENTER_CENTER, "▸", FontId::proportional(9.0), col);
            }
            let it_pos = origin + v2(n.terminal_pos(0, true));
            painter.text(it_pos + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, "i", FontId::monospace(11.0), theme::type_color(&DataType::I32));
            let p = origin + v2(n.terminal_pos(0, false));
            match kind {
                LoopKind::For => {
                    painter.text(p + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, "N", FontId::monospace(11.0), theme::type_color(&DataType::I32));
                }
                LoopKind::While => {
                    painter.text(p - Vec2::new(10.0, 0.0), Align2::RIGHT_CENTER, "⏺ stop", FontId::monospace(10.0), theme::type_color(&DataType::Bool));
                }
            }
        }
        NodeKind::Case { fixed, .. } => {
            painter.text(r.min + Vec2::new(10.0, top / 2.0), Align2::LEFT_CENTER, if fixed.is_some() { "Disable" } else { "Case" }, FontId::proportional(10.5), accent);
            frame_selector(app, ui, painter, n, r, accent);
            if fixed.is_none() {
                let p = origin + v2(n.terminal_pos(0, false));
                painter.text(p + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, "?", FontId::monospace(12.0), accent);
            }
        }
        NodeKind::Event { frames, .. } => {
            painter.text(r.min + Vec2::new(10.0, top / 2.0), Align2::LEFT_CENTER, "Event", FontId::proportional(10.5), accent);
            frame_selector(app, ui, painter, n, r, accent);
            let p = origin + v2(n.terminal_pos(0, false));
            painter.text(p + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, "⏱", FontId::proportional(11.0), accent);
            let cur = n.frame_view;
            if let Some(fr) = frames.get(cur).filter(|f| f.source.is_some()) {
                let p = origin + v2(n.terminal_pos(cur, true));
                painter.text(p + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, "NewVal", FontId::proportional(10.0), theme::type_color(&fr.ty));
            }
        }
        NodeKind::Sequence { frames } => {
            let fw = r.width() / *frames as f32;
            for f in 0..*frames {
                let x0 = r.left() + fw * f as f32;
                if f > 0 {
                    painter.line_segment([Pos2::new(x0, r.top()), Pos2::new(x0, r.bottom())], Stroke::new(1.5, border_col));
                }
                painter.text(Pos2::new(x0 + fw / 2.0, r.top() + top / 2.0), Align2::CENTER_CENTER, format!("frame {f}"), FontId::proportional(11.0), accent);
            }
        }
        _ => {}
    }

    // Output tunnels (Case/Event) sit on the right border.
    for (k, ty) in tunnel_types(&n.kind).iter().enumerate() {
        let y = origin.y + n.loop_reg_y(k);
        let col = theme::type_color(ty);
        let br = Rect::from_min_size(Pos2::new(r.right() - b, y - 11.0), Vec2::new(b, 22.0));
        painter.rect_filled(br, 2.0, theme::NODE_FILL);
        painter.rect_stroke(br, 2.0, Stroke::new(1.5, col), StrokeKind::Inside);
        painter.text(br.center(), Align2::CENTER_CENTER, "▸", FontId::proportional(9.0), col);
    }

    if !matches!(n.kind, NodeKind::Sequence { .. }) {
        let handle = Rect::from_min_size(r.max - Vec2::splat(16.0), Vec2::splat(16.0));
        painter.text(handle.center(), Align2::CENTER_CENTER, "◢", FontId::proportional(12.0), accent);
        let rz = ui.interact(handle, ui.scope_id().with(("resize", n.id)), Sense::drag());
        if rz.dragged() {
            let d = rz.drag_delta();
            app.pending.push(Command::Resize { id: n.id, size: [n.size[0] + d.x, n.size[1] + d.y] });
        }
        if rz.drag_stopped() {
            app.pending.push(Command::EndGesture);
        }
    } else {
        let handle = Rect::from_min_size(Pos2::new(r.right() - 16.0, r.bottom() - 16.0), Vec2::splat(16.0));
        let rz = ui.interact(handle, ui.scope_id().with(("resize", n.id)), Sense::drag());
        painter.text(handle.center(), Align2::CENTER_CENTER, "◢", FontId::proportional(12.0), accent);
        if rz.dragged() {
            let d = rz.drag_delta();
            app.pending.push(Command::Resize { id: n.id, size: [n.size[0], n.size[1] + d.y] });
        }
        if rz.drag_stopped() {
            app.pending.push(Command::EndGesture);
        }
    }

    draw_terminals(app, ui, painter, fr, n, true);
}

fn sample_bezier(pts: [Pos2; 4], n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            Pos2::new(
                w[0] * pts[0].x + w[1] * pts[1].x + w[2] * pts[2].x + w[3] * pts[3].x,
                w[0] * pts[0].y + w[1] * pts[1].y + w[2] * pts[2].y + w[3] * pts[3].y,
            )
        })
        .collect()
}

/// Wires follow LabVIEW conventions: thin for scalars, thick for arrays (with a hollow core),
/// thick and dashed for clusters. Returns the sampled path for hit-testing.
fn draw_wire(p: &egui::Painter, a: Pos2, b: Pos2, ty: &DataType, alpha: f32) -> Vec<Pos2> {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    let pts = [a, a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0), b];
    let col = theme::type_color(ty).linear_multiply(alpha);
    let width = theme::wire_width(ty);
    p.add(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(width, col)));
    let line = sample_bezier(pts, 40);
    match ty {
        DataType::Array(..) => {
            p.add(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(width - 2.4, theme::BG_DIAGRAM)));
        }
        DataType::Cluster(_) => {
            for s in Shape::dashed_line(&line, Stroke::new(1.6, Color32::from_black_alpha(190)), 5.0, 5.0) {
                p.add(s);
            }
        }
        _ => {}
    }
    line
}

/// Marks where a wire crosses a loop border: `[]` when auto-indexing, a solid square for last-value.
#[allow(clippy::too_many_arguments)]
fn draw_tunnel_markers(p: &egui::Painter, vi: &Vi, origin: Pos2, from: NodeId, to: NodeId, auto_index: bool, path: &[Pos2], ty: &DataType) {
    let (leaving, entering) = crossing_loops(vi, from, to);
    let col = theme::type_color(ty.base());
    for lid in leaving.iter().chain(entering.iter()) {
        let Some(node) = vi.node(*lid) else { continue };
        let rect = Rect::from_min_size(origin + v2(node.pos), v2(node.size));
        for pair in path.windows(2) {
            if rect.contains(pair[0]) != rect.contains(pair[1]) {
                let mid = pair[0].lerp(pair[1], 0.5);
                let m = Rect::from_center_size(mid, Vec2::splat(12.0));
                p.rect_filled(m, 1.0, theme::BG_DIAGRAM);
                p.rect_stroke(m, 1.0, Stroke::new(1.6, col), StrokeKind::Inside);
                if auto_index {
                    p.text(mid, Align2::CENTER_CENTER, "[]", FontId::monospace(9.0), col);
                } else {
                    p.rect_filled(m.shrink(3.5), 0.0, col);
                }
                break;
            }
        }
    }
}
