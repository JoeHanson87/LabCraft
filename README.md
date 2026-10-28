# LabCraft

LabCraft is a ground-up Rust implementation of visual dataflow programming in the spirit of LabVIEW: a **front panel**, a **block diagram**, a **typed runtime**, and an authoring surface designed for AI agents.

It is not binary-compatible with LabVIEW `.vi` files and does not use NI assets or trademarks. Files are portable JSON documents, and the primary authoring format is a small text DSL that agents can write and read reliably.

```
┌──────────────────────────────────────────────────────────────┐
│  Front panel            │  Block diagram                    │
│  controls / indicators  │  nodes / wires / structures        │
├──────────────────────────────────────────────────────────────┤
│  DSL ⇄ JSON IR ⇄ editor ⇄ CLI ⇄ MCP server ⇄ runtime        │
└──────────────────────────────────────────────────────────────┘
```

## Status

This repository is the **M2 walking skeleton**: the path from "agent writes a VI" to "VI runs and renders" works end to end.

- Canonical JSON IR with stable ids and keys, structural validation, and a typed connector pane.
- ~55 primitive nodes (numeric, boolean, string, array, cluster, file, timing, error handling) with declarative port schemas and aliases.
- Static type checker with diagnostics and machine-actionable fixes.
- Deterministic dataflow interpreter with `while`, `for`, `case`, `sequence`, `event`, tunnels and error propagation.
- DSL parser, builder, and round-trip emitter.
- Layout engine and SVG rendering for panels and diagrams.
- MCP server over stdio with 27 tools, resources, and prompts.
- `labcraft` CLI: create, run, validate, export, render, test, open the editor, and serve MCP.
- Native `eframe`/`egui` editor with a dark-first UI, panel/diagram views, palette, inspector, diagnostics, and Run.

## Build

Install a current stable Rust toolchain, then:

```bash
cargo build
cargo test
```

The GUI is built by default. For a headless CLI/MCP-only build:

```bash
cargo build -p lc-cli --no-default-features
```

## Quick start

```bash
# Create a VI from DSL
cargo run -p lc-cli -- new over-temperature \
  --name "Over Temperature" --dsl examples/over-temperature.dsl

# Run it with an input
cargo run -p lc-cli -- run over-temperature --input temp=35
# => {"indicators":{"alarm":true}, ...}

# Inspect the graph as DSL again
cargo run -p lc-cli -- export over-temperature

# Validate, render, or run embedded tests
cargo run -p lc-cli -- validate over-temperature
cargo run -p lc-cli -- render over-temperature --view both --out /tmp/vi.svg
cargo run -p lc-cli -- test over-temperature

# Recreate the bundled examples as JSON and DSL
cargo run -p lc-cli -- examples --write examples/out
```

Open the editor:

```bash
cargo run -p lc-cli -- gui over-temperature.lcvi.json
```

## MCP server

Start the server on stdio:

```bash
cargo run -p lc-cli -- mcp
```

Register it with an MCP client. For example, in Claude Desktop or a compatible host:

```json
{
  "mcpServers": {
    "labcraft": {
      "command": "/absolute/path/to/labcraft",
      "args": ["mcp"]
    }
  }
}
```

OpenCode configuration is the same shape, under `mcp` in `opencode.json`:

```json
{
  "mcp": {
    "labcraft": {
      "type": "local",
      "command": ["/absolute/path/to/labcraft", "mcp"],
      "enabled": true
    }
  }
}
```

### Tool surface

| Group | Tools |
| --- | --- |
| Lifecycle | `vi_create`, `vi_create_from_spec`, `vi_apply_dsl`, `vi_open`, `vi_save`, `vi_list`, `vi_delete` |
| Authoring | `vi_export_dsl`, `vi_get`, `vi_apply_ops`, `vi_diff`, `vi_autofix`, `vi_set_tests` |
| Verification | `vi_validate`, `vi_run`, `vi_run_tests`, `vi_render` |
| Panel | `panel_add_control`, `panel_update_control`, `panel_remove_control` |
| Diagram | `diagram_add_node`, `diagram_add_wire`, `diagram_remove_node` |
| Catalog | `nodes_search`, `nodes_get`, `nodes_categories`, `types_list` |

Resources expose the node library, type system, and each open VI as DSL or JSON. Prompts provide a `create_vi` workflow and a `debug_vi` workflow.

Every tool returns human-readable text plus `structuredContent`, and errors are returned as tool errors (`isError: true`) so an agent can recover without restarting the session.

## DSL

The DSL is intentionally small. A program has one `panel` block and one `diagram` block:

```text
vi "Over Temperature" {
  description "Raises an alarm above the limit."

  panel {
    control temp "Temperature" @(40, 60) { type: dbl, default: 25.0, unit: "C" }
    control limit "Limit" @(40, 130) { type: dbl, default: 30.0, unit: "C" }
    indicator alarm "Alarm" @(40, 210) { type: bool }
  }

  diagram {
    temp -> greater(limit) -> alarm
  }
}
```

### Panel declarations

```
control  <key> ["Label"] [@(x, y)] { type: dbl, default: 0.0, unit: "V", ... }
indicator <key> ["Label"] [@(x, y)] { type: bool }
```

Common properties: `type`, `default`, `widget`, `unit`, `min`, `max`, `step`, `decimals`, `color`, `options`, `width`, `height`. Types are `dbl`, `sgl`, `i32`, `u32`, `bool`, `string`, `path`, `timestamp`, `error`, `variant`, `waveform`, and `[]` for arrays.

### Diagram

```
let total = add(a, b)          # bind a node's output
a -> add(b) -> result          # pipeline; the piped value fills the first free input
let hot = temp > limit         # comparison sugar for greater(temp, limit)
let x = bundle(name: "A", count: 3)
let name = x.name              # cluster field access
```

Comparison operators are `>`, `>=`, `<`, `<=`, `==` and `!=`. They compile to the
same comparison nodes as their function forms, so both spellings produce the
same graph.

Structures are expressions and can declare output tunnels:

```
let sum = for i in 0..n {
  tunnel value = add(i, 1)
}

let grade = case score {
  0..59 { tunnel out = "F" }
  60..89 { tunnel out = "Pass" }
  default { tunnel out = "Out of scope" }
}

let n = while stop {
  tunnel count = increment(count)
}
```

Inside a loop, `iteration` is available; `for` also exposes its loop variable. Event frames expose `event_data`. Outer bindings used inside a structure are tunnelled automatically.

## File format

A VI is one JSON file ending in `.lcvi.json`. It carries `formatVersion`, stable node ids, globally unique keys, the panel, the diagram scopes, the connector pane, metadata, and embedded test cases. The CLI and MCP server both read and write this format; the DSL always compiles to it.

```jsonc
{
  "formatVersion": 1,
  "id": "vi_...",
  "name": "Over Temperature",
  "panel": { "controls": [ /* ... */ ] },
  "diagram": { "root": { "nodes": [ /* ... */ ] } },
  "connector": { "inputs": [], "outputs": [] }
}
```

## Workspace

| Crate | Responsibility |
| --- | --- |
| `lc-model` | Types, values, document IR, structural validation |
| `lc-nodes` | Node registry, port schemas, type rules, execution ops |
| `lc-runtime` | Type checker, dataflow interpreter, structures, tests |
| `lc-layout` | Deterministic layout and orthogonal wire routing |
| `lc-dsl` | Lexer, parser, builder, DSL emitter |
| `lc-theme` | Dark/light theme tokens and wire colors |
| `lc-render` | SVG rendering for panels and diagrams |
| `lc-mcp` | Hand-rolled JSON-RPC/MCP stdio server |
| `lc-editor` | Native `eframe`/`egui` editor |
| `lc-cli` | `labcraft` binary |

The runtime is headless and has no UI dependency. The editor, CLI, and MCP server are clients of the same document model; the MCP tool surface is the intended command bus for agent edits.

## Testing

```bash
cargo test --workspace
```

Coverage currently includes DSL parse/build/run round-trips, loop and case semantics, MCP tool and JSON-RPC lifecycle tests, theme tokens, and rendering smoke tests. Add new primitives with a port schema, an execution op, and at least one runtime or DSL test.

## Roadmap

- Command bus with undo/redo and collaborative editing.
- SubVIs, connector-pane calls, and plugin nodes for instruments.
- TCP/UDP/HTTP and serial I/O nodes on top of the existing file/timing primitives.
- Graph pan/zoom polish, wire editing, and property dialogs in the editor.
- Screenshot/golden tests for the editor and rendered SVG.
- Packaging: signed binaries, installers, and MCP registry entries.

## License

MIT. LabCraft is an independent project and is not affiliated with or endorsed by National Instruments.
