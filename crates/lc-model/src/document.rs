use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::types::{ConnectorPane, Type};
use crate::value::Value;
use crate::{new_id, LcError, Result};

pub const FORMAT_VERSION: u32 = 1;

/// Well-known node kinds. Plugin and subVI node kinds are arbitrary strings.
pub mod kinds {
    pub const CONTROL: &str = "control";
    pub const INDICATOR: &str = "indicator";
    pub const CONSTANT: &str = "constant";
    pub const WHILE: &str = "structure.while";
    pub const FOR: &str = "structure.for";
    pub const CASE: &str = "structure.case";
    pub const SEQUENCE: &str = "structure.sequence";
    pub const EVENT: &str = "structure.event";
    pub const LOOP_CONDITION: &str = "loop.condition";
    pub const SUBVI: &str = "subvi";
}

pub fn is_structure(kind: &str) -> bool {
    matches!(
        kind,
        kinds::WHILE
            | kinds::FOR
            | kinds::CASE
            | kinds::SEQUENCE
            | kinds::EVENT
            | kinds::LOOP_CONDITION
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl Default for Point {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.x + self.w && p.y <= self.y + self.h
    }
}

impl Default for Rect {
    fn default() -> Self {
        Self::new(24.0, 24.0, 140.0, 28.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Numeric,
    Slider,
    Toggle,
    Led,
    String,
    Path,
    Button,
    Chart,
    Graph,
    Enum,
    Table,
    Cluster,
    Array,
}

impl ControlKind {
    pub fn is_indicator(self) -> bool {
        matches!(
            self,
            ControlKind::Led | ControlKind::Chart | ControlKind::Graph | ControlKind::Table
        )
    }
    pub fn default_type(self) -> Type {
        match self {
            ControlKind::Numeric | ControlKind::Slider => Type::dbl(),
            ControlKind::Toggle | ControlKind::Led | ControlKind::Button => Type::Bool,
            ControlKind::String => Type::String,
            ControlKind::Path => Type::Path,
            ControlKind::Chart | ControlKind::Graph => Type::array_of(Type::dbl()),
            ControlKind::Enum => Type::Enum {
                values: vec!["Item 1".into(), "Item 2".into()],
            },
            ControlKind::Table => Type::array_of(Type::array_of(Type::dbl())),
            ControlKind::Cluster => Type::Cluster { fields: Vec::new() },
            ControlKind::Array => Type::array_of(Type::dbl()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ControlStyle {
    #[serde(default = "default_true")]
    pub show_label: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for ControlStyle {
    fn default() -> Self {
        Self {
            show_label: true,
            unit: None,
            min: None,
            max: None,
            step: None,
            decimals: None,
            options: Vec::new(),
            color: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: String,
    /// Stable, human/agent-chosen identifier used by the DSL and wiring tools.
    pub key: String,
    pub kind: ControlKind,
    /// True for indicators (outputs), false for controls (inputs).
    #[serde(default)]
    pub indicator: bool,
    pub label: String,
    pub ty: Type,
    pub default: Value,
    pub bounds: Rect,
    #[serde(default)]
    pub style: ControlStyle,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, Json>,
}

impl Control {
    pub fn new(key: impl Into<String>, kind: ControlKind, label: impl Into<String>) -> Self {
        let key = key.into();
        let ty = kind.default_type();
        Self {
            id: new_id("c"),
            key,
            kind,
            indicator: kind.is_indicator(),
            label: label.into(),
            default: Value::default_for(&ty),
            ty,
            bounds: Rect::default(),
            style: ControlStyle::default(),
            props: BTreeMap::new(),
        }
    }
    pub fn is_indicator(&self) -> bool {
        self.indicator
    }
    pub fn with_type(mut self, ty: Type) -> Self {
        self.default = Value::default_for(&ty);
        self.ty = ty;
        self
    }
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.bounds.x = x;
        self.bounds.y = y;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Decoration {
    pub id: String,
    pub kind: String,
    pub bounds: Rect,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, Json>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<Control>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decorations: Vec<Decoration>,
}

/// Where a node input gets its data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum SourceRef {
    Node {
        node: String,
        port: String,
    },
    Tunnel {
        structure: String,
        name: String,
    },
    ShiftRegister {
        structure: String,
        name: String,
    },
    Iteration {
        structure: String,
    },
    EventData {
        structure: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
    },
    Default {
        ty: Type,
    },
}

impl SourceRef {
    pub fn node(node: impl Into<String>, port: impl Into<String>) -> Self {
        Self::Node {
            node: node.into(),
            port: port.into(),
        }
    }
    pub fn source_node(&self) -> Option<&str> {
        match self {
            SourceRef::Node { node, .. } => Some(node),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TunnelMode {
    /// Pass the value through each iteration. On an input this iterates one
    /// dimension of an array; on an output it collects values into a new array.
    AutoIndex,
    /// Pass the last value produced (LabVIEW's default tunnel behavior).
    LastValue,
    /// Use the type's default when a case frame leaves the tunnel unwired.
    UseDefaultIfUnwired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TunnelDirection {
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Tunnel {
    pub name: String,
    pub ty: Type,
    pub mode: TunnelMode,
    pub direction: TunnelDirection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShiftRegister {
    pub name: String,
    pub ty: Type,
    pub init: SourceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    /// Case selector value, sequence index, or event specifier.
    pub label: String,
    pub scope: Scope,
}

impl Frame {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            scope: Scope::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    /// Stable key unique across the whole VI. Wires and agent tools use keys.
    pub key: String,
    pub kind: String,
    #[serde(default)]
    pub pos: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, Json>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, SourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tunnels: Vec<Tunnel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shift_registers: Vec<ShiftRegister>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
}

impl Node {
    pub fn new(kind: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            id: new_id("n"),
            key: key.into(),
            kind: kind.into(),
            pos: Point::default(),
            size: None,
            label: None,
            props: BTreeMap::new(),
            inputs: BTreeMap::new(),
            tunnels: Vec::new(),
            shift_registers: Vec::new(),
            frames: Vec::new(),
        }
    }
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.pos = Point::new(x, y);
        self
    }
    pub fn with_input(mut self, port: impl Into<String>, source: SourceRef) -> Self {
        self.inputs.insert(port.into(), source);
        self
    }
    pub fn with_prop(mut self, key: impl Into<String>, value: Json) -> Self {
        self.props.insert(key.into(), value);
        self
    }
    pub fn control_key(&self) -> Option<&str> {
        self.props.get("control").and_then(Json::as_str)
    }
    pub fn is_structure(&self, kind: &str) -> bool {
        self.kind == kind
    }
    pub fn frame(&self, label: &str) -> Option<&Frame> {
        self.frames.iter().find(|f| f.label == label)
    }
    pub fn frame_mut(&mut self, label: &str) -> Option<&mut Frame> {
        self.frames.iter_mut().find(|f| f.label == label)
    }
    pub fn tunnel(&self, name: &str) -> Option<&Tunnel> {
        self.tunnels.iter().find(|t| t.name == name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
    /// Values produced by a frame or loop body for its structure's outputs:
    /// `t:<tunnel>` for tunnels and `sr:<name>` for shift registers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, SourceRef>,
}

impl Scope {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn node_by_key(&self, key: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.key == key)
    }
    pub fn push(&mut self, node: Node) -> &mut Node {
        self.nodes.push(node);
        self.nodes.last_mut().expect("just pushed")
    }
    pub fn remove(&mut self, id: &str) -> Option<Node> {
        let idx = self.nodes.iter().position(|n| n.id == id)?;
        Some(self.nodes.remove(idx))
    }

    pub fn edges(&self) -> Vec<Edge> {
        let mut out = Vec::new();
        for node in &self.nodes {
            for (port, src) in &node.inputs {
                if let SourceRef::Node {
                    node: from,
                    port: from_port,
                } = src
                {
                    out.push(Edge {
                        from_node: from.clone(),
                        from_port: from_port.clone(),
                        to_node: node.id.clone(),
                        to_port: port.clone(),
                    });
                }
            }
        }
        out
    }

    pub fn for_each_node(&self, f: &mut impl FnMut(&Node)) {
        for n in &self.nodes {
            f(n);
            for fr in &n.frames {
                fr.scope.for_each_node(f);
            }
        }
    }

    pub fn for_each_node_mut(&mut self, f: &mut impl FnMut(&mut Node)) {
        for n in &mut self.nodes {
            f(n);
            for fr in &mut n.frames {
                fr.scope.for_each_node_mut(f);
            }
        }
    }

    /// Collect node ids reachable in this scope (not descending into frames).
    pub fn ids(&self) -> Vec<String> {
        self.nodes.iter().map(|n| n.id.clone()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagram {
    #[serde(default)]
    pub root: Scope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestCase {
    pub name: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub expected: BTreeMap<String, Value>,
    #[serde(default)]
    pub tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViDocument {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub panel: Panel,
    #[serde(default)]
    pub diagram: Diagram,
    #[serde(default)]
    pub connector: ConnectorPane,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestCase>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl ViDocument {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            id: new_id("vi"),
            name: name.into(),
            description: String::new(),
            revision: 0,
            panel: Panel::default(),
            diagram: Diagram::default(),
            connector: ConnectorPane::default(),
            tests: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let doc: Self = serde_json::from_str(text)?;
        if doc.format_version > FORMAT_VERSION {
            return Err(LcError::Validation(format!(
                "VI format version {} is newer than supported version {FORMAT_VERSION}",
                doc.format_version
            )));
        }
        Ok(doc)
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub fn control(&self, key: &str) -> Option<&Control> {
        self.panel.controls.iter().find(|c| c.key == key)
    }
    pub fn control_mut(&mut self, key: &str) -> Option<&mut Control> {
        self.panel.controls.iter_mut().find(|c| c.key == key)
    }
    pub fn control_by_id(&self, id: &str) -> Option<&Control> {
        self.panel.controls.iter().find(|c| c.id == id)
    }

    pub fn control_for_node(&self, node: &Node) -> Option<&Control> {
        let key = node.control_key()?;
        self.control(key)
    }

    pub fn node_by_id(&self, id: &str) -> Option<&Node> {
        fn find<'a>(scope: &'a Scope, id: &str) -> Option<&'a Node> {
            if let Some(n) = scope.nodes.iter().find(|n| n.id == id) {
                return Some(n);
            }
            for n in &scope.nodes {
                for fr in &n.frames {
                    if let Some(found) = find(&fr.scope, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find(&self.diagram.root, id)
    }

    pub fn node_by_key(&self, key: &str) -> Option<&Node> {
        fn find<'a>(scope: &'a Scope, key: &str) -> Option<&'a Node> {
            if let Some(n) = scope.nodes.iter().find(|n| n.key == key) {
                return Some(n);
            }
            for n in &scope.nodes {
                for fr in &n.frames {
                    if let Some(found) = find(&fr.scope, key) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find(&self.diagram.root, key)
    }

    pub fn node_by_key_mut(&mut self, key: &str) -> Option<&mut Node> {
        fn find<'a>(scope: &'a mut Scope, key: &str) -> Option<&'a mut Node> {
            if let Some(idx) = scope.nodes.iter().position(|n| n.key == key) {
                return scope.nodes.get_mut(idx);
            }
            for n in &mut scope.nodes {
                for fr in &mut n.frames {
                    if let Some(found) = find(&mut fr.scope, key) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find(&mut self.diagram.root, key)
    }

    pub fn for_each_node(&self, f: &mut impl FnMut(&Node)) {
        self.diagram.root.for_each_node(f);
    }

    pub fn for_each_node_mut(&mut self, f: &mut impl FnMut(&mut Node)) {
        self.diagram.root.for_each_node_mut(f);
    }

    /// Nodes that own a panel control/indicator, keyed by control key.
    pub fn terminal_nodes(&self) -> BTreeMap<String, &Node> {
        fn collect<'a>(scope: &'a Scope, out: &mut BTreeMap<String, &'a Node>) {
            for n in &scope.nodes {
                if let Some(key) = n.control_key() {
                    out.insert(key.to_string(), n);
                }
                for fr in &n.frames {
                    collect(&fr.scope, out);
                }
            }
        }
        let mut out = BTreeMap::new();
        collect(&self.diagram.root, &mut out);
        out
    }

    pub fn input_value(&self, key: &str) -> Value {
        self.control(key)
            .map(|c| c.default.clone())
            .unwrap_or(Value::Void)
    }
}
