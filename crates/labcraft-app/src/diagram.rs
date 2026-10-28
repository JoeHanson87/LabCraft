use crate::app::LabCraft;
use crate::{theme, valedit};
use eframe::egui::{self, epaint::CubicBezierShape, epaint::Shape, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use labcraft_core::{coercion_dots, resolve, Command, DataType, LoopKind, Node, NodeId, NodeKind, Spec, TypeMap, Value, LOOP_BORDER};
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

    let mut hit_input: Option<(NodeId, usize)> = None;
    if let Some(p) = pointer {
        for n in &nodes {
            for i in 0..n.kind.inputs().len() {
                if term_screen(origin, n, i, false).distance(p) < 10.0 {
                    hit_input = Some((n.id, i));
                }
            }
        }
    }
    let fr = Frame { origin, tm: &tm, dots: &dots, hit_input };

    let mut loops: Vec<&Node> = nodes.iter().filter(|n| n.kind.is_loop()).collect();
    loops.sort_by_key(|n| app.ed.vi.depth(n.id));
    for n in loops {
        draw_loop(app, ui, &painter, &fr, n);
    }

    for w in &app.ed.vi.wires {
        let (Some(a), Some(b)) = (app.ed.vi.node(w.from.0), app.ed.vi.node(w.to.0)) else { continue };
        let Some(ty) = tm.out(w.from.0, w.from.1) else { continue };
        draw_wire(&painter, term_screen(origin, a, w.from.1, true), term_screen(origin, b, w.to.1, false), ty, 1.0);
    }

    for n in nodes.iter().filter(|n| !n.kind.is_loop()) {
        let r = Rect::from_min_size(origin + v2(n.pos), v2(n.size));
        let sel = app.selected == Some(n.id);
        let body = ui.interact(r, ui.scope_id().with(("node", n.id)), Sense::click_and_drag());
        handle_drag(app, n, &body);
        body.context_menu(|ui| node_menu(app, ui, n, &tm));
        painter.rect_filled(r, 4.0, theme::NODE_FILL);
        painter.rect_stroke(r, 4.0, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { theme::ACCENT } else { theme::NODE_STROKE }), StrokeKind::Inside);
        let accent = tm.outs(n.id).first().map(theme::type_color).or_else(|| n.kind.inputs().first().map(|t| theme::type_color(&t.ty))).unwrap_or(theme::ACCENT);
        painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(4.0, r.height())), 2.0, accent);
        painter.text(r.min + Vec2::new(r.width() / 2.0, 14.0), Align2::CENTER_CENTER, n.kind.title(), FontId::proportional(13.0), Color32::from_gray(225));

        if let NodeKind::Constant(ty) = &n.kind {
            constant_body(app, ui, &painter, n, ty, r);
        }
        draw_terminals(app, ui, &painter, &fr, n, false);
    }

    if let (Some((id, i)), Some(p)) = (app.wiring, pointer) {
        if let (Some(n), Some(ty)) = (app.ed.vi.node(id), tm.out(id, i)) {
            draw_wire(&painter, term_screen(origin, n, i, true), p, ty, 0.6);
        }
    }

    constant_window(app, ui.ctx());

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
    if ui.button("Delete").clicked() {
        app.pending.push(Command::RemoveNode(n.id));
        ui.close();
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
        let p = term_screen(origin, n, i, false);
        let col = match fr.tm.input(&app.ed.vi, n.id, i) {
            Some(ty) => theme::type_color(ty),
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

fn draw_loop(app: &mut LabCraft, ui: &mut egui::Ui, painter: &egui::Painter, fr: &Frame, n: &Node) {
    let NodeKind::Loop { kind, regs } = &n.kind else { return };
    let origin = fr.origin;
    let r = Rect::from_min_size(origin + v2(n.pos), v2(n.size));
    let sel = app.selected == Some(n.id);
    let accent = match kind {
        LoopKind::While => Color32::from_rgb(0, 190, 170),
        LoopKind::For => Color32::from_rgb(90, 140, 255),
    };
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
        resp.context_menu(|ui| {
            ui.label(egui::RichText::new(n.kind.title()).strong());
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
        });
    }
    painter.rect_stroke(r, 3.0, Stroke::new(if sel { 2.5 } else { 1.5 }, border_col), StrokeKind::Inside);
    let inner = Rect::from_min_max(Pos2::new(r.left() + b, r.top() + top), Pos2::new(r.right() - b, r.bottom() - b));
    painter.rect_stroke(inner, 0.0, Stroke::new(1.0, border_col.linear_multiply(0.6)), StrokeKind::Inside);
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
/// thick and dashed for clusters.
fn draw_wire(p: &egui::Painter, a: Pos2, b: Pos2, ty: &DataType, alpha: f32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    let pts = [a, a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0), b];
    let col = theme::type_color(ty).linear_multiply(alpha);
    let width = theme::wire_width(ty);
    p.add(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(width, col)));
    match ty {
        DataType::Array(..) => {
            p.add(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(width - 2.4, theme::BG_DIAGRAM)));
        }
        DataType::Cluster(_) => {
            let line = sample_bezier(pts, 40);
            for s in Shape::dashed_line(&line, Stroke::new(1.6, Color32::from_black_alpha(190)), 5.0, 5.0) {
                p.add(s);
            }
        }
        _ => {}
    }
}
