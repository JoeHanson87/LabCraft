//! LabCraft's Model Context Protocol server.
//!
//! The server is intentionally hand-rolled: JSON-RPC 2.0 over newline-delimited
//! stdio, with a small, stable tool surface. Every mutation goes through a tool
//! so the same operations are available to humans (via the CLI/editor) and
//! agents, and the documents stay in the canonical JSON IR described by
//! `lc-model`.
//!
//! Tools are named after the thing they operate on (`vi_*`, `panel_*`,
//! `diagram_*`, `nodes_*`) and return both human-readable text and
//! `structuredContent`. Failures are tool errors (`isError: true`) rather than
//! protocol errors so agents can recover without reinitialising.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use lc_model::document::{
    kinds, Control, ControlKind, ControlStyle, Node, Scope, SourceRef, ViDocument,
};
use lc_model::types::{ConnectorPin, Direction};
use lc_model::{Type, Value};
use lc_nodes::Registry;
use serde_json::{json, Value as Json};

// ---- Server state -------------------------------------------------------

#[derive(Debug, Clone)]
struct DocEntry {
    doc: ViDocument,
    path: Option<PathBuf>,
}

#[derive(Default)]
pub struct ServerState {
    docs: BTreeMap<String, DocEntry>,
    order: Vec<String>,
}

impl ServerState {
    pub fn new() -> Self {
        Self::default()
    }

    fn resolve_id(&self, handle: &str) -> Option<String> {
        if self.docs.contains_key(handle) {
            return Some(handle.to_string());
        }
        self.order
            .iter()
            .find(|id| {
                self.docs
                    .get(*id)
                    .map(|e| e.doc.name == handle)
                    .unwrap_or(false)
            })
            .cloned()
    }

    fn entry(&self, handle: &str) -> Result<&DocEntry, String> {
        let id = self
            .resolve_id(handle)
            .ok_or_else(|| format!("unknown VI `{handle}`; call vi_list to see open VIs"))?;
        self.docs
            .get(&id)
            .ok_or_else(|| format!("unknown VI `{handle}`"))
    }

    fn entry_mut(&mut self, handle: &str) -> Result<&mut DocEntry, String> {
        let id = self
            .resolve_id(handle)
            .ok_or_else(|| format!("unknown VI `{handle}`; call vi_list to see open VIs"))?;
        self.docs
            .get_mut(&id)
            .ok_or_else(|| format!("unknown VI `{handle}`"))
    }

    fn insert(&mut self, doc: ViDocument, path: Option<PathBuf>) -> String {
        let id = doc.id.clone();
        if !self.order.contains(&id) {
            self.order.push(id.clone());
        }
        self.docs.insert(id.clone(), DocEntry { doc, path });
        id
    }
}

// ---- Public API ---------------------------------------------------------

/// Run the MCP server on stdin/stdout. Blocks until stdin closes.
pub fn serve_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut state = ServerState::new();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Json>(trimmed) {
            Ok(msg) => handle_message(&mut state, msg),
            Err(err) => Some(error_response(
                Json::Null,
                -32700,
                format!("invalid JSON: {err}"),
            )),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut writer, &response)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
}

/// Handle one JSON-RPC message. Returns `None` for notifications.
pub fn handle_message(state: &mut ServerState, msg: Json) -> Option<Json> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

    // Notifications (`initialized`, `notifications/cancelled`, ...) never get a
    // response.
    let id = id?;

    if method.is_empty() {
        return Some(error_response(id, -32600, "missing method".into()));
    }

    let result: Result<Json, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params
                .get("protocolVersion")
                .and_then(Json::as_str)
                .unwrap_or("2024-11-05"),
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
                "prompts": { "listChanged": false },
                "logging": {}
            },
            "serverInfo": {
                "name": "labcraft",
                "title": "LabCraft",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": INSTRUCTIONS
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool_message(state, &params),
        "resources/list" => Ok(list_resources(state)),
        "resources/read" => read_resource(state, &params),
        "prompts/list" => Ok(json!({ "prompts": prompt_definitions() })),
        "prompts/get" => get_prompt(&params),
        "logging/setLevel" => Ok(json!({})),
        other => Err((-32601, format!("method not found: {other}"))),
    };

    Some(match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
        Err((code, message)) => error_response(id, code, message),
    })
}

fn error_response(id: Json, code: i64, message: String) -> Json {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

const INSTRUCTIONS: &str = r#"LabCraft is a visual dataflow programming environment. A VI has a front panel (controls and indicators) and a block diagram (nodes and wires).

Recommended authoring flow:
1. `vi_create` or `vi_create_from_spec` with a DSL program (the fastest path).
2. `vi_validate` to read diagnostics, then `vi_run` with inputs to check behaviour.
3. `panel_add_control` / `diagram_add_node` / `diagram_add_wire` for surgical edits, or `vi_apply_ops` for batches.
4. `vi_export_dsl` to read the current graph back in text form, `vi_save` to persist JSON.

The DSL is the primary authoring surface:
  vi "Name" {
    panel { control temp "Temperature" { type: dbl, default: 25.0 } indicator alarm "Alarm" { type: bool } }
    diagram { temp -> greater(30.0) -> alarm }
  }

Node ids come from `nodes_search` / `nodes_get`. Wires reference node keys (or ids) and ports, for example `{ "node": "greater_1", "port": "result" }`."#;

// ---- Tool definitions ---------------------------------------------------

fn tool_definitions() -> Vec<Json> {
    vec![
        tool(
            "vi_create",
            "Create an empty VI. Returns its id, which every other tool accepts as `vi`.",
            object(
                json!({
                    "name": { "type": "string", "description": "Human-readable VI name" },
                    "description": { "type": "string" }
                }),
                &["name"],
            ),
        ),
        tool(
            "vi_create_from_spec",
            "Create a VI from DSL text or JSON IR. Prefer DSL for agent-authored programs.",
            object(
                json!({
                    "dsl": { "type": "string", "description": "LabCraft DSL program" },
                    "json": { "description": "VI document JSON (string or object)" },
                    "spec": { "description": "DSL string, or a VI JSON document" }
                }),
                &[],
            ),
        ),
        tool(
            "vi_apply_dsl",
            "Replace a VI's panel and diagram with a compiled DSL program. Keeps the VI id.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "dsl": { "type": "string" }
                }),
                &["vi", "dsl"],
            ),
        ),
        tool(
            "vi_export_dsl",
            "Export the VI as DSL text. Use this to inspect a graphically edited VI.",
            object(json!({ "vi": { "type": "string" } }), &["vi"]),
        ),
        tool(
            "vi_get",
            "Get a VI as JSON IR, DSL, or a compact summary.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "format": { "type": "string", "enum": ["summary", "json", "dsl"], "default": "summary" }
                }),
                &["vi"],
            ),
        ),
        tool(
            "vi_validate",
            "Type-check a VI and return diagnostics with machine-actionable fixes.",
            object(json!({ "vi": { "type": "string" } }), &["vi"]),
        ),
        tool(
            "vi_run",
            "Execute a VI headlessly. `inputs` maps control keys to JSON values.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "inputs": { "type": "object", "additionalProperties": true },
                    "trace": { "type": "boolean", "default": false },
                    "timeoutMs": { "type": "integer" },
                    "maxIterations": { "type": "integer" },
                    "allowFile": { "type": "boolean", "default": false }
                }),
                &["vi"],
            ),
        ),
        tool(
            "vi_run_tests",
            "Run the VI's embedded test cases.",
            object(json!({ "vi": { "type": "string" } }), &["vi"]),
        ),
        tool(
            "vi_save",
            "Save a VI as canonical JSON. Uses the path from vi_open/vi_save when omitted.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "path": { "type": "string" }
                }),
                &["vi"],
            ),
        ),
        tool(
            "vi_open",
            "Open a VI from a JSON file.",
            object(json!({ "path": { "type": "string" } }), &["path"]),
        ),
        tool(
            "vi_list",
            "List open VIs.",
            object(json!({}), &[]),
        ),
        tool(
            "vi_delete",
            "Remove a VI from the server's in-memory set (does not touch disk).",
            object(json!({ "vi": { "type": "string" } }), &["vi"]),
        ),
        tool(
            "nodes_search",
            "Search the standard node library. Returns ports, types and an example.",
            object(
                json!({
                    "query": { "type": "string" },
                    "category": { "type": "string" },
                    "limit": { "type": "integer", "default": 20 }
                }),
                &[],
            ),
        ),
        tool(
            "nodes_get",
            "Get one node definition by id or alias.",
            object(json!({ "id": { "type": "string" } }), &["id"]),
        ),
        tool(
            "nodes_categories",
            "List node categories and their node ids.",
            object(json!({}), &[]),
        ),
        tool(
            "types_list",
            "List the LabCraft type names accepted by panel and constant tools.",
            object(json!({}), &[]),
        ),
        tool(
            "vi_render",
            "Render the panel and/or diagram to SVG using the shared theme tokens.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "view": { "type": "string", "enum": ["panel", "diagram", "both"], "default": "both" },
                    "theme": { "type": "string", "enum": ["dark", "light"], "default": "dark" }
                }),
                &["vi"],
            ),
        ),
        tool(
            "panel_add_control",
            "Add a front-panel control or indicator. Also adds its block-diagram terminal unless `terminal` is false.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "key": { "type": "string", "description": "Stable identifier used by wires and inputs" },
                    "label": { "type": "string" },
                    "kind": { "type": "string", "enum": control_kinds() },
                    "type": { "description": "Type name (dbl, i32, bool, string, dbl[]) or Type JSON" },
                    "indicator": { "type": "boolean", "default": false },
                    "default": { "description": "Default value" },
                    "x": { "type": "number" }, "y": { "type": "number" },
                    "width": { "type": "number" }, "height": { "type": "number" },
                    "unit": { "type": "string" }, "min": { "type": "number" },
                    "max": { "type": "number" }, "step": { "type": "number" },
                    "decimals": { "type": "integer" }, "color": { "type": "string" },
                    "options": { "type": "array", "items": { "type": "string" } },
                    "terminal": { "type": "boolean", "default": true }
                }),
                &["vi", "key"],
            ),
        ),
        tool(
            "panel_update_control",
            "Update a front-panel control's label, type, widget, default or style.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "key": { "type": "string" },
                    "label": { "type": "string" },
                    "kind": { "type": "string", "enum": control_kinds() },
                    "type": {},
                    "indicator": { "type": "boolean" },
                    "default": {},
                    "x": { "type": "number" }, "y": { "type": "number" },
                    "width": { "type": "number" }, "height": { "type": "number" },
                    "unit": { "type": "string" }, "min": { "type": "number" },
                    "max": { "type": "number" }, "step": { "type": "number" },
                    "decimals": { "type": "integer" }, "color": { "type": "string" },
                    "options": { "type": "array", "items": { "type": "string" } }
                }),
                &["vi", "key"],
            ),
        ),
        tool(
            "panel_remove_control",
            "Remove a front-panel control and any terminals that reference it.",
            object(
                json!({ "vi": { "type": "string" }, "key": { "type": "string" } }),
                &["vi", "key"],
            ),
        ),
        tool(
            "diagram_add_node",
            "Add a node to the root diagram scope. Input specs are node keys/ids or constant specs.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "kind": { "type": "string", "description": "Node id from nodes_search, or control/indicator/constant" },
                    "key": { "type": "string" },
                    "label": { "type": "string" },
                    "inputs": { "type": "object", "additionalProperties": true },
                    "props": { "type": "object", "additionalProperties": true },
                    "x": { "type": "number" }, "y": { "type": "number" },
                    "width": { "type": "number" }, "height": { "type": "number" }
                }),
                &["vi", "kind"],
            ),
        ),
        tool(
            "diagram_add_wire",
            "Wire one node output to another node input. Use `{node, port}` or `\"node.port\"`.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "from": {},
                    "to": {}
                }),
                &["vi", "from", "to"],
            ),
        ),
        tool(
            "diagram_remove_node",
            "Remove a node by key or id.",
            object(
                json!({ "vi": { "type": "string" }, "node": { "type": "string" } }),
                &["vi", "node"],
            ),
        ),
        tool(
            "vi_apply_ops",
            "Apply several tool calls in order. Each op is `{tool, args}` (or `{op, ...args}`).",
            object(
                json!({
                    "vi": { "type": "string" },
                    "ops": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool": { "type": "string" },
                                "args": { "type": "object" }
                            }
                        }
                    }
                }),
                &["ops"],
            ),
        ),
        tool(
            "vi_diff",
            "Compare two VIs by panel keys and diagram node keys.",
            object(
                json!({ "a": { "type": "string" }, "b": { "type": "string" } }),
                &["a", "b"],
            ),
        ),
        tool(
            "vi_autofix",
            "Apply safe machine fixes from diagnostics (missing controls, terminal kind, port names).",
            object(json!({ "vi": { "type": "string" } }), &["vi"]),
        ),
        tool(
            "vi_set_tests",
            "Replace a VI's embedded test cases. Each case has name, inputs and expected indicators.",
            object(
                json!({
                    "vi": { "type": "string" },
                    "tests": { "type": "array", "items": { "type": "object" } }
                }),
                &["vi", "tests"],
            ),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Json) -> Json {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn object(properties: Json, required: &[&str]) -> Json {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn control_kinds() -> Vec<&'static str> {
    vec![
        "numeric", "slider", "toggle", "led", "string", "path", "button", "chart", "graph", "enum",
        "table", "cluster", "array",
    ]
}

// ---- tools/call ---------------------------------------------------------

fn call_tool_message(state: &mut ServerState, params: &Json) -> Result<Json, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Json::as_str)
        .ok_or((-32602, "tools/call requires `name`".to_string()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match call_tool(state, name, &args) {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": value,
                "isError": false
            }))
        }
        Err(message) => Ok(json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        })),
    }
}

fn call_tool(state: &mut ServerState, name: &str, args: &Json) -> Result<Json, String> {
    match name {
        "vi_create" => vi_create(state, args),
        "vi_create_from_spec" => vi_create_from_spec(state, args),
        "vi_apply_dsl" => vi_apply_dsl(state, args),
        "vi_export_dsl" => vi_export_dsl(state, args),
        "vi_get" => vi_get(state, args),
        "vi_validate" => vi_validate(state, args),
        "vi_run" => vi_run(state, args),
        "vi_run_tests" => vi_run_tests(state, args),
        "vi_save" => vi_save(state, args),
        "vi_open" => vi_open(state, args),
        "vi_list" => Ok(vi_list(state)),
        "vi_delete" => vi_delete(state, args),
        "nodes_search" => Ok(nodes_search(args)),
        "nodes_get" => nodes_get(args),
        "nodes_categories" => Ok(json!({ "categories": Registry::global().categories() })),
        "types_list" => Ok(types_list()),
        "vi_render" => vi_render(state, args),
        "panel_add_control" => panel_add_control(state, args),
        "panel_update_control" => panel_update_control(state, args),
        "panel_remove_control" => panel_remove_control(state, args),
        "diagram_add_node" => diagram_add_node(state, args),
        "diagram_add_wire" => diagram_add_wire(state, args),
        "diagram_remove_node" => diagram_remove_node(state, args),
        "vi_apply_ops" => vi_apply_ops(state, args),
        "vi_diff" => vi_diff(state, args),
        "vi_autofix" => vi_autofix(state, args),
        "vi_set_tests" => vi_set_tests(state, args),
        other => Err(format!("unknown tool `{other}`")),
    }
}

// ---- VI lifecycle -------------------------------------------------------

fn vi_create(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let name = req_str(args, "name")?;
    let mut doc = ViDocument::new(name);
    if let Some(d) = opt_str(args, "description") {
        doc.description = d.to_string();
    }
    lc_layout::layout_vi(&mut doc);
    let id = state.insert(doc, None);
    Ok(json!({ "vi": id, "summary": summarize(state.entry(&id)?.doc()) }))
}

fn vi_create_from_spec(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let entry = if let Some(dsl) = opt_str(args, "dsl") {
        parse_dsl_doc(dsl)?
    } else if let Some(spec) = args.get("json").or_else(|| args.get("spec")) {
        if let Some(text) = spec.as_str() {
            if text.trim_start().starts_with('{') {
                let doc = ViDocument::from_json(text).map_err(|e| e.to_string())?;
                (doc, Vec::new())
            } else {
                parse_dsl_doc(text)?
            }
        } else {
            let doc: ViDocument =
                serde_json::from_value(spec.clone()).map_err(|e| e.to_string())?;
            (doc, Vec::new())
        }
    } else {
        return Err("vi_create_from_spec needs `dsl`, `json` or `spec`".into());
    };

    let (mut doc, diagnostics) = entry;
    lc_layout::layout_vi(&mut doc);
    let id = state.insert(doc, None);
    Ok(json!({
        "vi": id,
        "diagnostics": diagnostics,
        "summary": summarize(state.entry(&id)?.doc())
    }))
}

fn parse_dsl_doc(dsl: &str) -> Result<(ViDocument, Vec<Json>), String> {
    let outcome = lc_dsl::build(dsl).map_err(|e| e.render())?;
    let diagnostics = diagnostics_json(&outcome.diagnostics);
    Ok((outcome.doc, diagnostics))
}

fn vi_apply_dsl(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let dsl = req_str(args, "dsl")?;
    let id = state
        .resolve_id(handle)
        .ok_or_else(|| format!("unknown VI `{handle}`"))?;
    let outcome = lc_dsl::build(dsl).map_err(|e| e.render())?;
    let mut doc = outcome.doc;
    doc.id = id.clone();
    lc_layout::layout_vi(&mut doc);
    let path = state.docs.get(&id).and_then(|e| e.path.clone());
    state.insert(doc, path);
    Ok(json!({
        "vi": id,
        "diagnostics": diagnostics_json(&outcome.diagnostics),
        "summary": summarize(state.entry(&id)?.doc())
    }))
}

fn vi_export_dsl(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let doc = state.entry(req_str(args, "vi")?)?.doc();
    Ok(json!({ "dsl": lc_dsl::emit(doc) }))
}

fn vi_get(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let doc = state.entry(req_str(args, "vi")?)?.doc();
    match opt_str(args, "format").unwrap_or("summary") {
        "json" => Ok(json!({ "vi": doc.id, "document": doc })),
        "dsl" => Ok(json!({ "vi": doc.id, "dsl": lc_dsl::emit(doc) })),
        _ => Ok(json!({ "vi": doc.id, "summary": summarize(doc) })),
    }
}

fn vi_validate(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let doc = state.entry(req_str(args, "vi")?)?.doc();
    let report = lc_runtime::compile(doc);
    let diagnostics = diagnostics_json(&report.diagnostics);
    Ok(json!({
        "vi": doc.id,
        "ok": report.ok(),
        "diagnostics": diagnostics
    }))
}

fn vi_run(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let doc = state.entry(req_str(args, "vi")?)?.doc().clone();
    let mut inputs = BTreeMap::new();
    if let Some(obj) = args.get("inputs").and_then(Json::as_object) {
        for (k, v) in obj {
            let value: Value =
                serde_json::from_value(v.clone()).map_err(|e| format!("input `{k}`: {e}"))?;
            inputs.insert(k.clone(), value);
        }
    }
    let opts = lc_runtime::RunOptions {
        inputs,
        trace: args.get("trace").and_then(Json::as_bool).unwrap_or(false),
        timeout_ms: args
            .get("timeoutMs")
            .and_then(Json::as_u64)
            .unwrap_or(15_000),
        max_iterations: args
            .get("maxIterations")
            .and_then(Json::as_u64)
            .unwrap_or(1_000_000),
        allow_file: args
            .get("allowFile")
            .and_then(Json::as_bool)
            .unwrap_or(false),
        ..lc_runtime::RunOptions::defaults()
    };
    match lc_runtime::run(&doc, &opts) {
        Ok(out) => {
            let value = serde_json::to_value(&out).map_err(|e| e.to_string())?;
            Ok(json!({ "vi": doc.id, "result": value }))
        }
        Err(lc_runtime::RunError::Compile(diags)) => Err(format!(
            "VI does not compile:\n{}",
            diagnostics_json(&diags)
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

fn vi_run_tests(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let doc = state.entry(req_str(args, "vi")?)?.doc();
    let report = lc_runtime::run_tests(doc);
    Ok(json!({
        "vi": doc.id,
        "report": serde_json::to_value(&report).map_err(|e| e.to_string())?
    }))
}

fn vi_save(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let id = state
        .resolve_id(handle)
        .ok_or_else(|| format!("unknown VI `{handle}`"))?;
    let path = match opt_str(args, "path") {
        Some(p) => PathBuf::from(p),
        None => state
            .docs
            .get(&id)
            .and_then(|e| e.path.clone())
            .ok_or_else(|| "no path supplied and this VI has not been saved before".to_string())?,
    };
    let entry = state.docs.get_mut(&id).ok_or("VI disappeared")?;
    entry.doc.save(&path).map_err(|e| e.to_string())?;
    entry.path = Some(path.clone());
    Ok(json!({ "vi": id, "path": path.display().to_string() }))
}

fn vi_open(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let path = PathBuf::from(req_str(args, "path")?);
    let doc = ViDocument::load(&path).map_err(|e| e.to_string())?;
    let summary = summarize(&doc);
    let id = state.insert(doc, Some(path.clone()));
    Ok(json!({
        "vi": id,
        "path": path.display().to_string(),
        "summary": summary
    }))
}

fn vi_list(state: &mut ServerState) -> Json {
    let mut out = Vec::new();
    for id in &state.order {
        if let Some(entry) = state.docs.get(id) {
            out.push(summarize(&entry.doc));
        }
    }
    json!({ "vis": out })
}

fn vi_delete(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let id = state
        .resolve_id(handle)
        .ok_or_else(|| format!("unknown VI `{handle}`"))?;
    state.docs.remove(&id);
    state.order.retain(|x| x != &id);
    Ok(json!({ "vi": id, "deleted": true }))
}

fn vi_set_tests(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let tests = args
        .get("tests")
        .and_then(Json::as_array)
        .ok_or("`tests` must be an array")?;
    let parsed: Vec<lc_model::document::TestCase> =
        serde_json::from_value(Json::Array(tests.clone())).map_err(|e| e.to_string())?;
    let entry = state.entry_mut(handle)?;
    entry.doc.tests = parsed;
    entry.doc.touch();
    Ok(json!({ "vi": entry.doc.id, "tests": entry.doc.tests }))
}

// ---- Panel editing ------------------------------------------------------

fn panel_add_control(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let key = req_str(args, "key")?;
    let entry = state.entry_mut(handle)?;
    if entry.doc.control(key).is_some() {
        return Err(format!("a control with key `{key}` already exists"));
    }
    let indicator = args
        .get("indicator")
        .and_then(Json::as_bool)
        .unwrap_or(false);
    let ty = match args.get("type") {
        Some(v) if !v.is_null() => parse_type_value(v)?,
        _ => {
            let kind = match opt_str(args, "kind") {
                Some(k) => parse_control_kind(k)?,
                None => ControlKind::Numeric,
            };
            kind.default_type()
        }
    };
    let kind = match opt_str(args, "kind") {
        Some(k) => parse_control_kind(k)?,
        None => default_kind_for(&ty, indicator),
    };
    let label = opt_str(args, "label").unwrap_or(key).to_string();
    let mut control = Control::new(key, kind, label);
    control.indicator = indicator;
    control.ty = ty.clone();
    control.default = default_value(args.get("default"), &ty);
    if let Some(v) = args.get("x").and_then(Json::as_f64) {
        control.bounds.x = v as f32;
    }
    if let Some(v) = args.get("y").and_then(Json::as_f64) {
        control.bounds.y = v as f32;
    }
    if let Some(v) = args.get("width").and_then(Json::as_f64) {
        control.bounds.w = v as f32;
    }
    if let Some(v) = args.get("height").and_then(Json::as_f64) {
        control.bounds.h = v as f32;
    }
    if let Some(v) = args.get("terminal").and_then(Json::as_bool) {
        if v {
            add_terminal(&mut entry.doc, key, indicator);
        }
    } else {
        add_terminal(&mut entry.doc, key, indicator);
    }
    apply_style_patch(&mut control.style, args);
    if let Some(options) = args.get("options").and_then(Json::as_array) {
        control.style.options = options
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect();
        if matches!(control.ty, Type::Enum { .. }) {
            control.ty = Type::Enum {
                values: control.style.options.clone(),
            };
        }
    }
    entry.control_connector_sync();
    entry.doc.panel.controls.push(control);
    entry.doc.touch();
    lc_layout::layout_vi(&mut entry.doc);
    Ok(json!({
        "vi": entry.doc.id,
        "key": key,
        "control": entry.doc.control(key)
    }))
}

fn panel_update_control(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let key = req_str(args, "key")?.to_string();
    let entry = state.entry_mut(handle)?;
    let control = entry
        .doc
        .control_mut(&key)
        .ok_or_else(|| format!("no control with key `{key}`"))?;
    if let Some(label) = opt_str(args, "label") {
        control.label = label.to_string();
    }
    if let Some(indicator) = args.get("indicator").and_then(Json::as_bool) {
        control.indicator = indicator;
    }
    if let Some(v) = args.get("type").filter(|v| !v.is_null()) {
        control.ty = parse_type_value(v)?;
        control.default = Value::default_for(&control.ty);
    }
    if let Some(kind) = opt_str(args, "kind") {
        control.kind = parse_control_kind(kind)?;
    }
    if let Some(v) = args.get("default") {
        control.default = default_value(Some(v), &control.ty);
    }
    if let Some(v) = args.get("x").and_then(Json::as_f64) {
        control.bounds.x = v as f32;
    }
    if let Some(v) = args.get("y").and_then(Json::as_f64) {
        control.bounds.y = v as f32;
    }
    if let Some(v) = args.get("width").and_then(Json::as_f64) {
        control.bounds.w = v as f32;
    }
    if let Some(v) = args.get("height").and_then(Json::as_f64) {
        control.bounds.h = v as f32;
    }
    apply_style_patch(&mut control.style, args);
    if let Some(options) = args.get("options").and_then(Json::as_array) {
        control.style.options = options
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect();
        control.ty = Type::Enum {
            values: control.style.options.clone(),
        };
    }
    let updated = control.clone();
    entry.control_connector_sync();
    entry.doc.touch();
    Ok(json!({ "vi": entry.doc.id, "key": key, "control": updated }))
}

fn panel_remove_control(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let key = req_str(args, "key")?.to_string();
    let entry = state.entry_mut(handle)?;
    let before = entry.doc.panel.controls.len();
    entry.doc.panel.controls.retain(|c| c.key != key);
    if entry.doc.panel.controls.len() == before {
        return Err(format!("no control with key `{key}`"));
    }
    remove_terminals(&mut entry.doc.diagram.root, &key);
    entry.control_connector_sync();
    entry.doc.touch();
    Ok(json!({ "vi": entry.doc.id, "key": key, "removed": true }))
}

impl DocEntry {
    fn doc(&self) -> &ViDocument {
        &self.doc
    }

    /// Keep the connector pane in sync with panel terminals. The connector is
    /// what a future subVI call node exposes.
    fn control_connector_sync(&mut self) {
        self.doc.connector.inputs.clear();
        self.doc.connector.outputs.clear();
        for c in &self.doc.panel.controls {
            let pin = ConnectorPin {
                name: c.label.clone(),
                ty: c.ty.clone(),
                direction: if c.is_indicator() {
                    Direction::Output
                } else {
                    Direction::Input
                },
            };
            if c.is_indicator() {
                self.doc.connector.outputs.push(pin);
            } else {
                self.doc.connector.inputs.push(pin);
            }
        }
    }
}

fn add_terminal(doc: &mut ViDocument, key: &str, indicator: bool) {
    let kind = if indicator {
        kinds::INDICATOR
    } else {
        kinds::CONTROL
    };
    let node_key = unique_node_key(doc, key);
    let mut node = Node::new(kind, node_key);
    node.props.insert("control".into(), json!(key));
    doc.diagram.root.nodes.push(node);
}

fn remove_terminals(scope: &mut Scope, key: &str) {
    scope
        .nodes
        .retain(|n| n.props.get("control").and_then(Json::as_str) != Some(key));
    for node in &mut scope.nodes {
        for frame in &mut node.frames {
            remove_terminals(&mut frame.scope, key);
        }
    }
}

fn apply_style_patch(style: &mut ControlStyle, args: &Json) {
    if let Some(v) = opt_str(args, "unit") {
        style.unit = Some(v.to_string());
    }
    if let Some(v) = args.get("min").and_then(Json::as_f64) {
        style.min = Some(v);
    }
    if let Some(v) = args.get("max").and_then(Json::as_f64) {
        style.max = Some(v);
    }
    if let Some(v) = args.get("step").and_then(Json::as_f64) {
        style.step = Some(v);
    }
    if let Some(v) = args.get("decimals").and_then(Json::as_u64) {
        style.decimals = Some(v.min(255) as u8);
    }
    if let Some(v) = opt_str(args, "color") {
        style.color = Some(v.to_string());
    }
    if let Some(v) = args.get("showLabel").and_then(Json::as_bool) {
        style.show_label = v;
    }
}

// ---- Diagram editing ----------------------------------------------------

fn diagram_add_node(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let kind = req_str(args, "kind")?.to_string();
    let entry = state.entry_mut(handle)?;
    let node_key = match opt_str(args, "key") {
        Some(k) => {
            if entry.doc.node_by_key(k).is_some() {
                return Err(format!("a node with key `{k}` already exists"));
            }
            k.to_string()
        }
        None => unique_node_key(&entry.doc, &kind.replace('.', "_")),
    };
    let mut node = Node::new(&kind, node_key.clone());
    if let Some(label) = opt_str(args, "label") {
        node.label = Some(label.to_string());
    }
    if let Some(pos) = args.get("x").and_then(Json::as_f64) {
        node.pos.x = pos as f32;
    }
    if let Some(pos) = args.get("y").and_then(Json::as_f64) {
        node.pos.y = pos as f32;
    }
    if let Some(w) = args.get("width").and_then(Json::as_f64) {
        if let Some(h) = args.get("height").and_then(Json::as_f64) {
            node.size = Some(lc_model::document::Point::new(w as f32, h as f32));
        }
    }
    if let Some(props) = args.get("props").and_then(Json::as_object) {
        for (k, v) in props {
            node.props.insert(k.clone(), v.clone());
        }
    }
    if let Some(inputs) = args.get("inputs").and_then(Json::as_object) {
        let snapshot = entry.doc.clone();
        for (port, spec) in inputs {
            let src = resolve_source(&snapshot, &mut entry.doc.diagram.root, spec)?;
            node.inputs.insert(port.clone(), src);
        }
    }
    let id = node.id.clone();
    entry.doc.diagram.root.nodes.push(node);
    entry.doc.touch();
    lc_layout::layout_vi(&mut entry.doc);
    Ok(json!({
        "vi": entry.doc.id,
        "node": id,
        "key": node_key,
        "kind": kind
    }))
}

fn diagram_add_wire(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let entry = state.entry_mut(handle)?;
    let from = args.get("from").ok_or("missing `from`")?.clone();
    let to = args.get("to").ok_or("missing `to`")?.clone();
    let snapshot = entry.doc.clone();
    let src = resolve_source(&snapshot, &mut entry.doc.diagram.root, &from)?;
    let (target_key, port) = parse_node_ref(&to)?;
    let target = entry
        .doc
        .node_by_key(&target_key)
        .or_else(|| entry.doc.node_by_id(&target_key))
        .ok_or_else(|| format!("unknown target node `{target_key}`"))?;
    let target_id = target.id.clone();
    let port = match port {
        Some(p) => p,
        None => default_input_port(target)?,
    };
    let node = find_node_mut(&mut entry.doc.diagram.root, &target_id)
        .ok_or_else(|| "target node disappeared".to_string())?;
    node.inputs.insert(port.clone(), src);
    entry.doc.touch();
    Ok(json!({
        "vi": entry.doc.id,
        "to": { "node": target_key, "port": port }
    }))
}

fn diagram_remove_node(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let key = req_str(args, "node")?.to_string();
    let entry = state.entry_mut(handle)?;
    let id = entry
        .doc
        .node_by_key(&key)
        .or_else(|| entry.doc.node_by_id(&key))
        .map(|n| n.id.clone())
        .ok_or_else(|| format!("unknown node `{key}`"))?;
    remove_node_deep(&mut entry.doc.diagram.root, &id);
    entry.doc.touch();
    Ok(json!({ "vi": entry.doc.id, "node": key, "removed": true }))
}

fn remove_node_deep(scope: &mut Scope, id: &str) {
    scope.nodes.retain(|n| n.id != id);
    for node in &mut scope.nodes {
        for (port, src) in node.inputs.clone() {
            if src.source_node() == Some(id) {
                node.inputs.remove(&port);
            }
        }
        for frame in &mut node.frames {
            remove_node_deep(&mut frame.scope, id);
        }
    }
}

fn unique_node_key(doc: &ViDocument, base: &str) -> String {
    let base = sanitize(base);
    if doc.node_by_key(&base).is_none() {
        return base;
    }
    let mut i = 2;
    loop {
        let key = format!("{base}_{i}");
        if doc.node_by_key(&key).is_none() {
            return key;
        }
        i += 1;
    }
}

fn default_input_port(node: &Node) -> Result<String, String> {
    if node.kind == kinds::INDICATOR {
        return Ok("value".into());
    }
    if let Some(def) = Registry::global().get(&node.kind) {
        if let Some(p) = def.inputs.first() {
            return Ok(p.name.to_string());
        }
    }
    Err(format!("`{}` has no input ports", node.key))
}

fn primary_port(node: &Node) -> Option<String> {
    match node.kind.as_str() {
        kinds::CONTROL | kinds::CONSTANT => Some("value".into()),
        kinds::INDICATOR => None,
        k if lc_model::document::is_structure(k) => {
            let outs: Vec<_> = node
                .tunnels
                .iter()
                .filter(|t| t.direction == lc_model::TunnelDirection::Out)
                .collect();
            if outs.len() == 1 {
                Some(format!("t:{}", outs[0].name))
            } else {
                None
            }
        }
        kind => Registry::global()
            .get(kind)
            .and_then(|d| d.outputs.first())
            .map(|p| p.name.to_string()),
    }
}

fn parse_node_ref(spec: &Json) -> Result<(String, Option<String>), String> {
    match spec {
        Json::String(s) => {
            if let Some((node, port)) = s.rsplit_once('.') {
                Ok((node.to_string(), Some(port.to_string())))
            } else {
                Ok((s.clone(), None))
            }
        }
        Json::Object(map) => {
            let node = map
                .get("node")
                .and_then(Json::as_str)
                .ok_or("node reference needs `node`")?
                .to_string();
            let port = map.get("port").and_then(Json::as_str).map(str::to_string);
            Ok((node, port))
        }
        _ => Err("expected a node reference (`\"key.port\"` or `{node, port}`)".into()),
    }
}

fn resolve_source(doc: &ViDocument, scope: &mut Scope, spec: &Json) -> Result<SourceRef, String> {
    if let Json::String(s) = spec {
        if let Some(text) = s.strip_prefix("$const:") {
            let value: Json =
                serde_json::from_str(text).map_err(|e| format!("constant JSON: {e}"))?;
            return add_constant(scope, value, None);
        }
    }
    if let Json::Object(map) = spec {
        if let Some(constant) = map.get("const") {
            let ty = map.get("type").map(parse_type_value).transpose()?;
            return add_constant(scope, constant.clone(), ty);
        }
    }
    let (key, port) = parse_node_ref(spec)?;
    let node = doc
        .node_by_key(&key)
        .or_else(|| doc.node_by_id(&key))
        .ok_or_else(|| format!("unknown node `{key}`"))?;
    let port = match port {
        Some(p) => p,
        None => primary_port(node).ok_or_else(|| {
            format!("node `{key}` has several/no outputs; specify `{key}.<port>`",)
        })?,
    };
    Ok(SourceRef::Node {
        node: node.id.clone(),
        port,
    })
}

fn add_constant(scope: &mut Scope, value: Json, ty: Option<Type>) -> Result<SourceRef, String> {
    let parsed: Value = serde_json::from_value(value).map_err(|e| format!("constant: {e}"))?;
    let ty = ty.unwrap_or_else(|| parsed.type_of());
    let key = unique_scope_key(scope, "const");
    let mut node = Node::new(kinds::CONSTANT, key);
    node.props.insert(
        "type".into(),
        serde_json::to_value(&ty).map_err(|e| e.to_string())?,
    );
    node.props.insert(
        "value".into(),
        serde_json::to_value(&parsed).map_err(|e| e.to_string())?,
    );
    node.size = Some(lc_model::document::Point::new(130.0, 34.0));
    let id = node.id.clone();
    scope.nodes.push(node);
    Ok(SourceRef::Node {
        node: id,
        port: "value".into(),
    })
}

fn unique_scope_key(scope: &Scope, base: &str) -> String {
    if scope.node_by_key(base).is_none() {
        return base.to_string();
    }
    let mut i = 2;
    loop {
        let key = format!("{base}_{i}");
        if scope.node_by_key(&key).is_none() {
            return key;
        }
        i += 1;
    }
}

fn find_node_mut<'a>(scope: &'a mut Scope, id: &str) -> Option<&'a mut Node> {
    if let Some(i) = scope.nodes.iter().position(|n| n.id == id) {
        return scope.nodes.get_mut(i);
    }
    for node in &mut scope.nodes {
        for frame in &mut node.frames {
            if let Some(found) = find_node_mut(&mut frame.scope, id) {
                return Some(found);
            }
        }
    }
    None
}

// ---- Batches, diff, autofix --------------------------------------------

fn vi_apply_ops(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let vi = opt_str(args, "vi").map(str::to_string);
    let ops = args
        .get("ops")
        .and_then(Json::as_array)
        .ok_or("`ops` must be an array")?;
    let mut results = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        let tool = op
            .get("tool")
            .or_else(|| op.get("op"))
            .and_then(Json::as_str)
            .ok_or_else(|| format!("op {i} needs `tool` or `op`"))?
            .to_string();
        let mut op_args = op.get("args").cloned().unwrap_or_else(|| op.clone());
        if let (Some(vi), Json::Object(map)) = (&vi, &mut op_args) {
            map.entry("vi".to_string()).or_insert_with(|| json!(vi));
        }
        match call_tool(state, &tool, &op_args) {
            Ok(v) => results.push(json!({ "op": i, "tool": tool, "ok": true, "result": v })),
            Err(e) => {
                results.push(json!({ "op": i, "tool": tool, "ok": false, "error": e }));
                break;
            }
        }
    }
    Ok(json!({ "results": results }))
}

fn vi_diff(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let a = state.entry(req_str(args, "a")?)?.doc().clone();
    let b = state.entry(req_str(args, "b")?)?.doc().clone();
    let controls_a = control_map(&a);
    let controls_b = control_map(&b);
    let nodes_a = node_map(&a);
    let nodes_b = node_map(&b);
    let mut added_controls = Vec::new();
    let mut removed_controls = Vec::new();
    let mut changed_controls = Vec::new();
    for (key, value) in &controls_b {
        match controls_a.get(key) {
            None => added_controls.push(key.clone()),
            Some(prev) if prev != value => changed_controls.push(key.clone()),
            _ => {}
        }
    }
    for key in controls_a.keys() {
        if !controls_b.contains_key(key) {
            removed_controls.push(key.clone());
        }
    }
    let mut added_nodes = Vec::new();
    let mut removed_nodes = Vec::new();
    let mut changed_nodes = Vec::new();
    for (key, value) in &nodes_b {
        match nodes_a.get(key) {
            None => added_nodes.push(key.clone()),
            Some(prev) if prev != value => changed_nodes.push(key.clone()),
            _ => {}
        }
    }
    for key in nodes_a.keys() {
        if !nodes_b.contains_key(key) {
            removed_nodes.push(key.clone());
        }
    }
    Ok(json!({
        "panel": {
            "added": added_controls,
            "removed": removed_controls,
            "changed": changed_controls
        },
        "diagram": {
            "added": added_nodes,
            "removed": removed_nodes,
            "changed": changed_nodes
        }
    }))
}

fn control_map(doc: &ViDocument) -> BTreeMap<String, Json> {
    doc.panel
        .controls
        .iter()
        .map(|c| (c.key.clone(), serde_json::to_value(c).unwrap_or(Json::Null)))
        .collect()
}

fn node_map(doc: &ViDocument) -> BTreeMap<String, Json> {
    let mut out = BTreeMap::new();
    doc.for_each_node(&mut |n| {
        let value = json!({
            "kind": n.kind,
            "inputs": n.inputs,
            "props": n.props
        });
        out.insert(n.key.clone(), value);
    });
    out
}

fn vi_autofix(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let handle = req_str(args, "vi")?;
    let entry = state.entry_mut(handle)?;
    let mut applied: Vec<String> = Vec::new();

    for _ in 0..16 {
        let report = lc_runtime::compile(&entry.doc);
        if report.ok() {
            break;
        }
        let mut changed = false;
        for diag in &report.diagnostics {
            let Some(fix) = diag.fixes.first() else {
                continue;
            };
            let Some(node_id) = diag.node.as_deref() else {
                continue;
            };
            match fix.as_str() {
                "change_control_kind" => {
                    let Some(node) = entry.doc.node_by_id(node_id) else {
                        continue;
                    };
                    let Some(key) = node.control_key().map(str::to_string) else {
                        continue;
                    };
                    let wants_indicator = node.kind == kinds::INDICATOR;
                    if let Some(control) = entry.doc.control_mut(&key) {
                        if control.indicator != wants_indicator {
                            control.indicator = wants_indicator;
                            applied.push(format!("set `{key}` indicator = {wants_indicator}"));
                            changed = true;
                        }
                    }
                }
                "add_panel_control" => {
                    let Some(node) = entry.doc.node_by_id(node_id) else {
                        continue;
                    };
                    let Some(key) = node.control_key().map(str::to_string) else {
                        continue;
                    };
                    if entry.doc.control(&key).is_none() {
                        let indicator = node.kind == kinds::INDICATOR;
                        let mut control =
                            Control::new(&key, default_kind_for(&Type::dbl(), indicator), &key);
                        control.indicator = indicator;
                        entry.doc.panel.controls.push(control);
                        applied.push(format!("added panel control `{key}`"));
                        changed = true;
                    }
                }
                "set_source_port" => {
                    let Some(node) = entry.doc.node_by_id(node_id) else {
                        continue;
                    };
                    let node_id = node.id.clone();
                    let known: Vec<String> = Registry::global()
                        .get(&node.kind)
                        .map(|d| d.outputs.iter().map(|p| p.name.to_string()).collect())
                        .unwrap_or_default();
                    if known.len() == 1 {
                        let only = known[0].clone();
                        let mut rewired = 0;
                        entry.doc.for_each_node_mut(&mut |n| {
                            for src in n.inputs.values_mut() {
                                if let SourceRef::Node { node: from, port } = src {
                                    if from == &node_id && port != &only {
                                        *port = only.clone();
                                        rewired += 1;
                                    }
                                }
                            }
                        });
                        if rewired > 0 {
                            applied.push(format!("rewired {rewired} wire(s) to `{only}`"));
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }

    let report = lc_runtime::compile(&entry.doc);
    entry.control_connector_sync();
    entry.doc.touch();
    Ok(json!({
        "vi": entry.doc.id,
        "applied": applied,
        "ok": report.ok(),
        "diagnostics": diagnostics_json(&report.diagnostics)
    }))
}

// ---- Node and type catalogs --------------------------------------------

fn nodes_search(args: &Json) -> Json {
    let query = opt_str(args, "query").unwrap_or("");
    let category = opt_str(args, "category");
    let limit = args
        .get("limit")
        .and_then(Json::as_u64)
        .unwrap_or(20)
        .min(200) as usize;
    let mut infos = if query.is_empty() {
        Registry::global().infos()
    } else {
        Registry::global().search(query)
    };
    if let Some(category) = category {
        infos.retain(|i| i.category.eq_ignore_ascii_case(category));
    }
    let total = infos.len();
    infos.truncate(limit);
    json!({ "total": total, "nodes": infos })
}

fn nodes_get(args: &Json) -> Result<Json, String> {
    let id = req_str(args, "id")?;
    Registry::global()
        .info(id)
        .map(|info| json!({ "node": info }))
        .ok_or_else(|| format!("unknown node `{id}`; use nodes_search to find ids"))
}

fn types_list() -> Json {
    let types = [
        ("void", "No value"),
        ("bool", "Boolean"),
        ("i8", "8-bit signed integer"),
        ("i16", "16-bit signed integer"),
        ("i32", "32-bit signed integer"),
        ("i64", "64-bit signed integer"),
        ("u8", "8-bit unsigned integer"),
        ("u16", "16-bit unsigned integer"),
        ("u32", "32-bit unsigned integer"),
        ("u64", "64-bit unsigned integer"),
        ("sgl", "Single-precision float"),
        ("dbl", "Double-precision float"),
        ("ext", "Extended-precision float"),
        ("string", "UTF-8 string"),
        ("path", "Filesystem path"),
        ("timestamp", "LabVIEW-style timestamp"),
        ("error", "Error cluster"),
        ("variant", "Variant / any"),
        ("waveform", "Waveform"),
        ("dbl[]", "1-D array of doubles (append [] per dimension)"),
    ];
    json!({
        "types": types.iter().map(|(name, description)| json!({
            "name": name,
            "description": description
        })).collect::<Vec<_>>()
    })
}

fn vi_render(state: &mut ServerState, args: &Json) -> Result<Json, String> {
    let doc = state.entry(req_str(args, "vi")?)?.doc();
    let view = opt_str(args, "view").unwrap_or("both");
    let theme = match opt_str(args, "theme").unwrap_or("dark") {
        "light" => lc_theme::Theme::light(),
        _ => lc_theme::Theme::dark(),
    };
    let opts = lc_render::RenderOptions::default();
    let mut out = json!({ "vi": doc.id });
    if view == "panel" || view == "both" {
        out["panel"] = json!(lc_render::panel_svg(doc, &theme, &opts));
    }
    if view == "diagram" || view == "both" {
        out["diagram"] = json!(lc_render::diagram_svg(doc, &theme, &opts));
    }
    Ok(out)
}

// ---- Resources and prompts ---------------------------------------------

fn list_resources(state: &mut ServerState) -> Json {
    let mut resources = vec![
        json!({
            "uri": "labcraft://nodes",
            "name": "Standard node library",
            "description": "All primitive nodes with ports, types and examples",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "labcraft://types",
            "name": "LabCraft type system",
            "description": "Type names accepted by panel and constant tools",
            "mimeType": "application/json"
        }),
    ];
    for id in &state.order {
        if let Some(entry) = state.docs.get(id) {
            resources.push(json!({
                "uri": format!("labcraft://vi/{id}"),
                "name": entry.doc.name,
                "description": "Current VI as DSL text",
                "mimeType": "text/plain"
            }));
            resources.push(json!({
                "uri": format!("labcraft://vi/{id}/json"),
                "name": format!("{} (JSON)", entry.doc.name),
                "description": "Current VI as canonical JSON IR",
                "mimeType": "application/json"
            }));
        }
    }
    json!({ "resources": resources })
}

fn read_resource(state: &mut ServerState, params: &Json) -> Result<Json, (i64, String)> {
    let uri = params
        .get("uri")
        .and_then(Json::as_str)
        .ok_or((-32602, "resources/read requires `uri`".to_string()))?;
    let (mime, text) = match uri {
        "labcraft://nodes" => (
            "application/json",
            serde_json::to_string_pretty(&Registry::global().infos()).unwrap_or_default(),
        ),
        "labcraft://types" => (
            "application/json",
            serde_json::to_string_pretty(&types_list()).unwrap_or_default(),
        ),
        other if other.starts_with("labcraft://vi/") => {
            let rest = &other["labcraft://vi/".len()..];
            let (id, json_mode) = match rest.strip_suffix("/json") {
                Some(id) => (id, true),
                None => (rest, false),
            };
            let entry = state
                .docs
                .get(id)
                .ok_or((-32602, format!("unknown VI resource `{uri}`")))?;
            if json_mode {
                ("application/json", entry.doc.to_json().unwrap_or_default())
            } else {
                ("text/plain", lc_dsl::emit(&entry.doc))
            }
        }
        other => return Err((-32602, format!("unknown resource `{other}`"))),
    };
    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": mime, "text": text }]
    }))
}

fn prompt_definitions() -> Vec<Json> {
    vec![
        json!({
            "name": "create_vi",
            "title": "Create a VI",
            "description": "Author a complete VI from a natural-language requirement using the DSL",
            "arguments": [
                { "name": "requirement", "description": "What the VI should do", "required": true }
            ]
        }),
        json!({
            "name": "debug_vi",
            "title": "Debug a VI",
            "description": "Diagnose a VI from its id, diagnostics and last run output",
            "arguments": [
                { "name": "vi", "description": "VI id or name", "required": true }
            ]
        }),
    ]
}

fn get_prompt(params: &Json) -> Result<Json, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Json::as_str)
        .ok_or((-32602, "prompts/get requires `name`".to_string()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let requirement = args
        .get("requirement")
        .and_then(Json::as_str)
        .unwrap_or("a simple calculation");
    let vi = args.get("vi").and_then(Json::as_str).unwrap_or("<vi-id>");
    let text = match name {
        "create_vi" => format!(
            "Create a LabCraft VI for: {requirement}\n\n\
             Use vi_create_from_spec with a DSL program. Rules:\n\
             - Declare every panel control/indicator before wiring it.\n\
             - Use `let` for intermediate values and `->` for pipelines.\n\
             - Finish by calling vi_validate and vi_run with representative inputs.\n\
             If the requirement needs a node that does not exist, say so and use the closest primitive composition."
        ),
        "debug_vi" => format!(
            "Debug the LabCraft VI `{vi}`.\n\n\
             1. Call vi_get with format \"dsl\" and vi_validate.\n\
             2. Read diagnostics in order; each has code, node, port and fixes.\n\
             3. Make one change at a time with vi_apply_dsl or targeted panel_/diagram_ tools.\n\
             4. Re-run vi_validate, then vi_run to confirm the indicator values."
        ),
        other => return Err((-32602, format!("unknown prompt `{other}`"))),
    };
    Ok(json!({
        "description": name,
        "messages": [{
            "role": "user",
            "content": { "type": "text", "text": text }
        }]
    }))
}

// ---- Shared helpers -----------------------------------------------------

fn req_str<'a>(args: &'a Json, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("missing required string argument `{key}`"))
}

fn opt_str<'a>(args: &'a Json, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Json::as_str)
}

fn default_value(spec: Option<&Json>, ty: &Type) -> Value {
    match spec {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone())
            .ok()
            .and_then(|value: Value| value.coerce_to(ty).ok().or(Some(value)))
            .unwrap_or_else(|| Value::default_for(ty)),
        _ => Value::default_for(ty),
    }
}

fn parse_control_kind(name: &str) -> Result<ControlKind, String> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "numeric" | "number" => ControlKind::Numeric,
        "slider" => ControlKind::Slider,
        "toggle" | "switch" => ControlKind::Toggle,
        "led" => ControlKind::Led,
        "string" | "text" => ControlKind::String,
        "path" => ControlKind::Path,
        "button" => ControlKind::Button,
        "chart" => ControlKind::Chart,
        "graph" => ControlKind::Graph,
        "enum" | "ring" => ControlKind::Enum,
        "table" => ControlKind::Table,
        "cluster" => ControlKind::Cluster,
        "array" => ControlKind::Array,
        other => return Err(format!("unknown control kind `{other}`")),
    })
}

fn default_kind_for(ty: &Type, indicator: bool) -> ControlKind {
    match ty {
        Type::Bool => {
            if indicator {
                ControlKind::Led
            } else {
                ControlKind::Toggle
            }
        }
        Type::String => ControlKind::String,
        Type::Path => ControlKind::Path,
        Type::Enum { .. } => ControlKind::Enum,
        Type::Cluster { .. } => ControlKind::Cluster,
        Type::Array { element, .. } => match element.as_ref() {
            Type::Numeric { .. } if indicator => ControlKind::Chart,
            Type::Array { .. } => ControlKind::Table,
            _ => ControlKind::Array,
        },
        _ => ControlKind::Numeric,
    }
}

fn parse_type_value(value: &Json) -> Result<Type, String> {
    if value.is_object() {
        return serde_json::from_value(value.clone()).map_err(|e| e.to_string());
    }
    let name = value.as_str().ok_or("type must be a string or Type JSON")?;
    parse_type_name(name)
}

fn parse_type_name(name: &str) -> Result<Type, String> {
    let mut base = name.trim();
    let mut dims = 0usize;
    while let Some(stripped) = base.strip_suffix("[]") {
        dims += 1;
        base = stripped.trim();
    }
    let mut ty = match base.to_ascii_lowercase().as_str() {
        "void" => Type::Void,
        "bool" | "boolean" => Type::Bool,
        "string" | "str" => Type::String,
        "path" => Type::Path,
        "timestamp" | "time" => Type::Timestamp,
        "error" | "err" => Type::Error,
        "variant" | "any" => Type::Variant,
        "waveform" => Type::Waveform,
        "i8" => Type::numeric(lc_model::NumericType::I8),
        "i16" => Type::numeric(lc_model::NumericType::I16),
        "i32" | "int" => Type::numeric(lc_model::NumericType::I32),
        "i64" => Type::numeric(lc_model::NumericType::I64),
        "u8" => Type::numeric(lc_model::NumericType::U8),
        "u16" => Type::numeric(lc_model::NumericType::U16),
        "u32" => Type::numeric(lc_model::NumericType::U32),
        "u64" => Type::numeric(lc_model::NumericType::U64),
        "sgl" | "f32" | "float" => Type::numeric(lc_model::NumericType::Sgl),
        "dbl" | "f64" | "double" | "numeric" => Type::dbl(),
        "ext" => Type::numeric(lc_model::NumericType::Ext),
        "cluster" => Type::Cluster { fields: Vec::new() },
        other => return Err(format!("unknown type `{other}`")),
    };
    if dims > 0 {
        ty = Type::Array {
            element: Box::new(ty),
            dims,
        };
    }
    Ok(ty)
}

fn summarize(doc: &ViDocument) -> Json {
    let mut nodes = 0usize;
    let mut structures = 0usize;
    doc.for_each_node(&mut |n| {
        nodes += 1;
        if lc_model::document::is_structure(&n.kind) {
            structures += 1;
        }
    });
    json!({
        "id": doc.id,
        "name": doc.name,
        "description": doc.description,
        "revision": doc.revision,
        "controls": doc.panel.controls.iter().map(|c| json!({
            "key": c.key,
            "label": c.label,
            "kind": c.kind,
            "indicator": c.is_indicator(),
            "type": c.ty.display(),
            "default": c.default,
            "unit": c.style.unit,
        })).collect::<Vec<_>>(),
        "counts": {
            "controls": doc.panel.controls.len(),
            "nodes": nodes,
            "structures": structures,
            "tests": doc.tests.len()
        }
    })
}

fn diagnostics_json(diags: &[lc_model::Diagnostic]) -> Vec<Json> {
    diags
        .iter()
        .map(|d| serde_json::to_value(d).unwrap_or_else(|_| json!({ "message": d.message })))
        .collect()
}

fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('n');
    }
    if s.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        s.insert(0, 'n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(state: &mut ServerState, name: &str, args: Json) -> Json {
        call_tool(state, name, &args).expect("tool call")
    }

    #[test]
    fn create_run_and_export_a_vi() {
        let mut state = ServerState::new();
        let created = call(
            &mut state,
            "vi_create_from_spec",
            json!({
                "dsl": r#"
                    vi "Over Temperature" {
                      panel {
                        control temp "Temperature" @(20, 20) { type: dbl, default: 35.0 }
                        indicator alarm "Alarm" @(20, 90) { type: bool }
                      }
                      diagram {
                        temp -> greater(30.0) -> alarm
                      }
                    }
                "#
            }),
        );
        let vi = created["vi"].as_str().expect("vi id").to_string();
        assert_eq!(created["diagnostics"].as_array().unwrap().len(), 0);

        let run = call(
            &mut state,
            "vi_run",
            json!({ "vi": vi, "inputs": { "temp": 35.0 } }),
        );
        assert_eq!(run["result"]["indicators"]["alarm"], json!(true), "{run}");

        let exported = call(&mut state, "vi_export_dsl", json!({ "vi": vi }));
        assert!(exported["dsl"].as_str().unwrap().contains("-> alarm"));
    }

    #[test]
    fn panel_and_wire_tools_compose() {
        let mut state = ServerState::new();
        let created = call(&mut state, "vi_create", json!({ "name": "Tooling" }));
        let vi = created["vi"].as_str().unwrap().to_string();
        call(
            &mut state,
            "panel_add_control",
            json!({
                "vi": vi, "key": "a", "type": "dbl", "default": 2.0
            }),
        );
        call(
            &mut state,
            "panel_add_control",
            json!({
                "vi": vi, "key": "out", "type": "dbl", "indicator": true
            }),
        );
        let added = call(
            &mut state,
            "diagram_add_node",
            json!({
                "vi": vi, "kind": "add", "key": "adder",
                "inputs": { "x": "a", "y": { "const": 3.0 } }
            }),
        );
        assert_eq!(added["key"], "adder");
        call(
            &mut state,
            "diagram_add_wire",
            json!({
                "vi": vi, "from": "adder.sum", "to": "out"
            }),
        );
        let run = call(&mut state, "vi_run", json!({ "vi": vi }));
        assert_eq!(run["result"]["indicators"]["out"], json!(5.0), "{run}");
    }

    #[test]
    fn jsonrpc_lifecycle() {
        let mut state = ServerState::new();
        let init = handle_message(
            &mut state,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2024-11-05" }
            }),
        )
        .expect("response");
        assert_eq!(init["result"]["serverInfo"]["name"], "labcraft");
        assert_eq!(
            init["result"]["capabilities"]["tools"]["listChanged"],
            false
        );

        let list = handle_message(
            &mut state,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        )
        .expect("response");
        let tools = list["result"]["tools"].as_array().expect("tools");
        assert!(tools.iter().any(|t| t["name"] == "vi_create_from_spec"));

        let call = handle_message(
            &mut state,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "vi_create",
                    "arguments": { "name": "Transport" }
                }
            }),
        )
        .expect("response");
        assert_eq!(call["result"]["isError"], false, "{call}");
        assert!(call["result"]["structuredContent"]["vi"]
            .as_str()
            .unwrap()
            .starts_with("vi"));

        // Notifications never produce a response.
        assert!(handle_message(
            &mut state,
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
        )
        .is_none());
    }

    #[test]
    fn invalid_dsl_is_a_tool_error() {
        let mut state = ServerState::new();
        let err = call_tool(
            &mut state,
            "vi_create_from_spec",
            &json!({ "dsl": "vi \"Broken\" { panel { control x } " }),
        )
        .unwrap_err();
        assert!(
            err.contains("missing") || err.contains("unexpected"),
            "{err}"
        );
    }
}
