//! Cross-phase diagnostic contract for the compiler.
//!
//! Core transport types live in foundation crate `radix-diagnostics`; the
//! phase catalogs and code lookup live in `radix-semantic` (they key on the
//! semantic error taxonomy). This module re-exports both for the historical
//! `radix_module::diagnostics::*` path and owns the remaining conversions and
//! ariadne rendering.

mod convert;
mod render;

#[cfg(test)]
mod catalog_test;

pub use convert::{
    convert_from_lex_error as from_lex_error, convert_from_locale_suggestion as from_locale_suggestion,
    convert_from_parse_error as from_parse_error, convert_from_semantic_error as from_semantic_error,
};
pub use radix_diagnostics::{
    Diagnostic, DiagnosticArg, DiagnosticPhase, DiagnosticSpec, LexErrorKind, ParseErrorKind, PhaseErrorKind,
    SemanticErrorKind, Severity, Span, WarningKind, diagnostic_identity, get_line_at_offset, locale_suggestion_spec,
    locale_unknown_spec, nearest_char_boundary, offset_to_line, parse_arg_value,
};
pub use radix_semantic::{
    all_diagnostic_specs, arg_value, lex_spec, parse_message, parse_spec, quote_arg, semantic_spec,
};
pub use render::{
    render_diagnostics, render_diagnostics_with_pack, render_expanded, render_expanded_diagnostics,
    render_expanded_diagnostics_with_pack, render_expanded_with_pack, render_plain, render_plain_with_pack,
};

/// Trait providing historical `Diagnostic::from_*` associated functions.
///
/// Import this trait (or `crate::diagnostics::DiagnosticConvert`) in modules
/// that call `Diagnostic::from_lex_error` and siblings.
pub use convert::DiagnosticConvert;

/// Return whether `code` is a known diagnostic catalog code.
///
/// Backed by [`all_diagnostic_specs`], this is the validation source for
/// `--deny <CODE>` CLI flags: an unknown code is rejected at parse time rather
/// than silently failing to match any diagnostic.
#[must_use]
pub fn is_known_diagnostic_code(code: &str) -> bool {
    all_diagnostic_specs().iter().any(|spec| spec.code == code)
}
