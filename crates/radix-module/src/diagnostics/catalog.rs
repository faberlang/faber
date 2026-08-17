//! Shared diagnostic catalog helpers (specs live in `radix-diagnostics`).
//!
//! Provides lookup and quoting utilities for [`DiagnosticArg`] slices used
//! during catalog rendering. Does not own the spec tables themselves;
//! those are defined in `catalog_syntax`, `catalog_semantic`, and the
//! leaf `radix-diagnostics` crate.

#![allow(dead_code)]

use radix_diagnostics::DiagnosticArg;
pub use radix_diagnostics::{locale_suggestion_spec, DiagnosticSpec};

#[must_use]
pub fn arg_value<'a>(args: &'a [DiagnosticArg], name: &str) -> Option<&'a str> {
    Some(args.iter().find(|arg| arg.name == name)?.value.as_str())
}

#[must_use]
pub fn quote_arg(args: &[DiagnosticArg], name: &str) -> Option<String> {
    arg_value(args, name).map(|value| format!("'{value}'"))
}

#[cfg(test)]
#[path = "catalog_test.rs"]
mod tests;
