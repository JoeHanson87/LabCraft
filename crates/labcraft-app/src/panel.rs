use crate::app::LabCraft;
use crate::theme;
use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, StrokeKind, Vec2};
use labcraft_core::{Command, NodeKind, Value};

pub fn show(app: &mut LabCraft, ui: &mut egui::Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click());
    let origin = resp.rect.min;
    painter.rect_filled(resp.rect, 0.0, theme::BG_PANEL);
    if resp.clicked() {
        app.selected = None;
    }

    let nodes = app.ed.vi.nodes.clone();
    for n in nodes.iter().filter(|n| n.kind.is_panel_item()) {
        let (name, ty, is_ctl) = match &n.kind {
            NodeKind::Control { name, ty } => (name, *ty, true),
            NodeKind::Indicator { name, ty } => (name, *ty, false),
            _ => continue,
        };
        let col = theme::type_color(ty);
        let r = Rect::from_min_size(origin + Vec2::new(n.panel_pos[0], n.panel_pos[1]), Vec2::new(180.0, 64.0));
        let sel = app.selected == Some(n.id);

        let grab = Rect::from_min_size(r.min, Vec2::new(r.width(), 20.0));
        let g = ui.interact(grab, ui.scope_id().with(("grab", n.id)), Sense::click_and_drag());
        if g.clicked() || g.drag_started() {
            app.selected = Some(n.id);
        }
        if g.dragged() {
            let d = g.drag_delta();
            app.pending.push(Command::MovePanelItem { id: n.id, pos: [n.panel_pos[0] + d.x, n.panel_pos[1] + d.y] });
        }

        painter.rect_filled(r, 4.0, theme::NODE_FILL);
        painter.rect_stroke(r, 4.0, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { theme::ACCENT } else { col.linear_multiply(0.6) }), StrokeKind::Inside);
        painter.text(r.min + Vec2::new(8.0, 10.0), Align2::LEFT_CENTER, name, FontId::proportional(12.0), col);
        painter.text(r.right_top() + Vec2::new(-8.0, 10.0), Align2::RIGHT_CENTER, if is_ctl { "CTL" } else { "IND" }, FontId::monospace(9.0), theme::TEXT_DIM);

        let body = Rect::from_min_max(r.min + Vec2::new(8.0, 26.0), r.max - Vec2::new(8.0, 8.0));
        let mut value = n.value.clone().unwrap_or(Value::default_for(ty));
        let before = value.clone();
        if is_ctl {
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| match &mut value {
                Value::Dbl(v) => { ui.add(egui::DragValue::new(v).speed(0.1)); }
                Value::Int(v) => { ui.add(egui::DragValue::new(v)); }
                Value::Bool(v) => { ui.checkbox(v, if *v { "TRUE" } else { "FALSE" }); }
                Value::Str(s) => { ui.add(egui::TextEdit::singleline(s).desired_width(body.width())); }
            });
        } else {
            painter.rect_filled(body, 3.0, egui::Color32::from_rgb(14, 15, 18));
            painter.rect_stroke(body, 3.0, Stroke::new(1.0, col.linear_multiply(0.5)), StrokeKind::Inside);
            painter.text(body.left_center() + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, value.to_string(), FontId::monospace(13.0), col);
        }
        if value != before {
            app.pending.push(Command::SetValue { id: n.id, value });
        }
    }
    if nodes.iter().all(|n| !n.kind.is_panel_item()) {
        painter.text(resp.rect.center(), Align2::CENTER_CENTER, "Add controls and indicators from the palette", FontId::proportional(14.0), theme::TEXT_DIM);
    }
}
