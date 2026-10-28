//! `labcraft` command line interface.
//!
//! The CLI is the bridge between agent output, files on disk, and the GUI. It
//! deliberately uses no argument-parsing crate: the command surface is tiny and
//! documented in `labcraft --help`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lc_model::document::ViDocument;
use lc_model::{Severity, Value};
use serde_json::json;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("labcraft: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.first().cloned() else {
        print_help();
        return Ok(());
    };
    let rest = &args[1..];
    match command.as_str() {
        "new" => cmd_new(rest),
        "run" => cmd_run(rest),
        "validate" | "check" => cmd_validate(rest),
        "export" | "dsl" => cmd_export(rest),
        "render" => cmd_render(rest),
        "test" => cmd_test(rest),
        "mcp" => lc_mcp::serve_stdio().map_err(|e| e.to_string()),
        "gui" | "open" => cmd_gui(rest),
        "nodes" => cmd_nodes(rest),
        "examples" => cmd_examples(rest),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("labcraft {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(format!("unknown command `{other}`; try `labcraft help`")),
    }
}

fn print_help() {
    println!(
        r#"labcraft {version} — visual dataflow programming for humans and agents

USAGE
  labcraft <command> [options]

COMMANDS
  new <file> [--name NAME] [--dsl FILE]   Create a VI from scratch or DSL
  run <file> [--input k=v] [--trace]      Run a VI headlessly
  validate <file>                         Type-check and list diagnostics
  export <file> [--format dsl|json]       Print DSL (default) or JSON
  render <file> [--view panel|diagram|both] [--theme dark|light] [--out FILE]
  test <file>                             Run embedded test cases
  nodes [query]                           Search the node library
  examples [--write DIR]                  Show or write the example VIs
  mcp                                     Start the MCP server on stdio
  gui [file]                              Open the visual editor
  version                                 Print version

Paths may omit the .lcvi.json extension. `--input` values are parsed as JSON
when possible, otherwise as strings. Examples:
  labcraft run over-temp.lcvi.json --input temp=35
  labcraft export over-temp.lcvi.json > over-temp.dsl
  labcraft new sum --name Sum --dsl sum.dsl
"#,
        version = env!("CARGO_PKG_VERSION")
    );
}

// ---- new ----------------------------------------------------------------

fn cmd_new(args: &[String]) -> Result<(), String> {
    let mut path = None;
    let mut name = None;
    let mut dsl = None;
    let mut description = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" | "-n" => {
                i += 1;
                name = args.get(i).cloned();
            }
            "--dsl" => {
                i += 1;
                dsl = args.get(i).cloned();
            }
            "--description" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    description = v.clone();
                }
            }
            other if !other.starts_with('-') => path = Some(other.to_string()),
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    let path = normalize_path(path.ok_or("`new` needs a file path")?);
    let name = name.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    });

    let mut doc = match dsl {
        Some(file) => {
            let text =
                std::fs::read_to_string(&file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
            let outcome = lc_dsl::build(&text).map_err(|e| e.render())?;
            print_diagnostics(&outcome.diagnostics);
            if has_errors(&outcome.diagnostics) {
                return Err("DSL produced errors; not saved".into());
            }
            outcome.doc
        }
        None => {
            let mut doc = ViDocument::new(name);
            doc.description = description;
            doc
        }
    };
    lc_layout::layout_vi(&mut doc);
    doc.save(&path).map_err(|e| e.to_string())?;
    println!(
        "created {} ({} controls, {} diagram nodes)",
        path.display(),
        doc.panel.controls.len(),
        count_nodes(&doc)
    );
    Ok(())
}

// ---- run ----------------------------------------------------------------

fn cmd_run(args: &[String]) -> Result<(), String> {
    let mut path = None;
    let mut inputs = BTreeMap::new();
    let mut trace = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" | "-i" => {
                i += 1;
                let pair = args.get(i).ok_or("`--input` needs k=v")?;
                let (k, v) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("input `{pair}` must be k=v"))?;
                inputs.insert(k.to_string(), parse_value(v));
            }
            "--trace" => trace = true,
            other if !other.starts_with('-') => path = Some(other.to_string()),
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    let doc = load_doc(&path.ok_or("`run` needs a VI file")?)?;
    let opts = lc_runtime::RunOptions {
        inputs,
        trace,
        ..lc_runtime::RunOptions::defaults()
    };
    match lc_runtime::run(&doc, &opts) {
        Ok(out) => {
            if let Some(err) = &out.error {
                eprintln!("runtime error: {err}");
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
            );
            if out.error.is_some() {
                return Err("VI reported a runtime error".into());
            }
            Ok(())
        }
        Err(lc_runtime::RunError::Compile(diags)) => {
            print_diagnostics(&diags);
            Err("VI does not compile".into())
        }
    }
}

fn parse_value(text: &str) -> Value {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        if let Ok(value) = serde_json::from_value(json) {
            return value;
        }
    }
    Value::string(text)
}

// ---- validate / export --------------------------------------------------

fn cmd_validate(args: &[String]) -> Result<(), String> {
    let doc = load_doc(args.first().ok_or("`validate` needs a VI file")?)?;
    let report = lc_runtime::compile(&doc);
    print_diagnostics(&report.diagnostics);
    if report.ok() {
        println!("ok: {} has no errors", doc.name);
        Ok(())
    } else {
        Err(format!(
            "{} error(s) in {}",
            report
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .count(),
            doc.name
        ))
    }
}

fn cmd_export(args: &[String]) -> Result<(), String> {
    let mut path = None;
    let mut format = "dsl".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" | "-f" => {
                i += 1;
                format = args.get(i).cloned().unwrap_or_else(|| "dsl".into());
            }
            "--json" => format = "json".into(),
            "--dsl" => format = "dsl".into(),
            other if !other.starts_with('-') => path = Some(other.to_string()),
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    let doc = load_doc(&path.ok_or("`export` needs a VI file")?)?;
    match format.as_str() {
        "dsl" => print!("{}", lc_dsl::emit(&doc)),
        "json" => println!("{}", doc.to_json().map_err(|e| e.to_string())?),
        other => return Err(format!("unknown format `{other}` (dsl or json)")),
    }
    Ok(())
}

// ---- render -------------------------------------------------------------

fn cmd_render(args: &[String]) -> Result<(), String> {
    let mut path = None;
    let mut view = "both".to_string();
    let mut theme = "dark".to_string();
    let mut out = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--view" => {
                i += 1;
                view = args.get(i).cloned().unwrap_or_else(|| "both".into());
            }
            "--theme" => {
                i += 1;
                theme = args.get(i).cloned().unwrap_or_else(|| "dark".into());
            }
            "--out" | "-o" => {
                i += 1;
                out = args.get(i).cloned();
            }
            other if !other.starts_with('-') => path = Some(other.to_string()),
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    let doc = load_doc(&path.ok_or("`render` needs a VI file")?)?;
    let theme = if theme == "light" {
        lc_theme::Theme::light()
    } else {
        lc_theme::Theme::dark()
    };
    let opts = lc_render::RenderOptions::default();
    let mut svgs = Vec::new();
    if view == "panel" || view == "both" {
        svgs.push(lc_render::panel_svg(&doc, &theme, &opts));
    }
    if view == "diagram" || view == "both" {
        svgs.push(lc_render::diagram_svg(&doc, &theme, &opts));
    }
    let text = svgs.join("\n");
    match out {
        Some(path) => {
            std::fs::write(&path, text).map_err(|e| format!("cannot write `{path}`: {e}"))?;
            println!("wrote {path}");
        }
        None => print!("{text}"),
    }
    Ok(())
}

// ---- tests --------------------------------------------------------------

fn cmd_test(args: &[String]) -> Result<(), String> {
    let doc = load_doc(args.first().ok_or("`test` needs a VI file")?)?;
    let report = lc_runtime::run_tests(&doc);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    if report.passed == report.total {
        Ok(())
    } else {
        Err(format!(
            "{} of {} test(s) failed",
            report.total - report.passed,
            report.total
        ))
    }
}

// ---- GUI ----------------------------------------------------------------

fn cmd_gui(args: &[String]) -> Result<(), String> {
    let path = args.first().map(|p| normalize_path(p.clone()));
    #[cfg(feature = "gui")]
    {
        lc_editor::run(path).map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = path;
        Err("this build has no GUI; rebuild with --features gui".into())
    }
}

// ---- nodes / examples ---------------------------------------------------

fn cmd_nodes(args: &[String]) -> Result<(), String> {
    let reg = lc_nodes::Registry::global();
    let infos = match args.first() {
        Some(query) => reg.search(query),
        None => reg.infos(),
    };
    for info in infos {
        println!("{:<20} {:<14} {}", info.id, info.category, info.name);
        println!("  {}", info.description);
        let inputs = info
            .inputs
            .iter()
            .map(|p| format!("{}: {:?}", p.name, p.ty))
            .collect::<Vec<_>>()
            .join(", ");
        let outputs = info
            .outputs
            .iter()
            .map(|p| format!("{}: {:?}", p.name, p.ty))
            .collect::<Vec<_>>()
            .join(", ");
        println!("  in  [{inputs}]");
        println!("  out [{outputs}]");
        println!("  e.g. {}", info.example);
    }
    Ok(())
}

const EXAMPLES: &[(&str, &str)] = &[
    (
        "over-temperature",
        include_str!("../../../examples/over-temperature.dsl"),
    ),
    ("average", include_str!("../../../examples/average.dsl")),
    ("grade", include_str!("../../../examples/grade.dsl")),
    ("count", include_str!("../../../examples/count.dsl")),
];

fn cmd_examples(args: &[String]) -> Result<(), String> {
    let mut write = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--write" | "-w" => {
                i += 1;
                write = args.get(i).cloned();
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    match write {
        Some(dir) => {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for (name, dsl) in EXAMPLES {
                let outcome = lc_dsl::build(dsl).map_err(|e| e.render())?;
                let path = Path::new(&dir).join(format!("{name}.lcvi.json"));
                outcome.doc.save(&path).map_err(|e| e.to_string())?;
                let dsl_path = Path::new(&dir).join(format!("{name}.dsl"));
                std::fs::write(dsl_path, dsl).map_err(|e| e.to_string())?;
                println!("wrote {}", path.display());
            }
            Ok(())
        }
        None => {
            for (name, dsl) in EXAMPLES {
                println!("--- {name} ---");
                print!("{dsl}");
                println!();
            }
            Ok(())
        }
    }
}

// ---- helpers ------------------------------------------------------------

fn normalize_path(path: impl Into<String>) -> PathBuf {
    let path = path.into();
    let p = PathBuf::from(&path);
    if p.extension().is_some() {
        p
    } else {
        PathBuf::from(format!("{path}.lcvi.json"))
    }
}

fn load_doc(path: &str) -> Result<ViDocument, String> {
    let path = normalize_path(path);
    ViDocument::load(&path).map_err(|e| format!("cannot load `{}`: {e}", path.display()))
}

fn count_nodes(doc: &ViDocument) -> usize {
    let mut n = 0;
    doc.for_each_node(&mut |_| n += 1);
    n
}

fn print_diagnostics(diags: &[lc_model::Diagnostic]) {
    for d in diags {
        let level = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        eprintln!("{level}[{}]: {}", d.code, d.message);
        if let Some(node) = &d.node {
            let port = d.port.as_deref().unwrap_or("");
            eprintln!("  at {node} {port}");
        }
        if !d.fixes.is_empty() {
            eprintln!("  fixes: {}", d.fixes.join(", "));
        }
    }
}

fn has_errors(diags: &[lc_model::Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

#[allow(dead_code)]
fn debug_json(value: impl serde::Serialize) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| json!(null).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_examples_build_and_run() {
        let expected = [
            ("over-temperature", "alarm", Value::Bool(false)),
            ("average", "mean", Value::num(2.0)),
            ("grade", "grade", Value::string("Pass")),
            ("count", "last", Value::int(5)),
        ];
        for (name, indicator, value) in expected {
            let dsl = EXAMPLES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, dsl)| *dsl)
                .expect("example exists");
            let outcome = lc_dsl::build(dsl).unwrap_or_else(|e| panic!("{name}: {}", e.render()));
            assert!(
                !outcome
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == Severity::Error),
                "{name}: {:#?}",
                outcome.diagnostics
            );
            let report = lc_runtime::compile(&outcome.doc);
            assert!(report.ok(), "{name}: {:#?}", report.diagnostics);
            let opts = lc_runtime::RunOptions {
                inputs: lc_runtime::default_inputs(&outcome.doc),
                ..lc_runtime::RunOptions::defaults()
            };
            let run =
                lc_runtime::run(&outcome.doc, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                run.indicators.get(indicator),
                Some(&value),
                "{name} indicator `{indicator}`"
            );
        }
    }

    #[test]
    fn examples_write_json_and_dsl() {
        let dir = std::env::temp_dir().join(format!("labcraft-examples-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        cmd_examples(&["--write".into(), dir.display().to_string()]).expect("write examples");
        for (name, _) in EXAMPLES {
            let json = dir.join(format!("{name}.lcvi.json"));
            let dsl = dir.join(format!("{name}.dsl"));
            assert!(json.exists(), "missing {}", json.display());
            assert!(dsl.exists(), "missing {}", dsl.display());
            let doc = ViDocument::load(&json).expect("load example");
            assert!(!doc.panel.controls.is_empty(), "{name} has no controls");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
