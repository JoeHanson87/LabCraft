use crate::app::LabCraft;
use crate::theme;
use eframe::egui::{self, epaint::CubicBezierShape, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use labcraft_core::{Command, DataType, LoopKind, Node, NodeId, NodeKind, Value, LOOP_BORDER};

const TERM_R: f32 = 5.0;

fn v2(p: [f32; 2]) -> Vec2 {
    Vec2::new(p[0], p[1])
}

fn term_screen(origin: Pos2, n: &Node, idx: usize, output: bool) -> Pos2 {
    origin + v2(n.terminal_pos(idx, output))
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

    let mut loops: Vec<&Node> = nodes.iter().filter(|n| n.kind.is_loop()).collect();
    loops.sort_by_key(|n| app.ed.vi.depth(n.id));
    for n in loops {
        draw_loop(app, ui, &painter, origin, n, hit_input);
    }

    for w in &app.ed.vi.wires {
        let (Some(a), Some(b)) = (app.ed.vi.node(w.from.0), app.ed.vi.node(w.to.0)) else { continue };
        let ty = a.kind.outputs()[w.from.1].ty;
        draw_wire(&painter, term_screen(origin, a, w.from.1, true), term_screen(origin, b, w.to.1, false), Stroke::new(2.5, theme::type_color(ty)));
    }

    for n in nodes.iter().filter(|n| !n.kind.is_loop()) {
        let r = Rect::from_min_size(origin + v2(n.pos), v2(n.size));
        let sel = app.selected == Some(n.id);
        let body = ui.interact(r, ui.scope_id().with(("node", n.id)), Sense::click_and_drag());
        handle_drag(app, n, &body);
        painter.rect_filled(r, 4.0, theme::NODE_FILL);
        painter.rect_stroke(r, 4.0, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { theme::ACCENT } else { theme::NODE_STROKE }), StrokeKind::Inside);
        let accent = n.kind.outputs().first().or(n.kind.inputs().first()).map(|t| theme::type_color(t.ty)).unwrap_or(theme::ACCENT);
        painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(4.0, r.height())), 2.0, accent);
        painter.text(r.min + Vec2::new(r.width() / 2.0, 14.0), Align2::CENTER_CENTER, n.kind.title(), FontId::proportional(13.0), Color32::from_gray(225));

        if let NodeKind::Constant(v0) = &n.kind {
            let ed = Rect::from_center_size(r.center() + Vec2::new(0.0, 10.0), Vec2::new(70.0, 22.0));
            let mut value = n.value.clone().unwrap_or(v0.clone());
            let before = value.clone();
            ui.scope_builder(egui::UiBuilder::new().max_rect(ed), |ui| match &mut value {
                Value::Dbl(v) => { ui.add(egui::DragValue::new(v).speed(0.05)); }
                Value::Int(v) => { ui.add(egui::DragValue::new(v)); }
                Value::Bool(v) => { ui.checkbox(v, ""); }
                Value::Str(s) => { ui.add(egui::TextEdit::singleline(s).desired_width(66.0)); }
                Value::ArrDbl(_) => {}
            });
            if value != before {
                app.pending.push(Command::SetValue { id: n.id, value });
            }
        }
        draw_terminals(app, ui, &painter, origin, n, hit_input, false);
    }

    if let (Some((id, i)), Some(p)) = (app.wiring, pointer) {
        if let Some(n) = app.ed.vi.node(id) {
            let ty = n.kind.outputs()[i].ty;
            draw_wire(&painter, term_screen(origin, n, i, true), p, Stroke::new(2.0, theme::type_color(ty).linear_multiply(0.7)));
        }
    }

    if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
        if let Some(id) = app.selected {
            app.pending.push(Command::RemoveNode(id));
        }
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

fn draw_terminals(app: &mut LabCraft, ui: &mut egui::Ui, painter: &egui::Painter, origin: Pos2, n: &Node, hit_input: Option<(NodeId, usize)>, square: bool) {
    let mark = |p: Pos2, col: Color32| {
        if square {
            painter.rect_filled(Rect::from_center_size(p, Vec2::splat(TERM_R * 2.0)), 1.0, col);
            painter.rect_stroke(Rect::from_center_size(p, Vec2::splat(TERM_R * 2.0)), 1.0, Stroke::new(1.0, Color32::from_black_alpha(160)), StrokeKind::Inside);
        } else {
            painter.circle_filled(p, TERM_R, col);
        }
    };
    for (i, t) in n.kind.inputs().iter().enumerate() {
        let p = term_screen(origin, n, i, false);
        mark(p, theme::type_color(t.ty));
        if hit_input == Some((n.id, i)) && app.wiring.is_some() {
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
        mark(p, theme::type_color(t.ty));
        if !square {
            painter.text(p - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, &t.name, FontId::proportional(10.0), theme::TEXT_DIM);
        }
        let tr = ui.interact(Rect::from_center_size(p, Vec2::splat(14.0)), ui.scope_id().with(("out", n.id, i)), Sense::drag());
        if tr.drag_started() {
            app.wiring = Some((n.id, i));
        }
        if tr.drag_stopped() {
            if let (Some(from), Some(to)) = (app.wiring.take(), hit_input) {
                app.pending.push(Command::Connect { from, to });
            }
        }
    }
}

fn draw_loop(app: &mut LabCraft, ui: &mut egui::Ui, painter: &egui::Painter, origin: Pos2, n: &Node, hit_input: Option<(NodeId, usize)>) {
    let NodeKind::Loop { kind, regs } = &n.kind else { return };
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
            ui.label(RichText2::title(&n.kind.title()));
            ui.menu_button("Add Shift Register", |ui| {
                for (label, ty) in [("Numeric", DataType::Dbl), ("Integer", DataType::Int), ("Boolean", DataType::Bool), ("String", DataType::Str), ("Array", DataType::ArrDbl)] {
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
    painter.rect_stroke(r.shrink2(Vec2::new(b, 0.0)).with_min_y(r.top() + top).with_max_y(r.bottom() - b), 0.0, Stroke::new(1.0, border_col.linear_multiply(0.6)), StrokeKind::Inside);
    painter.text(r.min + Vec2::new(10.0, top / 2.0), Align2::LEFT_CENTER, n.kind.title(), FontId::proportional(11.5), accent);

    for (k, ty) in regs.iter().enumerate() {
        let y = origin.y + n.loop_reg_y(k);
        let col = theme::type_color(*ty);
        for x in [r.left(), r.right() - b] {
            let br = Rect::from_min_size(Pos2::new(x, y - 11.0), Vec2::new(b, 22.0));
            painter.rect_filled(br, 2.0, theme::NODE_FILL);
            painter.rect_stroke(br, 2.0, Stroke::new(1.5, col), StrokeKind::Inside);
        }
        painter.text(Pos2::new(r.left() + b / 2.0, y), Align2::CENTER_CENTER, "▸", FontId::proportional(9.0), col);
        painter.text(Pos2::new(r.right() - b / 2.0, y), Align2::CENTER_CENTER, "▸", FontId::proportional(9.0), col);
    }

    let (it_pos, it_label) = (origin + v2(n.terminal_pos(0, true)), "i");
    painter.text(it_pos + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, it_label, FontId::monospace(11.0), theme::type_color(DataType::Int));
    match kind {
        LoopKind::For => {
            let p = origin + v2(n.terminal_pos(0, false));
            painter.text(p + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, "N", FontId::monospace(11.0), theme::type_color(DataType::Int));
        }
        LoopKind::While => {
            let p = origin + v2(n.terminal_pos(0, false));
            painter.text(p - Vec2::new(10.0, 0.0), Align2::RIGHT_CENTER, "⏺ stop", FontId::monospace(10.0), theme::type_color(DataType::Bool));
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

    draw_terminals(app, ui, painter, origin, n, hit_input, true);
}

struct RichText2;
impl RichText2 {
    fn title(s: &str) -> egui::RichText {
        egui::RichText::new(s).strong()
    }
}

fn draw_wire(p: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    p.add(CubicBezierShape::from_points_stroke([a, a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0), b], false, Color32::TRANSPARENT, stroke));
}
