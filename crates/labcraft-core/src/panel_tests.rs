//! M7 tests: resizable items, graph/table widgets, layout tools, decorations, property nodes, waveforms.

use crate::eval::{run, run_with, RunControl};
use crate::model::{chart_default, Command, EditError, Editor, NodeId, NodeKind, Vi, Widget};
use crate::panel::{arrange_rects, Arrange, DecoKind, GraphConfig, Prop};
use crate::types::{DataType, Value};
use std::sync::{Arc, Mutex};

fn ed() -> Editor {
    Editor::new(Vi::new("t"))
}
fn add(ed: &mut Editor, kind: NodeKind, widget: Option<Widget>) -> NodeId {
    ed.apply(Command::AddNode { kind, pos: [0.0, 0.0], widget }).unwrap().unwrap()
}
fn ctl(name: &str, ty: DataType) -> NodeKind {
    NodeKind::Control { name: name.into(), ty }
}
fn ind(name: &str, ty: DataType) -> NodeKind {
    NodeKind::Indicator { name: name.into(), ty }
}
fn konst(ed: &mut Editor, ty: DataType, v: Value) -> NodeId {
    let id = add(ed, NodeKind::Constant(ty), None);
    ed.apply(Command::SetValue { id, value: v }).unwrap();
    id
}
fn wire(ed: &mut Editor, from: (NodeId, usize), to: (NodeId, usize)) {
    ed.apply(Command::Connect { from, to }).unwrap();
}
fn deco(ed: &mut Editor) -> NodeId {
    add(ed, NodeKind::Decoration { deco: DecoKind::Box, text: String::new(), color: [90, 90, 120] }, None)
}

#[test]
fn only_resizable_widgets_resize_and_resizes_coalesce() {
    let mut e = ed();
    let g = add(&mut e, ind("g", DataType::arr_dbl()), Some(Widget::Graph));
    let k = add(&mut e, ctl("k", DataType::DBL), Some(Widget::Knob));
    assert_eq!(e.vi.node(g).unwrap().panel_size(), [360.0, 220.0]);
    e.apply(Command::ResizePanelItem { id: g, size: [500.0, 300.0] }).unwrap();
    e.apply(Command::ResizePanelItem { id: g, size: [520.0, 320.0] }).unwrap();
    e.apply(Command::ResizePanelItem { id: g, size: [5.0, 1.0] }).unwrap(); // clamped to a minimum
    assert_eq!(e.vi.node(g).unwrap().panel_size(), [30.0, 8.0]);
    assert_eq!(e.apply(Command::ResizePanelItem { id: k, size: [200.0, 200.0] }), Err(EditError::WrongNodeKind(k)));
    // one undo step for the whole drag
    e.undo().unwrap();
    assert_eq!(e.vi.node(g).unwrap().panel_size(), [360.0, 220.0]);
}

#[test]
fn widgets_fit_the_data_they_can_show() {
    let d = DataType::DBL;
    let two_d = DataType::array(d.clone(), 2);
    let xy = DataType::Cluster(vec![DataType::field("x", DataType::arr_dbl()), DataType::field("y", DataType::arr_dbl())]);
    let multi = DataType::Cluster(vec![DataType::field("a", d.clone()), DataType::field("b", d.clone()), DataType::field("c", d.clone())]);
    assert!(Widget::Graph.fits(&two_d, false) && Widget::IntensityGraph.fits(&two_d, false) && Widget::Table.fits(&two_d, false));
    assert!(Widget::Table.fits(&two_d, true) && !Widget::Graph.fits(&two_d, true));
    assert!(Widget::XyGraph.fits(&xy, false) && !Widget::XyGraph.fits(&multi, false));
    assert!(Widget::Graph.fits(&DataType::waveform(), false));
    assert!(Widget::Chart.fits(&multi, false) && Widget::Chart.fits(&d, false));
    assert!(Widget::ListBox.fits(&DataType::array(DataType::Str, 1), true) && Widget::Table.fits(&DataType::array(DataType::Str, 2), false));
    assert!(!Widget::ListBox.fits(&DataType::arr_dbl(), false));
    assert!(DataType::waveform().is_waveform() && !xy.is_waveform() && xy.is_xy() && multi.is_multi_chart());
    // multi-plot chart history has a column per plot
    assert_eq!(chart_default(&multi), Value::default_for(&DataType::array(DataType::DBL, 2)));
    let mut e = ed();
    let c = add(&mut e, ind("c", multi), Some(Widget::Chart));
    assert!(matches!(&e.vi.node(c).unwrap().value, Some(Value::Array(a)) if a.dims.len() == 2));
}

#[test]
fn arrange_geometry() {
    let r = |x: f32, y: f32, w: f32, h: f32| ([x, y], [w, h]);
    let items = [r(10.0, 10.0, 50.0, 20.0), r(100.0, 40.0, 30.0, 20.0), r(300.0, 5.0, 40.0, 20.0)];
    let left = arrange_rects(&items, Arrange::AlignLeft);
    assert!(left.iter().all(|r| r.0[0] == 10.0));
    let right = arrange_rects(&items, Arrange::AlignRight);
    assert!(right.iter().all(|r| r.0[0] + r.1[0] == 340.0));
    let top = arrange_rects(&items, Arrange::AlignTop);
    assert!(top.iter().all(|r| r.0[1] == 5.0));
    let dist = arrange_rects(&items, Arrange::DistributeHorizontally);
    // equal gaps between neighbours, outer items stay put
    let gap1 = dist[1].0[0] - (dist[0].0[0] + dist[0].1[0]);
    let gap2 = dist[2].0[0] - (dist[1].0[0] + dist[1].1[0]);
    assert!((gap1 - gap2).abs() < 1e-3 && dist[0].0[0] == 10.0 && dist[2].0[0] == 300.0);
    let same = arrange_rects(&items, Arrange::SameWidth);
    assert!(same.iter().all(|r| r.1[0] == 50.0));
    let center = arrange_rects(&items, Arrange::CenterVertically);
    let mid = (5.0 + 60.0) / 2.0;
    assert!(center.iter().all(|r| (r.0[1] + r.1[1] / 2.0 - mid).abs() < 1e-3));
}

#[test]
fn arrange_command_moves_items_and_reorders_z() {
    let mut e = ed();
    let a = add(&mut e, ctl("a", DataType::DBL), Some(Widget::NumCtl));
    let b = add(&mut e, ctl("b", DataType::DBL), Some(Widget::NumCtl));
    let c = add(&mut e, ctl("c", DataType::DBL), Some(Widget::NumCtl));
    for (id, pos) in [(a, [40.0, 10.0]), (b, [120.0, 90.0]), (c, [260.0, 30.0])] {
        e.apply(Command::MovePanelItem { id, pos }).unwrap();
        e.apply(Command::EndGesture).unwrap();
    }
    e.apply(Command::ArrangePanel { ids: vec![a, b, c], op: Arrange::AlignLeft }).unwrap();
    assert!([a, b, c].iter().all(|i| e.vi.node(*i).unwrap().panel_pos[0] == 40.0));
    e.undo().unwrap();
    assert_eq!(e.vi.node(b).unwrap().panel_pos[0], 120.0);
    e.apply(Command::ArrangePanel { ids: vec![a], op: Arrange::BringToFront }).unwrap();
    assert_eq!(e.vi.nodes.last().unwrap().id, a);
    e.apply(Command::ArrangePanel { ids: vec![c], op: Arrange::SendToBack }).unwrap();
    assert_eq!(e.vi.nodes.first().unwrap().id, c);
}

#[test]
fn decorations_are_panel_only_and_do_not_affect_runs() {
    let mut e = ed();
    let d = deco(&mut e);
    assert!(!e.vi.node(d).unwrap().kind.is_panel_item() && e.vi.node(d).unwrap().kind.is_decoration());
    assert_eq!(e.vi.node(d).unwrap().widget, Widget::Decoration);
    e.apply(Command::SetDecoration { id: d, text: Some("Section".into()), color: Some([1, 2, 3]) }).unwrap();
    e.apply(Command::ResizePanelItem { id: d, size: [400.0, 250.0] }).unwrap();
    assert_eq!(e.vi.node(d).unwrap().panel_size(), [400.0, 250.0]);
    assert!(matches!(&e.vi.node(d).unwrap().kind, NodeKind::Decoration { text, color, .. } if text == "Section" && *color == [1, 2, 3]));
    assert!(run(&e.vi).is_ok());
    // decorations are not valid data targets for SetType
    assert_eq!(e.apply(Command::SetType { id: d, ty: DataType::DBL }), Err(EditError::WrongNodeKind(d)));
}

#[test]
fn property_nodes_write_and_read_control_properties() {
    let mut e = ed();
    let target = add(&mut e, ctl("x", DataType::DBL), None);
    let hide = konst(&mut e, DataType::Bool, Value::Bool(true));
    let w = add(&mut e, NodeKind::Property { target: None, prop: Prop::Disabled, write: true }, None);
    e.apply(Command::SetPropertyTarget { id: w, target: Some(target) }).unwrap();
    wire(&mut e, (hide, 0), (w, 0));
    let r = add(&mut e, NodeKind::Property { target: None, prop: Prop::Visible, write: false }, None);
    e.apply(Command::SetPropertyTarget { id: r, target: Some(target) }).unwrap();
    let out = add(&mut e, ind("vis", DataType::Bool), None);
    wire(&mut e, (r, 0), (out, 0));
    e.apply(Command::SetItemProps { id: target, visible: Some(false), disabled: None, caption: Some(Some("Speed".into())) }).unwrap();
    let seen: Arc<Mutex<Vec<(NodeId, Prop, Value)>>> = Arc::default();
    let mut ctl = RunControl::headless();
    let sink = seen.clone();
    ctl.on_property = Box::new(move |id, p, v| sink.lock().unwrap().push((id, p, v.clone())));
    let res = run_with(&e.vi, &ctl).unwrap();
    assert_eq!(seen.lock().unwrap().as_slice(), &[(target, Prop::Disabled, Value::Bool(true))]);
    // reading Visible returns the panel item's own state
    assert_eq!(res.indicators.into_iter().find(|(i, _)| *i == out).unwrap().1, Value::Bool(false));
    // a property of the wrong type is rejected, and removing the target clears the node's reference
    let s = konst(&mut e, DataType::Str, Value::Str("x".into()));
    assert!(matches!(e.apply(Command::Connect { from: (s, 0), to: (w, 0) }), Err(EditError::TypeMismatch(..))));
    e.apply(Command::RemoveNode(target)).unwrap();
    assert!(matches!(&e.vi.node(w).unwrap().kind, NodeKind::Property { target: None, .. }));
}

#[test]
fn caption_property_reads_default_title() {
    let mut e = ed();
    let t = add(&mut e, ctl("Gain", DataType::DBL), None);
    let r = add(&mut e, NodeKind::Property { target: Some(t), prop: Prop::Caption, write: false }, None);
    let out = add(&mut e, ind("c", DataType::Str), None);
    wire(&mut e, (r, 0), (out, 0));
    let v = run(&e.vi).unwrap().indicators.into_iter().find(|(i, _)| *i == out).unwrap().1;
    assert_eq!(v, Value::Str("Gain".into()));
}

#[test]
fn waveform_build_and_get_components() {
    let mut e = ed();
    let y = konst(&mut e, DataType::arr_dbl(), Value::from_f64_vec(&[1.0, 2.0, 3.0]));
    let dt = konst(&mut e, DataType::DBL, Value::Dbl(0.5));
    let build = add(&mut e, NodeKind::BuildWaveform, None);
    wire(&mut e, (y, 0), (build, 2));
    wire(&mut e, (dt, 0), (build, 1));
    let wf = add(&mut e, ind("wf", DataType::waveform()), Some(Widget::Graph));
    wire(&mut e, (build, 0), (wf, 0));
    let get = add(&mut e, NodeKind::GetWaveform, None);
    wire(&mut e, (build, 0), (get, 0));
    let dt_out = add(&mut e, ind("dt", DataType::DBL), None);
    wire(&mut e, (get, 1), (dt_out, 0));
    let r = run(&e.vi).unwrap();
    let get_ind = |id: NodeId| r.indicators.iter().find(|(i, _)| *i == id).unwrap().1.clone();
    assert_eq!(get_ind(wf), Value::Cluster(vec![Value::Dbl(0.0), Value::Dbl(0.5), Value::from_f64_vec(&[1.0, 2.0, 3.0])]));
    assert_eq!(get_ind(dt_out), Value::Dbl(0.5));
    // a plain array cannot be wired where a waveform is expected
    assert!(matches!(e.apply(Command::Connect { from: (y, 0), to: (get, 0) }), Err(EditError::TypeMismatch(..))));
}

#[test]
fn old_saved_files_without_m7_fields_still_load() {
    let mut e = ed();
    let g = add(&mut e, ind("g", DataType::arr_dbl()), Some(Widget::Graph));
    e.apply(Command::SetGraphConfig { id: g, cfg: GraphConfig { cursors: vec![1.5], ..GraphConfig::default() } }).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&e.vi.to_json()).unwrap();
    for n in json["nodes"].as_array_mut().unwrap() {
        for key in ["panel_size_override", "graph", "visible", "disabled", "caption"] {
            n.as_object_mut().unwrap().remove(key);
        }
    }
    let vi = Vi::from_json(&json.to_string()).unwrap();
    let n = vi.node(g).unwrap();
    assert!(n.visible && !n.disabled && n.caption.is_none() && n.graph == GraphConfig::default());
    // and the full round trip keeps graph settings
    let back = Vi::from_json(&e.vi.to_json()).unwrap();
    assert_eq!(back.node(g).unwrap().graph.cursors, vec![1.5]);
}

#[test]
fn multi_move_is_one_undo_step_and_snap_rounds() {
    let mut e = ed();
    let a = add(&mut e, ctl("a", DataType::DBL), Some(Widget::NumCtl));
    let b = add(&mut e, ctl("b", DataType::DBL), Some(Widget::NumCtl));
    let start_a = e.vi.node(a).unwrap().panel_pos;
    for dx in [3.0, 7.0, 12.0] {
        e.apply(Command::MovePanelItems { moves: vec![(a, [start_a[0] + dx, start_a[1]]), (b, [100.0 + dx, 50.0])] }).unwrap();
    }
    e.apply(Command::EndGesture).unwrap();
    e.apply(Command::SnapPanel { ids: vec![a, b], grid: 10.0 }).unwrap();
    assert_eq!(e.vi.node(b).unwrap().panel_pos, [110.0, 50.0]);
    e.undo().unwrap(); // the snap
    assert_eq!(e.vi.node(b).unwrap().panel_pos, [112.0, 50.0]);
    e.undo().unwrap(); // the whole multi-move
    assert_eq!(e.vi.node(a).unwrap().panel_pos, start_a);
}
