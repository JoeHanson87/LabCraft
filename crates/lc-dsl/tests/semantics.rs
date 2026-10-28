use std::collections::BTreeMap;

use lc_dsl::build;
use lc_model::Value;

fn compile_and_run(src: &str, inputs: BTreeMap<String, Value>) -> lc_runtime::RunOutput {
    let outcome = build(src).expect("build");
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    lc_runtime::compile_and_run(&outcome.doc, inputs).expect("run")
}

fn input(key: &str, value: Value) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    map.insert(key.to_string(), value);
    map
}

const CASE: &str = r#"
vi "Case Grade" {
  panel {
    control score "Score" @(20, 20) { type: i32, default: 85 }
    indicator grade "Grade" @(20, 90) { type: string }
  }
  diagram {
    let g = case score {
      0..59 { tunnel out = "F" }
      60..89 { tunnel out = "Pass" }
      default { tunnel out = "Out of range" }
    }
    g -> grade
  }
}
"#;

#[test]
fn case_frames_and_default() {
    let run = compile_and_run(CASE, input("score", Value::int(42)));
    assert_eq!(run.indicators.get("grade"), Some(&Value::string("F")));

    let run = compile_and_run(CASE, input("score", Value::int(85)));
    assert_eq!(run.indicators.get("grade"), Some(&Value::string("Pass")));

    let run = compile_and_run(CASE, input("score", Value::int(120)));
    assert_eq!(
        run.indicators.get("grade"),
        Some(&Value::string("Out of range"))
    );
}

const WHILE: &str = r#"
vi "While Counter" {
  panel {
    indicator last "Last" @(20, 20) { type: i32 }
  }
  diagram {
    let result = while iteration >= 3 {
      tunnel value = iteration
    }
    result -> last
  }
}
"#;

#[test]
fn while_loop_stops_when_condition_is_true() {
    let run = compile_and_run(WHILE, BTreeMap::new());
    assert_eq!(run.indicators.get("last"), Some(&Value::int(3)));
}

const STRINGS: &str = r#"
vi "Greeting" {
  panel {
    control name "Name" @(20, 20) { type: string, default: "Ada" }
    indicator greeting "Greeting" @(20, 90) { type: string }
  }
  diagram {
    let text = concat_strings("Hello ", name, "!")
    text -> greeting
  }
}
"#;

#[test]
fn variadic_string_concatenation() {
    let run = compile_and_run(STRINGS, BTreeMap::new());
    assert_eq!(
        run.indicators.get("greeting"),
        Some(&Value::string("Hello Ada!"))
    );
}

const ARRAY_INDEX: &str = r#"
vi "Second Value" {
  panel {
    control values "Values" @(20, 20) { type: dbl[], default: [10.0, 20.0, 30.0] }
    indicator second "Second" @(20, 90) { type: dbl }
  }
  diagram {
    values -> index_array(1) -> second
  }
}
"#;

#[test]
fn pipeline_value_fills_first_free_port() {
    let run = compile_and_run(ARRAY_INDEX, BTreeMap::new());
    assert_eq!(run.indicators.get("second"), Some(&Value::num(20.0)));
}

const CLUSTER: &str = r#"
vi "Cluster Field" {
  panel {
    control a "A" @(20, 20) { type: dbl, default: 2.0 }
    indicator field "Field" @(20, 90) { type: dbl }
  }
  diagram {
    let record = { x: a, y: 3.0 }
    record.x -> field
  }
}
"#;

#[test]
fn cluster_literals_and_field_access() {
    let run = compile_and_run(CLUSTER, BTreeMap::new());
    assert_eq!(run.indicators.get("field"), Some(&Value::num(2.0)));
}

#[test]
fn type_errors_are_reported_without_running() {
    let src = r#"
vi "Type Error" {
  panel {
    control name "Name" @(20, 20) { type: string, default: "Ada" }
    indicator sum "Sum" @(20, 90) { type: dbl }
  }
  diagram {
    name -> add(1.0) -> sum
  }
}
"#;
    let outcome = build(src).expect("build");
    let report = lc_runtime::compile(&outcome.doc);
    assert!(!report.ok(), "expected a type error");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "TYPE_RESOLUTION" && d.message.contains("expected numeric")),
        "{:#?}",
        report.diagnostics
    );
}
