//! Design tokens for LabCraft. Every color comes from a theme file; nothing in
//! the UI is allowed to hardcode one. The same tokens feed the egui renderer and
//! the SVG exporter so agent-visible previews match the app.

use std::collections::BTreeMap;

use lc_model::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub fn hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let parse = |i: usize, len: usize| u8::from_str_radix(&s[i..i + len], 16).ok();
        match s.len() {
            3 => Some(Self::new(
                parse(0, 1)? * 17,
                parse(1, 1)? * 17,
                parse(2, 1)? * 17,
                255,
            )),
            6 => Some(Self::new(parse(0, 2)?, parse(2, 2)?, parse(4, 2)?, 255)),
            8 => Some(Self::new(
                parse(0, 2)?,
                parse(2, 2)?,
                parse(4, 2)?,
                parse(6, 2)?,
            )),
            _ => None,
        }
    }
    pub fn css(&self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
    pub fn with_alpha(&self, a: u8) -> Self {
        Self { a, ..*self }
    }
    pub fn mix(&self, other: &Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgba::new(
            f(self.r, other.r),
            f(self.g, other.g),
            f(self.b, other.b),
            f(self.a, other.a),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub name: String,
    /// "dark" or "light"
    pub appearance: String,
    pub tokens: BTreeMap<String, String>,
    pub wire: BTreeMap<String, String>,
}

impl Theme {
    pub fn dark() -> Self {
        let mut t = BTreeMap::new();
        for (k, v) in [
            ("bg.app", "#0B0E13"),
            ("bg.canvas", "#0F131A"),
            ("bg.panel", "#151A22"),
            ("bg.elevated", "#1A2029"),
            ("bg.overlay", "#1E2530"),
            ("bg.input", "#11161D"),
            ("border.subtle", "#242C37"),
            ("border.strong", "#333D4B"),
            ("text.primary", "#E8EDF5"),
            ("text.secondary", "#98A3B3"),
            ("text.muted", "#6B7686"),
            ("text.disabled", "#4A525F"),
            ("accent", "#5B9DFF"),
            ("accent.hover", "#7CB2FF"),
            ("accent.pressed", "#4A86E0"),
            ("accent.soft", "#1E3A63"),
            ("selection.fill", "#1E3A63"),
            ("selection.border", "#5B9DFF"),
            ("focus", "#7CB2FF"),
            ("run", "#3DDC97"),
            ("stop", "#FF5F56"),
            ("warn", "#F5B84C"),
            ("error", "#FF6B6B"),
            ("ai", "#A78BFA"),
            ("success", "#3DDC97"),
            ("grid.dot", "#1A212B"),
            ("grid.line", "#161C25"),
            ("shadow", "#00000066"),
        ] {
            t.insert(k.to_string(), v.to_string());
        }
        let mut w = BTreeMap::new();
        for (k, v) in [
            ("dbl", "#FFA657"),
            ("int", "#6EA8FF"),
            ("bool", "#4ADE80"),
            ("string", "#F472B6"),
            ("path", "#22D3EE"),
            ("cluster", "#C9A06A"),
            ("enum", "#A78BFA"),
            ("variant", "#94A3B8"),
            ("error", "#FBBF24"),
            ("refnum", "#2DD4BF"),
            ("waveform", "#E879F9"),
            ("broken", "#FF5F56"),
            ("coercion", "#F5B84C"),
        ] {
            w.insert(k.to_string(), v.to_string());
        }
        Self {
            name: "LabCraft Dark".into(),
            appearance: "dark".into(),
            tokens: t,
            wire: w,
        }
    }

    pub fn light() -> Self {
        let mut t = BTreeMap::new();
        for (k, v) in [
            ("bg.app", "#F7F8FA"),
            ("bg.canvas", "#FFFFFF"),
            ("bg.panel", "#FFFFFF"),
            ("bg.elevated", "#FFFFFF"),
            ("bg.overlay", "#FFFFFF"),
            ("bg.input", "#F3F5F8"),
            ("border.subtle", "#E3E7ED"),
            ("border.strong", "#CBD2DC"),
            ("text.primary", "#111827"),
            ("text.secondary", "#4B5563"),
            ("text.muted", "#6B7280"),
            ("text.disabled", "#9CA3AF"),
            ("accent", "#2563EB"),
            ("accent.hover", "#3B82F6"),
            ("accent.pressed", "#1D4ED8"),
            ("accent.soft", "#DBEAFE"),
            ("selection.fill", "#DBEAFE"),
            ("selection.border", "#2563EB"),
            ("focus", "#2563EB"),
            ("run", "#059669"),
            ("stop", "#DC2626"),
            ("warn", "#B45309"),
            ("error", "#DC2626"),
            ("ai", "#7C3AED"),
            ("success", "#059669"),
            ("grid.dot", "#E5E9F0"),
            ("grid.line", "#EEF1F5"),
            ("shadow", "#0F172A1F"),
        ] {
            t.insert(k.to_string(), v.to_string());
        }
        let mut w = BTreeMap::new();
        for (k, v) in [
            ("dbl", "#D97706"),
            ("int", "#2563EB"),
            ("bool", "#16A34A"),
            ("string", "#DB2777"),
            ("path", "#0891B2"),
            ("cluster", "#A16207"),
            ("enum", "#7C3AED"),
            ("variant", "#475569"),
            ("error", "#B45309"),
            ("refnum", "#0D9488"),
            ("waveform", "#A21CAF"),
            ("broken", "#DC2626"),
            ("coercion", "#B45309"),
        ] {
            w.insert(k.to_string(), v.to_string());
        }
        Self {
            name: "LabCraft Light".into(),
            appearance: "light".into(),
            tokens: t,
            wire: w,
        }
    }

    pub fn is_dark(&self) -> bool {
        self.appearance != "light"
    }

    pub fn color(&self, token: &str) -> Rgba {
        self.tokens
            .get(token)
            .and_then(|v| Rgba::hex(v))
            .unwrap_or(Rgba::new(255, 0, 255, 255))
    }

    pub fn wire_color(&self, ty: &Type) -> Rgba {
        let key = match ty {
            Type::Bool => "bool",
            Type::Numeric { numeric } if numeric.is_float() => "dbl",
            Type::Numeric { .. } => "int",
            Type::String => "string",
            Type::Path => "path",
            Type::Cluster { .. } => "cluster",
            Type::Enum { .. } => "enum",
            Type::Variant => "variant",
            Type::Error => "error",
            Type::Refnum { .. } => "refnum",
            Type::Waveform => "waveform",
            Type::Array { element, .. } => return self.wire_color(element),
            _ => "variant",
        };
        self.wire
            .get(key)
            .and_then(|v| Rgba::hex(v))
            .unwrap_or(self.color("text.muted"))
    }

    pub fn wire_color_named(&self, key: &str) -> Rgba {
        self.wire
            .get(key)
            .and_then(|v| Rgba::hex(v))
            .unwrap_or(self.color("text.muted"))
    }

    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }

    /// Bundled themes, with the dark theme first as the product default.
    pub fn all() -> Vec<Theme> {
        vec![Theme::dark(), Theme::light()]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_has_required_tokens() {
        for theme in Theme::all() {
            for token in [
                "bg.app",
                "bg.canvas",
                "bg.panel",
                "border.subtle",
                "text.primary",
                "text.secondary",
                "accent",
                "run",
                "stop",
                "ai",
            ] {
                assert!(
                    theme.tokens.contains_key(token),
                    "{} is missing {token}",
                    theme.name
                );
                assert!(Rgba::hex(&theme.tokens[token]).is_some());
            }
        }
    }

    #[test]
    fn hex_roundtrip() {
        let c = Rgba::hex("#0B0E13").unwrap();
        assert_eq!(c, Rgba::new(11, 14, 19, 255));
        assert_eq!(c.css(), "#0B0E13");
    }
}
