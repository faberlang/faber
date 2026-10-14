//! Renderers for the Radix ABI payload.
//!
//! Two formats are supported in this phase:
//! - JSON (`render_json`): canonical pretty JSON with a computed `fingerprint`.
//! - Rust (`render_rust`): a runtime-facing constants artifact.
//!
//! Determinism contract
//! --------------------
//! Both renderers are stable across runs for an unchanged payload. JSON keys
//! are sorted because `serde_json::Value` objects are `BTreeMap`-backed by
//! default; the Rust artifact emits sections in declaration order.

use crate::abi::{AbiPayload, fingerprint_hex};
use std::fmt::Write;

/// Render the payload as canonical pretty JSON, including a `fingerprint` field
/// computed over the payload (without the fingerprint).
///
/// # Errors
///
/// Returns `serde_json::Error` if the payload cannot be serialized to JSON or
/// the pretty-print formatting fails.
pub fn render_json(payload: &AbiPayload) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(payload)?;
    let canonical = serde_json::to_string(&value)?;
    let fingerprint = fingerprint_hex(canonical.as_bytes());
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert("fingerprint".to_owned(), serde_json::Value::String(fingerprint));
    }
    serde_json::to_string_pretty(&value)
}

/// Render the payload as a runtime-facing Rust constants artifact.
///
/// The artifact is a constants view of the Radix-owned contract. The runtime
/// (public `faber` crate) keeps its own C struct implementations; only symbol names,
/// enum codes, LLVM layout spellings, status codes, and diagnostic rows are
/// emitted here.
///
/// # Errors
///
/// Returns `serde_json::Error` if the payload fingerprint or rendering fails.
pub fn render_rust(payload: &AbiPayload) -> Result<String, serde_json::Error> {
    let fingerprint = payload.fingerprint()?;
    let mut out = String::new();
    out.push_str("//! Generated host ABI constants — emitted by `radix abi --format rust`.\n");
    out.push_str("//!\n");
    out.push_str("//! Authority: the Radix compiler binary (`radix abi`). Do not edit by hand.\n");
    writeln!(
        out,
        "//! Compiler: {} {}; ABI version: {}; surface: {}.",
        payload.compiler.name, payload.compiler.version, payload.abi_version, payload.surface,
    )
    .ok();
    writeln!(out, "//! Fingerprint: {fingerprint}.").ok();
    out.push_str("//!\n");
    out.push_str("//! This artifact is a constants view of the Radix-owned contract. The runtime\n");
    out.push_str("//! (public `faber` crate) keeps its own C struct implementations (FaberRtSliceV1,\n");
    out.push_str("//! FaberRtStatusV1, ...); only the symbol names, enum codes, layout spellings,\n");
    out.push_str("//! status codes, and diagnostic rows are generated here.\n\n");

    write!(out, "pub const ABI_VERSION: u32 = {};\n\n", payload.abi_version).ok();

    out.push_str("// --- Host ABI symbols ---\n");
    for row in &payload.symbols {
        writeln!(out, "pub const {}: &str = \"{}\";", row.name, row.value).ok();
    }
    out.push('\n');

    for family in &payload.enums {
        writeln!(out, "// --- {} ---", family.family).ok();
        writeln!(out, "pub type {} = {};", family.type_alias, family.repr).ok();
        for row in &family.rows {
            writeln!(out, "pub const {}: {} = {};", row.name, family.type_alias, row.value).ok();
        }
        out.push('\n');
    }

    out.push_str("// --- Status codes (runtime wraps these in FaberRtStatusV1) ---\n");
    for row in &payload.statuses {
        writeln!(out, "pub const {}: i32 = {};", row.name, row.value).ok();
    }
    out.push('\n');

    out.push_str("// --- Diagnostic symbols ---\n");
    out.push_str("pub const DIAGNOSTIC_SYMBOLS_V1: &[(&str, &str, &str)] = &[\n");
    for row in &payload.diagnostics {
        writeln!(out, "    (\"{}\", \"{}\", \"{}\"),", row.kind, row.carrier, row.symbol).ok();
    }
    out.push_str("];\n\n");
    out.push_str("pub fn diagnostic_symbol_v1(kind: &str, carrier: &str) -> Option<&'static str> {\n");
    out.push_str("    DIAGNOSTIC_SYMBOLS_V1\n");
    out.push_str("        .iter()\n");
    out.push_str("        .find(|(candidate_kind, candidate_carrier, _)| {\n");
    out.push_str("            *candidate_kind == kind && *candidate_carrier == carrier\n");
    out.push_str("        })\n");
    out.push_str("        .map(|(_, _, symbol)| *symbol)\n");
    out.push_str("}\n");

    Ok(out)
}
