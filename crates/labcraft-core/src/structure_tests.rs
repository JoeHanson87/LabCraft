//! M4 tests: Case/Event/Sequence/Disable structures, auto-indexing, feedback, variables, parallelism.

use crate::eval::{run, run_with, RunControl, RunError};
use crate::model::{Command, EditError, Editor, EventFrame, LoopKind, NodeId, NodeKind, Vi, MAX_FRAMES};
use crate::types::{DataType, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn ed() -> Editor {
    Editor::new(Vi::new("t"))
}
fn mk(ed: &mut Editor, kind: NodeKind, pos: [f32; 2]) -> NodeId {
    ed.apply(Command::AddNode { kind, pos, widget: None }).unwrap().unwrap()
}
fn konst(ed: &mut Editor, ty: DataType, v: Value, pos: [f32; 2]) -> NodeId {
    let id = mk(ed, NodeKind::Constant(ty), pos);
    ed.apply(Command::SetValue { id, value: v }).unwrap();
    id
}
fn wire(ed: &mut Editor, from: (NodeId, usize), to: (NodeId, usize)) {
    ed.apply(Command::Connect { from, to }).unwrap();
}
fn ind(ed: &mut Editor, ty: DataType) -> NodeId {
    mk(ed, NodeKind::Indicator { name: "out".into(), ty }, [-900.0, 0.0])
}
fn result(ed: &Editor, id: NodeId) -> Value {
    run(&ed.vi).unwrap().indicators.into_iter().find(|(i, _)| *i == id).unwrap().1
}
fn case(ed: &mut Editor, pos: [f32; 2]) -> NodeId {
    mk(ed, NodeKind::Case { labels: vec!["False".into(), "True".into()], default: None, outs: vec![], fixed: None }, pos)
}
fn view(ed: &mut Editor, id: NodeId, frame: usize) {
    ed.apply(Command::SetFrameView { id, frame }).unwrap();
}
fn s(v: &str) -> Value {
    Value::Str(v.into())
}
fn arr(v: &[f64]) -> Value {
    Value::from_f64_vec(v)
}

#[test]
fn case_boolean_with_output_tunnel() {
    let mut e = ed();
    let c = case(&mut e, [0.0, 0.0]);
    let sel = konst(&mut e, DataType::Bool, Value::Bool(true), [-900.0, -900.0]);
    wire(&mut e, (sel, 0), (c, 0));
    view(&mut e, c, 0);
    let no = konst(&mut e, DataType::Str, s("no"), [60.0, 60.0]);
    view(&mut e, c, 1);
    let yes = konst(&mut e, DataType::Str, s("yes"), [60.0, 60.0]);
    assert_eq!(e.vi.node(no).unwrap().frame, 0);
    assert_eq!(e.vi.node(yes).unwrap().frame, 1);
    let out = ind(&mut e, DataType::Str);
    // data must leave through a tunnel
    assert_eq!(e.apply(Command::Connect { from: (yes, 0), to: (out, 0) }), Err(EditError::NeedsTunnel(c)));
    e.apply(Command::AddTunnel { id: c, ty: DataType::Str }).unwrap();
    wire(&mut e, (no, 0), (c, NodeKind::tunnel_in_idx(0, 0)));
    wire(&mut e, (yes, 0), (c, NodeKind::tunnel_in_idx(0, 1)));
    // a wire from the other frame is rejected
    assert_eq!(e.apply(Command::Connect { from: (no, 0), to: (c, NodeKind::tunnel_in_idx(0, 1)) }), Err(EditError::Scope));
    wire(&mut e, (c, 0), (out, 0));
    assert_eq!(result(&e, out), s("yes"));
    e.apply(Command::SetValue { id: sel, value: Value::Bool(false) }).unwrap();
    assert_eq!(result(&e, out), s("no"));
}

#[test]
fn case_integer_ranges_lists_and_default() {
    let mut e = ed();
    let c = case(&mut e, [0.0, 0.0]);
    let sel = konst(&mut e, DataType::I32, Value::Int(0), [-900.0, -900.0]);
    wire(&mut e, (sel, 0), (c, 0)); // relabels to "0","1" with default frame 1
    e.apply(Command::AddFrame { id: c, after: 1 }).unwrap();
    e.apply(Command::SetFrameLabel { id: c, frame: 2, label: "2..5, 9".into() }).unwrap();
    e.apply(Command::AddTunnel { id: c, ty: DataType::Str }).unwrap();
    for (f, text) in [(0, "zero"), (1, "other"), (2, "range")] {
        view(&mut e, c, f);
        let k = konst(&mut e, DataType::Str, s(text), [60.0, 60.0]);
        wire(&mut e, (k, 0), (c, NodeKind::tunnel_in_idx(0, f)));
    }
    let out = ind(&mut e, DataType::Str);
    wire(&mut e, (c, 0), (out, 0));
    for (n, want) in [(0, "zero"), (3, "range"), (9, "range"), (7, "other"), (-4, "other")] {
        e.apply(Command::SetValue { id: sel, value: Value::Int(n) }).unwrap();
        assert_eq!(result(&e, out), s(want), "selector {n}");
    }
}

#[test]
fn case_enum_selector_takes_labels_from_enum() {
    let mut e = ed();
    let c = case(&mut e, [0.0, 0.0]);
    let ty = DataType::Enum(vec!["red".into(), "green".into(), "blue".into()]);
    let sel = konst(&mut e, ty, Value::Int(2), [-900.0, -900.0]);
    wire(&mut e, (sel, 0), (c, 0));
    let NodeKind::Case { labels, .. } = &e.vi.node(c).unwrap().kind else { panic!() };
    assert_eq!(labels, &vec!["red".to_string(), "green".into(), "blue".into()]);
    e.apply(Command::AddTunnel { id: c, ty: DataType::I32 }).unwrap();
    for f in 0..3 {
        view(&mut e, c, f);
        let k = konst(&mut e, DataType::I32, Value::Int(10 * (f as i64 + 1)), [60.0, 60.0]);
        wire(&mut e, (k, 0), (c, NodeKind::tunnel_in_idx(0, f)));
    }
    let out = ind(&mut e, DataType::I32);
    wire(&mut e, (c, 0), (out, 0));
    assert_eq!(result(&e, out), Value::Int(30));
}

#[test]
fn remove_frame_deletes_contents_and_renumbers() {
    let mut e = ed();
    let c = case(&mut e, [0.0, 0.0]);
    e.apply(Command::AddTunnel { id: c, ty: DataType::Str }).unwrap();
    let mut ks = vec![];
    for f in 0..2 {
        view(&mut e, c, f);
        let k = konst(&mut e, DataType::Str, s(["a", "b"][f]), [60.0, 60.0]);
        wire(&mut e, (k, 0), (c, NodeKind::tunnel_in_idx(0, f)));
        ks.push(k);
    }
    e.apply(Command::RemoveFrame { id: c, frame: 0 }).unwrap();
    assert!(e.vi.node(ks[0]).is_none());
    assert_eq!(e.vi.node(ks[1]).unwrap().frame, 0);
    // the surviving tunnel wire moved to frame 0
    assert!(e.vi.wires.iter().any(|w| w.from.0 == ks[1] && w.to == (c, NodeKind::tunnel_in_idx(0, 0))));
    assert_eq!(e.apply(Command::RemoveFrame { id: c, frame: 0 }), Err(EditError::LastFrame));
}

#[test]
fn for_loop_auto_indexes_in_and_out() {
    let mut e = ed();
    let l = mk(&mut e, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [0.0, 0.0]);
    let a = konst(&mut e, DataType::arr_dbl(), arr(&[1.0, 2.0, 3.0]), [-900.0, -900.0]);
    let two = konst(&mut e, DataType::DBL, Value::Dbl(2.0), [-900.0, -800.0]);
    let mul = mk(&mut e, NodeKind::Multiply, [100.0, 60.0]);
    let out = ind(&mut e, DataType::arr_dbl());
    wire(&mut e, (a, 0), (mul, 0));
    wire(&mut e, (two, 0), (mul, 1));
    wire(&mut e, (mul, 0), (out, 0));
    assert_eq!(e.vi.node(mul).unwrap().parent, Some(l));
    let w_in = e.vi.wires.iter().find(|w| w.to == (mul, 0)).unwrap();
    let w_scalar = e.vi.wires.iter().find(|w| w.to == (mul, 1)).unwrap();
    assert!(w_in.auto_index && !w_scalar.auto_index);
    // N comes from the array size
    assert_eq!(result(&e, out), arr(&[2.0, 4.0, 6.0]));
}

#[test]
fn nested_for_loops_gather_a_2d_array() {
    let mut e = ed();
    let outer = mk(&mut e, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [0.0, 0.0]);
    e.apply(Command::Resize { id: outer, size: [900.0, 500.0] }).unwrap();
    let inner = mk(&mut e, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [400.0, 60.0]);
    e.apply(Command::Drop(inner)).unwrap();
    assert_eq!(e.vi.node(inner).unwrap().parent, Some(outer));
    let n2 = konst(&mut e, DataType::I32, Value::Int(2), [-900.0, 0.0]);
    let n3 = konst(&mut e, DataType::I32, Value::Int(3), [60.0, 150.0]); // inside outer, outside inner
    wire(&mut e, (n2, 0), (outer, 0));
    wire(&mut e, (n3, 0), (inner, 0));
    let ten = konst(&mut e, DataType::I32, Value::Int(10), [60.0, 220.0]);
    let mul = mk(&mut e, NodeKind::Multiply, [80.0, 90.0]);
    let add = mk(&mut e, NodeKind::Add, [480.0, 100.0]);
    assert_eq!(e.vi.node(add).unwrap().parent, Some(inner));
    // a node inside the inner loop cannot feed the loop's own outer N terminal
    let bad = konst(&mut e, DataType::I32, Value::Int(1), [440.0, 200.0]);
    assert_eq!(e.apply(Command::Connect { from: (bad, 0), to: (inner, 0) }), Err(EditError::Scope));
    let out = ind(&mut e, DataType::array(DataType::I32, 2));
    wire(&mut e, (outer, 0), (mul, 0));
    wire(&mut e, (ten, 0), (mul, 1));
    wire(&mut e, (mul, 0), (add, 0));
    wire(&mut e, (inner, 0), (add, 1));
    wire(&mut e, (add, 0), (out, 0));
    let Value::Array(a) = result(&e, out) else { panic!() };
    assert_eq!(a.dims, vec![2, 3]);
    let got: Vec<i64> = a.data.iter().map(|v| v.as_i64().unwrap()).collect();
    assert_eq!(got, vec![0, 1, 2, 10, 11, 12]);
}

#[test]
fn while_loop_gathers_only_when_indexing_is_on() {
    let mut e = ed();
    let l = mk(&mut e, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
    let limit = konst(&mut e, DataType::DBL, Value::Dbl(1.5), [-900.0, -900.0]);
    let gt = mk(&mut e, NodeKind::Greater, [60.0, 60.0]);
    let inc = mk(&mut e, NodeKind::Increment, [60.0, 130.0]);
    wire(&mut e, (l, 0), (gt, 0));
    wire(&mut e, (limit, 0), (gt, 1));
    wire(&mut e, (gt, 0), (l, 0));
    wire(&mut e, (l, 0), (inc, 0));
    let last = ind(&mut e, DataType::I32);
    wire(&mut e, (inc, 0), (last, 0));
    assert!(!e.vi.wires.iter().find(|w| w.to == (last, 0)).unwrap().auto_index);
    assert_eq!(result(&e, last), Value::Int(3));
    // an array indicator can only be wired with indexing on, which is chosen automatically
    let all = ind(&mut e, DataType::array(DataType::I32, 1));
    wire(&mut e, (inc, 0), (all, 0));
    assert!(e.vi.wires.iter().find(|w| w.to == (all, 0)).unwrap().auto_index);
    let Value::Array(a) = result(&e, all) else { panic!() };
    assert_eq!(a.data, vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    // switching indexing off on a wire whose target is an array removes the wire (type no longer fits)
    e.apply(Command::SetWireIndexing { to: (all, 0), enabled: false }).unwrap();
    assert!(!e.vi.wires.iter().any(|w| w.to == (all, 0)));
}

#[test]
fn feedback_node_remembers_previous_iteration() {
    let mut e = ed();
    let l = mk(&mut e, NodeKind::Loop { kind: LoopKind::For, regs: vec![] }, [0.0, 0.0]);
    let n = konst(&mut e, DataType::I32, Value::Int(4), [-900.0, 0.0]);
    wire(&mut e, (n, 0), (l, 0));
    let fb = mk(&mut e, NodeKind::Feedback { ty: DataType::I32 }, [60.0, 60.0]);
    let add = mk(&mut e, NodeKind::Add, [200.0, 60.0]);
    wire(&mut e, (add, 0), (fb, 0));
    wire(&mut e, (fb, 0), (add, 0));
    wire(&mut e, (l, 0), (add, 1));
    let out = ind(&mut e, DataType::array(DataType::I32, 1));
    wire(&mut e, (add, 0), (out, 0));
    let Value::Array(a) = result(&e, out) else { panic!() };
    let got: Vec<i64> = a.data.iter().map(|v| v.as_i64().unwrap()).collect();
    assert_eq!(got, vec![0, 1, 3, 6]); // running sum of i
}

#[test]
fn local_variable_reads_control_and_writes_indicator() {
    let mut e = ed();
    let x = mk(&mut e, NodeKind::Control { name: "x".into(), ty: DataType::DBL }, [-900.0, 0.0]);
    let y = mk(&mut e, NodeKind::Indicator { name: "y".into(), ty: DataType::DBL }, [-900.0, 100.0]);
    e.apply(Command::SetValue { id: x, value: Value::Dbl(5.0) }).unwrap();
    let rd = mk(&mut e, NodeKind::Local { target: None, ty: DataType::DBL, write: false }, [-900.0, 200.0]);
    e.apply(Command::SetLocalTarget { id: rd, target: Some(x) }).unwrap();
    let one = konst(&mut e, DataType::DBL, Value::Dbl(1.0), [-900.0, 300.0]);
    let add = mk(&mut e, NodeKind::Add, [-900.0, 400.0]);
    let wr = mk(&mut e, NodeKind::Local { target: None, ty: DataType::DBL, write: true }, [-900.0, 500.0]);
    e.apply(Command::SetLocalTarget { id: wr, target: Some(y) }).unwrap();
    wire(&mut e, (rd, 0), (add, 0));
    wire(&mut e, (one, 0), (add, 1));
    wire(&mut e, (add, 0), (wr, 0));
    assert_eq!(result(&e, y), Value::Dbl(6.0));
    // retyping the target updates the local variable
    e.apply(Command::SetType { id: x, ty: DataType::Str }).unwrap();
    assert!(matches!(&e.vi.node(rd).unwrap().kind, NodeKind::Local { ty, .. } if *ty == DataType::Str));
}

#[test]
fn disable_structure_runs_only_the_enabled_frame() {
    let mut e = ed();
    let d = mk(&mut e, NodeKind::Case { labels: vec!["Disabled".into(), "Enabled".into()], default: None, outs: vec![], fixed: Some(1) }, [0.0, 0.0]);
    view(&mut e, d, 0);
    let a = konst(&mut e, DataType::I32, Value::Int(1), [40.0, 60.0]);
    let b = konst(&mut e, DataType::I32, Value::Int(0), [40.0, 120.0]);
    let div = mk(&mut e, NodeKind::Divide, [160.0, 60.0]); // would fail if it ran
    wire(&mut e, (a, 0), (div, 0));
    wire(&mut e, (b, 0), (div, 1));
    view(&mut e, d, 1);
    let on = konst(&mut e, DataType::Str, s("on"), [60.0, 60.0]);
    e.apply(Command::AddTunnel { id: d, ty: DataType::Str }).unwrap();
    wire(&mut e, (on, 0), (d, NodeKind::tunnel_in_idx(0, 1)));
    let out = ind(&mut e, DataType::Str);
    wire(&mut e, (d, 0), (out, 0));
    assert_eq!(result(&e, out), s("on"));
    // enabling the failing frame makes the run fail
    e.apply(Command::SetEnabledFrame { id: d, frame: 0 }).unwrap();
    assert!(matches!(run(&e.vi), Err(RunError::DivideByZero(_))));
}

fn watchdog(abort: Arc<std::sync::atomic::AtomicBool>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(8));
        abort.store(true, Ordering::Relaxed);
    });
}

#[test]
fn independent_structures_run_in_parallel_and_globals_are_shared() {
    let mut e = ed();
    // Loop A spins until the global "go" becomes true.
    let a = mk(&mut e, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
    let rd = mk(&mut e, NodeKind::Global { name: "go".into(), ty: DataType::Bool, write: false }, [60.0, 60.0]);
    wire(&mut e, (rd, 0), (a, 0));
    let ms = konst(&mut e, DataType::I32, Value::Int(2), [60.0, 120.0]);
    let wait = mk(&mut e, NodeKind::WaitMs, [180.0, 120.0]);
    wire(&mut e, (ms, 0), (wait, 0));
    // Sequence B waits 60 ms in frame 0, then sets the global in frame 1.
    let b = mk(&mut e, NodeKind::Sequence { frames: 2 }, [600.0, 0.0]);
    let ms2 = konst(&mut e, DataType::I32, Value::Int(60), [640.0, 60.0]);
    let wait2 = mk(&mut e, NodeKind::WaitMs, [740.0, 60.0]);
    wire(&mut e, (ms2, 0), (wait2, 0));
    let t = konst(&mut e, DataType::Bool, Value::Bool(true), [840.0, 60.0]);
    let wr = mk(&mut e, NodeKind::Global { name: "go".into(), ty: DataType::Bool, write: true }, [940.0, 60.0]);
    assert_eq!((e.vi.node(wait2).unwrap().frame, e.vi.node(wr).unwrap().frame), (0, 1));
    wire(&mut e, (t, 0), (wr, 0));
    let ctl = RunControl::headless();
    watchdog(ctl.abort.clone());
    let start = Instant::now();
    let r = run_with(&e.vi, &ctl);
    assert!(r.is_ok(), "{:?}", r.err()); // would hang (then abort) if the loop ran before the sequence
    assert!(start.elapsed() < Duration::from_secs(6));
    let _ = b;
}

#[test]
fn event_structure_reacts_to_control_value_change() {
    let mut e = ed();
    let go = mk(&mut e, NodeKind::Control { name: "Go".into(), ty: DataType::Bool }, [-900.0, 0.0]);
    let l = mk(&mut e, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
    let ev = mk(&mut e, NodeKind::Event { frames: vec![], outs: vec![] }, [40.0, 50.0]);
    e.apply(Command::Drop(ev)).unwrap();
    assert_eq!(e.vi.node(ev).unwrap().parent, Some(l));
    e.apply(Command::AddEventFrame { id: ev, source: Some(go) }).unwrap();
    e.apply(Command::AddTunnel { id: ev, ty: DataType::Bool }).unwrap();
    wire(&mut e, (ev, 0), (ev, NodeKind::tunnel_in_idx(0, 0))); // NewVal -> tunnel
    wire(&mut e, (ev, MAX_FRAMES), (l, 0)); // tunnel -> loop stop
    let ctl = RunControl::headless();
    watchdog(ctl.abort.clone());
    let live = ctl.live_controls.clone();
    let flipper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        live.lock().unwrap().insert(go, Value::Bool(true));
    });
    let r = run_with(&e.vi, &ctl);
    flipper.join().unwrap();
    assert!(r.is_ok(), "{:?}", r.err());
}

#[test]
fn event_structure_timeout_frame() {
    let mut e = ed();
    let go = mk(&mut e, NodeKind::Control { name: "Go".into(), ty: DataType::Bool }, [-900.0, 0.0]);
    let l = mk(&mut e, NodeKind::Loop { kind: LoopKind::While, regs: vec![] }, [0.0, 0.0]);
    e.apply(Command::Resize { id: l, size: [700.0, 500.0] }).unwrap();
    let ev = mk(&mut e, NodeKind::Event { frames: vec![], outs: vec![] }, [40.0, 50.0]);
    e.apply(Command::Drop(ev)).unwrap();
    e.apply(Command::AddEventFrame { id: ev, source: Some(go) }).unwrap();
    e.apply(Command::AddEventFrame { id: ev, source: None }).unwrap();
    assert!(matches!(&e.vi.node(ev).unwrap().kind, NodeKind::Event { frames, .. } if frames.len() == 2 && frames[1] == EventFrame { label: "Timeout".into(), source: None, ty: DataType::Bool }));
    let to = konst(&mut e, DataType::I32, Value::Int(25), [-900.0, 100.0]);
    wire(&mut e, (to, 0), (ev, 0));
    e.apply(Command::AddTunnel { id: ev, ty: DataType::Bool }).unwrap();
    let stop = konst(&mut e, DataType::Bool, Value::Bool(true), [60.0, 100.0]);
    assert_eq!((e.vi.node(stop).unwrap().parent, e.vi.node(stop).unwrap().frame), (Some(ev), 1)); // timeout frame is displayed
    wire(&mut e, (stop, 0), (ev, NodeKind::tunnel_in_idx(0, 1)));
    wire(&mut e, (ev, MAX_FRAMES), (l, 0));
    let ctl = RunControl::headless();
    watchdog(ctl.abort.clone());
    let start = Instant::now();
    assert!(run_with(&e.vi, &ctl).is_ok());
    assert!(start.elapsed() >= Duration::from_millis(20));
}
