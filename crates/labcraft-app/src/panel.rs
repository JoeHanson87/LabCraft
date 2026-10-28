use crate::app::LabCraft;
use crate::theme;
use crate::valedit;
use eframe::egui::{self, epaint::Shape, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use labcraft_core::{Command, DataType, Node, NodeKind, Value, Widget};

const SWAPPABLE: [Widget; 18] = [
    Widget::NumCtl, Widget::Knob, Widget::Slide, Widget::NumInd, Widget::Gauge, Widget::Thermometer, Widget::Tank,
    Widget::Toggle, Widget::PushButton, Widget::Led, Widget::StrCtl, Widget::StrInd, Widget::EnumCtl, Widget::EnumInd,
    Widget::Graph, Widget::ArrayInd, Widget::ArrayCtl, Widget::ClusterCtl,
];

fn v2(p: [f32; 2]) -> Vec2 {
    Vec2::new(p[0], p[1])
}

fn fmt(v: f64) -> String {
    format!("{v:.3}")
}

fn frac(v: f64, range: [f64; 2]) -> f32 {
    if range[1] == range[0] {
        return 0.0;
    }
    (((v - range[0]) / (range[1] - range[0])) as f32).clamp(0.0, 1.0)
}

fn polar(c: Pos2, r: f32, deg: f32) -> Pos2 {
    let a = deg.to_radians();
    c + Vec2::new(a.cos(), a.sin()) * r
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let steps = 48;
    (0..=steps).map(|i| polar(c, r, a0 + (a1 - a0) * i as f32 / steps as f32)).collect()
}

fn sunken(p: &Painter, r: Rect) {
    p.rect_filled(r, 2.0, theme::SUNKEN);
    p.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(72)), StrokeKind::Inside);
    p.line_segment([r.left_top() + Vec2::new(1.0, 1.0), r.right_top() + Vec2::new(-1.0, 1.0)], Stroke::new(1.0, Color32::BLACK));
    p.line_segment([r.left_top() + Vec2::new(1.0, 1.0), r.left_bottom() + Vec2::new(1.0, -1.0)], Stroke::new(1.0, Color32::BLACK));
}

fn triangle(p: &Painter, pts: [Pos2; 3], col: Color32) {
    p.add(Shape::convex_polygon(pts.to_vec(), col, Stroke::NONE));
}

fn tick_labels(p: &Painter, pos: Pos2, align: Align2, text: String) {
    p.text(pos, align, text, FontId::proportional(9.5), theme::TEXT_DIM);
}

pub fn show(app: &mut LabCraft, ui: &mut Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click());
    let origin = resp.rect.min;
    painter.rect_filled(resp.rect, 0.0, theme::BG_PANEL);
    let mut gx = resp.rect.left();
    while gx < resp.rect.right() {
        painter.line_segment([Pos2::new(gx, resp.rect.top()), Pos2::new(gx, resp.rect.bottom())], Stroke::new(1.0, Color32::from_white_alpha(4)));
        gx += 40.0;
    }
    let mut gy = resp.rect.top();
    while gy < resp.rect.bottom() {
        painter.line_segment([Pos2::new(resp.rect.left(), gy), Pos2::new(resp.rect.right(), gy)], Stroke::new(1.0, Color32::from_white_alpha(4)));
        gy += 40.0;
    }
    if resp.clicked() {
        app.selected = None;
    }

    let nodes = app.ed.vi.nodes.clone();
    let items: Vec<&Node> = nodes.iter().filter(|n| n.kind.is_panel_item()).collect();
    for n in &items {
        let (name, ty, is_ctl) = match &n.kind {
            NodeKind::Control { name, ty } => (name, ty.clone(), true),
            NodeKind::Indicator { name, ty } => (name, ty.clone(), false),
            _ => continue,
        };
        let col = theme::type_color(&ty);
        let size = v2(n.panel_size());
        let r = Rect::from_min_size(origin + v2(n.panel_pos), size);
        if app.selected == Some(n.id) {
            painter.rect_stroke(r.expand(4.0), 3.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Outside);
        }

        let label_r = Rect::from_min_size(r.min, Vec2::new(size.x, 16.0));
        let g = ui.interact(label_r, ui.scope_id().with(("grab", n.id)), Sense::click_and_drag());
        if g.clicked() || g.drag_started() {
            app.selected = Some(n.id);
        }
        if g.dragged() {
            let d = g.drag_delta();
            app.pending.push(Command::MovePanelItem { id: n.id, pos: [n.panel_pos[0] + d.x, n.panel_pos[1] + d.y] });
        }
        if g.drag_stopped() {
            app.pending.push(Command::EndGesture);
        }
        g.context_menu(|ui| item_menu(app, ui, n, &ty, is_ctl));
        painter.text(r.min + Vec2::new(2.0, 8.0), Align2::LEFT_CENTER, name, FontId::proportional(11.5), Color32::from_gray(190));

        let body = Rect::from_min_max(r.min + Vec2::new(0.0, 18.0), r.max);
        let id = ui.scope_id().with(("w", n.id));
        let value = n.value.clone().unwrap_or(Value::default_for(&ty));
        let range = n.range;
        let new = match n.widget {
            Widget::NumCtl => num_ctl(ui, &painter, id, body, &value, col),
            Widget::NumInd => {
                let r = Rect::from_min_size(body.min, Vec2::new(body.width(), 26.0));
                sunken(&painter, r);
                painter.text(r.right_center() - Vec2::new(6.0, 0.0), Align2::RIGHT_CENTER, fmt(value.as_f64().unwrap_or(0.0)), FontId::monospace(14.0), col);
                None
            }
            Widget::Knob => knob(ui, &painter, id, body, &value, range, col, is_ctl),
            Widget::Slide => slide(ui, &painter, id, body, &value, range, col, is_ctl),
            Widget::Gauge => {
                gauge(&painter, body, value.as_f64().unwrap_or(0.0), range, col);
                None
            }
            Widget::Thermometer => {
                thermometer(&painter, body, value.as_f64().unwrap_or(0.0), range);
                None
            }
            Widget::Tank => {
                tank(&painter, body, value.as_f64().unwrap_or(0.0), range);
                None
            }
            Widget::Toggle => toggle(ui, &painter, id, body, matches!(value, Value::Bool(true))),
            Widget::PushButton => push_button(ui, &painter, id, body, matches!(value, Value::Bool(true)), name),
            Widget::Led => {
                led(&painter, body, matches!(value, Value::Bool(true)));
                None
            }
            Widget::StrCtl => str_ctl(ui, &painter, body, &value),
            Widget::StrInd => {
                let r = Rect::from_min_size(body.min, Vec2::new(body.width(), 26.0));
                sunken(&painter, r);
                painter.with_clip_rect(r).text(r.left_center() + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, value.to_string(), FontId::monospace(13.0), col);
                None
            }
            Widget::Chart | Widget::Graph => {
                plot(&painter, body, &value.to_f64_vec(), n.widget == Widget::Chart);
                None
            }
            Widget::EnumCtl | Widget::EnumInd => {
                let r = Rect::from_min_size(body.min, Vec2::new(body.width(), 26.0));
                let (DataType::Enum(items), Value::Int(i)) = (&ty, &value) else { continue };
                if is_ctl {
                    sunken(&painter, r);
                    let mut idx = *i;
                    ui.scope_builder(UiBuilder::new().max_rect(r.shrink(1.0)).id_salt(("enum", n.id)), |ui| {
                        egui::ComboBox::from_id_salt(("enumcb", n.id)).width(r.width() - 10.0).selected_text(items.get(idx as usize).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                            for (k, it) in items.iter().enumerate() {
                                ui.selectable_value(&mut idx, k as i64, it);
                            }
                        });
                    });
                    (idx != *i).then(|| Value::Int(idx))
                } else {
                    sunken(&painter, r);
                    painter.text(r.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, items.get(*i as usize).cloned().unwrap_or_default(), FontId::proportional(13.0), col);
                    None
                }
            }
            Widget::ArrayCtl | Widget::ArrayInd | Widget::ClusterCtl | Widget::ClusterInd => {
                let frame = body.shrink2(Vec2::new(0.0, 0.0));
                painter.rect_filled(frame, 3.0, Color32::from_rgb(26, 28, 34));
                painter.rect_stroke(frame, 3.0, Stroke::new(1.5, col.linear_multiply(0.7)), StrokeKind::Inside);
                let mut v = value.clone();
                let changed = ui
                    .scope_builder(UiBuilder::new().max_rect(frame.shrink(6.0)).id_salt(("val", n.id)), |ui| {
                        ui.set_clip_rect(frame.shrink(1.0));
                        valedit::edit_value(ui, &ty, &mut v, is_ctl)
                    })
                    .inner;
                changed.then_some(v)
            }
        };
        if let Some(v) = new {
            app.pending.push(Command::SetValue { id: n.id, value: v });
        }
    }
    if items.is_empty() {
        painter.text(resp.rect.center(), Align2::CENTER_CENTER, "Add controls and indicators from the palette", FontId::proportional(14.0), theme::TEXT_DIM);
    }
}

fn item_menu(app: &mut LabCraft, ui: &mut Ui, n: &Node, ty: &DataType, is_ctl: bool) {
    let options: Vec<Widget> = SWAPPABLE.iter().copied().filter(|w| w.fits(ty, is_ctl) && !matches!(w, Widget::Graph if n.widget == Widget::Chart)).collect();
    if options.len() > 1 {
        ui.menu_button("Replace with", |ui| {
            for w in &options {
                if ui.selectable_label(n.widget == *w, w.label()).clicked() {
                    app.pending.push(Command::SetWidget { id: n.id, widget: *w });
                    ui.close();
                }
            }
        });
    }
    if n.widget != Widget::Chart {
        let mut t = ty.clone();
        ui.menu_button(format!("Data type: {}", ty.label()), |ui| {
            if valedit::type_menu(ui, &mut t) {
                app.pending.push(Command::SetType { id: n.id, ty: t.clone() });
            }
        });
    }
    if n.widget.is_numeric_scaled() {
        let mut range = n.range;
        ui.horizontal(|ui| {
            ui.label("Min");
            let a = ui.add(egui::DragValue::new(&mut range[0]).speed(0.1)).changed();
            ui.label("Max");
            let b = ui.add(egui::DragValue::new(&mut range[1]).speed(0.1)).changed();
            if a || b {
                app.pending.push(Command::SetRange { id: n.id, range });
            }
        });
    }
    if ui.button("Delete").clicked() {
        app.pending.push(Command::RemoveNode(n.id));
        ui.close();
    }
}

fn wrap_num(orig: &Value, x: f64) -> Value {
    match orig {
        Value::Int(_) => Value::Int(x.round() as i64),
        _ => Value::Dbl(x),
    }
}

fn num_ctl(ui: &mut Ui, p: &Painter, id: egui::Id, body: Rect, value: &Value, col: Color32) -> Option<Value> {
    let v = value.as_f64().unwrap_or(0.0);
    let disp = Rect::from_min_size(body.min + Vec2::new(18.0, 0.0), Vec2::new(body.width() - 18.0, 26.0));
    sunken(p, disp);
    let mut out = None;
    for (i, dir) in [1.0f64, -1.0].into_iter().enumerate() {
        let rc = Rect::from_min_size(body.min + Vec2::new(0.0, 13.0 * i as f32), Vec2::new(16.0, 13.0));
        let r = ui.interact(rc, id.with(i), Sense::click());
        p.rect_filled(rc.shrink(0.5), 2.0, if r.hovered() { Color32::from_gray(78) } else { Color32::from_gray(52) });
        let c = rc.center();
        let s = dir as f32;
        triangle(p, [c + Vec2::new(-4.0, 2.0 * s), c + Vec2::new(4.0, 2.0 * s), c + Vec2::new(0.0, -2.5 * s)], Color32::from_gray(210));
        if r.clicked() {
            out = Some(wrap_num(value, v + dir));
        }
    }
    let mut x = v;
    let changed = ui
        .scope_builder(UiBuilder::new().max_rect(disp.shrink(1.0)), |ui| {
            let vis = ui.visuals_mut();
            for w in [&mut vis.widgets.inactive, &mut vis.widgets.hovered, &mut vis.widgets.active] {
                w.weak_bg_fill = Color32::TRANSPARENT;
                w.bg_fill = Color32::TRANSPARENT;
                w.bg_stroke = Stroke::NONE;
            }
            vis.override_text_color = Some(col);
            ui.add_sized(disp.size() - Vec2::splat(2.0), egui::DragValue::new(&mut x).speed(0.05)).changed()
        })
        .inner;
    if changed {
        out = Some(wrap_num(value, x));
    }
    out
}

fn knob(ui: &mut Ui, p: &Painter, id: egui::Id, body: Rect, value: &Value, range: [f64; 2], col: Color32, is_ctl: bool) -> Option<Value> {
    let v = value.as_f64().unwrap_or(0.0);
    let c = Pos2::new(body.center().x, body.top() + 58.0);
    let r = 38.0;
    for i in 0..=10 {
        let a = 135.0 + 270.0 * i as f32 / 10.0;
        p.line_segment([polar(c, r + 4.0, a), polar(c, r + if i % 5 == 0 { 11.0 } else { 8.0 }, a)], Stroke::new(1.0, Color32::from_gray(150)));
    }
    tick_labels(p, polar(c, r + 20.0, 135.0), Align2::CENTER_CENTER, format!("{}", range[0]));
    tick_labels(p, polar(c, r + 20.0, 45.0), Align2::CENTER_CENTER, format!("{}", range[1]));
    p.circle_filled(c, r + 1.0, Color32::from_gray(28));
    p.circle_filled(c, r - 1.0, Color32::from_gray(88));
    p.circle_filled(c, r - 4.0, Color32::from_gray(58));
    p.circle_filled(c + Vec2::new(-4.0, -5.0), r - 14.0, Color32::from_white_alpha(10));
    let f = frac(v, range);
    let ang = 135.0 + 270.0 * f;
    p.line_segment([polar(c, 8.0, ang), polar(c, r - 6.0, ang)], Stroke::new(3.5, col));
    p.circle_filled(c, 5.0, Color32::from_gray(40));
    let ro = Rect::from_min_size(Pos2::new(body.center().x - 40.0, body.bottom() - 22.0), Vec2::new(80.0, 20.0));
    sunken(p, ro);
    p.text(ro.center(), Align2::CENTER_CENTER, fmt(v), FontId::monospace(12.0), col);
    if !is_ctl {
        return None;
    }
    let r = ui.interact(Rect::from_center_size(c, Vec2::splat(2.0 * (r + 10.0))), id, Sense::click_and_drag());
    if let (true, Some(pt)) = (r.dragged() || r.clicked(), r.interact_pointer_pos()) {
        let d = pt - c;
        let rel = (d.y.atan2(d.x).to_degrees() - 135.0).rem_euclid(360.0);
        let f = if rel <= 270.0 { rel / 270.0 } else if rel < 315.0 { 1.0 } else { 0.0 };
        return Some(wrap_num(value, range[0] + f as f64 * (range[1] - range[0])));
    }
    None
}

fn slide(ui: &mut Ui, p: &Painter, id: egui::Id, body: Rect, value: &Value, range: [f64; 2], col: Color32, is_ctl: bool) -> Option<Value> {
    let v = value.as_f64().unwrap_or(0.0);
    let track = Rect::from_min_size(Pos2::new(body.left() + 12.0, body.top() + 6.0), Vec2::new(body.width() - 24.0, 8.0));
    sunken(p, track);
    let f = frac(v, range);
    p.rect_filled(Rect::from_min_size(track.min, Vec2::new(track.width() * f, 8.0)).shrink(1.0), 1.0, col.linear_multiply(0.45));
    for i in 0..=10 {
        let x = track.left() + track.width() * i as f32 / 10.0;
        p.line_segment([Pos2::new(x, track.bottom() + 5.0), Pos2::new(x, track.bottom() + if i % 5 == 0 { 12.0 } else { 9.0 })], Stroke::new(1.0, Color32::from_gray(140)));
    }
    tick_labels(p, Pos2::new(track.left(), track.bottom() + 20.0), Align2::CENTER_CENTER, format!("{}", range[0]));
    tick_labels(p, Pos2::new(track.right(), track.bottom() + 20.0), Align2::CENTER_CENTER, format!("{}", range[1]));
    p.text(Pos2::new(track.center().x, track.bottom() + 20.0), Align2::CENTER_CENTER, fmt(v), FontId::monospace(11.0), col);
    let tx = track.left() + track.width() * f;
    let thumb = Rect::from_center_size(Pos2::new(tx, track.center().y), Vec2::new(12.0, 24.0));
    p.rect_filled(thumb, 3.0, Color32::from_gray(105));
    p.rect_stroke(thumb, 3.0, Stroke::new(1.0, Color32::from_gray(30)), StrokeKind::Inside);
    p.line_segment([thumb.center_top() + Vec2::new(0.0, 4.0), thumb.center_bottom() - Vec2::new(0.0, 4.0)], Stroke::new(1.5, col));
    if !is_ctl {
        return None;
    }
    let area = Rect::from_min_size(body.min, Vec2::new(body.width(), 30.0));
    let r = ui.interact(area, id, Sense::click_and_drag());
    if let (true, Some(pt)) = (r.dragged() || r.clicked(), r.interact_pointer_pos()) {
        let f = ((pt.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
        return Some(wrap_num(value, range[0] + f * (range[1] - range[0])));
    }
    None
}

fn gauge(p: &Painter, body: Rect, v: f64, range: [f64; 2], col: Color32) {
    let c = Pos2::new(body.center().x, body.top() + 96.0);
    let r = 78.0;
    let mut face = arc(c, r, 180.0, 360.0);
    face.push(c);
    p.add(Shape::convex_polygon(face, Color32::from_rgb(20, 22, 27), Stroke::new(2.0, Color32::from_gray(90))));
    let f = frac(v, range);
    p.add(Shape::line(arc(c, r - 8.0, 180.0, 180.0 + 180.0 * f.max(0.001)), Stroke::new(5.0, col.linear_multiply(0.55))));
    for i in 0..=20 {
        let a = 180.0 + 180.0 * i as f32 / 20.0;
        let len = if i % 5 == 0 { 10.0 } else { 5.0 };
        p.line_segment([polar(c, r - 2.0, a), polar(c, r - 2.0 - len, a)], Stroke::new(1.0, Color32::from_gray(170)));
    }
    tick_labels(p, c + Vec2::new(-r + 10.0, 10.0), Align2::CENTER_CENTER, format!("{}", range[0]));
    tick_labels(p, c + Vec2::new(r - 10.0, 10.0), Align2::CENTER_CENTER, format!("{}", range[1]));
    let a = 180.0 + 180.0 * f;
    p.line_segment([c, polar(c, r - 14.0, a)], Stroke::new(2.5, Color32::from_rgb(255, 90, 80)));
    p.circle_filled(c, 6.0, Color32::from_gray(170));
    let ro = Rect::from_center_size(c + Vec2::new(0.0, 24.0), Vec2::new(86.0, 20.0));
    sunken(p, ro);
    p.text(ro.center(), Align2::CENTER_CENTER, fmt(v), FontId::monospace(12.0), col);
}

fn thermometer(p: &Painter, body: Rect, v: f64, range: [f64; 2]) {
    let cx = body.left() + 62.0;
    let bulb = Pos2::new(cx, body.bottom() - 16.0);
    let tube = Rect::from_min_max(Pos2::new(cx - 7.0, body.top() + 6.0), Pos2::new(cx + 7.0, bulb.y));
    let fill = Color32::from_rgb(235, 70, 60);
    p.rect_filled(tube, 7.0, theme::SUNKEN);
    p.rect_stroke(tube, 7.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Inside);
    p.circle_filled(bulb, 15.0, Color32::from_gray(90));
    p.circle_filled(bulb, 13.5, fill);
    let f = frac(v, range);
    let top = tube.bottom() - (tube.height()) * f;
    p.rect_filled(Rect::from_min_max(Pos2::new(tube.left() + 2.5, top), Pos2::new(tube.right() - 2.5, tube.bottom())), 0.0, fill);
    for i in 0..=10 {
        let y = tube.bottom() - tube.height() * i as f32 / 10.0;
        p.line_segment([Pos2::new(tube.left() - 4.0, y), Pos2::new(tube.left() - if i % 5 == 0 { 12.0 } else { 8.0 }, y)], Stroke::new(1.0, Color32::from_gray(150)));
    }
    tick_labels(p, Pos2::new(tube.left() - 15.0, tube.top() + 3.0), Align2::RIGHT_CENTER, format!("{}", range[1]));
    tick_labels(p, Pos2::new(tube.left() - 15.0, tube.bottom() - 3.0), Align2::RIGHT_CENTER, format!("{}", range[0]));
    p.text(bulb, Align2::CENTER_CENTER, format!("{v:.1}"), FontId::proportional(9.0), Color32::WHITE);
}

fn tank(p: &Painter, body: Rect, v: f64, range: [f64; 2]) {
    let t = Rect::from_min_max(Pos2::new(body.left() + 38.0, body.top() + 4.0), Pos2::new(body.right() - 12.0, body.bottom() - 4.0));
    p.rect_filled(t, 3.0, theme::SUNKEN);
    let f = frac(v, range);
    let level = Rect::from_min_max(Pos2::new(t.left() + 2.0, t.bottom() - (t.height() - 4.0) * f - 2.0), t.max - Vec2::splat(2.0));
    p.rect_filled(level, 1.0, Color32::from_rgb(40, 130, 220));
    p.line_segment([level.left_top(), level.right_top()], Stroke::new(2.0, Color32::from_rgb(120, 200, 255)));
    p.rect_stroke(t, 3.0, Stroke::new(1.5, Color32::from_gray(100)), StrokeKind::Inside);
    for i in 0..=10 {
        let y = t.bottom() - t.height() * i as f32 / 10.0;
        p.line_segment([Pos2::new(t.left() - 3.0, y), Pos2::new(t.left() - if i % 5 == 0 { 11.0 } else { 7.0 }, y)], Stroke::new(1.0, Color32::from_gray(150)));
    }
    tick_labels(p, Pos2::new(t.left() - 13.0, t.top() + 4.0), Align2::RIGHT_CENTER, format!("{}", range[1]));
    tick_labels(p, Pos2::new(t.left() - 13.0, t.bottom() - 4.0), Align2::RIGHT_CENTER, format!("{}", range[0]));
    p.text(t.center(), Align2::CENTER_CENTER, format!("{v:.2}"), FontId::proportional(11.0), Color32::WHITE);
}

fn toggle(ui: &mut Ui, p: &Painter, id: egui::Id, body: Rect, on: bool) -> Option<Value> {
    let r = Rect::from_center_size(body.center() - Vec2::new(0.0, 6.0), Vec2::new(60.0, 28.0));
    p.rect_filled(r, 14.0, if on { Color32::from_rgb(30, 130, 70) } else { Color32::from_gray(38) });
    p.rect_stroke(r, 14.0, Stroke::new(1.5, Color32::from_gray(100)), StrokeKind::Inside);
    let x = if on { r.right() - 14.0 } else { r.left() + 14.0 };
    p.circle_filled(Pos2::new(x, r.center().y), 11.0, Color32::from_gray(205));
    p.circle_filled(Pos2::new(x - 2.0, r.center().y - 2.0), 5.0, Color32::from_white_alpha(70));
    p.text(Pos2::new(r.center().x, r.bottom() + 9.0), Align2::CENTER_CENTER, if on { "ON" } else { "OFF" }, FontId::monospace(10.0), theme::type_color(&DataType::Bool));
    ui.interact(r, id, Sense::click()).clicked().then(|| Value::Bool(!on))
}

fn push_button(ui: &mut Ui, p: &Painter, id: egui::Id, body: Rect, on: bool, name: &str) -> Option<Value> {
    let r = Rect::from_center_size(body.center(), Vec2::new(96.0, 38.0));
    let top = if on { Color32::from_rgb(30, 140, 90) } else { Color32::from_gray(92) };
    p.rect_filled(r.translate(Vec2::new(0.0, 2.0)), 6.0, Color32::from_gray(20));
    p.rect_filled(r, 6.0, top);
    p.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width(), r.height() / 2.0)).shrink(1.0), 5.0, Color32::from_white_alpha(14));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, Color32::from_gray(150)), StrokeKind::Inside);
    p.text(r.center(), Align2::CENTER_CENTER, name, FontId::proportional(13.0), Color32::WHITE);
    ui.interact(r, id, Sense::click()).clicked().then(|| Value::Bool(!on))
}

fn led(p: &Painter, body: Rect, on: bool) {
    let c = body.center() - Vec2::new(0.0, 2.0);
    p.circle_filled(c, 26.0, Color32::from_gray(34));
    p.circle_stroke(c, 25.0, Stroke::new(2.0, Color32::from_gray(110)));
    let (base, hi) = if on { (Color32::from_rgb(40, 225, 90), Color32::from_rgb(190, 255, 200)) } else { (Color32::from_rgb(12, 62, 28), Color32::from_rgb(40, 100, 56)) };
    p.circle_filled(c, 21.0, base);
    p.circle_filled(c + Vec2::new(-6.0, -7.0), 7.0, hi.linear_multiply(0.7));
    if on {
        p.circle_stroke(c, 24.0, Stroke::new(3.0, Color32::from_rgba_unmultiplied(60, 255, 110, 60)));
    }
}

fn str_ctl(ui: &mut Ui, p: &Painter, body: Rect, value: &Value) -> Option<Value> {
    let r = Rect::from_min_size(body.min, Vec2::new(body.width(), 26.0));
    sunken(p, r);
    let mut s = if let Value::Str(s) = value { s.clone() } else { String::new() };
    let changed = ui
        .scope_builder(UiBuilder::new().max_rect(r.shrink(2.0)), |ui| {
            let vis = ui.visuals_mut();
            vis.widgets.inactive.bg_fill = Color32::TRANSPARENT;
            vis.extreme_bg_color = Color32::TRANSPARENT;
            ui.add(egui::TextEdit::singleline(&mut s).frame(egui::Frame::NONE).desired_width(r.width() - 8.0).text_color(theme::type_color(&DataType::Str))).changed()
        })
        .inner;
    changed.then(|| Value::Str(s))
}

fn plot(p: &Painter, body: Rect, data: &[f64], chart: bool) {
    let plot = Rect::from_min_max(body.min + Vec2::new(46.0, 4.0), body.max - Vec2::new(8.0, 22.0));
    p.rect_filled(body, 3.0, Color32::from_rgb(20, 22, 27));
    p.rect_filled(plot, 0.0, Color32::from_rgb(10, 11, 14));
    let (mut lo, mut hi) = data.iter().fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    if data.is_empty() {
        (lo, hi) = (0.0, 1.0);
    } else if (hi - lo).abs() < 1e-9 {
        (lo, hi) = (lo - 1.0, hi + 1.0);
    } else {
        let pad = (hi - lo) * 0.05;
        (lo, hi) = (lo - pad, hi + pad);
    }
    let grid = Stroke::new(1.0, Color32::from_rgb(40, 44, 54));
    for i in 0..=4 {
        let f = i as f32 / 4.0;
        let y = plot.bottom() - plot.height() * f;
        let x = plot.left() + plot.width() * f;
        p.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], grid);
        p.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], grid);
        tick_labels(p, Pos2::new(plot.left() - 4.0, y), Align2::RIGHT_CENTER, format!("{:.2}", lo + (hi - lo) * f as f64));
        let n = data.len().max(1);
        tick_labels(p, Pos2::new(x, plot.bottom() + 9.0), Align2::CENTER_CENTER, format!("{}", ((n - 1) as f32 * f).round() as usize));
    }
    p.rect_stroke(plot, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Inside);
    let line_col = Color32::from_rgb(90, 240, 170);
    if data.is_empty() {
        p.text(plot.center(), Align2::CENTER_CENTER, if chart { "waiting for data…" } else { "no data" }, FontId::proportional(11.0), theme::TEXT_DIM);
    } else {
        let step = ((data.len() as f32 / plot.width().max(1.0)).ceil() as usize).max(1);
        let n = data.len();
        let pt = |i: usize| {
            let x = if n == 1 { plot.left() } else { plot.left() + plot.width() * i as f32 / (n - 1) as f32 };
            Pos2::new(x, plot.bottom() - plot.height() * ((data[i] - lo) / (hi - lo)) as f32)
        };
        let mut pts: Vec<Pos2> = (0..n).step_by(step).map(pt).collect();
        if (n - 1) % step != 0 {
            pts.push(pt(n - 1));
        }
        let clipped = p.with_clip_rect(plot);
        if pts.len() == 1 {
            clipped.circle_filled(pts[0], 2.5, line_col);
        } else {
            clipped.add(Shape::line(pts, Stroke::new(1.6, line_col)));
        }
    }
    let lg = Rect::from_min_size(Pos2::new(plot.right() - 62.0, plot.top() + 4.0), Vec2::new(58.0, 14.0));
    p.rect_filled(lg, 2.0, Color32::from_black_alpha(140));
    p.line_segment([lg.left_center() + Vec2::new(4.0, 0.0), lg.left_center() + Vec2::new(16.0, 0.0)], Stroke::new(2.0, line_col));
    p.text(lg.left_center() + Vec2::new(20.0, 0.0), Align2::LEFT_CENTER, "Plot 0", FontId::proportional(9.5), Color32::from_gray(200));
}
