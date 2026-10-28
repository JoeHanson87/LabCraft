//! SVG rendering for front panels and block diagrams.
//!
//! Rendering uses exactly the same theme tokens and geometry as the editor, so
//! an agent inspecting `vi_render` output sees what a human sees. Rendered
//! output is also the basis for screenshot regression tests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use lc_layout::{geometry, route_wire};
use lc_model::document::{kinds, Control, ControlKind, Node, Rect, Scope, ViDocument};
use lc_model::{Type, Value};
use lc_nodes::Registry;
use lc_theme::{Rgba, Theme};

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub scale: f32,
    pub show_grid: bool,
    /// Node ids to outline in the AI-attribution color.
    pub highlight: Vec<String>,
    pub padding: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            show_grid: true,
            highlight: Vec::new(),
            padding: 32.0,
        }
    }
}

pub fn panel_svg(doc: &ViDocument, theme: &Theme, opts: &RenderOptions) -> String {
    let bounds = panel_bounds(doc);
    let w = (bounds.w + opts.padding * 2.0).max(480.0);
    let h = (bounds.h + opts.padding * 2.0).max(360.0);
    let mut s = String::new();
    header(&mut s, w, h, theme, opts.scale);
    let _ = writeln!(
        s,
        r#"<rect x="0" y="0" width="{w}" height="{h}" fill="{}"/>"#,
        theme.color("bg.canvas").css()
    );
    let frame = Rect::new(
        opts.padding / 2.0,
        opts.padding / 2.0,
        w - opts.padding,
        h - opts.padding,
    );
    let _ = writeln!(
        s,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="10" fill="{}" stroke="{}"/>"#,
        frame.x,
        frame.y,
        frame.w,
        frame.h,
        theme.color("bg.panel").css(),
        theme.color("border.subtle").css()
    );
    let _ = writeln!(
        s,
        r#"<text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="13" font-weight="600">{}</text>"#,
        frame.x + 18.0,
        frame.y + 26.0,
        theme.color("text.primary").css(),
        esc(&doc.name)
    );
    let _ = writeln!(
        s,
        r#"<text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="11">Front Panel</text>"#,
        frame.x + 18.0,
        frame.y + 42.0,
        theme.color("text.muted").css()
    );
    let offset_x = bounds.x - opts.padding / 2.0;
    let offset_y = bounds.y - opts.padding / 2.0 + 40.0;
    let _ = writeln!(
        s,
        r#"<g transform="translate({},{})">"#,
        -offset_x + frame.x + 10.0,
        -offset_y + frame.y + 60.0
    );
    for c in &doc.panel.controls {
        control_svg(&mut s, c, theme, opts);
    }
    let _ = writeln!(s, "</g>");
    s.push_str("</svg>");
    s
}

pub fn diagram_svg(doc: &ViDocument, theme: &Theme, opts: &RenderOptions) -> String {
    let bounds = diagram_bounds(doc);
    let w = (bounds.w + opts.padding * 2.0).max(640.0);
    let h = (bounds.h + opts.padding * 2.0).max(420.0);
    let mut s = String::new();
    header(&mut s, w, h, theme, opts.scale);
    let _ = writeln!(
        s,
        r#"<rect x="0" y="0" width="{w}" height="{h}" fill="{}"/>"#,
        theme.color("bg.canvas").css()
    );
    if opts.show_grid {
        let step = 16.0;
        let mut y = 0.0;
        while y < h {
            let mut x = 0.0;
            while x < w {
                let _ = writeln!(
                    s,
                    r#"<circle cx="{x}" cy="{y}" r="0.8" fill="{}"/>"#,
                    theme.color("grid.dot").css()
                );
                x += step;
            }
            y += step;
        }
    }
    let _ = writeln!(
        s,
        r#"<g transform="translate({},{})">"#,
        opts.padding - bounds.x,
        opts.padding - bounds.y
    );
    let compiled = lc_runtime::compile(doc).compiled;
    draw_scope(
        &mut s,
        doc,
        &doc.diagram.root,
        theme,
        opts,
        compiled.as_ref(),
    );
    let _ = writeln!(s, "</g>");
    // Header label after content so it stays legible.
    let _ = writeln!(
        s,
        r#"<text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="11" font-weight="600" letter-spacing="0.08em">BLOCK DIAGRAM</text>"#,
        opts.padding,
        opts.padding - 12.0,
        theme.color("text.muted").css()
    );
    s.push_str("</svg>");
    s
}

fn draw_scope(
    s: &mut String,
    doc: &ViDocument,
    scope: &Scope,
    theme: &Theme,
    opts: &RenderOptions,
    compiled: Option<&lc_runtime::CompiledVi>,
) {
    let reg = Registry::global();
    // Wires behind nodes.
    for node in &scope.nodes {
        let to_g = geometry(node, reg);
        for (port, src) in &node.inputs {
            let lc_model::SourceRef::Node {
                node: from,
                port: from_port,
            } = src
            else {
                continue;
            };
            let Some(src_node) = scope.node(from) else {
                continue;
            };
            let from_g = geometry(src_node, reg);
            let (Some(a), Some(b)) = (from_g.output(from_port), to_g.input(port)) else {
                continue;
            };
            let color = compiled
                .and_then(|c| c.output_type(from, from_port))
                .map(|t| theme.wire_color(t))
                .unwrap_or_else(|| theme.color("accent"));
            let pts = route_wire(a, b);
            let mut d = String::new();
            for (i, p) in pts.iter().enumerate() {
                if i == 0 {
                    let _ = write!(d, "M {:.1} {:.1}", p.x, p.y);
                } else {
                    let _ = write!(d, " L {:.1} {:.1}", p.x, p.y);
                }
            }
            let _ = writeln!(
                s,
                r#"<path d="{}" fill="none" stroke="{}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>"#,
                d,
                color.css()
            );
            let _ = writeln!(
                s,
                r#"<circle cx="{:.1}" cy="{:.1}" r="2.5" fill="{}"/>"#,
                b.x,
                b.y,
                color.css()
            );
        }
    }

    for node in &scope.nodes {
        node_svg(s, doc, node, theme, opts, compiled);
        if node.frames.is_empty() {
            continue;
        }
        let g = geometry(node, reg);
        let _ = writeln!(
            s,
            r#"<rect x="{}" y="{}" width="{}" height="{}" rx="8" fill="none" stroke="{}" stroke-dasharray="4 4"/>"#,
            g.rect.x + 12.0,
            g.rect.y + 52.0,
            (g.rect.w - 24.0).max(20.0),
            (g.rect.h - 64.0).max(20.0),
            theme.color("border.subtle").css()
        );
        for (i, frame) in node.frames.iter().enumerate() {
            if i > 0 && node.kind != kinds::SEQUENCE {
                continue;
            }
            draw_scope(s, doc, &frame.scope, theme, opts, compiled);
            let label = if node.kind == kinds::CASE {
                frame.label.clone()
            } else if node.kind == kinds::WHILE || node.kind == kinds::FOR {
                "body".to_string()
            } else {
                frame.label.clone()
            };
            let _ = writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="18" rx="4" fill="{}"/>"#,
                g.rect.x + 18.0,
                g.rect.y + 46.0,
                54.0 + label.len() as f32 * 6.0,
                theme.color("bg.elevated").css()
            );
            let _ = writeln!(
                s,
                r#"<text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="10">{}</text>"#,
                g.rect.x + 26.0,
                g.rect.y + 59.0,
                theme.color("text.secondary").css(),
                esc(&label)
            );
        }
    }
}

fn node_svg(
    s: &mut String,
    doc: &ViDocument,
    node: &Node,
    theme: &Theme,
    opts: &RenderOptions,
    compiled: Option<&lc_runtime::CompiledVi>,
) {
    let reg = Registry::global();
    let g = geometry(node, reg);
    let r = g.rect;
    let title = node_title(doc, node);
    let is_ai = opts.highlight.contains(&node.id);
    let border = if is_ai {
        theme.color("ai")
    } else if node.kind == kinds::CONSTANT {
        theme.color("border.subtle")
    } else {
        theme.color("border.strong")
    };
    let fill = if node.kind == kinds::CONSTANT {
        theme.color("bg.canvas")
    } else {
        theme.color("bg.elevated")
    };
    let _ = writeln!(
        s,
        r#"<g><rect x="{}" y="{}" width="{}" height="{}" rx="7" fill="{}" stroke="{}" stroke-width="{}"/>"#,
        r.x,
        r.y,
        r.w,
        r.h,
        fill.css(),
        border.css(),
        if is_ai { 2 } else { 1 }
    );
    if node.kind != kinds::CONSTANT {
        let _ = writeln!(
            s,
            r#"<path d="M {},{} h {} a 7 7 0 0 1 7 7 v 13 h -{} z" fill="{}"/>"#,
            r.x + 1.0,
            r.y + 1.0,
            r.w - 16.0,
            r.w - 2.0,
            theme.color("accent.soft").css()
        );
        let _ = writeln!(
            s,
            r#"<text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="12" font-weight="600">{}</text>"#,
            r.x + 12.0,
            r.y + 19.0,
            theme.color("text.primary").css(),
            esc(&title)
        );
    } else {
        let _ = writeln!(
            s,
            r#"<text x="{}" y="{}" fill="{}" font-family="JetBrains Mono,monospace" font-size="12">{}</text>"#,
            r.x + 10.0,
            r.y + 21.0,
            theme.color("text.primary").css(),
            esc(&constant_text(node))
        );
    }

    // Control and indicator terminals show their data type so the diagram is
    // readable without opening the panel or the inspector.
    if matches!(node.kind.as_str(), kinds::CONTROL | kinds::INDICATOR) {
        if let Some(ty) = control_type(doc, node) {
            let _ = writeln!(
                s,
                r#"<text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="9" letter-spacing="0.04em">{}</text>"#,
                r.x + 10.0,
                r.y + r.h - 8.0,
                theme.color("text.muted").css(),
                esc(&ty.display().to_uppercase())
            );
        }
    }

    for (name, p) in &g.inputs {
        let ty = compiled
            .and_then(|c| c.input_type(&node.id, name).cloned())
            .or_else(|| control_type(doc, node));
        let color = ty
            .as_ref()
            .map(|t| theme.wire_color(t))
            .unwrap_or_else(|| theme.color("text.muted"));
        let _ = writeln!(
            s,
            r#"<circle cx="{:.1}" cy="{:.1}" r="4" fill="{}" stroke="{}"/>"#,
            p.x,
            p.y,
            color.css(),
            theme.color("bg.canvas").css()
        );
        if r.h > 84.0 {
            let _ = writeln!(
                s,
                r#"<text x="{:.1}" y="{:.1}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="9">{}</text>"#,
                p.x + 9.0,
                p.y + 3.0,
                theme.color("text.muted").css(),
                esc(name)
            );
        }
    }
    for (name, p) in &g.outputs {
        let ty = compiled
            .and_then(|c| c.output_type(&node.id, name).cloned())
            .or_else(|| control_type(doc, node));
        let color = ty
            .as_ref()
            .map(|t| theme.wire_color(t))
            .unwrap_or_else(|| theme.color("text.muted"));
        let _ = writeln!(
            s,
            r#"<circle cx="{:.1}" cy="{:.1}" r="4" fill="{}" stroke="{}"/>"#,
            p.x,
            p.y,
            color.css(),
            theme.color("bg.canvas").css()
        );
        if r.h > 84.0 {
            let _ = writeln!(
                s,
                r#"<text x="{:.1}" y="{:.1}" text-anchor="end" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="9">{}</text>"#,
                p.x - 9.0,
                p.y + 3.0,
                theme.color("text.muted").css(),
                esc(name)
            );
        }
    }
    s.push_str("</g>");
}

fn control_type(doc: &ViDocument, node: &Node) -> Option<Type> {
    doc.control_for_node(node).map(|c| c.ty.clone())
}

fn node_title(doc: &ViDocument, node: &Node) -> String {
    if let Some(label) = &node.label {
        return label.clone();
    }
    if let Some(c) = doc.control_for_node(node) {
        return c.label.clone();
    }
    if let Some(def) = Registry::global().get(&node.kind) {
        return def.name.to_string();
    }
    node.kind.clone()
}

fn constant_text(node: &Node) -> String {
    node.props
        .get("value")
        .and_then(|v| serde_json::from_value::<Value>(v.clone()).ok())
        .map(|v| v.display())
        .unwrap_or_else(|| "?".into())
}

fn control_svg(s: &mut String, c: &Control, theme: &Theme, opts: &RenderOptions) {
    let r = c.bounds;
    let label_color = theme.color("text.secondary");
    let text = theme.color("text.primary");
    let border = theme.color("border.strong");
    let input = theme.color("bg.input");
    let accent = theme.color("accent");
    let _ = writeln!(
        s,
        r#"<g><text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="11" font-weight="500">{}</text>"#,
        r.x,
        r.y - 6.0,
        label_color.css(),
        esc(&c.label)
    );
    let value = &c.default;
    match c.kind {
        ControlKind::Led => {
            let on = value.as_bool();
            let color = if on {
                theme.color("run")
            } else {
                theme.color("border.strong")
            };
            let _ = writeln!(
                s,
                r#"<circle cx="{}" cy="{}" r="8" fill="{}"/>"#,
                r.x + 12.0,
                r.y + r.h / 2.0,
                color.css()
            );
            let _ = writeln!(
                s,
                r#"<text x="{}" y="{}" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="12">{}</text>"#,
                r.x + 28.0,
                r.y + r.h / 2.0 + 4.0,
                text.css(),
                if on { "ON" } else { "OFF" }
            );
        }
        ControlKind::Toggle => {
            let on = value.as_bool();
            let _ = writeln!(
                s,
                r#"<rect x="{}" y="{}" width="46" height="24" rx="12" fill="{}"/><circle cx="{}" cy="{}" r="9" fill="{}"/>"#,
                r.x,
                r.y,
                if on { accent.css() } else { input.css() },
                if on { r.x + 34.0 } else { r.x + 13.0 },
                r.y + 12.0,
                if on {
                    "#FFFFFF".to_string()
                } else {
                    border.css()
                }
            );
        }
        ControlKind::Button => {
            let _ = writeln!(
                s,
                r##"<rect x="{}" y="{}" width="104" height="30" rx="7" fill="{}" stroke="{}"/><text x="{}" y="{}" text-anchor="middle" fill="#FFFFFF" font-family="Inter,system-ui,sans-serif" font-size="12">{}</text>"##,
                r.x,
                r.y,
                accent.css(),
                accent.css(),
                r.x + 52.0,
                r.y + 20.0,
                esc(&c.label)
            );
        }
        ControlKind::Chart | ControlKind::Graph => {
            let _ = writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="6" fill="{}" stroke="{}"/>"#,
                r.x,
                r.y,
                r.w,
                r.h.max(120.0),
                input.css(),
                border.css()
            );
            let data: Vec<f64> = match value {
                Value::Array(a) => a.data.iter().map(|v| v.as_f64()).collect(),
                _ => vec![0.0, 0.4, 0.2, 0.8, 0.5, 1.0, 0.6],
            };
            if !data.is_empty() {
                let n = data.len();
                let max = data.iter().cloned().fold(1.0f64, f64::max);
                let mut d = String::new();
                for (i, v) in data.iter().enumerate() {
                    let x = r.x + 10.0 + (r.w - 20.0) * i as f32 / (n.max(2) - 1) as f32;
                    let y =
                        r.y + r.h.max(120.0) - 10.0 - ((v / max) as f32) * (r.h.max(120.0) - 20.0);
                    if i == 0 {
                        let _ = write!(d, "M {x:.1} {y:.1}");
                    } else {
                        let _ = write!(d, " L {x:.1} {y:.1}");
                    }
                }
                let _ = writeln!(
                    s,
                    r#"<path d="{}" fill="none" stroke="{}" stroke-width="2"/>"#,
                    d,
                    theme.wire_color_named("dbl").css()
                );
            }
        }
        ControlKind::Table => {
            let _ = writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="6" fill="{}" stroke="{}"/>"#,
                r.x,
                r.y,
                r.w,
                r.h.max(100.0),
                input.css(),
                border.css()
            );
        }
        _ => {
            let _ = writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="6" fill="{}" stroke="{}"/>"#,
                r.x,
                r.y,
                r.w,
                r.h,
                input.css(),
                border.css()
            );
            let shown = match c.kind {
                ControlKind::Enum => value.display(),
                _ => value.display(),
            };
            let _ = writeln!(
                s,
                r#"<text x="{}" y="{}" fill="{}" font-family="JetBrains Mono,monospace" font-size="12">{}</text>"#,
                r.x + 10.0,
                r.y + r.h / 2.0 + 4.0,
                text.css(),
                esc(&shown)
            );
            if let Some(unit) = &c.style.unit {
                let _ = writeln!(
                    s,
                    r#"<text x="{}" y="{}" text-anchor="end" fill="{}" font-family="Inter,system-ui,sans-serif" font-size="10">{}</text>"#,
                    r.x + r.w - 10.0,
                    r.y + r.h / 2.0 + 4.0,
                    theme.color("text.muted").css(),
                    esc(unit)
                );
            }
        }
    }
    if opts.highlight.contains(&c.id) {
        let _ = writeln!(
            s,
            r#"<rect x="{}" y="{}" width="{}" height="{}" rx="8" fill="none" stroke="{}" stroke-width="2"/>"#,
            r.x - 4.0,
            r.y - 4.0,
            r.w + 8.0,
            r.h + 8.0,
            theme.color("ai").css()
        );
    }
    s.push_str("</g>");
}

fn header(s: &mut String, w: f32, h: f32, theme: &Theme, scale: f32) {
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        w * scale,
        h * scale,
        w,
        h
    );
    let _ = writeln!(
        s,
        r#"<style>text{{white-space:pre}}</style><rect width="100%" height="100%" fill="{}"/>"#,
        theme.color("bg.app").css()
    );
}

fn panel_bounds(doc: &ViDocument) -> Rect {
    let mut max_x = 640.0f32;
    let mut max_y = 480.0f32;
    let mut min_x = 0.0f32;
    let mut min_y = 0.0f32;
    for c in &doc.panel.controls {
        max_x = max_x.max(c.bounds.x + c.bounds.w + 24.0);
        max_y = max_y.max(c.bounds.y + c.bounds.h + 24.0);
        min_x = min_x.min(c.bounds.x - 24.0);
        min_y = min_y.min(c.bounds.y - 24.0);
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

pub fn diagram_bounds(doc: &ViDocument) -> Rect {
    let reg = Registry::global();
    fn walk(scope: &Scope, reg: &Registry, min: &mut (f32, f32), max: &mut (f32, f32)) {
        for n in &scope.nodes {
            let g = geometry(n, reg);
            min.0 = min.0.min(g.rect.x);
            min.1 = min.1.min(g.rect.y);
            max.0 = max.0.max(g.rect.x + g.rect.w);
            max.1 = max.1.max(g.rect.y + g.rect.h);
            for f in &n.frames {
                walk(&f.scope, reg, min, max);
            }
        }
    }
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    walk(&doc.diagram.root, reg, &mut min, &mut max);
    if min.0 == f32::MAX {
        return Rect::new(0.0, 0.0, 900.0, 600.0);
    }
    Rect::new(
        min.0,
        min.1,
        (max.0 - min.0).max(320.0),
        (max.1 - min.1).max(240.0),
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render a small theme preview (used by the settings dialog and docs).
pub fn swatches(theme: &Theme) -> Vec<(String, Rgba)> {
    let mut v: Vec<(String, Rgba)> = theme
        .tokens
        .keys()
        .map(|k| (k.clone(), theme.color(k)))
        .collect();
    v.extend(
        theme
            .wire
            .keys()
            .map(|k| (format!("wire.{k}"), theme.wire_color_named(k))),
    );
    v
}

/// Best-effort type list used for legends.
pub fn wire_legend(theme: &Theme) -> BTreeMap<String, Rgba> {
    theme
        .wire
        .keys()
        .map(|k| (k.clone(), theme.wire_color_named(k)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lc_model::document::{kinds, Control, ControlKind, Node, Point};

    fn sample() -> ViDocument {
        let mut doc = ViDocument::new("Render Smoke");
        let mut temp = Control::new("temp", ControlKind::Numeric, "Temperature");
        temp.ty = Type::dbl();
        temp.default = Value::num(25.0);
        temp.bounds = Rect::new(40.0, 60.0, 170.0, 40.0);
        doc.panel.controls.push(temp);
        let alarm = Control::new("alarm", ControlKind::Led, "Alarm");
        doc.panel.controls.push(alarm);

        let mut constant = Node::new(kinds::CONSTANT, "limit");
        constant.pos = Point::new(60.0, 80.0);
        constant
            .props
            .insert("value".into(), serde_json::json!(30.0));
        doc.diagram.root.nodes.push(constant);
        let greater = Node::new("greater", "too_hot");
        doc.diagram.root.nodes.push(greater);
        doc
    }

    #[test]
    fn renders_panel_and_diagram_svg() {
        let doc = sample();
        let theme = Theme::dark();
        let opts = RenderOptions::default();
        let panel = panel_svg(&doc, &theme, &opts);
        let diagram = diagram_svg(&doc, &theme, &opts);
        assert!(panel.starts_with("<svg"));
        assert!(diagram.starts_with("<svg"));
        assert!(panel.len() > 500);
        assert!(diagram.contains("Greater?"), "{diagram}");
        assert!(diagram.contains("BLOCK DIAGRAM"));
        assert!(panel.contains("Temperature"));
    }

    #[test]
    fn light_theme_changes_background() {
        let doc = sample();
        let opts = RenderOptions::default();
        let dark = panel_svg(&doc, &Theme::dark(), &opts);
        let light = panel_svg(&doc, &Theme::light(), &opts);
        assert_ne!(dark, light);
    }
}
