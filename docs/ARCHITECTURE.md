# LabCRAFT architecture

This is the map for anyone picking up the project. Read it once, then use the **recipes** at the bottom.

## Design principle: the document is the product

`labcraft-core` owns the VI as plain data plus a **`Command` API**. The egui editor (`labcraft-app`) never edits a VI directly; it queues `Command`s. The planned MCP server and text DSL (milestone M11) will be additional clients of the same API, which is why they will be able to do everything the GUI can.

```
            ┌──────────────┐  Command   ┌──────────────────────────┐
 egui UI ──▶│   Editor     │───────────▶│ Vi (nodes, wires)        │
 (app)      │ undo / redo  │            │  + typing (resolve)      │
            └──────────────┘            └────────────┬─────────────┘
 future MCP / DSL ─────────▲                         │ run_with()
                                                     ▼
                                          evaluator (threads, events)
```

## Crates

| Crate | Role |
|---|---|
| `labcraft-core` | Everything that is not drawing. No GUI dependency. |
| `labcraft-app` | `eframe`/`egui` editor binary named `labcraft`. |

### `labcraft-core/src`

| File | Contents |
|---|---|
| `types.rs` | `NumKind` (I8…U64, SGL, DBL), `DataType` (`Num`, `Bool`, `Str`, `Enum`, `Array(elem, dims)`, `Cluster(fields)`), `Value` (`Dbl`, `Int`, `Bool`, `Str`, `Array(ArrayVal{dims, data})`, `Cluster`), `Value::coerce`. Enum values are stored as `Value::Int` (item index). Arrays are row-major and rectangular. U64 is limited to the i64 range. |
| `model.rs` | `Vi`, `Node`, `Wire`, `NodeKind`, `Terminal`/`Spec`, `Widget`, **`Command`**, **`Editor`** (undo/redo), structure geometry and frame logic. The biggest file; `Editor::exec` is the single place edits are applied. |
| `typing.rs` | Type inference for polymorphic nodes: `resolve(vi) -> TypeMap` (real output type of every terminal), `result_types`, `check_connect` (wire validation), `wire_type`/`crossing_loops` (auto-indexing), `coercion_dots`. |
| `panel.rs` | Panel-only configuration that does not affect dataflow: `GraphConfig` (axes, legend, cursors), `DecoKind`, `Prop` (Property Node properties), and `arrange_rects` (layout maths for `Command::ArrangePanel`). |
| `ops.rs` | Value-level operations: `broadcast` (arithmetic/logic over arrays and clusters), integer wrap-around, array build/index/init/transpose, comparisons. |
| `eval.rs` | Dataflow runtime (`run`, `run_with`, `RunControl`) and the node/structure `compute`. Tests for the language live in `mod tests` and in `structure_tests.rs`. |

### `labcraft-app/src`

| File | Contents |
|---|---|
| `app.rs` | `LabCraft` state, toolbar, **palettes** (`panel_palette`, `diagram_palette`), example VIs (`demo_*`), background run thread + message pump, tests (headless demo runs, render smoke test). |
| `diagram.rs` | Block diagram canvas: nodes, structures (`draw_structure`), wires (`draw_wire`, tunnel markers), terminals, context menus, wire popup. |
| `panel.rs` | Front panel canvas and widget drawing (knob, gauge, table, listbox, decorations, …), selection / rubber-band / drag / resize handling, and the control/indicator context menu. |
| `graph.rs` | Graph and chart rendering: `series_for` maps a value to plots (waveform, multi-plot rows/columns, XY), `draw_lines`, `draw_intensity`, cursors, legend, and `config_menu`. |
| `valedit.rs` | Generic value editor (`edit_value`) and type editor (`type_menu`) shared by panel widgets, constants and menus. |
| `theme.rs` | Dark theme, LabVIEW wire/type colours, wire widths. |

## Core concepts

### Nodes, terminals, wires

A `Node` has a `NodeKind`, a diagram position/size, a panel position/widget (for controls and indicators), an optional `value`, and `parent`/`frame` if it sits inside a structure. `NodeKind::inputs()`/`outputs()` define terminals; a `Terminal` has a declared `ty`, a `Spec` (`Exact`, `NumLike`, `BoolLike`, `Any`, `AnyArray`, `AnyCluster`), and flags `inner` (inside a structure border) and `optional`.

Terminal **index conventions** (wires reference `(NodeId, index)`, so these are load-bearing):

| Node | Inputs | Outputs |
|---|---|---|
| Loop | `[N or stop, init0, next0, init1, next1, …]` | `[i, cur0, final0, cur1, final1, …]` |
| Case / Disable | `[selector, tunnel k·frame f at 1 + k·MAX_FRAMES + f]` | `[tunnel k at k]` |
| Event | `[timeout, tunnels (same stride)]` | `[NewVal for frame f at f (< MAX_FRAMES), tunnel k at MAX_FRAMES + k]` |
| Bundle `{n}` | `[cluster, e0…]` | `[cluster]` |
| Unbundle `{n}` | `[cluster]` | `[e0…]` |

Appending (shift register, tunnel, element) never renumbers existing terminals; keep it that way.

### Structures, frames, tunnels

Structures (`Loop`, `Case`, `Sequence`, `Event`) contain nodes via `Node.parent` and `Node.frame`. `frame_view` is which frame of a Case/Event is displayed (UI state; not undoable). Containment is geometric: dropping a node (`Command::Drop`) re-parents it to the innermost structure under its centre, and picks the frame (`Vi::frame_at`).

* Loops: a wire crossing the border is an **implicit tunnel**. If `Wire.auto_index` is set it gathers (leaving) or slices (entering) per iteration; defaults to on for For loops. `typing::wire_type` and `eval` (`apply_entering`, `tun`) must agree on the resulting type.
* Case/Event only run one frame, so data leaving needs an **explicit output tunnel** (`NodeKind::Case.outs`); `Vi::check_frames` rejects direct wires out (`EditError::NeedsTunnel`) and wires between frames.
* Flat Sequence runs frames in order with a shared "done" set, so cross-frame wires (forward only) work and implicit tunnels are fine.

### Front-panel items

Controls, indicators and decorations (`NodeKind::Decoration`, panel-only, skipped by the diagram and the evaluator) carry extra view state on `Node`: `panel_pos`, `panel_size_override` (only honoured when `Widget::resizable()` or for decorations), `graph: GraphConfig`, `visible`, `disabled`, `caption`. `Widget::fits(ty, is_ctl)` decides which widgets can show a type and drives the "Replace with" menu — add a new widget there. A *waveform* is the cluster `{t0, dt, Y}` (`DataType::waveform()`); XY data is a cluster of two numeric arrays; a multi-plot chart takes a cluster of numerics and stores its history as an `N×plots` array (`chart_default`). Panel z-order is the order of `Vi::nodes` (decorations are always drawn first). Property Nodes write through `RunControl::on_property` and the GUI applies the result on its thread, the same pattern as indicator updates.

### Type inference

Polymorphic nodes have no fixed output type. `typing::resolve` walks the wires and applies `result_types` per node kind, memoised with cycle protection (shift registers/tunnels/feedback carry declared types). Connecting a wire runs `check_connect`; after type-changing commands `prune_invalid_wires` drops wires that no longer type-check. Untouched shift registers, tunnels and Feedback Nodes adopt the type of the first wire connected.

### Evaluation

`run_with(vi, &RunControl)` builds a `TypeMap`, then `eval_scope` executes a scope's nodes in dependency order. `deps`/`lift` treat a structure as one unit (it waits for every outside source feeding any of its descendants). Ready structures run **concurrently** (`fire_parallel` forks the context, runs on scoped threads, merges). `RunControl` carries the abort flag, live control values (so a Stop button works inside a running loop), shared globals, and callbacks for indicator updates / local-variable control writes. The GUI runs the VI on a background thread and receives `Msg`s on a channel (`app.rs::pump`).

### Editor / undo

`Editor::apply` snapshots the whole `Vi` per command (simple and correct at current sizes). `SetValue` and `SetFrameView` are not undoable; consecutive `MoveNode`/`MovePanelItem`/`Resize`/`SetType` on the same node coalesce until `EndGesture`.

## Recipes

### Add a function node
1. Add a variant to `NodeKind` (`model.rs`).
2. Fill in `title()`, `inputs()`, `outputs()` (and `default_size()` if not the standard box).
3. If the output type depends on input types, add an arm to `typing::result_types`; use `Spec::NumLike`/`BoolLike`/`Any` on polymorphic inputs and handle special checks in `typing::check_connect`.
4. Implement it in `eval.rs::compute` (rust will point out non-exhaustive matches). Put reusable value logic in `ops.rs`.
5. Add it to `diagram_palette()` in `app.rs` (a test builds every palette entry).
6. Add a core test that builds a tiny VI with `Editor`, `run()`s it, and asserts the result.

### Add a front-panel widget
1. Add a `Widget` variant; update `Widget::fits`, `panel_size`, `label` (`model.rs`).
2. Add the drawing/interaction arm in `panel.rs::show` (and `SWAPPABLE` so it appears in "Replace with").
3. Add a palette entry in `panel_palette()` in `app.rs`.

### Add a graph or panel widget variant
Follow *Add a front-panel widget*; for graphs add the data-to-plots mapping to `graph::series_for` and a drawing function in `graph.rs`, wire it into the `match n.widget` in `panel.rs::show`, and extend `GraphConfig` (core `panel.rs`) + `graph::config_menu` for any new setting. View-only settings should use `Command::SetGraphConfig` (not undoable).

### Add a structure
Use Case/Event as the template: new `NodeKind` variant (+ `is_structure`, `frame_count`, terminal layout in `terminal_pos`/`terminal_visible`), commands for its frames/tunnels, an arm in `eval.rs::fire`, and drawing + menu in `diagram.rs` (`draw_structure`, `structure_menu`). Decide whether it is frame-gated (`is_frame_gated`) — that controls the output-tunnel requirement.

### Add a command
Add the `Command` variant, an arm in `Editor::exec`, decide whether it is undoable (`Editor::apply`), and cover it with a core test. UI code should only push it to `app.pending`.

## Toward the agent interface (M11)

The intended shape, so current work stays compatible:

* An **MCP server** exposing the `Command` set (`create_vi`, `add_node`, `connect`, `set_value`, `run`, `validate`, `get_vi`, `screenshot`, …) with tool schemas generated from the node/type registry, so every new node is automatically agent-usable.
* A **declarative text DSL** that compiles to commands and decompiles back, so an agent can author a whole VI in one shot.
* **Live sync** between a running editor and agent edits, **auto-layout** so agents don't specify pixel positions, and headless `labcraft run/validate` CLI.

Keep new functionality expressible as `Command`s and keep validation errors structured (`EditError`, `RunError`) — those are what agents will read.
