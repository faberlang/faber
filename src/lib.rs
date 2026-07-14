//! User-facing Faber project and package orchestration.
//!
//! This crate owns the `faber` package-tool surface: project layout discovery,
//! source package loading, explain-command rendering, and the thin integration
//! points that hand validated source to the compiler library in `radix`.
//! Compiler feature work belongs in `radix`; this crate keeps CLI/package policy
//! close to user workflows.
//!
//! The public modules intentionally mirror user-facing capabilities: `package`
//! for project compilation and `explain` for language reference lookup. Private
//! helpers stay crate-local so lower-level compiler APIs do not inherit package
//! tool assumptions.

pub mod core_support;

mod explain_render;
mod input_shape;
mod io_buf;
pub(crate) mod library;

mod reference_parse;

pub mod diagnostic_explain;
pub mod explain;
pub mod package;
pub mod reference;

pub(crate) const PACKAGE_DIAGNOSTIC_CODE: &str = "PKG001";

pub(crate) fn package_diagnostic_error(
    message: impl Into<String>,
) -> radix::diagnostics::Diagnostic {
    radix::diagnostics::Diagnostic::error(message).with_code(PACKAGE_DIAGNOSTIC_CODE)
}

#[cfg(test)]
mod core_support_test;
#[cfg(test)]
mod diagnostic_explain_test;
#[cfg(test)]
mod explain_test;
#[cfg(test)]
mod input_shape_test;
#[cfg(test)]
mod reference_pack_test_support;
#[cfg(test)]
mod reference_parse_test;
#[cfg(test)]
mod reference_test;
