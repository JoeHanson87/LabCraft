//! Graph and chart rendering: waveform/multi-plot lines, XY, intensity (heat map), legends,
//! autoscaling or fixed axes, and draggable cursors.

use crate::theme;
use eframe::egui::{self, epaint::Shape, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use labcraft_core::{DataType, GraphConfig, Node, Value, Widget};

pub const PLOT_COLORS: [Color32; 8] = [
    Color32::from_rgb(90, 240, 170),
    Color32::from_rgb(255, 170, 60),
    Color32::from_rgb(110, 170, 255),
    Color32::from_rgb(240, 100, 190),
    Color32::from_rgb(240, 225, 90),
    Color32::from_rgb(170, 140, 255),
    Color32::from_rgb(255, 110, 100),
    Color32::from_rgb(150, 220, 255),
];

pub struct Series {
    pub xs: Option<Vec<f64>>,
    pub ys: Vec<f64>,
}

fn nums(v: &Value) -> Vec<f64> {
    match v {
        Value::Array(a) => a.data.iter().filter_map(|x| x.as_f64()).collect(),
        _ => vec![],
    }
}

/// The plots a graph/chart widget shows for a value of the node's type.
pub fn series_for(widget: Widget, ty: &DataType, v: &Value) -> Vec<Series> {
    match (widget, v) {
        (Widget::XyGraph, Value::Cluster(c)) if c.len() == 2 => vec![Series { xs: Some(nums(&c[0])), ys: nums(&c[1]) }],
        (Widget::Graph, Value::Cluster(c)) if ty.is_waveform() && c.len() == 3 => {
            let (t0, dt) = (c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(1.0));
            let ys = nums(&c[2]);
            let xs = (0..ys.len()).map(|i| t0 + dt * i as f64).collect();
            vec![Series { xs: Some(xs), ys }]
        }
        (Widget::Graph, Value::Array(a)) if a.dims.len() == 2 => {
            // each row is a plot
            let cols = a.dims[1].max(1);
            a.data.chunks(cols).map(|row| Series { xs: None, ys: row.iter().filter_map(|x| x.as_f64()).collect() }).collect()
        }
        (Widget::Chart, Value::Array(a)) if a.dims.len() == 2 => {
            // each column is a plot; one row per sample
            let (rows, cols) = (a.dims[0], a.dims[1].max(1));
            (0..cols).map(|c| Series { xs: None, ys: (0..rows).filter_map(|r| a.data.get(r * cols + c).and_then(|x| x.as_f64())).collect() }).collect()
        }
        (_, Value::Array(_)) => vec![Series { xs: None, ys: nums(v) }],
        _ => vec![],
    }
}

pub fn plot_count(widget: Widget, ty: &DataType, v: &Value) -> usize {
    series_for(widget, ty, v).len()
}

fn fmt_tick(v: f64) -> String {
    let a = v.abs();
    if a != 0.0 && !(0.01..10000.0).contains(&a) { format!("{v:.1e}") } else if a >= 100.0 { format!("{v:.0}") } else { format!("{v:.2}") }
}

fn label(p: &Painter, pos: Pos2, align: Align2, text: String) {
    p.text(pos, align, text, FontId::proportional(9.5), theme::TEXT_DIM);
}

fn data_range(vals: impl Iterator<Item = f64>) -> Option<[f64; 2]> {
    let (lo, hi) = vals.filter(|v| v.is_finite()).fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v), h.max(v)));
    (lo <= hi).then_some([lo, hi])
}

fn pad(r: [f64; 2], frac: f64) -> [f64; 2] {
    if (r[1] - r[0]).abs() < 1e-12 {
        [r[0] - 1.0, r[1] + 1.0]
    } else {
        let p = (r[1] - r[0]) * frac;
        [r[0] - p, r[1] + p]
    }
}

/// Linear interpolation of a series at `x` (for cursor readouts).
fn value_at(s: &Series, x: f64) -> Option<f64> {
    let n = s.ys.len();
    if n == 0 {
        return None;
    }
    let xs: Vec<f64> = s.xs.clone().unwrap_or_else(|| (0..n).map(|i| i as f64).collect());
    let n = n.min(xs.len());
    (0..n.saturating_sub(1))
        .find(|&i| (xs[i] <= x && x <= xs[i + 1]) || (xs[i] >= x && x >= xs[i + 1]))
        .map(|i| {
            let span = xs[i + 1] - xs[i];
            if span == 0.0 { s.ys[i] } else { s.ys[i] + (s.ys[i + 1] - s.ys[i]) * (x - xs[i]) / span }
        })
        .or_else(|| (n == 1).then(|| s.ys[0]))
}

/// Draw a line graph / chart / XY graph. Returns the updated config if a cursor was dragged.
pub fn draw_lines(ui: &mut Ui, p: &Painter, body: Rect, n: &Node, series: &[Series], chart: bool) -> Option<GraphConfig> {
    let cfg = &n.graph;
    let plot = Rect::from_min_max(body.min + Vec2::new(46.0, 4.0), body.max - Vec2::new(8.0, 24.0));
    p.rect_filled(body, 3.0, Color32::from_rgb(20, 22, 27));
    p.rect_filled(plot, 0.0, Color32::from_rgb(10, 11, 14));

    let all_x = series.iter().flat_map(|s| match &s.xs {
        Some(xs) => xs.clone(),
        None => (0..s.ys.len()).map(|i| i as f64).collect(),
    });
    let x_auto = data_range(all_x).map(|r| if r[0] == r[1] { [r[0], r[0] + 1.0] } else { r }).unwrap_or([0.0, 1.0]);
    let y_auto = data_range(series.iter().flat_map(|s| s.ys.iter().copied())).map(|r| pad(r, 0.05)).unwrap_or([0.0, 1.0]);
    let xr = if cfg.autoscale_x { x_auto } else { cfg.x_range };
    let yr = if cfg.autoscale_y { y_auto } else { cfg.y_range };
    let (xspan, yspan) = ((xr[1] - xr[0]).max(1e-12), (yr[1] - yr[0]).max(1e-12));

    let grid = Stroke::new(1.0, Color32::from_rgb(40, 44, 54));
    for i in 0..=4 {
        let f = i as f32 / 4.0;
        let (y, x) = (plot.bottom() - plot.height() * f, plot.left() + plot.width() * f);
        p.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], grid);
        p.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], grid);
        label(p, Pos2::new(plot.left() - 4.0, y), Align2::RIGHT_CENTER, fmt_tick(yr[0] + yspan * f as f64));
        label(p, Pos2::new(x, plot.bottom() + 9.0), Align2::CENTER_CENTER, fmt_tick(xr[0] + xspan * f as f64));
    }
    p.rect_stroke(plot, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Inside);
    if !cfg.x_label.is_empty() {
        label(p, Pos2::new(plot.center().x, body.bottom() - 4.0), Align2::CENTER_CENTER, cfg.x_label.clone());
    }
    if !cfg.y_label.is_empty() {
        label(p, body.left_top() + Vec2::new(4.0, 8.0), Align2::LEFT_CENTER, cfg.y_label.clone());
    }

    let clipped = p.with_clip_rect(plot);
    let to_screen = |x: f64, y: f64| Pos2::new(plot.left() + plot.width() * ((x - xr[0]) / xspan) as f32, plot.bottom() - plot.height() * ((y - yr[0]) / yspan) as f32);
    if series.iter().all(|s| s.ys.is_empty()) {
        p.text(plot.center(), Align2::CENTER_CENTER, if chart { "waiting for data…" } else { "no data" }, FontId::proportional(11.0), theme::TEXT_DIM);
    }
    for (k, s) in series.iter().enumerate() {
        let n_pts = s.ys.len();
        if n_pts == 0 {
            continue;
        }
        let col = PLOT_COLORS[k % PLOT_COLORS.len()];
        let step = ((n_pts as f32 / plot.width().max(1.0)).ceil() as usize).max(1);
        let x_at = |i: usize| s.xs.as_ref().and_then(|xs| xs.get(i).copied()).unwrap_or(i as f64);
        let mut pts: Vec<Pos2> = (0..n_pts).step_by(step).map(|i| to_screen(x_at(i), s.ys[i])).collect();
        if (n_pts - 1) % step != 0 {
            pts.push(to_screen(x_at(n_pts - 1), s.ys[n_pts - 1]));
        }
        if pts.len() == 1 {
            clipped.circle_filled(pts[0], 2.5, col);
        } else {
            clipped.add(Shape::line(pts, Stroke::new(1.6, col)));
        }
    }

    if cfg.show_legend && !series.is_empty() {
        let rows = series.len().min(6);
        let lg = Rect::from_min_size(Pos2::new(plot.right() - 92.0, plot.top() + 4.0), Vec2::new(88.0, 4.0 + 13.0 * rows as f32));
        p.rect_filled(lg, 2.0, Color32::from_black_alpha(150));
        for k in 0..rows {
            let y = lg.top() + 4.0 + 13.0 * k as f32 + 6.0;
            let name = cfg.plot_names.get(k).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| format!("Plot {k}"));
            p.line_segment([Pos2::new(lg.left() + 5.0, y), Pos2::new(lg.left() + 17.0, y)], Stroke::new(2.0, PLOT_COLORS[k % PLOT_COLORS.len()]));
            p.text(Pos2::new(lg.left() + 21.0, y), Align2::LEFT_CENTER, name, FontId::proportional(9.5), Color32::from_gray(205));
        }
    }

    // Cursors: draggable vertical lines with a readout of each plot's value at that X.
    let mut updated = None;
    for (i, cx) in cfg.cursors.iter().enumerate() {
        let x = plot.left() + plot.width() * ((cx - xr[0]) / xspan) as f32;
        if x < plot.left() || x > plot.right() {
            continue;
        }
        let col = Color32::from_rgb(255, 230, 90);
        for s in Shape::dashed_line(&[Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.2, col), 4.0, 3.0) {
            p.add(s);
        }
        let handle = Rect::from_min_max(Pos2::new(x - 5.0, plot.top()), Pos2::new(x + 5.0, plot.bottom()));
        let r = ui.interact(handle, ui.scope_id().with(("cursor", n.id, i)), Sense::drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if let (true, Some(pt)) = (r.dragged(), r.interact_pointer_pos()) {
            let mut c = cfg.clone();
            c.cursors[i] = xr[0] + xspan * ((pt.x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64;
            updated = Some(c);
        }
        let mut text = format!("C{i}  x={}", fmt_tick(*cx));
        for (k, s) in series.iter().enumerate().take(4) {
            if let Some(y) = value_at(s, *cx) {
                text.push_str(&format!("  y{k}={}", fmt_tick(y)));
            }
        }
        let ty_row = plot.top() + 6.0 + 12.0 * i as f32;
        p.text(Pos2::new(plot.left() + 4.0, ty_row), Align2::LEFT_CENTER, text, FontId::monospace(9.5), col);
    }
    updated
}

/// Jet-style colour map for t in 0..=1.
pub fn colormap(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let stops = [(0.0, [20.0, 30.0, 160.0]), (0.25, [30.0, 150.0, 230.0]), (0.5, [60.0, 210.0, 90.0]), (0.75, [245.0, 225.0, 60.0]), (1.0, [230.0, 50.0, 40.0])];
    for w in stops.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t1 {
            let f = (t - t0) / (t1 - t0);
            return Color32::from_rgb((c0[0] + (c1[0] - c0[0]) * f) as u8, (c0[1] + (c1[1] - c0[1]) * f) as u8, (c0[2] + (c1[2] - c0[2]) * f) as u8);
        }
    }
    Color32::WHITE
}

/// Draw an intensity graph (2D array shown as a heat map, row 0 at the bottom).
pub fn draw_intensity(p: &Painter, body: Rect, n: &Node, v: &Value) {
    let cfg = &n.graph;
    let plot = Rect::from_min_max(body.min + Vec2::new(38.0, 4.0), body.max - Vec2::new(50.0, 22.0));
    p.rect_filled(body, 3.0, Color32::from_rgb(20, 22, 27));
    p.rect_filled(plot, 0.0, Color32::from_rgb(10, 11, 14));
    let Value::Array(a) = v else { return };
    if a.dims.len() != 2 || a.data.is_empty() {
        p.text(plot.center(), Align2::CENTER_CENTER, "no data", FontId::proportional(11.0), theme::TEXT_DIM);
        return;
    }
    let (rows, cols) = (a.dims[0], a.dims[1]);
    let zs: Vec<f64> = a.data.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
    let zr = if cfg.autoscale_z { pad(data_range(zs.iter().copied()).unwrap_or([0.0, 1.0]), 0.0) } else { cfg.z_range };
    let zspan = (zr[1] - zr[0]).max(1e-12);
    // draw at most ~160 cells per axis
    let (rs, cs) = ((rows / 160).max(1), (cols / 160).max(1));
    let (cw, ch) = (plot.width() / cols as f32, plot.height() / rows as f32);
    let clipped = p.with_clip_rect(plot);
    for r in (0..rows).step_by(rs) {
        for c in (0..cols).step_by(cs) {
            let t = ((zs[r * cols + c] - zr[0]) / zspan) as f32;
            let rect = Rect::from_min_size(Pos2::new(plot.left() + cw * c as f32, plot.bottom() - ch * (r + rs) as f32), Vec2::new(cw * cs as f32 + 0.5, ch * rs as f32 + 0.5));
            clipped.rect_filled(rect, 0.0, colormap(t));
        }
    }
    p.rect_stroke(plot, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Inside);
    for (i, f) in [0.0f32, 0.5, 1.0].into_iter().enumerate() {
        label(p, Pos2::new(plot.left() - 4.0, plot.bottom() - plot.height() * f), Align2::RIGHT_CENTER, format!("{:.0}", rows as f32 * f));
        label(p, Pos2::new(plot.left() + plot.width() * f, plot.bottom() + 9.0), Align2::CENTER_CENTER, format!("{:.0}", cols as f32 * f));
        let _ = i;
    }
    // colour scale bar
    let bar = Rect::from_min_max(Pos2::new(plot.right() + 10.0, plot.top()), Pos2::new(plot.right() + 22.0, plot.bottom()));
    let steps = 40;
    for s in 0..steps {
        let t0 = s as f32 / steps as f32;
        let seg = Rect::from_min_max(Pos2::new(bar.left(), bar.bottom() - bar.height() * (t0 + 1.0 / steps as f32)), Pos2::new(bar.right(), bar.bottom() - bar.height() * t0));
        p.rect_filled(seg, 0.0, colormap(t0 + 0.5 / steps as f32));
    }
    p.rect_stroke(bar, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Inside);
    label(p, Pos2::new(bar.right() + 3.0, bar.top() + 4.0), Align2::LEFT_CENTER, fmt_tick(zr[1]));
    label(p, Pos2::new(bar.right() + 3.0, bar.bottom() - 4.0), Align2::LEFT_CENTER, fmt_tick(zr[0]));
}

/// Context-menu editor for graph settings; returns the new config when something changed.
pub fn config_menu(ui: &mut Ui, n: &Node, plots: usize) -> Option<GraphConfig> {
    let mut c = n.graph.clone();
    let before = c.clone();
    let intensity = n.widget == Widget::IntensityGraph;
    let range_row = |ui: &mut Ui, name: &str, auto: &mut bool, r: &mut [f64; 2]| {
        ui.checkbox(auto, format!("Autoscale {name}"));
        if !*auto {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut r[0]).speed(0.1).prefix("min "));
                ui.add(egui::DragValue::new(&mut r[1]).speed(0.1).prefix("max "));
            });
        }
    };
    if intensity {
        range_row(ui, "color scale", &mut c.autoscale_z, &mut c.z_range);
    } else {
        range_row(ui, "X", &mut c.autoscale_x, &mut c.x_range);
        range_row(ui, "Y", &mut c.autoscale_y, &mut c.y_range);
        ui.checkbox(&mut c.show_legend, "Show legend");
        ui.horizontal(|ui| {
            ui.label("X label");
            ui.add(egui::TextEdit::singleline(&mut c.x_label).desired_width(90.0));
        });
        ui.horizontal(|ui| {
            ui.label("Y label");
            ui.add(egui::TextEdit::singleline(&mut c.y_label).desired_width(90.0));
        });
        if plots > 0 {
            ui.menu_button("Plot names", |ui| {
                if c.plot_names.len() < plots {
                    c.plot_names.resize(plots, String::new());
                }
                for k in 0..plots.min(8) {
                    ui.horizontal(|ui| {
                        ui.colored_label(PLOT_COLORS[k % PLOT_COLORS.len()], "■");
                        ui.add(egui::TextEdit::singleline(&mut c.plot_names[k]).hint_text(format!("Plot {k}")).desired_width(90.0));
                    });
                }
            });
        }
        if n.widget != Widget::Chart {
            ui.horizontal(|ui| {
                if ui.button("Add cursor").clicked() {
                    let mid = if c.autoscale_x { c.cursors.last().map(|x| x + 1.0).unwrap_or(0.0) } else { (c.x_range[0] + c.x_range[1]) / 2.0 };
                    c.cursors.push(mid);
                }
                if ui.add_enabled(!c.cursors.is_empty(), egui::Button::new("Clear cursors")).clicked() {
                    c.cursors.clear();
                }
            });
        }
    }
    (c != before).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use labcraft_core::ArrayVal;

    #[test]
    fn waveform_xy_and_multi_plot_series() {
        let wf = Value::Cluster(vec![Value::Dbl(1.0), Value::Dbl(0.5), Value::from_f64_vec(&[10.0, 20.0, 30.0])]);
        let s = series_for(Widget::Graph, &DataType::waveform(), &wf);
        assert_eq!(s[0].xs.as_ref().unwrap(), &vec![1.0, 1.5, 2.0]);
        let xy = Value::Cluster(vec![Value::from_f64_vec(&[1.0, 2.0]), Value::from_f64_vec(&[5.0, 6.0])]);
        let s = series_for(Widget::XyGraph, &DataType::Bool, &xy);
        assert_eq!((s[0].xs.clone().unwrap(), s[0].ys.clone()), (vec![1.0, 2.0], vec![5.0, 6.0]));
        // 2D graph: each row is a plot
        let two_d = Value::Array(ArrayVal { dims: vec![2, 3], data: (0..6).map(|i| Value::Dbl(i as f64)).collect() });
        let s = series_for(Widget::Graph, &DataType::Bool, &two_d);
        assert_eq!((s.len(), s[1].ys.clone()), (2, vec![3.0, 4.0, 5.0]));
    }

    #[test]
    fn cursor_readout_interpolates() {
        let s = Series { xs: Some(vec![0.0, 10.0]), ys: vec![0.0, 100.0] };
        assert_eq!(value_at(&s, 2.5), Some(25.0));
        assert_eq!(value_at(&s, 50.0), None);
    }

    #[test]
    fn colormap_runs_blue_to_red() {
        let (lo, hi) = (colormap(0.0), colormap(1.0));
        assert!(lo.b() > lo.r() && hi.r() > hi.b());
        assert_eq!(colormap(-3.0), lo);
    }
}
