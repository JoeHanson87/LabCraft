# LabCRAFT

A ground-up, native **Rust** clone of National Instruments LabVIEW, built so that **AI agents can program VIs as easily as humans can** — and meant to compile for macOS, Linux and Windows.

> **Status: early but real.** The dataflow language, type system, structures and a dark-themed editor all work. See [`docs/ROADMAP.md`](docs/ROADMAP.md) for what exists and what is next.

## Why another LabVIEW?

LabVIEW VIs are opaque binary files edited only with a mouse, which is hostile to automation. LabCRAFT inverts that:

- The **VI is a plain data model** (`labcraft-core`, no GUI dependency, JSON-serialisable).
- **Every edit is a typed `Command`** (add node, connect, set type, add frame, …) with validation and undo. The GUI and — soon — an MCP server / text DSL are thin clients of the same API, so anything a human can drag, an agent can do.
- Programs run **headlessly** (`labcraft_core::run`), which makes CI and agent verification loops cheap.

## Quick start

Requires a recent stable Rust toolchain (developed on 1.99).

```sh
git clone https://github.com/JoeHanson87/LabCraft.git
cd LabCraft
cargo run --release        # launches the editor
cargo test                 # core, structures, and UI smoke tests
```

The app opens on an example. Use the toolbar **Examples** menu:

| Example | Shows |
|---|---|
| Loops and Graphs | For/While loops, shift registers, waveform chart & graph, gauge, knob, Stop button |
| Arrays and Clusters | typed numerics (U8 wrap-around), array/cluster polymorphism, enums, Select, Bundle/Unbundle |
| Structures | Case (enum selector), auto-indexing, Feedback Node, Event structure + Local Variable |

Basics: **Cmd/Ctrl+E** toggles Front Panel ⇄ Block Diagram, **Cmd/Ctrl+R** runs, **Cmd/Ctrl+Z** undoes. Drag from an output terminal to an input terminal to wire (type mismatches are rejected with a message). **Right-click** nearly anything — controls, constants, structure borders, nodes, terminals, wires — for its options.

## What works today

- **Language:** dataflow execution with While/For loops, shift registers, tunnels with **auto-indexing**, Case / Flat Sequence / Event / Diagram-Disable structures, Feedback Node, Local & Global variables, **parallel execution** of independent structures.
- **Types:** I8–I64, U8–U32, U64 (i64 range), SGL, DBL; Boolean; String; Enum; N-dimensional arrays; nested clusters; error cluster. Polymorphic nodes (an `array × scalar`, `I32 + DBL`, cluster-wise arithmetic) with coercion dots and LabVIEW-style integer wrap-around.
- **Front panel:** numeric control/indicator, knob, slide, gauge, thermometer, tank, toggle, push button, LED, string, enum, array, cluster, waveform chart and graph.
- **Editor:** dark theme by default, collapsible palettes, undo/redo, save/open (JSON), typed coloured wires (thin scalars, thick hollow arrays, dashed clusters), live indicator updates while running, abort.

## Repository layout

```
crates/
  labcraft-core/   headless VI model, Command API, type inference, evaluator   (no GUI deps)
  labcraft-app/    egui/eframe editor: front panel, block diagram, palettes
docs/
  ARCHITECTURE.md  how it fits together + recipes for adding nodes/widgets/structures
  ROADMAP.md       milestones, status, and the full remaining feature list
CONTRIBUTING.md    conventions and workflow
```

## Roadmap at a glance

✅ M1 model & GUI skeleton · ✅ M2 runtime, loops, graphs · ✅ M3 type system · ✅ M4 structures
⏭ **M7 front-panel depth** → M5 subVIs & projects → M9 editor ergonomics → M6 function library → M10 debugging → M8 I/O → **M11 agent interface (MCP + text DSL)** → M12 packaging & performance

Open work is tracked as GitHub milestones/issues; the detailed checklists live in [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Contributing

Pick an open issue (each milestone has one), read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md), and open a PR. The architecture doc has step-by-step recipes for the most common additions (a new node, a new panel widget, a new structure, a new command).

## License

MIT (see `Cargo.toml`); add a `LICENSE` file when ready to publish formally.
