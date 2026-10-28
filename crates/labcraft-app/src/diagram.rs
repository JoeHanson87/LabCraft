use crate::app::LabCraft;
use crate::theme;
use eframe::egui::{self, epaint::CubicBezierShape, Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use labcraft_core::{Command, Node, NodeId};

const W: f32 = 120.0;
const TERM_R: f32 = 5.0;

fn size(n: &Node) -> Vec2 {
    let rows = n.kind.inputs().len().max(n.kind.outputs().len()).max(1) as f32;
    Vec2::new(W, 28.0 + rows * 22.0)
}

fn term_pos(origin: Pos2, n: &Node, idx: usize, output: bool) -> Pos2 {
    let s = size(n);
    let count = if output { n.kind.outputs().len() } else { n.kind.inputs().len() }.max(1) as f32;
    let y = 28.0 + (idx as f32 + 0.5) * (s.y - 28.0) / count;
    let x = if output { s.x } else { 0.0 };
    origin + Vec2::new(n.pos[0] + x, n.pos[1] + y)
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

    for w in &app.ed.vi.wires {
        let (Some(a), Some(b)) = (app.ed.vi.node(w.from.0), app.ed.vi.node(w.to.0)) else { continue };
        let p0 = term_pos(origin, a, w.from.1, true);
        let p1 = term_pos(origin, b, w.to.1, false);
        let ty = a.kind.outputs()[w.from.1].ty;
        draw_wire(&painter, p0, p1, Stroke::new(2.5, theme::type_color(ty)));
    }

    let mut hit_input: Option<(NodeId, usize)> = None;
    if let Some(p) = pointer {
        for n in &nodes {
            for i in 0..n.kind.inputs().len() {
                if term_pos(origin, n, i, false).distance(p) < 10.0 {
                    hit_input = Some((n.id, i));
                }
            }
        }
    }

    for n in &nodes {
        let r = Rect::from_min_size(origin + Vec2::new(n.pos[0], n.pos[1]), size(n));
        let sel = app.selected == Some(n.id);
        let body = ui.interact(r, ui.id().with(("node", n.id)), Sense::click_and_drag());
        if body.clicked() || body.drag_started() {
            app.selected = Some(n.id);
        }
        if body.dragged() {
            let d = body.drag_delta();
            app.pending.push(Command::MoveNode { id: n.id, pos: [n.pos[0] + d.x, n.pos[1] + d.y] });
        }
        let border = if sel { theme::ACCENT } else { theme::NODE_STROKE };
        painter.rect_filled(r, 4.0, theme::NODE_FILL);
        painter.rect_stroke(r, 4.0, Stroke::new(if sel { 2.0 } else { 1.0 }, border), StrokeKind::Inside);
        let accent = n.kind.outputs().first().or(n.kind.inputs().first()).map(|t| theme::type_color(t.ty)).unwrap_or(theme::ACCENT);
        painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(4.0, r.height())), 2.0, accent);
        painter.text(r.min + Vec2::new(W / 2.0, 14.0), Align2::CENTER_CENTER, n.kind.title(), FontId::proportional(13.0), egui::Color32::from_gray(225));

        for (i, t) in n.kind.inputs().iter().enumerate() {
            let p = term_pos(origin, n, i, false);
            let hot = hit_input == Some((n.id, i)) && app.wiring.is_some();
            painter.circle_filled(p, TERM_R, theme::type_color(t.ty));
            if hot {
                painter.circle_stroke(p, TERM_R + 3.0, Stroke::new(1.5, theme::ACCENT));
            }
            painter.text(p + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, t.name, FontId::proportional(10.0), theme::TEXT_DIM);
            let tr = ui.interact(Rect::from_center_size(p, Vec2::splat(14.0)), ui.id().with(("in", n.id, i)), Sense::click());
            if tr.secondary_clicked() {
                app.pending.push(Command::Disconnect { to: (n.id, i) });
            }
        }
        for (i, t) in n.kind.outputs().iter().enumerate() {
            let p = term_pos(origin, n, i, true);
            painter.circle_filled(p, TERM_R, theme::type_color(t.ty));
            painter.text(p - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, t.name, FontId::proportional(10.0), theme::TEXT_DIM);
            let tr = ui.interact(Rect::from_center_size(p, Vec2::splat(14.0)), ui.id().with(("out", n.id, i)), Sense::drag());
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

    if let (Some((id, i)), Some(p)) = (app.wiring, pointer) {
        if let Some(n) = app.ed.vi.node(id) {
            let ty = n.kind.outputs()[i].ty;
            draw_wire(&painter, term_pos(origin, n, i, true), p, Stroke::new(2.0, theme::type_color(ty).linear_multiply(0.7)));
        }
    }

    if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
        if let Some(id) = app.selected {
            app.pending.push(Command::RemoveNode(id));
        }
    }
}

fn draw_wire(p: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    p.add(CubicBezierShape::from_points_stroke(
        [a, a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0), b],
        false,
        egui::Color32::TRANSPARENT,
        stroke,
    ));
}
