use crate::app::LabCraft;
use crate::graph;
use crate::theme;
use crate::valedit;
use eframe::egui::{self, epaint::Shape, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use labcraft_core::{Command, DataType, Node, NodeId, NodeKind, Value, Widget};

const SWAPPABLE: [Widget; 23] = [
    Widget::NumCtl, Widget::Knob, Widget::Slide, Widget::NumInd, Widget::Gauge, Widget::Thermometer, Widget::Tank,
    Widget::Toggle, Widget::PushButton, Widget::Led, Widget::StrCtl, Widget::StrInd, Widget::EnumCtl, Widget::EnumInd,
    Widget::Graph, Widget::XyGraph, Widget::IntensityGraph, Widget::Table, Widget::ListBox,
    Widget::ArrayInd, Widget::ArrayCtl, Widget::ClusterCtl, Widget::ClusterInd,
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

/// Plain click selects one item; Shift/Cmd-click adds or removes it from the selection.
fn select_click(app: &mut LabCraft, ui: &Ui, id: NodeId) {
    let additive = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    if additive {
        if !app.panel_sel.remove(&id) {
            app.panel_sel.insert(id);
        }
    } else if !app.panel_sel.contains(&id) {
        app.panel_sel.clear();
        app.panel_sel.insert(id);
    }
    app.selected = Some(id);
}

/// Shared drag handling for panel items and decorations: moves the whole selection together.
fn item_drag(app: &mut LabCraft, ui: &Ui, n: &Node, g: &egui::Response) {
    if g.clicked() || g.drag_started() {
        select_click(app, ui, n.id);
    }
    if g.dragged() {
        let d = g.drag_delta();
        let ids: Vec<NodeId> = if app.panel_sel.contains(&n.id) { app.panel_sel.iter().copied().collect() } else { vec![n.id] };
        let moves: Vec<(NodeId, [f32; 2])> = ids
            .iter()
            .filter_map(|id| app.ed.vi.node(*id))
            .map(|m| (m.id, [m.panel_pos[0] + d.x, m.panel_pos[1] + d.y]))
            .collect();
        app.pending.push(Command::MovePanelItems { moves });
    }
    if g.drag_stopped() {
        app.pending.push(Command::EndGesture);
        if app.snap_grid {
            let ids: Vec<NodeId> = if app.panel_sel.contains(&n.id) { app.panel_sel.iter().copied().collect() } else { vec![n.id] };
            app.pending.push(Command::SnapPanel { ids, grid: 10.0 });
        }
    }
}

fn resize_handle(app: &mut LabCraft, ui: &mut Ui, painter: &Painter, n: &Node, r: Rect) {
    let handle = Rect::from_min_size(r.max - Vec2::splat(14.0), Vec2::splat(14.0));
    painter.text(handle.center(), Align2::CENTER_CENTER, "◢", FontId::proportional(12.0), theme::ACCENT);
    let rz = ui.interact(handle, ui.scope_id().with(("presize", n.id)), Sense::drag());
    if rz.hovered() || rz.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
    }
    if rz.dragged() {
        let d = rz.drag_delta();
        let s = n.panel_size();
        app.pending.push(Command::ResizePanelItem { id: n.id, size: [s[0] + d.x, s[1] + d.y] });
    }
    if rz.drag_stopped() {
        app.pending.push(Command::EndGesture);
    }
}

fn draw_decoration(app: &mut LabCraft, ui: &mut Ui, painter: &Painter, origin: Pos2, n: &Node) {
    let NodeKind::Decoration { deco, text, color } = &n.kind else { return };
    let col = Color32::from_rgb(color[0], color[1], color[2]);
    let r = Rect::from_min_size(origin + v2(n.panel_pos), v2(n.panel_size()));
    let selected = app.panel_sel.contains(&n.id);
    match deco {
        labcraft_core::DecoKind::Box => {
            painter.rect_filled(r, 6.0, col.linear_multiply(0.12));
            painter.rect_stroke(r, 6.0, Stroke::new(1.5, col.linear_multiply(0.8)), StrokeKind::Inside);
            if !text.is_empty() {
                painter.text(r.left_top() + Vec2::new(12.0, 8.0), Align2::LEFT_TOP, text, FontId::proportional(12.0), col);
            }
        }
        labcraft_core::DecoKind::Line => {
            let (a, b) = if r.width() >= r.height() { (r.left_center(), r.right_center()) } else { (r.center_top(), r.center_bottom()) };
            painter.line_segment([a, b], Stroke::new(2.5, col));
        }
        labcraft_core::DecoKind::Label => {
            painter.text(r.left_center() + Vec2::new(2.0, 0.0), Align2::LEFT_CENTER, if text.is_empty() { "Label" } else { text }, FontId::proportional(15.0), col);
        }
    }
    if selected {
        painter.rect_stroke(r.expand(3.0), 3.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Outside);
    }
    let g = ui.interact(r, ui.scope_id().with(("deco", n.id)), Sense::click_and_drag());
    item_drag(app, ui, n, &g);
    g.context_menu(|ui| {
        let mut t = text.clone();
        ui.horizontal(|ui| {
            ui.label("Text");
            if ui.add(egui::TextEdit::singleline(&mut t).desired_width(120.0)).changed() {
                app.pending.push(Command::SetDecoration { id: n.id, text: Some(t.clone()), color: None });
            }
        });
        let mut c = *color;
        if ui.color_edit_button_srgb(&mut c).changed() {
            app.pending.push(Command::SetDecoration { id: n.id, text: None, color: Some(c) });
        }
        if ui.button("Send to Back").clicked() {
            app.pending.push(Command::ArrangePanel { ids: vec![n.id], op: labcraft_core::Arrange::SendToBack });
            ui.close();
        }
        if ui.button("Delete").clicked() {
            app.pending.push(Command::RemoveNode(n.id));
            ui.close();
        }
    });
    if app.selected == Some(n.id) && !matches!(deco, labcraft_core::DecoKind::Label) {
        resize_handle(app, ui, painter, n, r);
    }
}

pub fn show(app: &mut LabCraft, ui: &mut Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let origin = resp.rect.min;
    painter.rect_filled(resp.rect, 0.0, theme::BG_PANEL);
    let step = if app.snap_grid { 10.0 } else { 40.0 };
    let alpha = if app.snap_grid { 6 } else { 4 };
    let mut gx = resp.rect.left();
    while gx < resp.rect.right() {
        painter.line_segment([Pos2::new(gx, resp.rect.top()), Pos2::new(gx, resp.rect.bottom())], Stroke::new(1.0, Color32::from_white_alpha(alpha)));
        gx += step;
    }
    let mut gy = resp.rect.top();
    while gy < resp.rect.bottom() {
        painter.line_segment([Pos2::new(resp.rect.left(), gy), Pos2::new(resp.rect.right(), gy)], Stroke::new(1.0, Color32::from_white_alpha(alpha)));
        gy += step;
    }

    let running = app.is_running();
    let nodes = app.ed.vi.nodes.clone();
    let pointer = ui.input(|i| i.pointer.interact_pos());

    // Rubber-band selection on empty canvas.
    if resp.drag_started() {
        app.panel_band = pointer;
    }
    if let (Some(a), Some(b)) = (app.panel_band, pointer) {
        let band = Rect::from_two_pos(a, b);
        painter.rect_filled(band, 0.0, theme::ACCENT.linear_multiply(0.12));
        painter.rect_stroke(band, 0.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Inside);
    }
    if resp.drag_stopped() {
        if let (Some(a), Some(b)) = (app.panel_band.take(), pointer) {
            let band = Rect::from_two_pos(a, b);
            if !ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                app.panel_sel.clear();
            }
            for n in nodes.iter().filter(|n| n.kind.is_panel_item() || n.kind.is_decoration()) {
                if band.intersects(Rect::from_min_size(origin + v2(n.panel_pos), v2(n.panel_size()))) {
                    app.panel_sel.insert(n.id);
                }
            }
            app.selected = app.panel_sel.iter().next().copied();
        }
    }
    if resp.clicked() {
        app.selected = None;
        app.panel_sel.clear();
    }

    // Decorations sit behind controls.
    let ordered: Vec<&Node> = nodes.iter().filter(|n| n.kind.is_decoration()).chain(nodes.iter().filter(|n| n.kind.is_panel_item())).collect();
    let mut any = false;
    for n in ordered {
        any = true;
        if running && !n.visible {
            continue;
        }
        if n.kind.is_decoration() {
            draw_decoration(app, ui, &painter, origin, n);
            continue;
        }
        let (name, ty, is_ctl) = match &n.kind {
            NodeKind::Control { name, ty } => (name, ty.clone(), true),
            NodeKind::Indicator { name, ty } => (name, ty.clone(), false),
            _ => continue,
        };
        // Items hidden by a property show dimmed while editing.
        let mut painter = painter.clone();
        if !n.visible {
            painter.set_opacity(0.3);
        }
        let col = theme::type_color(&ty);
        let size = v2(n.panel_size());
        let r = Rect::from_min_size(origin + v2(n.panel_pos), size);
        if app.panel_sel.contains(&n.id) {
            painter.rect_stroke(r.expand(4.0), 3.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Outside);
        }

        let label_r = Rect::from_min_size(r.min, Vec2::new(size.x, 16.0));
        let g = ui.interact(label_r, ui.scope_id().with(("grab", n.id)), Sense::click_and_drag());
        item_drag(app, ui, n, &g);
        g.context_menu(|ui| item_menu(app, ui, n, &ty, is_ctl));
        let shown = n.caption.clone().unwrap_or_else(|| name.clone());
        painter.text(r.min + Vec2::new(2.0, 8.0), Align2::LEFT_CENTER, &shown, FontId::proportional(11.5), Color32::from_gray(190));

        let body = Rect::from_min_max(r.min + Vec2::new(0.0, 18.0), r.max);
        let id = ui.scope_id().with(("w", n.id));
        let value = n.value.clone().unwrap_or(Value::default_for(&ty));
        let range = n.range;
        let new = ui
            .scope(|ui| {
                if n.disabled {
                    ui.disable();
                }
                match n.widget {
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
                    Widget::PushButton => push_button(ui, &painter, id, body, matches!(value, Value::Bool(true)), &shown),
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
                    Widget::Chart | Widget::Graph | Widget::XyGraph => {
                        let series = graph::series_for(n.widget, &ty, &value);
                        if let Some(cfg) = graph::draw_lines(ui, &painter, body, n, &series, n.widget == Widget::Chart) {
                            app.pending.push(Command::SetGraphConfig { id: n.id, cfg });
                        }
                        None
                    }
                    Widget::IntensityGraph => {
                        graph::draw_intensity(&painter, body, n, &value);
                        None
                    }
                    Widget::Table => table_widget(ui, body, &ty, &value, is_ctl, n.id),
                    Widget::ListBox => listbox_widget(ui, body, &value, is_ctl, n.id),
                    Widget::EnumCtl | Widget::EnumInd => {
                        let r = Rect::from_min_size(body.min, Vec2::new(body.width(), 26.0));
                        let (DataType::Enum(items), Value::Int(i)) = (&ty, &value) else { return None };
                        sunken(&painter, r);
                        if is_ctl {
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
                            painter.text(r.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, items.get(*i as usize).cloned().unwrap_or_default(), FontId::proportional(13.0), col);
                            None
                        }
                    }
                    Widget::ArrayCtl | Widget::ArrayInd | Widget::ClusterCtl | Widget::ClusterInd => {
                        let frame = body;
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
                    Widget::Decoration => None,
                }
            })
            .inner;
        if n.disabled {
            painter.rect_filled(r, 3.0, Color32::from_black_alpha(90));
        }
        if let Some(v) = new {
            app.pending.push(Command::SetValue { id: n.id, value: v });
        }
        if app.selected == Some(n.id) && n.widget.resizable() {
            resize_handle(app, ui, &painter, n, r);
        }
    }
    if !any {
        painter.text(resp.rect.center(), Align2::CENTER_CENTER, "Add controls and indicators from the palette", FontId::proportional(14.0), theme::TEXT_DIM);
    }
}

fn table_widget(ui: &mut Ui, body: Rect, ty: &DataType, value: &Value, editable: bool, id: NodeId) -> Option<Value> {
    let (DataType::Array(elem, 2), Value::Array(a)) = (ty, value) else { return None };
    ui.painter().rect_filled(body, 3.0, Color32::from_rgb(20, 22, 27));
    ui.painter().rect_stroke(body, 3.0, Stroke::new(1.0, Color32::from_gray(80)), StrokeKind::Inside);
    let mut arr = a.clone();
    let mut changed = false;
    ui.scope_builder(UiBuilder::new().max_rect(body.shrink(4.0)).id_salt(("table", id)), |ui| {
        ui.set_clip_rect(body.shrink(1.0));
        if editable {
            ui.horizontal(|ui| {
                for (d, name) in ["rows", "cols"].into_iter().enumerate() {
                    let mut n = arr.dims[d];
                    if ui.add(egui::DragValue::new(&mut n).range(0..=500).prefix(format!("{name} "))).changed() {
                        valedit::resize_array(&mut arr, elem, d, n);
                        changed = true;
                    }
                }
            });
        }
        let (rows, cols) = (arr.dims[0], arr.dims[1].max(1));
        egui::ScrollArea::both().id_salt(("tscroll", id)).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new(("tgrid", id)).striped(true).spacing([6.0, 3.0]).show(ui, |ui| {
                ui.label("");
                for c in 0..cols {
                    ui.label(egui::RichText::new(format!("{c}")).size(10.0).color(theme::TEXT_DIM));
                }
                ui.end_row();
                for r in 0..rows {
                    ui.label(egui::RichText::new(format!("{r}")).size(10.0).color(theme::TEXT_DIM));
                    for c in 0..cols {
                        if let Some(cell) = arr.data.get_mut(r * cols + c) {
                            changed |= valedit::edit_value(ui, elem, cell, editable);
                        }
                    }
                    ui.end_row();
                }
            });
        });
    });
    changed.then(|| Value::Array(arr))
}

fn listbox_widget(ui: &mut Ui, body: Rect, value: &Value, editable: bool, id: NodeId) -> Option<Value> {
    let Value::Array(a) = value else { return None };
    ui.painter().rect_filled(body, 3.0, theme::SUNKEN);
    ui.painter().rect_stroke(body, 3.0, Stroke::new(1.0, Color32::from_gray(80)), StrokeKind::Inside);
    let mut arr = a.clone();
    let mut changed = false;
    ui.scope_builder(UiBuilder::new().max_rect(body.shrink(4.0)).id_salt(("list", id)), |ui| {
        ui.set_clip_rect(body.shrink(1.0));
        egui::ScrollArea::vertical().id_salt(("lscroll", id)).auto_shrink([false, false]).show(ui, |ui| {
            for (i, cell) in arr.data.iter_mut().enumerate() {
                let Value::Str(s) = cell else { continue };
                if editable {
                    changed |= ui.add(egui::TextEdit::singleline(s).desired_width(body.width() - 20.0)).changed();
                } else {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(body.width() - 14.0, 18.0), Sense::hover());
                    if i % 2 == 1 {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_white_alpha(8));
                    }
                    ui.painter().text(rect.left_center() + Vec2::new(4.0, 0.0), Align2::LEFT_CENTER, s.as_str(), FontId::proportional(12.5), theme::type_color(&DataType::Str));
                }
            }
            if editable {
                ui.horizontal(|ui| {
                    if ui.small_button("＋").clicked() {
                        arr.data.push(Value::Str(String::new()));
                        changed = true;
                    }
                    if ui.small_button("－").clicked() && arr.data.pop().is_some() {
                        changed = true;
                    }
                });
            }
        });
    });
    if changed {
        arr.dims = vec![arr.data.len()];
    }
    changed.then(|| Value::Array(arr))
}

fn item_menu(app: &mut LabCraft, ui: &mut Ui, n: &Node, ty: &DataType, is_ctl: bool) {
    let options: Vec<Widget> = SWAPPABLE.iter().copied().filter(|w| w.fits(ty, is_ctl)).collect();
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
    if matches!(n.widget, Widget::Chart | Widget::Graph | Widget::XyGraph | Widget::IntensityGraph) {
        let value = n.value.clone().unwrap_or(Value::default_for(ty));
        let plots = graph::plot_count(n.widget, ty, &value);
        ui.menu_button("Graph properties", |ui| {
            if let Some(cfg) = graph::config_menu(ui, n, plots) {
                app.pending.push(Command::SetGraphConfig { id: n.id, cfg });
            }
        });
    }
    ui.menu_button("Properties", |ui| {
        let (mut visible, mut disabled) = (n.visible, n.disabled);
        if ui.checkbox(&mut visible, "Visible").changed() {
            app.pending.push(Command::SetItemProps { id: n.id, visible: Some(visible), disabled: None, caption: None });
        }
        if ui.checkbox(&mut disabled, "Disabled").changed() {
            app.pending.push(Command::SetItemProps { id: n.id, visible: None, disabled: Some(disabled), caption: None });
        }
        let mut cap = n.caption.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Caption");
            if ui.add(egui::TextEdit::singleline(&mut cap).desired_width(100.0)).changed() {
                let value = if cap.is_empty() { None } else { Some(cap.clone()) };
                app.pending.push(Command::SetItemProps { id: n.id, visible: None, disabled: None, caption: Some(value) });
            }
        });
    });
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
    ui.menu_button("Arrange", |ui| {
        for (label, op) in [("Bring to Front", labcraft_core::Arrange::BringToFront), ("Send to Back", labcraft_core::Arrange::SendToBack)] {
            if ui.button(label).clicked() {
                app.pending.push(Command::ArrangePanel { ids: vec![n.id], op });
                ui.close();
            }
        }
    });
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
