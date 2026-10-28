use std::collections::BTreeMap;

use lc_dsl::{build, emit, parse};
use lc_model::Value;

const OVER_TEMP: &str = r#"
vi "Over Temperature" {
  description "Raises an alarm above the limit."

  panel {
    control temp "Temperature" @(40, 60) { type: dbl, default: 35.0, unit: "C" }
    indicator alarm "Alarm" @(40, 150) { type: bool }
  }

  diagram {
    let limit = 30.0
    temp -> greater(limit) -> alarm
  }
}
"#;

#[test]
fn parses_over_temperature() {
    let program = parse(OVER_TEMP).expect("parse");
    assert_eq!(program.name, "Over Temperature");
    assert_eq!(program.panel.len(), 2);
    assert_eq!(program.body.len(), 2);
}

#[test]
fn builds_and_runs_over_temperature() {
    let outcome = build(OVER_TEMP).expect("build");
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    let run = lc_runtime::compile_and_run(&outcome.doc, lc_runtime::default_inputs(&outcome.doc))
        .expect("run");
    assert_eq!(run.indicators.get("alarm"), Some(&Value::Bool(true)));
}

#[test]
fn emits_dsl_that_round_trips() {
    let first = build(OVER_TEMP).expect("build").doc;
    let text = emit(&first);
    assert!(text.contains("vi \"Over Temperature\""));
    assert!(text.contains("control temp"));
    assert!(text.contains("indicator alarm"));
    assert!(text.contains("-> alarm"), "{text}");

    let second = build(&text).expect("rebuild").doc;
    let run = lc_runtime::compile_and_run(&second, lc_runtime::default_inputs(&second))
        .expect("run round trip");
    assert_eq!(run.indicators.get("alarm"), Some(&Value::Bool(true)));
}

#[test]
fn for_loop_accumulates_through_tunnel() {
    let src = r#"
vi "Sum" {
  panel {
    control n "Count" @(20, 20) { type: i32, default: 5 }
    indicator total "Total" @(20, 90) { type: i32 }
  }

  diagram {
    let sum = for i in 0..n {
      tunnel total = add(i, 1)
    }
    sum -> total
  }
}
"#;
    // The emitter prefers sinks, but the language also accepts a binding used
    // as a source. Build it directly to exercise loops.
    let doc = build(src).expect("build").doc;
    let mut inputs = BTreeMap::new();
    inputs.insert("n".to_string(), Value::int(5));
    let run = lc_runtime::compile_and_run(&doc, inputs).expect("run");
    assert_eq!(run.indicators.get("total"), Some(&Value::int(5)));
}
