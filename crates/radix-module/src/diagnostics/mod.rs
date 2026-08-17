//! Cross-phase diagnostic contract for the compiler.
//!
//! Core transport types live in foundation crate `radix-diagnostics`. This
//! module re-exports them for the historical `radix_module::diagnostics::*` path and
//! keeps phase catalogs, conversion, and ariadne rendering in package `radix`
//! until parser/semantic enums are extracted.

mod catalog;
mod catalog_semantic;
mod catalog_syntax;
mod convert;
mod render;

pub use catalog::{arg_value, quote_arg};
pub use catalog_semantic::{semantic_message, semantic_spec};
pub use catalog_syntax::{lex_spec, parse_message, parse_spec};
pub use convert::{
    convert_from_lex_error as from_lex_error, convert_from_locale_suggestion as from_locale_suggestion,
    convert_from_parse_error as from_parse_error, convert_from_semantic_error as from_semantic_error,
};
pub use radix_diagnostics::{
    diagnostic_identity, get_line_at_offset, locale_suggestion_spec, locale_unknown_spec, nearest_char_boundary,
    parse_arg_value, unresolved_intrinsic_spec, unresolved_keyword_spec, unresolved_library_member_spec, Diagnostic,
    DiagnosticArg, DiagnosticPhase, DiagnosticSpec, LexErrorKind, ParseErrorKind, PhaseErrorKind, SemanticErrorKind,
    Severity, Span, WarningKind,
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

pub(crate) fn all_diagnostic_specs() -> Vec<DiagnosticSpec> {
    let mut specs = vec![
        unresolved_keyword_spec(),
        unresolved_intrinsic_spec(),
        unresolved_library_member_spec(),
        locale_suggestion_spec(),
        locale_unknown_spec(),
    ];
    specs.extend(
        catalog_syntax::LEX_ERROR_KINDS
            .iter()
            .copied()
            .map(catalog_syntax::lex_spec),
    );
    specs.extend(
        catalog_syntax::PARSE_ERROR_KINDS
            .iter()
            .copied()
            .map(catalog_syntax::parse_spec),
    );
    specs.extend(
        catalog_semantic::SEMANTIC_ERROR_KINDS
            .iter()
            .copied()
            .map(catalog_semantic::semantic_spec),
    );
    specs.extend(
        catalog_semantic::WARNING_KINDS
            .iter()
            .copied()
            .map(|kind| catalog_semantic::semantic_spec(crate::semantic::SemanticErrorKind::Warning(kind))),
    );
    specs
}

/// Return whether `code` is a known diagnostic catalog code.
///
/// Backed by [`all_diagnostic_specs`], this is the validation source for
/// `--deny <CODE>` CLI flags: an unknown code is rejected at parse time rather
/// than silently failing to match any diagnostic.
#[must_use]
pub fn is_known_diagnostic_code(code: &str) -> bool {
    all_diagnostic_specs().iter().any(|spec| spec.code == code)
}
