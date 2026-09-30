//! `faber:toml` kernel module dispatch.
//!
//! Mirrors the `faber:json` module: `parse` (strict TOML → valor tree),
//! `try_parse` (lenient parse → `nihil` on failure), `stringify` (valor tree →
//! TOML text). The verb surface matches the `norma:toml` public facade
//! (`parse`/`stringify`/`try_parse`), enforced by the kernel-manifest parity test.

use super::super::value::MapKey;
use super::super::{RunnerError, Value};
use super::args::expect_str;
use radix_lexer::Span;
use radix_runtime_contract::instans::{Instans, InstansPraecisio};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn dispatch(verb: &str, args: &[Value], span: Span) -> Result<Value, RunnerError> {
    match verb {
        "parse" => {
            let [raw] = args else {
                return Err(RunnerError::internal(span, "toml.parse provider arity"));
            };
            let raw = expect_str(raw, span, "toml.parse input")?;
            parse_toml(&raw, span)
        }
        "stringify" => {
            let [valor] = args else {
                return Err(RunnerError::internal(span, "toml.stringify provider arity"));
            };
            serialize_toml(valor, span).map(Value::Str)
        }
        "try_parse" => {
            let [raw] = args else {
                return Err(RunnerError::internal(span, "toml.try_parse provider arity"));
            };
            let raw = expect_str(raw, span, "toml.try_parse input")?;
            Ok(parse_toml(&raw, span).unwrap_or(Value::Nil))
        }
        other => Err(RunnerError::unsupported(
            span,
            &format!("kernel toml verb `{other}`"),
        )),
    }
}

fn parse_toml(raw: &str, span: Span) -> Result<Value, RunnerError> {
    // WHY: `toml::Value::from_str` parses a single *value*, not a document —
    // table headers (`[owner]`) and arrays of tables (`[[bin]]`) are
    // document-level constructs. A document root is always a table.
    let table: toml::Table = toml::from_str(raw)
        .map_err(|err| RunnerError::internal(span, format!("toml.parse failed: {err}")))?;
    table_to_value(table, span)
}

fn table_to_value(table: toml::Table, span: Span) -> Result<Value, RunnerError> {
    let mut out = rustc_hash::FxHashMap::default();
    for (key, item) in table {
        out.insert(MapKey::Str(key), toml_to_value(item, span)?);
    }
    Ok(Value::Map(Rc::new(RefCell::new(out))))
}

fn serialize_toml(value: &Value, span: Span) -> Result<String, RunnerError> {
    let table = value_to_toml_table(value, span)?;
    toml::to_string(&table)
        .map_err(|err| RunnerError::internal(span, format!("toml.stringify failed: {err}")))
}

fn toml_to_value(value: toml::Value, span: Span) -> Result<Value, RunnerError> {
    match value {
        toml::Value::String(value) => Ok(Value::Str(value)),
        toml::Value::Integer(value) => Ok(Value::Int(value)),
        toml::Value::Float(value) => Ok(Value::Float(value)),
        toml::Value::Boolean(value) => Ok(Value::Bool(value)),
        toml::Value::Datetime(value) => datetime_to_instans(&value, span).map(Value::Instans),
        toml::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(toml_to_value(item, span)?);
            }
            Ok(Value::Array(Rc::new(RefCell::new(out))))
        }
        toml::Value::Table(items) => {
            let mut out = rustc_hash::FxHashMap::default();
            for (key, item) in items {
                out.insert(MapKey::Str(key), toml_to_value(item, span)?);
            }
            Ok(Value::Map(Rc::new(RefCell::new(out))))
        }
    }
}

/// Map a TOML datetime into an absolute [`Instans`].
///
/// WHY: the `faber:toml` wire contract (`norma/src/toml.fab`) lowers TOML
/// datetimes to `Valor::Instans` and never produces `Valor::Nihil`. TOML
/// permits offset-free local forms; an [`Instans`] is an absolute point in
/// time, so local date-times and local dates are read as UTC (`Z`), matching
/// how the runner re-emits them.
///
/// EDGE: a bare local time (no date component) cannot name an absolute
/// instant and fails closed — `solve` errors, `tempta` yields `nihil`.
fn datetime_to_instans(
    datetime: &toml::value::Datetime,
    span: Span,
) -> Result<Instans, RunnerError> {
    let rendered = datetime.to_string();
    let with_zone = if rendered_has_zone(&rendered) {
        rendered.clone()
    } else if rendered.len() == 10 {
        // Local date (`1979-05-27`): midnight UTC.
        format!("{rendered}T00:00:00Z")
    } else {
        // Local date-time (`1979-05-27T07:32:00`) or bare local time.
        format!("{rendered}Z")
    };
    let praecisio = instans_precision(datetime.time.and_then(|time| time.nanosecond));
    Instans::from_rfc3339(&with_zone, praecisio).ok_or_else(|| {
        RunnerError::internal(
            span,
            format!("toml datetime cannot be an absolute instans: {rendered}"),
        )
    })
}

/// Coarsest [`InstansPraecisio`] that exactly represents a TOML fractional
/// second — the TOML value's own significant-figure precision, so `pange`
/// re-emits without fabricated trailing zeros and `solve` never truncates.
fn instans_precision(nanosecond: Option<u32>) -> InstansPraecisio {
    match nanosecond {
        None | Some(0) => InstansPraecisio::Secunda,
        Some(ns) if ns % 1_000_000 == 0 => InstansPraecisio::Millisecunda,
        Some(ns) if ns % 1_000 == 0 => InstansPraecisio::Microsecunda,
        Some(_) => InstansPraecisio::Nanosecunda,
    }
}

/// Whether a rendered TOML datetime already carries an explicit zone offset
/// (`Z`/`z` or `±HH:MM`) past its date component.
fn rendered_has_zone(rendered: &str) -> bool {
    rendered
        .bytes()
        .skip(8)
        .any(|byte| matches!(byte, b'Z' | b'z' | b'+' | b'-'))
}

fn value_to_toml_table(value: &Value, span: Span) -> Result<toml::Table, RunnerError> {
    match value {
        Value::Map(items) => {
            let items = items.borrow();
            let mut keys = items.keys().collect::<Vec<_>>();
            keys.sort_by_key(|key| format_map_key(key));
            let mut table = toml::Table::new();
            #[allow(clippy::iter_without_into_iter)]
            for key in keys {
                let MapKey::Str(key_text) = key else {
                    return Err(RunnerError::internal(span, "map key is not textus toml"));
                };
                let item = items
                    .get(key)
                    .ok_or_else(|| RunnerError::internal(span, "toml map key lookup failed"))?;
                table.insert(key_text.clone(), value_to_toml(item, span)?);
            }
            Ok(table)
        }
        Value::Record(fields, _) | Value::Struct(_, fields) => {
            let fields = fields.borrow();
            let mut keys = fields.keys().copied().collect::<Vec<_>>();
            keys.sort_by_key(|key| key.0);
            let mut table = toml::Table::new();
            #[allow(clippy::iter_without_into_iter)]
            for key in keys {
                let item = fields
                    .get(&key)
                    .ok_or_else(|| RunnerError::internal(span, "toml record key lookup failed"))?;
                table.insert(format!("s{}", key.0), value_to_toml(item, span)?);
            }
            Ok(table)
        }
        _ => Err(RunnerError::internal(
            span,
            "toml.stringify root must be a map",
        )),
    }
}

fn value_to_toml(value: &Value, span: Span) -> Result<toml::Value, RunnerError> {
    match value {
        Value::Bool(value) => Ok(toml::Value::Boolean(*value)),
        Value::Int(value) => Ok(toml::Value::Integer(*value)),
        Value::UInt(value) => i64::try_from(*value)
            .map(toml::Value::Integer)
            .map_err(|_| {
                RunnerError::internal(span, format!("uint outside toml i64 range: {value}"))
            }),
        // TOML integers are 64-bit: an `inf` beyond them has no TOML form.
        Value::Magnus(value) => Err(RunnerError::internal(
            span,
            format!("inf outside toml i64 range: {value}"),
        )),
        Value::Dec(..) => Err(RunnerError::internal(
            span,
            "decimal intermediate reached TOML before a store",
        )),
        Value::Float(value) => Ok(toml::Value::Float(*value)),
        Value::Str(value) => Ok(toml::Value::String(value.clone())),
        Value::Ascii(b) => Ok(toml::Value::String(char::from(*b).to_string())),
        Value::Char(ch) => Ok(toml::Value::String(ch.to_string())),
        Value::Instans(value) => {
            let datetime = value
                .to_rfc3339()
                .parse::<toml::value::Datetime>()
                .map_err(|err| {
                    RunnerError::internal(span, format!("instans is not a toml datetime: {err}"))
                })?;
            Ok(toml::Value::Datetime(datetime))
        }
        Value::Octeti(bytes) => {
            let items = bytes
                .iter()
                .map(|byte| toml::Value::Integer(i64::from(*byte)))
                .collect();
            Ok(toml::Value::Array(items))
        }
        Value::Array(items) => {
            let items = items.borrow();
            let mut out = Vec::with_capacity(items.len());
            for item in &*items {
                out.push(value_to_toml(item, span)?);
            }
            Ok(toml::Value::Array(out))
        }
        Value::Map(_) | Value::Record(..) | Value::Struct(_, _) => {
            Ok(toml::Value::Table(value_to_toml_table(value, span)?))
        }
        Value::Tensor(tensor) => {
            let tensor = tensor.borrow();
            let mut out = Vec::with_capacity(tensor.data.len());
            for item in &tensor.data {
                out.push(value_to_toml(item, span)?);
            }
            Ok(toml::Value::Array(out))
        }
        Value::Nil | Value::Unit => Err(RunnerError::internal(span, "toml has no nihil value")),
        Value::Function(_)
        | Value::Closure(_, _, _)
        | Value::ClosureEnvironment(_, _)
        | Value::SharedCell(_)
        | Value::Regex(_)
        | Value::Sermo(_)
        | Value::Set(_)
        | Value::EnumVariant(_, _)
        | Value::Tuple(_)
        | Value::Vector(_)
        | Value::Matrix(_)
        | Value::GradientHandle(_)
        | Value::Cursor(_) => Err(RunnerError::internal(
            span,
            "value variant is outside toml kernel",
        )),
    }
}

fn format_map_key(key: &MapKey) -> String {
    key.document_key()
}
