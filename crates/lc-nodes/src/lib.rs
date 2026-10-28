//! Node registry, type resolution rules and the standard primitive library.
//!
//! Every node is described declaratively so the same metadata drives the
//! palette, the type checker, generated MCP schemas and the docs. Agents depend
//! on these ids and port names, so they are treated as a stable public API.

mod exec;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use lc_model::document::{kinds, Node, SourceRef, ViDocument};
use lc_model::{NumericType, Type, Value};
use serde::Serialize;

pub use exec::{execute, ExecCtx, ExecError};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Bool,
    Numeric,
    Int,
    String,
    Path,
    Error,
    Variant,
    Any,
    Cluster,
    NumericArray,
    Array(Box<PinType>),
}

impl PinType {
    pub fn display(&self) -> String {
        match self {
            PinType::Bool => "bool".into(),
            PinType::Numeric => "numeric".into(),
            PinType::Int => "int".into(),
            PinType::String => "string".into(),
            PinType::Path => "path".into(),
            PinType::Error => "error".into(),
            PinType::Variant => "variant".into(),
            PinType::Any => "any".into(),
            PinType::Cluster => "cluster".into(),
            PinType::NumericArray => "numeric[]".into(),
            PinType::Array(inner) => format!("{}[]", inner.display()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Pin {
    pub name: &'static str,
    pub ty: PinType,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Numeric1,
    Numeric2,
    NumericCompare,
    Bool2,
    Bool1,
    Select,
    ToString,
    ToNumeric,
    ToInt,
    ToBool,
    StringLength,
    Concat,
    Substring,
    String1,
    Replace,
    FormatNumber,
    ScanNumber,
    BuildArray,
    IndexArray,
    ArraySize,
    ArraySubset,
    ArraySort,
    ArrayReverse,
    ArraySum,
    ArrayMean,
    ArraySearch,
    Bundle,
    Unbundle,
    UnbundleByName,
    Constant,
    Control,
    Indicator,
    AnyToVoid,
    Identity,
    ErrorStatus,
    ErrorClear,
    Wait,
    Tick,
    Random,
    FileRead,
    FileWrite,
    JsonParse,
    JsonStringify,
    Structure,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Quotient,
    Negate,
    Abs,
    Increment,
    Decrement,
    Round,
    Sqrt,
    Pow,
    Min,
    Eq,
    Ne,
    Gt,
    Lt,
    Gte,
    Lte,
    And,
    Or,
    Not,
    Xor,
    Select,
    ToString,
    ToDbl,
    ToI32,
    ToBool,
    StringLength,
    Concat,
    Substring,
    Upper,
    Lower,
    Trim,
    Replace,
    FormatNumber,
    ScanNumber,
    BuildArray,
    IndexArray,
    ArraySize,
    ArraySubset,
    ArraySort,
    ArrayReverse,
    ArraySum,
    ArrayMean,
    ArraySearch,
    Bundle,
    Unbundle,
    UnbundleByName,
    Constant,
    Control,
    Indicator,
    Log,
    Message,
    ErrorStatus,
    ErrorClear,
    Wait,
    Tick,
    Random,
    FileRead,
    FileWrite,
    FileAppend,
    JsonParse,
    JsonStringify,
    Noop,
}

pub struct NodeDef {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub keywords: &'static [&'static str],
    pub inputs: Vec<Pin>,
    pub outputs: Vec<Pin>,
    pub rule: Rule,
    pub op: Op,
    pub variadic: bool,
    /// A short example expression used by docs and agent prompts.
    pub example: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub inputs: Vec<Pin>,
    pub outputs: Vec<Pin>,
    pub variadic: bool,
    pub example: String,
}

pub struct Registry {
    defs: BTreeMap<&'static str, NodeDef>,
    aliases: BTreeMap<&'static str, &'static str>,
}

impl Registry {
    pub fn global() -> &'static Registry {
        static REG: OnceLock<Registry> = OnceLock::new();
        REG.get_or_init(Registry::build)
    }

    pub fn get(&self, id: &str) -> Option<&NodeDef> {
        self.defs.get(id).or_else(|| {
            self.aliases
                .get(id)
                .and_then(|canonical| self.defs.get(canonical))
        })
    }

    pub fn defs(&self) -> impl Iterator<Item = &NodeDef> {
        self.defs.values()
    }

    pub fn info(&self, id: &str) -> Option<NodeInfo> {
        let d = self.get(id)?;
        Some(NodeInfo {
            id: d.id.to_string(),
            name: d.name.to_string(),
            category: d.category.to_string(),
            description: d.description.to_string(),
            keywords: d.keywords.iter().map(|s| s.to_string()).collect(),
            inputs: d.inputs.clone(),
            outputs: d.outputs.clone(),
            variadic: d.variadic,
            example: d.example.to_string(),
        })
    }

    pub fn infos(&self) -> Vec<NodeInfo> {
        let mut v: Vec<_> = self.defs().filter_map(|d| self.info(d.id)).collect();
        v.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
        v
    }

    pub fn categories(&self) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for d in self.defs() {
            out.entry(d.category.to_string())
                .or_default()
                .push(d.id.to_string());
        }
        for v in out.values_mut() {
            v.sort();
        }
        out
    }

    pub fn search(&self, query: &str) -> Vec<NodeInfo> {
        let q = query.to_lowercase();
        let mut scored: Vec<(i32, NodeInfo)> = Vec::new();
        for d in self.defs() {
            let mut score = 0;
            if d.id == q || d.name.to_lowercase() == q {
                score += 100;
            }
            if d.id.contains(&q) {
                score += 40;
            }
            if d.name.to_lowercase().contains(&q) {
                score += 30;
            }
            if d.keywords.iter().any(|k| k.to_lowercase().contains(&q)) {
                score += 20;
            }
            if d.description.to_lowercase().contains(&q) {
                score += 5;
            }
            if score > 0 {
                if let Some(info) = self.info(d.id) {
                    scored.push((score, info));
                }
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    fn build() -> Registry {
        let mut defs = BTreeMap::new();
        let mut aliases = BTreeMap::new();

        let mut add = |d: NodeDef| {
            defs.insert(d.id, d);
        };

        // ---- Numeric -----------------------------------------------------
        add(def(
            "add",
            "Add",
            "Numeric",
            "Add two values or arrays element-wise.",
            &["+", "sum", "arithmetic"],
            vec![num_in("x"), num_in("y")],
            vec![out("sum", PinType::Numeric)],
            Rule::Numeric2,
            Op::Add,
            "temp -> add(1.0)",
        ));
        add(def(
            "subtract",
            "Subtract",
            "Numeric",
            "Subtract y from x.",
            &["-", "minus", "arithmetic"],
            vec![num_in("x"), num_in("y")],
            vec![out("difference", PinType::Numeric)],
            Rule::Numeric2,
            Op::Sub,
            "x -> subtract(y)",
        ));
        add(def(
            "multiply",
            "Multiply",
            "Numeric",
            "Multiply two values or arrays.",
            &["*", "product", "arithmetic"],
            vec![num_in("x"), num_in("y")],
            vec![out("product", PinType::Numeric)],
            Rule::Numeric2,
            Op::Mul,
            "a -> multiply(b)",
        ));
        add(def(
            "divide",
            "Divide",
            "Numeric",
            "Divide x by y.",
            &["/", "quotient", "arithmetic"],
            vec![num_in("x"), num_in("y")],
            vec![out("quotient", PinType::Numeric)],
            Rule::Numeric2,
            Op::Div,
            "total -> divide(count)",
        ));
        add(def(
            "quotient",
            "Quotient & Remainder",
            "Numeric",
            "Integer quotient and remainder.",
            &["mod", "div", "integer"],
            vec![int_in("x"), int_in("y")],
            vec![
                out("quotient", PinType::Int),
                out("remainder", PinType::Int),
            ],
            Rule::Numeric2,
            Op::Quotient,
            "value -> quotient(10)",
        ));
        add(def(
            "negate",
            "Negate",
            "Numeric",
            "Negate a numeric value.",
            &["-", "invert"],
            vec![num_in("x")],
            vec![out("result", PinType::Numeric)],
            Rule::Numeric1,
            Op::Negate,
            "x -> negate",
        ));
        add(def(
            "abs",
            "Absolute Value",
            "Numeric",
            "Absolute value.",
            &["magnitude", "abs"],
            vec![num_in("x")],
            vec![out("result", PinType::Numeric)],
            Rule::Numeric1,
            Op::Abs,
            "delta -> abs",
        ));
        add(def(
            "increment",
            "Increment",
            "Numeric",
            "Add one.",
            &["+1", "count"],
            vec![num_in("x")],
            vec![out("result", PinType::Numeric)],
            Rule::Numeric1,
            Op::Increment,
            "i -> increment",
        ));
        add(def(
            "decrement",
            "Decrement",
            "Numeric",
            "Subtract one.",
            &["-1"],
            vec![num_in("x")],
            vec![out("result", PinType::Numeric)],
            Rule::Numeric1,
            Op::Decrement,
            "n -> decrement",
        ));
        add(def(
            "round",
            "Round to Nearest",
            "Numeric",
            "Round to the nearest integer.",
            &["round", "integer"],
            vec![num_in("x")],
            vec![out("result", PinType::Numeric)],
            Rule::Numeric1,
            Op::Round,
            "x -> round",
        ));
        add(def(
            "sqrt",
            "Square Root",
            "Numeric",
            "Square root.",
            &["root", "math"],
            vec![num_in("x")],
            vec![out("result", PinType::Numeric)],
            Rule::Numeric1,
            Op::Sqrt,
            "variance -> sqrt",
        ));
        add(def(
            "pow",
            "Power of X",
            "Numeric",
            "Raise x to the power of y.",
            &["exponent", "power", "math"],
            vec![num_in("x"), num_in("y")],
            vec![out("result", PinType::Numeric)],
            Rule::Numeric2,
            Op::Pow,
            "base -> pow(2.0)",
        ));
        add(def(
            "min",
            "Min & Max",
            "Numeric",
            "Minimum and maximum of two values.",
            &["minimum", "maximum", "clamp"],
            vec![num_in("x"), num_in("y")],
            vec![out("min", PinType::Numeric), out("max", PinType::Numeric)],
            Rule::Numeric2,
            Op::Min,
            "a -> min(b)",
        ));
        add(def(
            "random",
            "Random Number",
            "Numeric",
            "Uniform random number in [0,1).",
            &["random", "rand"],
            vec![],
            vec![out("value", PinType::Numeric)],
            Rule::Random,
            Op::Random,
            "random()",
        ));
        add(def(
            "tick",
            "Tick Count",
            "Timing",
            "Milliseconds since the VI started.",
            &["time", "clock", "ms"],
            vec![],
            vec![out("milliseconds", PinType::Int)],
            Rule::Tick,
            Op::Tick,
            "tick()",
        ));
        add(def(
            "wait",
            "Wait (ms)",
            "Timing",
            "Wait for a number of milliseconds.",
            &["sleep", "delay", "timing"],
            vec![int_in("milliseconds")],
            vec![],
            Rule::Wait,
            Op::Wait,
            "wait(250)",
        ));

        // ---- Comparison & boolean ---------------------------------------
        add(def(
            "equal",
            "Equal?",
            "Comparison",
            "True when both values are equal.",
            &["==", "compare", "eq"],
            vec![num_in("x"), num_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::NumericCompare,
            Op::Eq,
            "x -> equal(0)",
        ));
        add(def(
            "not_equal",
            "Not Equal?",
            "Comparison",
            "True when values differ.",
            &["!=", "compare", "ne"],
            vec![num_in("x"), num_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::NumericCompare,
            Op::Ne,
            "x -> not_equal(0)",
        ));
        add(def(
            "greater",
            "Greater?",
            "Comparison",
            "True when x is greater than y.",
            &[">", "compare", "gt"],
            vec![num_in("x"), num_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::NumericCompare,
            Op::Gt,
            "temp -> greater(30.0)",
        ));
        add(def(
            "less",
            "Less?",
            "Comparison",
            "True when x is less than y.",
            &["<", "compare", "lt"],
            vec![num_in("x"), num_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::NumericCompare,
            Op::Lt,
            "temp -> less(0.0)",
        ));
        add(def(
            "greater_equal",
            "Greater Or Equal?",
            "Comparison",
            "True when x is at least y.",
            &[">=", "compare"],
            vec![num_in("x"), num_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::NumericCompare,
            Op::Gte,
            "x -> greater_equal(1.0)",
        ));
        add(def(
            "less_equal",
            "Less Or Equal?",
            "Comparison",
            "True when x is at most y.",
            &["<=", "compare"],
            vec![num_in("x"), num_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::NumericCompare,
            Op::Lte,
            "x -> less_equal(100.0)",
        ));
        add(def(
            "and",
            "And",
            "Boolean",
            "Logical AND.",
            &["&&", "logic", "boolean"],
            vec![bool_in("x"), bool_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::Bool2,
            Op::And,
            "a -> and(b)",
        ));
        add(def(
            "or",
            "Or",
            "Boolean",
            "Logical OR.",
            &["||", "logic", "boolean"],
            vec![bool_in("x"), bool_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::Bool2,
            Op::Or,
            "a -> or(b)",
        ));
        add(def(
            "not",
            "Not",
            "Boolean",
            "Logical NOT.",
            &["!", "invert", "boolean"],
            vec![bool_in("x")],
            vec![out("result", PinType::Bool)],
            Rule::Bool1,
            Op::Not,
            "alarm -> not",
        ));
        add(def(
            "xor",
            "Exclusive Or",
            "Boolean",
            "Logical XOR.",
            &["xor", "logic"],
            vec![bool_in("x"), bool_in("y")],
            vec![out("result", PinType::Bool)],
            Rule::Bool2,
            Op::Xor,
            "a -> xor(b)",
        ));
        add(def(
            "select",
            "Select",
            "Comparison",
            "Choose between two values based on a boolean selector.",
            &["if", "ternary", "mux", "condition"],
            vec![
                bool_in("selector"),
                Pin {
                    name: "true",
                    ty: PinType::Any,
                    required: true,
                    default: None,
                    description: "Value when selector is true",
                },
                Pin {
                    name: "false",
                    ty: PinType::Any,
                    required: true,
                    default: None,
                    description: "Value when selector is false",
                },
            ],
            vec![out("result", PinType::Any)],
            Rule::Select,
            Op::Select,
            "ok -> select(1, 0)",
        ));

        // ---- Conversion -------------------------------------------------
        add(def(
            "to_string",
            "Number To String",
            "String",
            "Convert a value to its string representation.",
            &["format", "cast", "text"],
            vec![Pin {
                name: "value",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Value to convert",
            }],
            vec![out("string", PinType::String)],
            Rule::ToString,
            Op::ToString,
            "temp -> to_string",
        ));
        add(def(
            "to_double",
            "To Double",
            "Numeric",
            "Convert to double precision.",
            &["cast", "dbl", "real"],
            vec![Pin {
                name: "value",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Value to convert",
            }],
            vec![out("value", PinType::Numeric)],
            Rule::ToNumeric,
            Op::ToDbl,
            "text -> to_double",
        ));
        add(def(
            "to_int",
            "To Integer",
            "Numeric",
            "Convert to a 32-bit integer.",
            &["cast", "i32", "int"],
            vec![Pin {
                name: "value",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Value to convert",
            }],
            vec![out("value", PinType::Int)],
            Rule::ToInt,
            Op::ToI32,
            "value -> to_int",
        ));
        add(def(
            "to_bool",
            "To Boolean",
            "Boolean",
            "Convert a value to boolean.",
            &["cast", "bool"],
            vec![Pin {
                name: "value",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Value to convert",
            }],
            vec![out("value", PinType::Bool)],
            Rule::ToBool,
            Op::ToBool,
            "value -> to_bool",
        ));

        // ---- Strings ----------------------------------------------------
        add(def(
            "string_length",
            "String Length",
            "String",
            "Number of bytes in a string.",
            &["len", "size", "string"],
            vec![str_in("string")],
            vec![out("length", PinType::Int)],
            Rule::StringLength,
            Op::StringLength,
            "text -> string_length",
        ));
        add(def_variadic(
            "concat_strings",
            "Concatenate Strings",
            "String",
            "Join any number of strings.",
            &["concat", "join", "append", "string"],
            vec![out("string", PinType::String)],
            Rule::Concat,
            Op::Concat,
            "a -> concat_strings(\" \", b)",
        ));
        add(def(
            "substring",
            "Substring",
            "String",
            "Extract part of a string.",
            &["slice", "substr", "string"],
            vec![str_in("string"), int_in("offset"), int_in("length")],
            vec![out("substring", PinType::String)],
            Rule::Substring,
            Op::Substring,
            "text -> substring(0, 5)",
        ));
        add(def(
            "to_upper",
            "To Upper Case",
            "String",
            "Convert a string to upper case.",
            &["uppercase", "string"],
            vec![str_in("string")],
            vec![out("string", PinType::String)],
            Rule::String1,
            Op::Upper,
            "text -> to_upper",
        ));
        add(def(
            "to_lower",
            "To Lower Case",
            "String",
            "Convert a string to lower case.",
            &["lowercase", "string"],
            vec![str_in("string")],
            vec![out("string", PinType::String)],
            Rule::String1,
            Op::Lower,
            "text -> to_lower",
        ));
        add(def(
            "trim",
            "Trim Whitespace",
            "String",
            "Remove leading and trailing whitespace.",
            &["strip", "string"],
            vec![str_in("string")],
            vec![out("string", PinType::String)],
            Rule::String1,
            Op::Trim,
            "text -> trim",
        ));
        add(def(
            "replace",
            "Search And Replace String",
            "String",
            "Replace every occurrence of a substring.",
            &["replace", "substitute", "string"],
            vec![str_in("string"), str_in("search"), str_in("replace")],
            vec![out("result", PinType::String)],
            Rule::Replace,
            Op::Replace,
            "text -> replace(\"a\", \"b\")",
        ));
        add(def(
            "format_number",
            "Format Into String",
            "String",
            "Format a numeric value into a string.",
            &["format", "printf", "string"],
            vec![
                str_in("format"),
                Pin {
                    name: "value",
                    ty: PinType::Any,
                    required: true,
                    default: None,
                    description: "Value to format",
                },
            ],
            vec![out("string", PinType::String)],
            Rule::FormatNumber,
            Op::FormatNumber,
            "value -> format_number(\"%.2f\")",
        ));
        add(def(
            "scan_number",
            "Scan From String",
            "String",
            "Parse a leading number from a string.",
            &["parse", "atoi", "scan"],
            vec![str_in("string")],
            vec![
                out("value", PinType::Numeric),
                out("success", PinType::Bool),
            ],
            Rule::ScanNumber,
            Op::ScanNumber,
            "text -> scan_number",
        ));

        // ---- Arrays -----------------------------------------------------
        add(def_variadic(
            "build_array",
            "Build Array",
            "Array",
            "Concatenate elements or arrays into an array.",
            &["array", "create", "collect"],
            vec![out("array", PinType::Array(Box::new(PinType::Any)))],
            Rule::BuildArray,
            Op::BuildArray,
            "[a, b, c]",
        ));
        add(def(
            "index_array",
            "Index Array",
            "Array",
            "Read one element from an array.",
            &["array", "element", "index", "get"],
            vec![
                Pin {
                    name: "array",
                    ty: PinType::Array(Box::new(PinType::Any)),
                    required: true,
                    default: None,
                    description: "Array to read",
                },
                int_in("index"),
            ],
            vec![out("element", PinType::Any)],
            Rule::IndexArray,
            Op::IndexArray,
            "values -> index_array(0)",
        ));
        add(def(
            "array_size",
            "Array Size",
            "Array",
            "Total number of elements in an array.",
            &["length", "count", "size"],
            vec![Pin {
                name: "array",
                ty: PinType::Array(Box::new(PinType::Any)),
                required: true,
                default: None,
                description: "Array",
            }],
            vec![out("size", PinType::Int)],
            Rule::ArraySize,
            Op::ArraySize,
            "values -> array_size",
        ));
        add(def(
            "array_subset",
            "Array Subset",
            "Array",
            "Take a contiguous slice of an array.",
            &["slice", "range", "subarray"],
            vec![
                Pin {
                    name: "array",
                    ty: PinType::Array(Box::new(PinType::Any)),
                    required: true,
                    default: None,
                    description: "Source array",
                },
                int_in("index"),
                int_in("length"),
            ],
            vec![out("subset", PinType::Array(Box::new(PinType::Any)))],
            Rule::ArraySubset,
            Op::ArraySubset,
            "values -> array_subset(0, 10)",
        ));
        add(def(
            "sort_array",
            "Sort 1D Array",
            "Array",
            "Sort an array ascending.",
            &["order", "sort"],
            vec![Pin {
                name: "array",
                ty: PinType::Array(Box::new(PinType::Any)),
                required: true,
                default: None,
                description: "Array to sort",
            }],
            vec![out("sorted", PinType::Array(Box::new(PinType::Any)))],
            Rule::ArraySort,
            Op::ArraySort,
            "values -> sort_array",
        ));
        add(def(
            "reverse_array",
            "Reverse 1D Array",
            "Array",
            "Reverse an array.",
            &["reverse", "flip"],
            vec![Pin {
                name: "array",
                ty: PinType::Array(Box::new(PinType::Any)),
                required: true,
                default: None,
                description: "Array to reverse",
            }],
            vec![out("reversed", PinType::Array(Box::new(PinType::Any)))],
            Rule::ArrayReverse,
            Op::ArrayReverse,
            "values -> reverse_array",
        ));
        add(def(
            "array_sum",
            "Sum Of Array Elements",
            "Array",
            "Add all numeric elements.",
            &["sum", "total", "reduce"],
            vec![Pin {
                name: "array",
                ty: PinType::NumericArray,
                required: true,
                default: None,
                description: "Numeric array",
            }],
            vec![out("sum", PinType::Numeric)],
            Rule::ArraySum,
            Op::ArraySum,
            "values -> array_sum",
        ));
        add(def(
            "array_mean",
            "Mean Of Array",
            "Array",
            "Arithmetic mean of a numeric array.",
            &["average", "mean", "stats"],
            vec![Pin {
                name: "array",
                ty: PinType::NumericArray,
                required: true,
                default: None,
                description: "Numeric array",
            }],
            vec![out("mean", PinType::Numeric)],
            Rule::ArrayMean,
            Op::ArrayMean,
            "values -> array_mean",
        ));
        add(def(
            "array_search",
            "Search 1D Array",
            "Array",
            "Find the index of a value, or -1.",
            &["find", "index", "search"],
            vec![
                Pin {
                    name: "array",
                    ty: PinType::Array(Box::new(PinType::Any)),
                    required: true,
                    default: None,
                    description: "Array to search",
                },
                Pin {
                    name: "value",
                    ty: PinType::Any,
                    required: true,
                    default: None,
                    description: "Value to find",
                },
            ],
            vec![out("index", PinType::Int)],
            Rule::ArraySearch,
            Op::ArraySearch,
            "values -> array_search(3)",
        ));

        // ---- Clusters ---------------------------------------------------
        add(def(
            "bundle",
            "Bundle",
            "Cluster",
            "Combine values into a cluster.",
            &["cluster", "pack", "struct"],
            vec![any_in("a"), any_in("b"), any_in("c"), any_in("d")],
            vec![out("cluster", PinType::Cluster)],
            Rule::Bundle,
            Op::Bundle,
            "{a, b, c}",
        ));
        add(def(
            "unbundle",
            "Unbundle",
            "Cluster",
            "Split a cluster into its fields by position.",
            &["cluster", "unpack", "struct"],
            vec![Pin {
                name: "cluster",
                ty: PinType::Cluster,
                required: true,
                default: None,
                description: "Cluster to split",
            }],
            vec![
                out("a", PinType::Any),
                out("b", PinType::Any),
                out("c", PinType::Any),
                out("d", PinType::Any),
            ],
            Rule::Unbundle,
            Op::Unbundle,
            "point -> unbundle",
        ));
        add(def(
            "unbundle_by_name",
            "Unbundle By Name",
            "Cluster",
            "Read named cluster fields. Set props.fields to the field names.",
            &["cluster", "field", "by name"],
            vec![Pin {
                name: "cluster",
                ty: PinType::Cluster,
                required: true,
                default: None,
                description: "Cluster to read",
            }],
            vec![],
            Rule::UnbundleByName,
            Op::UnbundleByName,
            "record -> unbundle_by_name(fields: [\"x\"])",
        ));

        // ---- Error handling ---------------------------------------------
        add(def(
            "error_status",
            "Error Status",
            "Error",
            "Extract the status boolean from an error cluster.",
            &["error", "status"],
            vec![Pin {
                name: "error",
                ty: PinType::Error,
                required: true,
                default: None,
                description: "Error cluster",
            }],
            vec![out("status", PinType::Bool)],
            Rule::ErrorStatus,
            Op::ErrorStatus,
            "error -> error_status",
        ));
        add(def(
            "error_clear",
            "Clear Error",
            "Error",
            "Clear the status of an error cluster.",
            &["error", "clear", "reset"],
            vec![Pin {
                name: "error",
                ty: PinType::Error,
                required: true,
                default: None,
                description: "Error cluster",
            }],
            vec![out("error", PinType::Error)],
            Rule::ErrorClear,
            Op::ErrorClear,
            "error -> error_clear",
        ));

        // ---- Files & data ----------------------------------------------
        add(def(
            "file_read",
            "Read Text File",
            "File",
            "Read a UTF-8 text file relative to the project.",
            &["file", "read", "text", "io"],
            vec![str_in("path")],
            vec![out("text", PinType::String), out("error", PinType::Error)],
            Rule::FileRead,
            Op::FileRead,
            "path -> file_read",
        ));
        add(def(
            "file_write",
            "Write Text File",
            "File",
            "Write UTF-8 text to a project file.",
            &["file", "write", "save", "io"],
            vec![str_in("path"), str_in("text")],
            vec![out("success", PinType::Bool), out("error", PinType::Error)],
            Rule::FileWrite,
            Op::FileWrite,
            "path -> file_write(text)",
        ));
        add(def(
            "json_parse",
            "JSON Parse",
            "Data",
            "Parse JSON text into a variant.",
            &["json", "parse", "data"],
            vec![str_in("text")],
            vec![out("value", PinType::Variant), out("error", PinType::Error)],
            Rule::JsonParse,
            Op::JsonParse,
            "text -> json_parse",
        ));
        add(def(
            "json_stringify",
            "JSON Stringify",
            "Data",
            "Serialize a value to JSON text.",
            &["json", "stringify", "data"],
            vec![Pin {
                name: "value",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Value to serialize",
            }],
            vec![out("text", PinType::String)],
            Rule::JsonStringify,
            Op::JsonStringify,
            "value -> json_stringify",
        ));

        // ---- Logging ----------------------------------------------------
        add(def(
            "log",
            "Log Message",
            "Output",
            "Append a message to the run log.",
            &["print", "log", "debug", "output"],
            vec![Pin {
                name: "value",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Message value",
            }],
            vec![],
            Rule::AnyToVoid,
            Op::Log,
            "value -> log",
        ));
        add(def(
            "message",
            "One Button Dialog",
            "Output",
            "Show a message (headless runs record it).",
            &["dialog", "alert", "message"],
            vec![str_in("message")],
            vec![],
            Rule::AnyToVoid,
            Op::Message,
            "\"Done\" -> message",
        ));

        // ---- Terminals --------------------------------------------------
        add(def(
            "control",
            "Control Terminal",
            "Terminal",
            "Reads a front-panel control value.",
            &["input", "terminal", "control"],
            vec![],
            vec![out("value", PinType::Any)],
            Rule::Control,
            Op::Control,
            "temp",
        ));
        add(def(
            "indicator",
            "Indicator Terminal",
            "Terminal",
            "Writes a value to a front-panel indicator.",
            &["output", "terminal", "indicator"],
            vec![Pin {
                name: "value",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Value to display",
            }],
            vec![],
            Rule::Indicator,
            Op::Indicator,
            "-> alarm",
        ));
        add(def(
            "constant",
            "Constant",
            "Terminal",
            "A typed literal value.",
            &["literal", "value", "const"],
            vec![],
            vec![out("value", PinType::Any)],
            Rule::Constant,
            Op::Constant,
            "30.0",
        ));

        // ---- Structures (handled by the runtime) -----------------------
        add(def(
            "structure.while",
            "While Loop",
            "Structures",
            "Repeat a diagram until its condition is met.",
            &["loop", "while", "repeat"],
            vec![],
            vec![],
            Rule::Structure,
            Op::Noop,
            "while condition { ... }",
        ));
        add(def(
            "structure.for",
            "For Loop",
            "Structures",
            "Repeat a diagram N times, with optional auto-indexing.",
            &["loop", "for", "iterate"],
            vec![int_in("n")],
            vec![],
            Rule::Structure,
            Op::Noop,
            "for i in 0..10 { ... }",
        ));
        add(def(
            "structure.case",
            "Case Structure",
            "Structures",
            "Choose one frame based on a selector value.",
            &["case", "switch", "branch"],
            vec![Pin {
                name: "selector",
                ty: PinType::Any,
                required: true,
                default: None,
                description: "Selector value",
            }],
            vec![],
            Rule::Structure,
            Op::Noop,
            "case selector { ... }",
        ));
        add(def(
            "structure.sequence",
            "Flat Sequence",
            "Structures",
            "Execute frames left to right.",
            &["sequence", "order", "frames"],
            vec![],
            vec![],
            Rule::Structure,
            Op::Noop,
            "sequence { ... }",
        ));
        add(def(
            "structure.event",
            "Event Structure",
            "Structures",
            "Handle front-panel events.",
            &["event", "ui", "handler"],
            vec![Pin {
                name: "timeout",
                ty: PinType::Int,
                required: false,
                default: Some(Value::int(-1)),
                description: "Timeout in ms, -1 waits forever",
            }],
            vec![],
            Rule::Structure,
            Op::Noop,
            "event \"Button\" { ... }",
        ));
        add(def(
            "loop.condition",
            "Loop Condition",
            "Structures",
            "Stop/continue terminal inside a loop body.",
            &["condition", "stop", "while"],
            vec![bool_in("value")],
            vec![],
            Rule::Noop,
            Op::Noop,
            "",
        ));

        // Convenience aliases used by the DSL and by models.
        for (alias, canonical) in [
            ("+", "add"),
            ("-", "subtract"),
            ("*", "multiply"),
            ("/", "divide"),
            (">", "greater"),
            ("<", "less"),
            ("==", "equal"),
            ("!=", "not_equal"),
            (">=", "greater_equal"),
            ("<=", "less_equal"),
            ("&&", "and"),
            ("||", "or"),
            ("concat", "concat_strings"),
            ("str", "to_string"),
            ("array", "build_array"),
            ("index", "index_array"),
            ("size", "array_size"),
        ] {
            aliases.insert(alias, canonical);
        }

        Registry { defs, aliases }
    }
}

#[allow(clippy::too_many_arguments)]
fn def(
    id: &'static str,
    name: &'static str,
    category: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
    inputs: Vec<Pin>,
    outputs: Vec<Pin>,
    rule: Rule,
    op: Op,
    example: &'static str,
) -> NodeDef {
    NodeDef {
        id,
        name,
        category,
        description,
        keywords,
        inputs,
        outputs,
        rule,
        op,
        variadic: false,
        example,
    }
}

#[allow(clippy::too_many_arguments)]
fn def_variadic(
    id: &'static str,
    name: &'static str,
    category: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
    outputs: Vec<Pin>,
    rule: Rule,
    op: Op,
    example: &'static str,
) -> NodeDef {
    NodeDef {
        id,
        name,
        category,
        description,
        keywords,
        inputs: vec![Pin {
            name: "in0",
            ty: PinType::Any,
            required: false,
            default: None,
            description: "Elements, in order",
        }],
        outputs,
        rule,
        op,
        variadic: true,
        example,
    }
}

fn pin(name: &'static str, ty: PinType, required: bool, description: &'static str) -> Pin {
    Pin {
        name,
        ty,
        required,
        default: None,
        description,
    }
}
fn num_in(name: &'static str) -> Pin {
    pin(name, PinType::Numeric, true, "Numeric input")
}
fn int_in(name: &'static str) -> Pin {
    pin(name, PinType::Int, true, "Integer input")
}
fn bool_in(name: &'static str) -> Pin {
    pin(name, PinType::Bool, true, "Boolean input")
}
fn str_in(name: &'static str) -> Pin {
    pin(name, PinType::String, true, "String input")
}
fn any_in(name: &'static str) -> Pin {
    pin(name, PinType::Any, false, "Optional value")
}
fn out(name: &'static str, ty: PinType) -> Pin {
    pin(name, ty, false, "Output")
}

/// Resolved pin types for one node instance.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    pub inputs: BTreeMap<String, Type>,
    pub outputs: BTreeMap<String, Type>,
    /// For unbundle nodes: output port -> cluster field index.
    pub cluster_fields: BTreeMap<String, usize>,
}

impl Resolved {
    pub fn output(&self, port: &str) -> Option<&Type> {
        self.outputs.get(port)
    }
}

/// Resolve a node's terminal types. `inputs` contains the actual type of every
/// *connected* source port. Unconnected ports fall back to their declared type.
pub fn resolve(
    def: &NodeDef,
    node: &Node,
    doc: &ViDocument,
    inputs: &BTreeMap<String, Type>,
) -> Result<Resolved, String> {
    let mut r = Resolved::default();
    macro_rules! pin_in {
        ($p:expr, $t:expr $(,)?) => {
            r.inputs.insert($p.to_string(), $t);
        };
    }
    macro_rules! pin_out {
        ($p:expr, $t:expr $(,)?) => {
            r.outputs.insert($p.to_string(), $t);
        };
    }

    match def.rule {
        Rule::Control => {
            let key = node
                .control_key()
                .ok_or_else(|| "control terminal has no `control` property".to_string())?;
            let c = doc.control(key).ok_or_else(|| {
                format!("control terminal references missing panel control `{key}`")
            })?;
            pin_out!("value", c.ty.clone());
        }
        Rule::Indicator => {
            let key = node
                .control_key()
                .ok_or_else(|| "indicator terminal has no `control` property".to_string())?;
            let c = doc
                .control(key)
                .ok_or_else(|| format!("indicator references missing panel control `{key}`"))?;
            pin_in!("value", c.ty.clone());
        }
        Rule::Constant => {
            let ty = node
                .props
                .get("type")
                .and_then(|v| serde_json::from_value::<Type>(v.clone()).ok())
                .unwrap_or_else(|| {
                    node.props
                        .get("value")
                        .and_then(|v| serde_json::from_value::<Value>(v.clone()).ok())
                        .map(|v| v.type_of())
                        .unwrap_or(Type::Void)
                });
            pin_out!("value", ty);
        }
        Rule::Structure => {
            return Err("structures are resolved by the runtime".into());
        }
        Rule::Noop => {}
        Rule::Numeric1 => {
            let (ty, _) = numeric_of(inputs.get("x"))?;
            pin_in!("x", ty.clone());
            pin_out!("result", ty);
        }
        Rule::Numeric2 => {
            let (xt, yt, out) = numeric_pair(inputs.get("x"), inputs.get("y"))?;
            pin_in!("x", xt);
            pin_in!("y", yt);
            if def.id == "min" {
                pin_out!("min", out.clone());
                pin_out!("max", out);
            } else if def.id == "quotient" {
                pin_out!("quotient", Type::i32());
                pin_out!("remainder", Type::i32());
            } else if def.outputs.len() == 1 {
                pin_out!(def.outputs[0].name, out);
            } else {
                for p in &def.outputs {
                    pin_out!(p.name, out.clone());
                }
            }
        }
        Rule::NumericCompare => {
            let (xt, yt, out) = if inputs.get("x").map(|t| *t == Type::String).unwrap_or(false)
                && inputs.get("y").map(|t| *t == Type::String).unwrap_or(false)
                && matches!(def.op, Op::Eq | Op::Ne)
            {
                (Type::String, Type::String, Type::Bool)
            } else {
                numeric_pair(inputs.get("x"), inputs.get("y"))?
            };
            pin_in!("x", xt);
            pin_in!("y", yt);
            let result = match out {
                Type::Array { dims, .. } => Type::Array {
                    element: Box::new(Type::Bool),
                    dims,
                },
                _ => Type::Bool,
            };
            pin_out!("result", result);
        }
        Rule::Bool2 => {
            pin_in!("x", Type::Bool);
            pin_in!("y", Type::Bool);
            pin_out!("result", Type::Bool);
        }
        Rule::Bool1 => {
            pin_in!("x", Type::Bool);
            pin_out!("result", Type::Bool);
        }
        Rule::Select => {
            let t = inputs.get("true").cloned().unwrap_or(Type::Void);
            let f = inputs.get("false").cloned().unwrap_or(Type::Void);
            let out = if matches!(t, Type::Void) { f } else { t };
            pin_in!("selector", Type::Bool);
            pin_in!("true", out.clone());
            pin_in!("false", out.clone());
            pin_out!("result", out);
        }
        Rule::ToString | Rule::AnyToVoid => {
            let ty = inputs
                .get("value")
                .or_else(|| inputs.get("message"))
                .cloned()
                .unwrap_or(Type::String);
            pin_in!("value", ty);
            if !r.inputs.contains_key("message") {
                pin_in!("message", Type::String);
            }
            if def.rule == Rule::ToString {
                pin_out!("string", Type::String);
            }
        }
        Rule::ToNumeric => {
            pin_in!("value", Type::dbl());
            pin_out!("value", Type::dbl());
        }
        Rule::ToInt => {
            pin_in!("value", Type::i32());
            pin_out!("value", Type::i32());
        }
        Rule::ToBool => {
            pin_in!("value", Type::Bool);
            pin_out!("value", Type::Bool);
        }
        Rule::StringLength => {
            pin_in!("string", Type::String);
            pin_out!("length", Type::i32());
        }
        Rule::Concat => {
            for (port, ty) in inputs {
                pin_in!(
                    port,
                    if ty.accepts(&Type::String) {
                        Type::String
                    } else {
                        ty.clone()
                    }
                );
            }
            pin_out!("string", Type::String);
        }
        Rule::Substring => {
            pin_in!("string", Type::String);
            pin_in!("offset", Type::i32());
            pin_in!("length", Type::i32());
            pin_out!("substring", Type::String);
        }
        Rule::String1 => {
            pin_in!("string", Type::String);
            pin_out!("string", Type::String);
        }
        Rule::Replace => {
            pin_in!("string", Type::String);
            pin_in!("search", Type::String);
            pin_in!("replace", Type::String);
            pin_out!("result", Type::String);
        }
        Rule::FormatNumber => {
            pin_in!("format", Type::String);
            pin_in!("value", Type::dbl());
            pin_out!("string", Type::String);
        }
        Rule::ScanNumber => {
            pin_in!("string", Type::String);
            pin_out!("value", Type::dbl());
            pin_out!("success", Type::Bool);
        }
        Rule::BuildArray => {
            let mut ports: Vec<&String> = inputs.keys().collect();
            sort_ports(&mut ports);
            let mut elem: Option<Type> = None;
            for p in &ports {
                let t = &inputs[*p];
                if matches!(t, Type::Void) {
                    continue;
                }
                elem = Some(match elem {
                    None => t.clone(),
                    Some(prev) => unify(&prev, t)?,
                });
            }
            let elem = elem.ok_or_else(|| "Build Array has no connected inputs".to_string())?;
            for p in &ports {
                pin_in!(p, unify(&elem, &inputs[*p])?);
            }
            let dims = elem.dims() + 1;
            let base = match elem {
                Type::Array { element, .. } => *element,
                other => other,
            };
            pin_out!(
                "array",
                Type::Array {
                    element: Box::new(base),
                    dims,
                },
            );
        }
        Rule::IndexArray => {
            let arr = inputs
                .get("array")
                .ok_or_else(|| "Index Array requires an array input".to_string())?;
            let (element, dims) = match arr {
                Type::Array { element, dims } => ((**element).clone(), *dims),
                other => (other.clone(), 0),
            };
            pin_in!("array", arr.clone());
            pin_in!("index", Type::i32());
            let out = if dims <= 1 {
                element
            } else {
                Type::Array {
                    element: Box::new(element),
                    dims: dims - 1,
                }
            };
            pin_out!("element", out);
        }
        Rule::ArraySize => {
            let arr = inputs
                .get("array")
                .ok_or_else(|| "Array Size requires an array input".to_string())?;
            pin_in!("array", arr.clone());
            pin_out!("size", Type::i32());
        }
        Rule::ArraySubset => {
            let arr = inputs
                .get("array")
                .ok_or_else(|| "Array Subset requires an array input".to_string())?;
            pin_in!("array", arr.clone());
            pin_in!("index", Type::i32());
            pin_in!("length", Type::i32());
            pin_out!("subset", arr.clone());
        }
        Rule::ArraySort | Rule::ArrayReverse => {
            let arr = inputs
                .get("array")
                .ok_or_else(|| "node requires an array input".to_string())?;
            pin_in!("array", arr.clone());
            pin_out!(def.outputs[0].name, arr.clone());
        }
        Rule::ArraySum | Rule::ArrayMean => {
            let arr = inputs
                .get("array")
                .ok_or_else(|| "node requires a numeric array".to_string())?;
            let elem = arr
                .element()
                .ok_or_else(|| "input must be an array".to_string())?;
            let n = elem
                .as_numeric()
                .ok_or_else(|| "array elements must be numeric".to_string())?;
            pin_in!("array", arr.clone());
            pin_out!(
                def.outputs[0].name,
                if def.rule == Rule::ArrayMean {
                    Type::dbl()
                } else {
                    Type::numeric(n)
                },
            );
        }
        Rule::ArraySearch => {
            let arr = inputs
                .get("array")
                .ok_or_else(|| "Search 1D Array requires an array input".to_string())?;
            let elem = arr
                .element()
                .ok_or_else(|| "input must be an array".to_string())?
                .clone();
            pin_in!("array", arr.clone());
            pin_in!("value", elem);
            pin_out!("index", Type::i32());
        }
        Rule::Bundle => {
            let names = prop_string_list(node, "fields");
            let mut fields = Vec::new();
            for (i, p) in def.inputs.iter().enumerate() {
                if let Some(t) = inputs.get(p.name) {
                    let name = names.get(i).cloned().unwrap_or_else(|| p.name.to_string());
                    fields.push(lc_model::ClusterField {
                        name,
                        ty: t.clone(),
                    });
                    pin_in!(p.name, t.clone());
                }
            }
            pin_out!("cluster", Type::Cluster { fields });
        }
        Rule::Unbundle => {
            let cluster = inputs
                .get("cluster")
                .ok_or_else(|| "Unbundle requires a cluster input".to_string())?;
            let fields = match cluster {
                Type::Cluster { fields } => fields.clone(),
                other => {
                    return Err(format!(
                        "Unbundle expects a cluster, got {}",
                        other.display()
                    ))
                }
            };
            pin_in!("cluster", cluster.clone());
            for (i, p) in def.outputs.iter().enumerate() {
                if let Some(f) = fields.get(i) {
                    pin_out!(p.name, f.ty.clone());
                    r.cluster_fields.insert(p.name.to_string(), i);
                }
            }
        }
        Rule::UnbundleByName => {
            let cluster = inputs
                .get("cluster")
                .ok_or_else(|| "Unbundle By Name requires a cluster input".to_string())?;
            let fields = match cluster {
                Type::Cluster { fields } => fields.clone(),
                other => {
                    return Err(format!(
                        "Unbundle By Name expects a cluster, got {}",
                        other.display()
                    ))
                }
            };
            pin_in!("cluster", cluster.clone());
            let names = prop_string_list(node, "fields");
            if names.is_empty() {
                return Err("Unbundle By Name needs props.fields".into());
            }
            for name in names {
                let idx = fields
                    .iter()
                    .position(|f| f.name == name)
                    .ok_or_else(|| format!("cluster has no field `{name}`"))?;
                pin_out!(&fields[idx].name, fields[idx].ty.clone());
                r.cluster_fields.insert(fields[idx].name.clone(), idx);
            }
        }
        Rule::ErrorStatus => {
            pin_in!("error", Type::Error);
            pin_out!("status", Type::Bool);
        }
        Rule::ErrorClear => {
            pin_in!("error", Type::Error);
            pin_out!("error", Type::Error);
        }
        Rule::Wait => {
            pin_in!("milliseconds", Type::i32());
        }
        Rule::Tick => {
            pin_out!("milliseconds", Type::i32());
        }
        Rule::Random => {
            pin_out!("value", Type::dbl());
        }
        Rule::FileRead => {
            pin_in!("path", Type::String);
            pin_out!("text", Type::String);
            pin_out!("error", Type::Error);
        }
        Rule::FileWrite => {
            pin_in!("path", Type::String);
            pin_in!("text", Type::String);
            pin_out!("success", Type::Bool);
            pin_out!("error", Type::Error);
        }
        Rule::JsonParse => {
            pin_in!("text", Type::String);
            pin_out!("value", Type::Variant);
            pin_out!("error", Type::Error);
        }
        Rule::JsonStringify => {
            pin_in!(
                "value",
                inputs.get("value").cloned().unwrap_or(Type::Variant)
            );
            pin_out!("text", Type::String);
        }
        Rule::Identity => {}
    }

    // Any declared pin that was not given a type falls back to a concrete type.
    for p in def.inputs.iter().filter(|_p| !def.variadic) {
        if !r.inputs.contains_key(p.name) {
            r.inputs.insert(p.name.to_string(), pin_fallback(&p.ty));
        }
    }
    for p in &def.outputs {
        if !r.outputs.contains_key(p.name) {
            r.outputs.insert(p.name.to_string(), pin_fallback(&p.ty));
        }
    }

    Ok(r)
}

fn pin_fallback(p: &PinType) -> Type {
    match p {
        PinType::Bool => Type::Bool,
        PinType::Numeric => Type::dbl(),
        PinType::Int => Type::i32(),
        PinType::String | PinType::Path => Type::String,
        PinType::Error => Type::Error,
        PinType::Variant => Type::Variant,
        PinType::Any => Type::Void,
        PinType::Cluster => Type::Cluster { fields: vec![] },
        PinType::NumericArray => Type::array_of(Type::dbl()),
        PinType::Array(inner) => Type::array_of(pin_fallback(inner)),
    }
}

fn numeric_of(t: Option<&Type>) -> Result<(Type, bool), String> {
    let t = t.ok_or_else(|| "missing numeric input".to_string())?;
    Ok(match t {
        Type::Numeric { .. } => (t.clone(), false),
        Type::Array { element, dims } => {
            let n = element
                .as_numeric()
                .ok_or_else(|| "array elements must be numeric".to_string())?;
            (
                Type::Array {
                    element: Box::new(Type::numeric(n)),
                    dims: *dims,
                },
                true,
            )
        }
        Type::Enum { .. } => (Type::i32(), false),
        other => return Err(format!("expected numeric input, got {}", other.display())),
    })
}

fn num_kind(t: &Type) -> NumericType {
    match t {
        Type::Numeric { numeric } => *numeric,
        Type::Array { element, .. } => element.as_numeric().unwrap_or(NumericType::Dbl),
        _ => NumericType::Dbl,
    }
}

/// Compute per-input coerced types and the output type for a binary numeric op.
fn numeric_pair(x: Option<&Type>, y: Option<&Type>) -> Result<(Type, Type, Type), String> {
    let (xt, xa) = numeric_of(x)?;
    let (yt, ya) = numeric_of(y)?;
    let promoted = Type::numeric(Type::promote_numeric(num_kind(&xt), num_kind(&yt)));
    if xa && ya && xt.dims() != yt.dims() {
        return Err(format!(
            "array rank mismatch: {} vs {}",
            xt.display(),
            yt.display()
        ));
    }
    let xt_target = if xa {
        Type::Array {
            element: Box::new(promoted.clone()),
            dims: xt.dims(),
        }
    } else {
        promoted.clone()
    };
    let yt_target = if ya {
        Type::Array {
            element: Box::new(promoted.clone()),
            dims: yt.dims(),
        }
    } else {
        promoted.clone()
    };
    let out = if xa || ya {
        Type::Array {
            element: Box::new(promoted),
            dims: if xa { xt.dims() } else { yt.dims() },
        }
    } else {
        promoted
    };
    Ok((xt_target, yt_target, out))
}

fn unify(a: &Type, b: &Type) -> Result<Type, String> {
    if a == b {
        return Ok(a.clone());
    }
    match (a, b) {
        (Type::Numeric { numeric: x }, Type::Numeric { numeric: y }) => {
            Ok(Type::numeric(Type::promote_numeric(*x, *y)))
        }
        (
            Type::Array {
                element: ea,
                dims: da,
            },
            Type::Array {
                element: eb,
                dims: db,
            },
        ) if da == db => Ok(Type::Array {
            element: Box::new(unify(ea, eb)?),
            dims: *da,
        }),
        (Type::Void, t) | (t, Type::Void) => Ok(t.clone()),
        (Type::Any, t) | (t, Type::Any) => Ok(t.clone()),
        _ => Err(format!(
            "cannot combine {} with {}",
            a.display(),
            b.display()
        )),
    }
}

pub fn sort_ports(ports: &mut Vec<&String>) {
    ports.sort_by(|a, b| {
        let ai = port_index(a);
        let bi = port_index(b);
        ai.cmp(&bi).then(a.cmp(b))
    });
}

fn port_index(port: &str) -> u64 {
    let digits: String = port
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return u64::MAX;
    }
    let n: String = digits.chars().rev().collect();
    n.parse().unwrap_or(u64::MAX)
}

pub fn prop_string_list(node: &Node, key: &str) -> Vec<String> {
    node.props
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Helper used by the runtime to identify structure nodes and their frames.
pub fn is_terminal(kind: &str) -> bool {
    matches!(kind, kinds::CONTROL | kinds::INDICATOR | kinds::CONSTANT)
}

/// Edge sources for subVI-style nodes are not implemented in v1; the builder
/// emits `subvi` nodes only for documentation graphs.
pub fn subvi_placeholder() -> SourceRef {
    SourceRef::Default { ty: Type::Void }
}
