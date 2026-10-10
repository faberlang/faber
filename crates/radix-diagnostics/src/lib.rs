//! Diagnostic transport types and source spans for the Radix compiler.
//!
//! This crate is the Shape **1B** foundation home for diagnostics. It must not
//! depend on lexer, parser, semantic, or driver crates. Phase-specific
//! conversion (`from_lex_error`, catalogs keyed by phase error enums) lives in
//! the `radix` package until those enums are also extracted.
//!
//! # Invariants
//!
//! - [`Span`] is the shared source-location currency for later foundation crates.
//! - [`DiagnosticArg`] names are stable template keys, not positional slots.
//! - Missing file, span, source line, or help is valid degradation, not a crash.

mod diagnostic;
mod span;
mod spec;

pub use diagnostic::{
    Diagnostic, DiagnosticArg, DiagnosticPhase, LexErrorKind, ParseErrorKind, PhaseErrorKind,
    SemanticErrorKind, Severity, WarningKind, diagnostic_identity, get_line_at_offset,
    nearest_char_boundary, offset_to_line, parse_arg_value,
};
pub use span::Span;
pub use spec::{
    DiagnosticSpec, locale_suggestion_spec, locale_unknown_spec, mixed_script_identifier_spec,
};

#[cfg(test)]
#[path = "diagnostic_test.rs"]
mod diagnostic_tests;

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
