//! Front-panel configuration that does not affect dataflow: graph settings, decorations,
//! control properties and layout operations.

use serde::{Deserialize, Serialize};

/// Per-graph display settings (axes, legend, cursors).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphConfig {
    pub autoscale_x: bool,
    pub autoscale_y: bool,
    /// Colour scale of an intensity graph.
    pub autoscale_z: bool,
    pub x_range: [f64; 2],
    pub y_range: [f64; 2],
    pub z_range: [f64; 2],
    /// Cursor positions in X data coordinates.
    pub cursors: Vec<f64>,
    pub show_legend: bool,
    pub plot_names: Vec<String>,
    pub x_label: String,
    pub y_label: String,
}

impl Default for GraphConfig {
    fn default() -> Self {
        GraphConfig {
            autoscale_x: true,
            autoscale_y: true,
            autoscale_z: true,
            x_range: [0.0, 1.0],
            y_range: [0.0, 1.0],
            z_range: [0.0, 1.0],
            cursors: vec![],
            show_legend: true,
            plot_names: vec![],
            x_label: String::new(),
            y_label: String::new(),
        }
    }
}

/// Decorations are panel-only shapes and labels; they never appear on the diagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecoKind {
    Box,
    Line,
    Label,
}

impl DecoKind {
    pub fn default_size(self) -> [f32; 2] {
        match self {
            DecoKind::Box => [200.0, 120.0],
            DecoKind::Line => [180.0, 10.0],
            DecoKind::Label => [140.0, 26.0],
        }
    }
}

/// Control properties a Property Node can read or write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Prop {
    Visible,
    Disabled,
    Caption,
}

impl Prop {
    pub const ALL: [Prop; 3] = [Prop::Visible, Prop::Disabled, Prop::Caption];

    pub fn label(self) -> &'static str {
        match self {
            Prop::Visible => "Visible",
            Prop::Disabled => "Disabled",
            Prop::Caption => "Caption",
        }
    }
    pub fn ty(self) -> crate::types::DataType {
        match self {
            Prop::Visible | Prop::Disabled => crate::types::DataType::Bool,
            Prop::Caption => crate::types::DataType::Str,
        }
    }
}

/// Layout operations on a set of panel items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arrange {
    AlignLeft,
    AlignRight,
    AlignTop,
    AlignBottom,
    CenterHorizontally,
    CenterVertically,
    DistributeHorizontally,
    DistributeVertically,
    SameWidth,
    SameHeight,
    BringToFront,
    SendToBack,
}

pub type Rect = ([f32; 2], [f32; 2]);

/// New `(position, size)` for each `(position, size)` after applying `op`.
/// Z-order operations leave geometry untouched.
pub fn arrange_rects(items: &[Rect], op: Arrange) -> Vec<Rect> {
    let mut out: Vec<Rect> = items.to_vec();
    if items.is_empty() {
        return out;
    }
    let left = items.iter().map(|r| r.0[0]).fold(f32::MAX, f32::min);
    let top = items.iter().map(|r| r.0[1]).fold(f32::MAX, f32::min);
    let right = items.iter().map(|r| r.0[0] + r.1[0]).fold(f32::MIN, f32::max);
    let bottom = items.iter().map(|r| r.0[1] + r.1[1]).fold(f32::MIN, f32::max);
    match op {
        Arrange::AlignLeft => out.iter_mut().for_each(|r| r.0[0] = left),
        Arrange::AlignRight => out.iter_mut().for_each(|r| r.0[0] = right - r.1[0]),
        Arrange::AlignTop => out.iter_mut().for_each(|r| r.0[1] = top),
        Arrange::AlignBottom => out.iter_mut().for_each(|r| r.0[1] = bottom - r.1[1]),
        Arrange::CenterHorizontally => out.iter_mut().for_each(|r| r.0[0] = (left + right) / 2.0 - r.1[0] / 2.0),
        Arrange::CenterVertically => out.iter_mut().for_each(|r| r.0[1] = (top + bottom) / 2.0 - r.1[1] / 2.0),
        Arrange::DistributeHorizontally | Arrange::DistributeVertically => {
            let axis = if op == Arrange::DistributeHorizontally { 0 } else { 1 };
            if items.len() > 2 {
                let mut order: Vec<usize> = (0..items.len()).collect();
                order.sort_by(|a, b| items[*a].0[axis].total_cmp(&items[*b].0[axis]));
                let (lo, hi) = (if axis == 0 { left } else { top }, if axis == 0 { right } else { bottom });
                let total: f32 = items.iter().map(|r| r.1[axis]).sum();
                let gap = (hi - lo - total) / (items.len() - 1) as f32;
                let mut cursor = lo;
                for i in order {
                    out[i].0[axis] = cursor;
                    cursor += items[i].1[axis] + gap;
                }
            }
        }
        Arrange::SameWidth => {
            let w = items.iter().map(|r| r.1[0]).fold(0.0, f32::max);
            out.iter_mut().for_each(|r| r.1[0] = w);
        }
        Arrange::SameHeight => {
            let h = items.iter().map(|r| r.1[1]).fold(0.0, f32::max);
            out.iter_mut().for_each(|r| r.1[1] = h);
        }
        Arrange::BringToFront | Arrange::SendToBack => {}
    }
    out
}
