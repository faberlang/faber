//! Radix-owned ABI contract payload and `radix abi` introspection entry.
//!
//! This module assembles the structured ABI payload from [`host_abi`] and
//! exposes the deterministic fingerprint and render dispatch used by the
//! `radix abi` command. The individual contract constants live in
//! [`host_abi`]; this module is the renderable view over them.
//!
//! Authority model
//! ---------------
//! The Radix binary is the ABI authority. `AbiPayload::current()` captures the
//! surface this compiler build emits; renderers turn it into JSON or a
//! runtime-facing Rust artifact. Radix never reads a generated runtime artifact
//! back as an input — drift is caught by dev-only coherence tests.

pub mod host_abi;
mod render;

pub use render::{render_json, render_rust};

use handroll::FxHasher;
use serde::Serialize;
use std::hash::{Hash, Hasher};

/// Compiler name reported by `radix abi`.
pub const COMPILER_NAME: &str = "radix";

/// Compiler version reported by `radix abi` (the Radix crate version).
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compiler and version metadata block.
#[derive(Serialize, Debug)]
pub struct CompilerMeta {
    /// Compiler binary name.
    pub name: &'static str,
    /// Compiler crate version (`CARGO_PKG_VERSION`).
    pub version: &'static str,
}

/// Named string constant row, e.g. a symbol or LLVM layout spelling.
#[derive(Serialize, Debug)]
pub struct NamedStr {
    pub name: &'static str,
    pub value: &'static str,
}

/// Named `u32` enum code row.
#[derive(Serialize, Debug)]
pub struct NamedU32 {
    pub name: &'static str,
    pub value: u32,
}

/// Named `i32` status code row.
#[derive(Serialize, Debug)]
pub struct NamedI32 {
    pub name: &'static str,
    pub value: i32,
}

/// One ABI enum family (type alias, representation, and member rows).
#[derive(Serialize, Debug)]
pub struct EnumFamily {
    /// Render family key, e.g. `"value_kind"`.
    pub family: &'static str,
    /// Rust type alias name emitted alongside the constants.
    pub type_alias: &'static str,
    /// Underlying integer representation, e.g. `"u32"`.
    pub repr: &'static str,
    /// Member rows in declaration order.
    pub rows: Vec<NamedU32>,
}

/// Diagnostic carrier symbol row: `(kind, carrier, runtime_symbol)`.
#[derive(Serialize, Debug)]
pub struct DiagnosticRow {
    pub kind: &'static str,
    pub carrier: &'static str,
    pub symbol: &'static str,
}

/// Canonical ABI payload emitted by this Radix binary.
///
/// Serialized by the `radix abi` renderers. The fingerprint is computed from
/// this payload (see [`AbiPayload::fingerprint`]) and is intentionally not a
/// field of the struct so it cannot contribute to its own hash.
#[derive(Serialize, Debug)]
pub struct AbiPayload {
    pub compiler: CompilerMeta,
    pub abi_version: u32,
    pub surface: &'static str,
    pub symbols: Vec<NamedStr>,
    pub enums: Vec<EnumFamily>,
    pub statuses: Vec<NamedI32>,
    pub diagnostics: Vec<DiagnosticRow>,
}

impl AbiPayload {
    /// Build the ABI payload for the current Radix build.
    #[must_use]
    pub fn current() -> AbiPayload {
        AbiPayload {
            compiler: CompilerMeta { name: COMPILER_NAME, version: COMPILER_VERSION },
            abi_version: host_abi::ABI_VERSION,
            surface: host_abi::ABI_SURFACE_NAME,
            symbols: {
                let mut s = named_str_rows(host_abi::SYMBOL_ROWS);
                s.extend(named_str_rows(host_abi::GRADIENT_SYMBOL_ROWS));
                s
            },
            enums: vec![
                enum_family("value_kind", "FaberRtValueKindV1", "u32", host_abi::VALUE_KIND_ROWS),
                enum_family(
                    "instans_precision",
                    "FaberRtInstansPrecisionV1",
                    "u32",
                    host_abi::INSTANS_PRECISION_ROWS,
                ),
                enum_family(
                    "array_range_mode",
                    "FaberRtArrayRangeModeV1",
                    "u32",
                    host_abi::ARRAY_RANGE_MODE_ROWS,
                ),
                enum_family(
                    "array_option_mode",
                    "FaberRtArrayOptionModeV1",
                    "u32",
                    host_abi::ARRAY_OPTION_MODE_ROWS,
                ),
                enum_family(
                    "genus_field_policy",
                    "FaberRtGenusFieldPolicyV1",
                    "u8",
                    host_abi::GENUS_FIELD_POLICY_ROWS,
                ),
            ],
            statuses: host_abi::STATUS_CODES
                .iter()
                .map(|(name, value)| NamedI32 { name, value: *value })
                .collect(),
            diagnostics: host_abi::DIAGNOSTIC_SYMBOLS_V1
                .iter()
                .map(|(kind, carrier, symbol)| DiagnosticRow { kind, carrier, symbol })
                .collect(),
        }
    }

    /// Deterministic content fingerprint over the canonical payload bytes.
    ///
    /// Non-cryptographic content hash (`FxHash`) of the canonical JSON
    /// serialization (sorted object keys). Stable across runs for an unchanged
    /// payload; meant for drift detection, bug reports, and runtime pairing —
    /// not for security.
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if the payload cannot be serialized to JSON.
    pub fn fingerprint(&self) -> Result<String, serde_json::Error> {
        let value = serde_json::to_value(self)?;
        let canonical = serde_json::to_string(&value)?;
        Ok(fingerprint_hex(canonical.as_bytes()))
    }
}

fn named_str_rows(rows: &[(&'static str, &'static str)]) -> Vec<NamedStr> {
    rows.iter()
        .map(|(name, value)| NamedStr { name, value })
        .collect()
}

fn enum_family(
    family: &'static str,
    type_alias: &'static str,
    repr: &'static str,
    rows: &[(&'static str, u32)],
) -> EnumFamily {
    EnumFamily {
        family,
        type_alias,
        repr,
        rows: rows
            .iter()
            .map(|(name, value)| NamedU32 { name, value: *value })
            .collect(),
    }
}

/// Hex content fingerprint over raw bytes using `FxHash`.
pub(crate) fn fingerprint_hex(bytes: &[u8]) -> String {
    let mut hasher = FxHasher::default();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
