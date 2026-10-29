# LabCRAFT roadmap

Legend: ✅ done · ⏭ next · ☐ not started. Order below is the agreed build order (front-panel depth pulled forward for visual fidelity). Each milestone should end with a demo VI (add it to the **Examples** menu in `app.rs`) and tests.

## Status

| # | Milestone | Status |
|---|---|---|
| M1 | Core model + dark egui GUI skeleton | ✅ |
| M2 | Runtime: loops, shift registers, arrays, graphs, LabVIEW-style widgets | ✅ |
| M3 | Type system & data structures | ✅ |
| M4 | Structures | ✅ |
| **M7** | **Front-panel depth** | 🟡 first slice done (graphs, tables, decorations, layout, property nodes, waveforms); tab/tree/image/ring/3D/classic-look remain |
| M5 | subVIs & project model | ☐ |
| M9 | Editor ergonomics | ☐ |
| M6 | Function library breadth | ☐ |
| M10 | Debugging & verification | ☐ |
| M8 | I/O & file handling | ☐ |
| M11 | Agent interface (MCP + text DSL) | ☐ |
| M12 | Packaging & platform | ☐ |

---

## ✅ Completed

**M1** – `labcraft-core` VI model with `Command` API and undo/redo; egui editor with Front Panel / Block Diagram views, palettes, wiring, run.

**M2** – While/For loops, shift registers, tunnels; arrays; background execution with live indicator updates and abort; widgets: numeric control/indicator, knob, slide, gauge, thermometer, tank, toggle, push button, LED, string, waveform chart & graph; collapsible palettes.

**M3** – I8–I64/U8–U32/U64/SGL/DBL, Boolean, String, Enum, N-D arrays, clusters, error cluster; polymorphic arithmetic/logic/comparison over scalars/arrays/clusters; integer wrap-around and saturating conversion; coercion dots; Bundle/Unbundle (+By Name), array/cluster/enum panel widgets, type-editing menus; shift registers adopt wired type.

**M4** – Case structure (Boolean/integer/string/enum selectors, ranges & lists, default, output tunnels), Diagram Disable, Flat Sequence, Event structure (Value Change + Timeout), auto-indexing tunnels (incl. nested → N-D), Feedback Node, Local & Global variables, parallel execution of independent structures.

---

## 🟡 M7 – Front-panel depth (first slice done)

Done:
- ✅ Resizable panel items (drag the corner handle of the selected item; `Command::ResizePanelItem`, `Widget::resizable`)
- ✅ Graphs: multi-plot waveform graph (2D array, one plot per row), **XY graph**, **intensity graph** (heat map + colour bar), multi-plot chart (cluster of numerics), legends with plot names, axis labels, autoscale or fixed X/Y/colour ranges, **draggable cursors** with interpolated readouts (`labcraft-app/src/graph.rs`, `GraphConfig`)
- ✅ Table (2D string/numeric arrays, editable, resizable rows/cols) and Listbox (string array)
- ✅ Decorations: box, line, free label (panel-only; never on the diagram), colour/text editable
- ✅ Waveform data type (`t0`, `dt`, `Y`) with Build Waveform / Get Waveform Components; waveform graph uses real time on X
- ✅ Property Nodes (read/write): Visible, Disabled, Caption; items hidden at runtime, dimmed while editing
- ✅ Layout tools: multi-select (Shift/Cmd-click, rubber band), align, distribute, same width/height, bring-to-front / send-to-back, snap to grid; multi-selection drags as one undo step
- ✅ "Front Panel Showcase" example

Remaining:
- ☐ 3D graph
- ☐ Tab control, tree control, image display, ring control
- ☐ Dynamic Data type
- ☐ More Property Node properties (value signaling flavours, colours, blinking, position/size, plot colours) and Invoke Nodes
- ☐ Classic vs modern look toggle (widget drawing hard-codes the dark palette in `panel.rs`/`graph.rs`; needs a theme struct) and a general customize-control / appearance pass for existing widgets
- ☐ Group / ungroup panel items; lock position
- ☐ Resizable array/cluster controls with drag handles on the element area (the whole item resizes today); array index display
- ☐ Graph: log scales, mixed plot styles (points/bars), per-plot colours, zoom/pan on the graph, scale-format dialog
- ☐ Listbox selection output and multi-column listbox; table headers and cell formatting

## M5 – subVIs & project model

- ☐ Connector pane + icon editor (patterns; required/recommended/optional terminals)
- ☐ VI hierarchy: drop a VI on another diagram as a subVI; recursion, reentrancy
- ☐ Project Explorer (`.lvproj`-style), libraries (`.lvlib`), folders
- ☐ VI Properties (priority, reentrancy, window appearance, documentation); Context Help
- ☐ Express-VI-style configurable nodes (Formula, Simulate Signal, Build Text)
- ☐ Typedefs / custom controls (`.ctl`), strict typedefs *(deferred from M3)*

## M9 – Editor ergonomics

- ☐ Pan/zoom canvas (currently the canvas does not scroll — large diagrams get cut off)
- ☐ Orthogonal wire auto-routing, wire junctions/branches, bendable wires, Clean Up Diagram
- ☐ Multi-select, rubber-band, copy/paste, duplicate, align/distribute
- ☐ Quick Drop (Ctrl+Space), Tools palette & automatic tool selection
- ☐ Real file dialogs, recent files, tabs/window menu, Find, Navigation window
- ☐ Preferences: theme (dark default / light / classic), grid, fonts
- ☐ Drag-to-create tunnels and shift registers directly on structure borders; create terminal-attached constants/controls/indicators from a right-click on a terminal

## M6 – Function library breadth

- ☐ Numeric: trig/hyperbolic, power/log, rounding, quotient & remainder, bit ops, Compound Arithmetic, complex *(deferred from M3)*
- ☐ Boolean/comparison: Max & Min, In Range and Coerce, Compound
- ☐ String: format/scan, search/replace, split, regex, conversions; Path & Timestamp types *(deferred)*
- ☐ Array/Cluster: Replace, Insert, Delete, Sort, Search, Reverse, Subset, Decimate, Interleave, Cluster↔Array
- ☐ Timing: Wait Until Next ms Multiple, Tick Count, Get Date/Time, Elapsed Time
- ☐ Formula Node / MathScript-lite
- ☐ Signal processing & statistics: FFT, filters, windowing, mean/stddev/histogram, curve fit
- ☐ Variant type *(deferred)*
- ☐ Float divide-by-zero currently follows IEEE; integer divide-by-zero is a run error — revisit to match LabVIEW

## M10 – Debugging & verification

- ☐ Highlight Execution with animated data bubbles; single-step; breakpoints; probes & custom probes; retain wire values
- ☐ Error list + broken-run-arrow with click-to-select node
- ☐ VI Analyzer-lite (unwired terminals, dead code, race conditions)
- ☐ Unit-test framework (assertions, runner VIs)
- ☐ Execution profiler (per-VI timing, memory)

## M8 – I/O & file handling

- ☐ File I/O (text, binary, spreadsheet, TDMS-like, INI), file dialogs, path functions
- ☐ Serial (VISA-lite), TCP/UDP, basic HTTP
- ☐ Simulated DAQ-style virtual instruments
- ☐ Dialogs; VI-server-style open/run by path

## M11 – Agent interface (the differentiator)

- ☐ `labcraft` CLI: `run`, `validate`, `edit` headless
- ☐ **MCP server** (stdio + HTTP) exposing the `Command` set, `list_palette`/`search_nodes`, `validate` (structured errors), `run` (outputs, per-node values, timing), `get_vi`, `screenshot` (render diagram/panel to PNG)
- ☐ Tool schemas **generated from the node/type registry** so every node is agent-usable automatically
- ☐ **Text DSL** that compiles to commands and decompiles from a VI
- ☐ Live sync with the running GUI (agent cursor, attributed undo history)
- ☐ Auto-layout so agents never specify pixel positions
- ☐ Human-readable, diff-friendly VI file format (`.lcvi`) with stable IDs (current save is JSON of the whole `Vi`)

## M12 – Packaging & platform

- ☐ Cross-platform CI builds (macOS, Linux, Windows), installers, icons, crash reporter
- ☐ Faster execution backend (compile to bytecode/closures instead of the tree-walking interpreter); benchmarks
- ☐ Plugin system; `.vi` import experiment

---

## Known limitations / deferred from completed milestones

- **Hidden / disabled panel items:** while *not* running, hidden items are shown dimmed so they remain editable; they disappear only during a run.
- **Rendering is unverified by eye in CI.** UI is covered by a headless render smoke test only; visual regressions need a human (or screenshot tooling).
- **Canvas has no pan/zoom** (M9); very large diagrams run off-screen.
- **Undo** snapshots the whole `Vi` per command (fine now; revisit if VIs get large).
- **U64** is limited to the i64 range; **complex, Path, Timestamp, Variant, Ring, typedefs** not implemented.
- **Stacked Sequence** not implemented (Flat Sequence only).
- **Event structure:** Value Change + Timeout only (no mouse/key events, no event filtering/locking); changes between structure executions collapse into one event.
- **Parallel structures** run on a forked copy of the data; cross-structure signalling must use Globals / live controls. Local/Global variable access is lock-protected but not otherwise race-protected (same caveat as LabVIEW).
- **Auto-index toggling** drops the wire if the destination's type no longer fits (the indicator must be retyped first, or just re-wire).
- Unwired indicators do not error (matches LabVIEW), but there is no broken-run arrow for other problems yet (M10).
- Save/Open use `<VI name>.json` in the working directory (no file dialog yet, M9).
