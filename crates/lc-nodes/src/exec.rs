use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use lc_model::document::Node;
use lc_model::{ArrayValue, ClusterValue, ErrorCluster, NumericValue, Value};

use crate::{NodeDef, Op, Resolved};

pub type ExecError = String;

/// Execution context shared by node implementations. It deliberately has no
/// dependency on the runtime or the UI so plugin nodes can use the same trait.
pub struct ExecCtx {
    /// Current front-panel control values, keyed by control key.
    pub panel: BTreeMap<String, Value>,
    /// Values written to indicators during this run.
    pub indicators: BTreeMap<String, Value>,
    pub logs: Vec<String>,
    pub messages: Vec<String>,
    pub file_root: Option<PathBuf>,
    pub allow_file: bool,
    pub max_wait_ms: u64,
    pub started: Instant,
    rng: u64,
}

impl Default for ExecCtx {
    fn default() -> Self {
        Self {
            panel: BTreeMap::new(),
            indicators: BTreeMap::new(),
            logs: Vec::new(),
            messages: Vec::new(),
            file_root: None,
            allow_file: true,
            max_wait_ms: 10_000,
            started: Instant::now(),
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl ExecCtx {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn control(&self, key: &str) -> Value {
        self.panel.get(key).cloned().unwrap_or(Value::Void)
    }

    fn next_random(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn execute(
    def: &NodeDef,
    node: &Node,
    resolved: &Resolved,
    inputs: &BTreeMap<String, Value>,
    ctx: &mut ExecCtx,
) -> Result<BTreeMap<String, Value>, ExecError> {
    let mut out = BTreeMap::new();
    macro_rules! put {
        ($k:expr, $v:expr $(,)?) => {{
            out.insert($k.to_string(), $v);
        }};
    }
    let first = || def.outputs.first().map(|p| p.name).unwrap_or("result");

    match def.op {
        Op::Add => put!(
            first().to_string(),
            binary_numeric(&arg(inputs, "x"), &arg(inputs, "y"), |a, b| a + b)?,
        ),
        Op::Sub => put!(
            first().to_string(),
            binary_numeric(&arg(inputs, "x"), &arg(inputs, "y"), |a, b| a - b)?,
        ),
        Op::Mul => put!(
            first().to_string(),
            binary_numeric(&arg(inputs, "x"), &arg(inputs, "y"), |a, b| a * b)?,
        ),
        Op::Div => put!(
            first().to_string(),
            binary_numeric(&arg(inputs, "x"), &arg(inputs, "y"), |a, b| {
                if b == 0.0 {
                    f64::NAN
                } else {
                    a / b
                }
            })?,
        ),
        Op::Pow => put!(
            first().to_string(),
            binary_numeric(&arg(inputs, "x"), &arg(inputs, "y"), |a, b| a.powf(b))?,
        ),
        Op::Min => {
            let x = arg(inputs, "x");
            let y = arg(inputs, "y");
            put!("min", binary_numeric(&x, &y, f64::min)?);
            put!("max", binary_numeric(&x, &y, f64::max)?);
        }
        Op::Quotient => {
            let x = arg(inputs, "x").as_i64();
            let y = arg(inputs, "y").as_i64();
            if y == 0 {
                return Err("integer division by zero".into());
            }
            put!("quotient", Value::int(x / y));
            put!("remainder", Value::int(x % y));
        }
        Op::Negate => put!(
            first().to_string(),
            unary_numeric(&arg(inputs, "x"), |v| -v)?
        ),
        Op::Abs => put!(
            first().to_string(),
            unary_numeric(&arg(inputs, "x"), f64::abs)?
        ),
        Op::Increment => put!(
            first().to_string(),
            unary_numeric(&arg(inputs, "x"), |v| v + 1.0)?
        ),
        Op::Decrement => put!(
            first().to_string(),
            unary_numeric(&arg(inputs, "x"), |v| v - 1.0)?
        ),
        Op::Round => put!(
            first().to_string(),
            unary_numeric(&arg(inputs, "x"), f64::round)?
        ),
        Op::Sqrt => put!(
            first().to_string(),
            unary_numeric(&arg(inputs, "x"), f64::sqrt)?
        ),
        Op::Eq => put!(
            first().to_string(),
            compare(
                &arg(inputs, "x"),
                &arg(inputs, "y"),
                |a, b| a == b,
                |a, b| a == b
            )?
        ),
        Op::Ne => put!(
            first().to_string(),
            compare(
                &arg(inputs, "x"),
                &arg(inputs, "y"),
                |a, b| a != b,
                |a, b| a != b
            )?
        ),
        Op::Gt => put!(
            first().to_string(),
            compare(
                &arg(inputs, "x"),
                &arg(inputs, "y"),
                |a, b| a > b,
                |a, b| a > b
            )?
        ),
        Op::Lt => put!(
            first().to_string(),
            compare(
                &arg(inputs, "x"),
                &arg(inputs, "y"),
                |a, b| a < b,
                |a, b| a < b
            )?
        ),
        Op::Gte => put!(
            first().to_string(),
            compare(
                &arg(inputs, "x"),
                &arg(inputs, "y"),
                |a, b| a >= b,
                |a, b| a >= b
            )?
        ),
        Op::Lte => put!(
            first().to_string(),
            compare(
                &arg(inputs, "x"),
                &arg(inputs, "y"),
                |a, b| a <= b,
                |a, b| a <= b
            )?
        ),
        Op::And => put!(
            first().to_string(),
            Value::Bool(arg(inputs, "x").as_bool() && arg(inputs, "y").as_bool())
        ),
        Op::Or => put!(
            first().to_string(),
            Value::Bool(arg(inputs, "x").as_bool() || arg(inputs, "y").as_bool())
        ),
        Op::Xor => put!(
            first().to_string(),
            Value::Bool(arg(inputs, "x").as_bool() ^ arg(inputs, "y").as_bool())
        ),
        Op::Not => put!(
            first().to_string(),
            Value::Bool(!arg(inputs, "x").as_bool())
        ),
        Op::Select => {
            let picked = if arg(inputs, "selector").as_bool() {
                arg(inputs, "true")
            } else {
                arg(inputs, "false")
            };
            put!(first().to_string(), picked);
        }
        Op::ToString => put!(
            first().to_string(),
            Value::string(arg(inputs, "value").display())
        ),
        Op::ToDbl => put!(
            first().to_string(),
            Value::num(arg(inputs, "value").as_f64())
        ),
        Op::ToI32 => put!(
            first().to_string(),
            Value::int(arg(inputs, "value").as_i64())
        ),
        Op::ToBool => put!(
            first().to_string(),
            Value::Bool(arg(inputs, "value").as_bool())
        ),
        Op::StringLength => put!(
            first().to_string(),
            Value::int(arg(inputs, "string").as_str().len() as i64),
        ),
        Op::Concat => {
            let mut ports: Vec<&String> = inputs.keys().collect();
            crate::sort_ports(&mut ports);
            let s = ports
                .iter()
                .map(|p| arg(inputs, p).as_str())
                .collect::<String>();
            put!(first().to_string(), Value::string(s));
        }
        Op::Substring => {
            let s = arg(inputs, "string").as_str();
            let offset = arg(inputs, "offset").as_i64().max(0) as usize;
            let len = arg(inputs, "length").as_i64().max(0) as usize;
            let bytes = s.as_bytes();
            let end = offset.saturating_add(len).min(bytes.len());
            let slice = if offset >= bytes.len() {
                ""
            } else {
                std::str::from_utf8(&bytes[offset..end]).unwrap_or("")
            };
            put!(first().to_string(), Value::string(slice));
        }
        Op::Upper => put!(
            first().to_string(),
            Value::string(arg(inputs, "string").as_str().to_uppercase())
        ),
        Op::Lower => put!(
            first().to_string(),
            Value::string(arg(inputs, "string").as_str().to_lowercase())
        ),
        Op::Trim => put!(
            first().to_string(),
            Value::string(arg(inputs, "string").as_str().trim())
        ),
        Op::Replace => {
            let s = arg(inputs, "string").as_str();
            let search = arg(inputs, "search").as_str();
            let replace = arg(inputs, "replace").as_str();
            let result = if search.is_empty() {
                s
            } else {
                s.replace(&search, &replace)
            };
            put!(first().to_string(), Value::string(result));
        }
        Op::FormatNumber => {
            let fmt = arg(inputs, "format").as_str();
            let v = arg(inputs, "value");
            put!(first().to_string(), Value::string(format_value(&fmt, &v)));
        }
        Op::ScanNumber => {
            let s = arg(inputs, "string").as_str();
            let token: String = s
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
                .collect();
            match token.parse::<f64>() {
                Ok(v) => {
                    put!("value", Value::num(v));
                    put!("success", Value::Bool(true));
                }
                Err(_) => {
                    put!("value", Value::num(0.0));
                    put!("success", Value::Bool(false));
                }
            }
        }
        Op::BuildArray => {
            let mut ports: Vec<&String> = inputs.keys().collect();
            crate::sort_ports(&mut ports);
            if ports.is_empty() {
                return Err("Build Array has no inputs".into());
            }
            let values: Vec<Value> = ports.iter().map(|p| arg(inputs, p)).collect();
            let all_arrays = values.iter().all(|v| matches!(v, Value::Array(_)));
            if all_arrays {
                let mut dims = vec![values.len()];
                let mut data = Vec::new();
                let mut inner: Option<Vec<usize>> = None;
                for v in &values {
                    if let Value::Array(a) = v {
                        if let Some(d) = &inner {
                            if d != &a.dims {
                                return Err("Build Array requires arrays of equal shape".into());
                            }
                        } else {
                            inner = Some(a.dims.clone());
                        }
                        data.extend(a.data.iter().cloned());
                    }
                }
                dims.extend(inner.unwrap_or_default());
                put!(
                    first().to_string(),
                    Value::Array(ArrayValue::new(dims, data))
                );
            } else {
                let dims = vec![values.len()];
                put!(
                    first().to_string(),
                    Value::Array(ArrayValue::new(dims, values))
                );
            }
        }
        Op::IndexArray => {
            let v = arg(inputs, "array");
            let index = arg(inputs, "index").as_i64().max(0) as usize;
            match v {
                Value::Array(a) => {
                    if a.dims.is_empty() {
                        return Err("cannot index a zero-dimensional array".into());
                    }
                    if a.dims.len() == 1 {
                        let item = a.data.get(index).cloned().ok_or_else(|| {
                            format!("index {index} is out of range (size {})", a.data.len())
                        })?;
                        put!(first().to_string(), item);
                    } else {
                        let stride: usize = a.dims[1..].iter().product();
                        let start = index
                            .checked_mul(stride)
                            .ok_or_else(|| "index overflow".to_string())?;
                        if start >= a.data.len() {
                            return Err(format!("index {index} is out of range"));
                        }
                        let end = (start + stride).min(a.data.len());
                        put!(
                            first().to_string(),
                            Value::Array(ArrayValue::new(
                                a.dims[1..].to_vec(),
                                a.data[start..end].to_vec()
                            )),
                        );
                    }
                }
                other => {
                    return Err(format!(
                        "Index Array expects an array, got {}",
                        other.type_of().display()
                    ))
                }
            }
        }
        Op::ArraySize => {
            let n = arg(inputs, "array")
                .as_array()
                .map(|a| a.data.len())
                .unwrap_or(0);
            put!(first().to_string(), Value::int(n as i64));
        }
        Op::ArraySubset => {
            let v = arg(inputs, "array");
            let index = arg(inputs, "index").as_i64().max(0) as usize;
            let length = arg(inputs, "length").as_i64().max(0) as usize;
            match v {
                Value::Array(a) => {
                    if a.dims.is_empty() {
                        return Err("cannot subset a zero-dimensional array".into());
                    }
                    if a.dims.len() == 1 {
                        let end = (index + length).min(a.data.len());
                        let data = if index < a.data.len() {
                            a.data[index..end].to_vec()
                        } else {
                            Vec::new()
                        };
                        put!(
                            first().to_string(),
                            Value::Array(ArrayValue::new(vec![data.len()], data)),
                        );
                    } else {
                        let stride: usize = a.dims[1..].iter().product();
                        let start = index * stride;
                        let mut data = Vec::new();
                        for i in 0..length {
                            let s = start + i * stride;
                            let e = (s + stride).min(a.data.len());
                            if s < e {
                                data.extend(a.data[s..e].iter().cloned());
                            }
                        }
                        let mut dims = a.dims.clone();
                        dims[0] = length;
                        put!(
                            first().to_string(),
                            Value::Array(ArrayValue::new(dims, data))
                        );
                    }
                }
                other => {
                    return Err(format!(
                        "Array Subset expects an array, got {}",
                        other.type_of().display()
                    ))
                }
            }
        }
        Op::ArraySort | Op::ArrayReverse => {
            let v = arg(inputs, "array");
            match v {
                Value::Array(mut a) => {
                    if def.op == Op::ArraySort {
                        a.data.sort_by(compare_values);
                    } else {
                        a.data.reverse();
                    }
                    put!(first().to_string(), Value::Array(a));
                }
                other => {
                    return Err(format!(
                        "node expects an array, got {}",
                        other.type_of().display()
                    ))
                }
            }
        }
        Op::ArraySum | Op::ArrayMean => {
            let a = arg(inputs, "array")
                .as_array()
                .cloned()
                .ok_or_else(|| "node expects a numeric array".to_string())?;
            let sum: f64 = a.data.iter().map(|v| v.as_f64()).sum();
            let value = if def.op == Op::ArrayMean {
                if a.data.is_empty() {
                    0.0
                } else {
                    sum / a.data.len() as f64
                }
            } else {
                sum
            };
            put!(first().to_string(), Value::num(value));
        }
        Op::ArraySearch => {
            let a = arg(inputs, "array")
                .as_array()
                .cloned()
                .ok_or_else(|| "Search 1D Array expects an array".to_string())?;
            let needle = arg(inputs, "value");
            let idx = a
                .data
                .iter()
                .position(|v| values_equal(v, &needle))
                .map(|i| i as i64)
                .unwrap_or(-1);
            put!(first().to_string(), Value::int(idx));
        }
        Op::Bundle => {
            let mut fields = Vec::new();
            for p in &def.inputs {
                if let Some(v) = inputs.get(p.name) {
                    fields.push(v.clone());
                }
            }
            if fields.is_empty() {
                return Err("Bundle has no connected fields".into());
            }
            put!(
                first().to_string(),
                Value::Cluster(ClusterValue::new(fields))
            );
        }
        Op::Unbundle | Op::UnbundleByName => {
            let c = arg(inputs, "cluster");
            let fields = match &c {
                Value::Cluster(c) => c.fields.clone(),
                other => {
                    return Err(format!(
                        "Unbundle expects a cluster, got {}",
                        other.type_of().display()
                    ))
                }
            };
            for (port, idx) in &resolved.cluster_fields {
                if let Some(v) = fields.get(*idx) {
                    put!(port.clone(), v.clone());
                }
            }
        }
        Op::ErrorStatus => {
            let status = matches!(arg(inputs, "error"), Value::Error(e) if e.status);
            put!(first().to_string(), Value::Bool(status));
        }
        Op::ErrorClear => {
            let mut e = match arg(inputs, "error") {
                Value::Error(e) => e,
                _ => ErrorCluster::ok(),
            };
            e.status = false;
            e.code = 0;
            e.source.clear();
            put!(first().to_string(), Value::Error(e));
        }
        Op::Wait => {
            let ms = arg(inputs, "milliseconds").as_i64().max(0) as u64;
            let ms = ms.min(ctx.max_wait_ms);
            if ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
        }
        Op::Tick => {
            let ms = ctx.started.elapsed().as_millis() as i64;
            put!(first().to_string(), Value::int(ms));
        }
        Op::Random => {
            let v = ctx.next_random();
            put!(first().to_string(), Value::num(v));
        }
        Op::Log => {
            ctx.logs.push(arg(inputs, "value").display());
        }
        Op::Message => {
            ctx.messages.push(arg(inputs, "message").as_str());
        }
        Op::FileRead => {
            let path = arg(inputs, "path").as_str();
            match read_file(ctx, &path) {
                Ok(text) => {
                    put!("text", Value::string(text));
                    put!("error", Value::Error(ErrorCluster::ok()));
                }
                Err(e) => {
                    put!("text", Value::string(""));
                    put!("error", Value::Error(ErrorCluster::err(1, e)));
                }
            }
        }
        Op::FileWrite | Op::FileAppend => {
            let path = arg(inputs, "path").as_str();
            let text = arg(inputs, "text").as_str();
            let append = def.op == Op::FileAppend;
            match write_file(ctx, &path, &text, append) {
                Ok(()) => {
                    put!("success", Value::Bool(true));
                    put!("error", Value::Error(ErrorCluster::ok()));
                }
                Err(e) => {
                    put!("success", Value::Bool(false));
                    put!("error", Value::Error(ErrorCluster::err(2, e)));
                }
            }
        }
        Op::JsonParse => {
            let text = arg(inputs, "text").as_str();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(j) => {
                    put!("value", json_to_value(j));
                    put!("error", Value::Error(ErrorCluster::ok()));
                }
                Err(e) => {
                    put!("value", Value::Void);
                    put!("error", Value::Error(ErrorCluster::err(3, e.to_string())));
                }
            }
        }
        Op::JsonStringify => {
            let v = arg(inputs, "value");
            let text = serde_json::to_string(&v).unwrap_or_else(|_| "null".into());
            put!(first().to_string(), Value::string(text));
        }
        Op::Constant => {
            let v = node
                .props
                .get("value")
                .and_then(|j| serde_json::from_value::<Value>(j.clone()).ok())
                .or_else(|| {
                    node.props
                        .get("type")
                        .and_then(|j| serde_json::from_value::<lc_model::Type>(j.clone()).ok())
                        .map(|t| Value::default_for(&t))
                })
                .unwrap_or(Value::Void);
            put!(first().to_string(), v);
        }
        Op::Control => {
            let key = node
                .control_key()
                .ok_or_else(|| "control terminal has no control key".to_string())?;
            put!(first().to_string(), ctx.control(key));
        }
        Op::Indicator => {
            let key = node
                .control_key()
                .ok_or_else(|| "indicator has no control key".to_string())?;
            let v = arg(inputs, "value");
            ctx.indicators.insert(key.to_string(), v);
        }
        Op::Noop => {}
    }

    Ok(out)
}

fn arg(inputs: &BTreeMap<String, Value>, name: &str) -> Value {
    inputs.get(name).cloned().unwrap_or(Value::Void)
}

fn unary_numeric(v: &Value, f: impl Fn(f64) -> f64 + Copy) -> Result<Value, ExecError> {
    match v {
        Value::Array(a) => {
            let data = a.data.iter().map(|x| Value::num(f(x.as_f64()))).collect();
            Ok(Value::Array(ArrayValue::new(a.dims.clone(), data)))
        }
        other => Ok(Value::num(f(other.as_f64()))),
    }
}

fn binary_numeric(
    x: &Value,
    y: &Value,
    f: impl Fn(f64, f64) -> f64 + Copy,
) -> Result<Value, ExecError> {
    match (x, y) {
        (Value::Array(a), Value::Array(b)) => {
            if a.dims != b.dims || a.data.len() != b.data.len() {
                return Err("array shapes must match".into());
            }
            let data = a
                .data
                .iter()
                .zip(&b.data)
                .map(|(p, q)| Value::num(f(p.as_f64(), q.as_f64())))
                .collect();
            Ok(Value::Array(ArrayValue::new(a.dims.clone(), data)))
        }
        (Value::Array(a), s) => {
            let data = a
                .data
                .iter()
                .map(|p| Value::num(f(p.as_f64(), s.as_f64())))
                .collect();
            Ok(Value::Array(ArrayValue::new(a.dims.clone(), data)))
        }
        (s, Value::Array(b)) => {
            let data = b
                .data
                .iter()
                .map(|q| Value::num(f(s.as_f64(), q.as_f64())))
                .collect();
            Ok(Value::Array(ArrayValue::new(b.dims.clone(), data)))
        }
        (p, q) => Ok(Value::num(f(p.as_f64(), q.as_f64()))),
    }
}

fn compare(
    x: &Value,
    y: &Value,
    num: impl Fn(f64, f64) -> bool + Copy,
    text: impl Fn(&str, &str) -> bool + Copy,
) -> Result<Value, ExecError> {
    match (x, y) {
        (Value::Array(a), Value::Array(b)) => {
            if a.dims != b.dims {
                return Err("array shapes must match".into());
            }
            let data = a
                .data
                .iter()
                .zip(&b.data)
                .map(|(p, q)| {
                    if let (Value::Str(a), Value::Str(b)) = (p, q) {
                        Value::Bool(text(a, b))
                    } else {
                        Value::Bool(num(p.as_f64(), q.as_f64()))
                    }
                })
                .collect();
            Ok(Value::Array(ArrayValue::new(a.dims.clone(), data)))
        }
        (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(text(a, b))),
        _ => Ok(Value::Bool(num(x.as_f64(), y.as_f64()))),
    }
}

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => a
            .as_f64()
            .partial_cmp(&b.as_f64())
            .unwrap_or(std::cmp::Ordering::Equal),
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => (a.as_f64() - b.as_f64()).abs() < f64::EPSILON,
    }
}

fn format_value(fmt: &str, v: &Value) -> String {
    if fmt.contains("{}") {
        return fmt.replace("{}", &v.display());
    }
    let Some(pos) = fmt.find('%') else {
        return v.display();
    };
    let mut end = pos + 1;
    let mut precision: Option<usize> = None;
    let mut spec = 'f';
    let bytes = fmt.as_bytes();
    while end < bytes.len() {
        let c = bytes[end] as char;
        end += 1;
        match c {
            '.' => {}
            d if d.is_ascii_digit() => precision = Some(d.to_digit(10).unwrap() as usize),
            'f' | 'F' | 'd' | 'i' | 's' | 'e' | 'E' | 'g' | 'G' => {
                spec = c.to_ascii_lowercase();
                break;
            }
            _ => {
                end -= 1;
                break;
            }
        }
    }
    let body = match spec {
        'd' | 'i' => format!("{}", v.as_i64()),
        's' => v.as_str(),
        'e' => format!("{:.*e}", precision.unwrap_or(6), v.as_f64()),
        'g' => format!("{}", v.as_f64()),
        _ => format!("{:.*}", precision.unwrap_or(6), v.as_f64()),
    };
    format!("{}{}{}", &fmt[..pos], body, &fmt[end..])
}

fn sandbox(ctx: &ExecCtx, raw: &str) -> Result<PathBuf, String> {
    if !ctx.allow_file {
        return Err("file access is disabled for this run".into());
    }
    if raw.trim().is_empty() {
        return Err("empty path".into());
    }
    let p = Path::new(raw);
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("paths may not contain `..`".into());
    }
    if p.is_absolute() {
        return Err("absolute paths are not allowed; use a project-relative path".into());
    }
    let root = ctx
        .file_root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    Ok(root.join(p))
}

fn read_file(ctx: &ExecCtx, path: &str) -> Result<String, String> {
    let p = sandbox(ctx, path)?;
    std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
}

fn write_file(ctx: &ExecCtx, path: &str, text: &str, append: bool) -> Result<(), String> {
    let p = sandbox(ctx, path)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&p)
        .map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

fn json_to_value(j: serde_json::Value) -> Value {
    match j {
        serde_json::Value::Null => Value::Void,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::int(i)
            } else if let Some(u) = n.as_u64() {
                Value::Num(NumericValue::UInt(u))
            } else {
                Value::num(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::string(s),
        serde_json::Value::Array(a) => Value::Array(ArrayValue::from_vec(
            a.into_iter().map(json_to_value).collect(),
        )),
        serde_json::Value::Object(map) => Value::Cluster(ClusterValue::new(
            map.into_iter().map(|(_, v)| json_to_value(v)).collect(),
        )),
    }
}
