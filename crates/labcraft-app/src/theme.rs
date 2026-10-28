use eframe::egui::{self, Color32, CornerRadius, Stroke};
use labcraft_core::DataType;

pub const BG_DIAGRAM: Color32 = Color32::from_rgb(24, 26, 31);
pub const BG_PANEL: Color32 = Color32::from_rgb(34, 37, 44);
pub const GRID: Color32 = Color32::from_rgb(40, 43, 51);
pub const ACCENT: Color32 = Color32::from_rgb(0, 190, 220);
pub const NODE_FILL: Color32 = Color32::from_rgb(52, 56, 66);
pub const NODE_STROKE: Color32 = Color32::from_rgb(110, 118, 135);
pub const SUNKEN: Color32 = Color32::from_rgb(12, 13, 16);
pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 156, 170);

/// LabVIEW-style wire colours: blue integers, orange floats, green booleans, pink strings.
pub fn type_color(t: &DataType) -> Color32 {
    match t {
        DataType::Num(k) if k.is_float() => Color32::from_rgb(255, 140, 30),
        DataType::Num(_) => Color32::from_rgb(70, 130, 255),
        DataType::Bool => Color32::from_rgb(60, 205, 95),
        DataType::Str => Color32::from_rgb(235, 105, 205),
        DataType::Enum(_) => Color32::from_rgb(60, 170, 200),
        DataType::Array(e, _) => type_color(e),
        DataType::Cluster(_) if t.is_error_cluster() => Color32::from_rgb(225, 195, 60),
        DataType::Cluster(_) => Color32::from_rgb(200, 140, 80),
    }
}

/// Wire thickness: scalars thin, arrays thick, clusters thick with a dark core.
pub fn wire_width(t: &DataType) -> f32 {
    match t {
        DataType::Array(_, n) => 3.0 + *n as f32,
        DataType::Cluster(_) => 4.5,
        _ => 2.2,
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(28, 30, 36);
    v.window_fill = Color32::from_rgb(34, 37, 44);
    v.extreme_bg_color = Color32::from_rgb(18, 19, 23);
    v.selection.bg_fill = ACCENT.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    let r = CornerRadius::same(4);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(48, 52, 62);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(62, 68, 82);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals(v);
}
