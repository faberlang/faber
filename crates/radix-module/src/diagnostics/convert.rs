//! Phase-error → [`Diagnostic`] conversion (stays in package `radix`).
//!
//! These converters depend on lexer/parser/semantic error enums and therefore
//! cannot live in `radix-diagnostics` without creating a Cargo cycle. Call sites
//! keep the historical inherent-method names via the inherent impl block below
//! once those types are co-located; until then free functions are used through
//! the façade re-exports.

use crate::lexer::{LexError, LocaleSuggestion};
use crate::parser::ParseError;
use crate::semantic::SemanticError;
use radix_diagnostics::{diagnostic_identity, Diagnostic, DiagnosticArg, DiagnosticPhase, PhaseErrorKind, Severity};
use radix_semantic::catalog;
use radix_semantic::catalog_semantic;
use radix_semantic::catalog_syntax;

/// Normalize a lexer error using the diagnostic code catalog.
///
/// Preserves the typed [`LexErrorKind`] on the diagnostic so programmatic
/// consumers do not reconstruct identity from catalog codes or arg strings.
#[must_use]
pub fn convert_from_lex_error(file: &str, source: &str, err: &LexError) -> Diagnostic {
    let spec = catalog_syntax::lex_spec(err.kind);
    Diagnostic::error(diagnostic_identity(spec.code, &err.args))
        .with_phase(DiagnosticPhase::Lex)
        .with_code(spec.code)
        .with_lex_kind(err.kind)
        .with_args(err.args.clone())
        .with_source_context(file, source, err.span)
}

/// Normalize a reader-locale spelling suggestion into a non-fatal warning.
#[must_use]
pub fn convert_from_locale_suggestion(file: &str, source: &str, suggestion: &LocaleSuggestion) -> Diagnostic {
    let spec = catalog::locale_suggestion_spec();

    Diagnostic::warning(diagnostic_identity(spec.code, &[]))
        .with_phase(DiagnosticPhase::Tool)
        .with_code(spec.code)
        .with_args(vec![
            DiagnosticArg::new("pack", suggestion.pack_id.clone()),
            DiagnosticArg::new("spelling", suggestion.spelling.clone()),
            DiagnosticArg::new("suggestion", suggestion.suggestion.clone()),
        ])
        .with_source_context(file, source, suggestion.span)
}

/// Normalize a parser error using the diagnostic code catalog.
///
/// When the parser bridges a lexer failure (`ParseErrorKind::LexError`), the
/// typed [`LexErrorKind`] is carried on [`ParseError::error_kind`] and
/// preserved on the output diagnostic. Direct lexer conversion
/// ([`convert_from_lex_error`]) is the preferred path for new code.
#[must_use]
pub fn convert_from_parse_error(file: &str, source: &str, err: &ParseError) -> Diagnostic {
    if let Some(PhaseErrorKind::Lex(kind)) = err.error_kind {
        let spec = catalog_syntax::lex_spec(kind);
        return Diagnostic::error(diagnostic_identity(spec.code, &err.args))
            .with_phase(DiagnosticPhase::Lex)
            .with_code(spec.code)
            .with_lex_kind(kind)
            .with_args(err.args.clone())
            .with_source_context(file, source, err.span);
    }
    let spec = catalog_syntax::parse_spec(err.kind);
    Diagnostic::error(diagnostic_identity(spec.code, &err.args))
        .with_phase(DiagnosticPhase::Parse)
        .with_code(spec.code)
        .with_parse_kind(err.kind)
        .with_args(err.args.clone())
        .with_source_context(file, source, err.span)
}

/// Normalize a semantic error while preserving semantic warning policy.
#[must_use]
pub fn convert_from_semantic_error(file: &str, source: &str, err: &SemanticError) -> Diagnostic {
    let severity = if err.is_error() {
        Severity::Error
    } else {
        Severity::Warning
    };

    let spec = catalog_semantic::semantic_spec(err.kind);

    Diagnostic::new(severity, diagnostic_identity(spec.code, &err.args))
        .with_phase(DiagnosticPhase::Analysis)
        .with_code(spec.code)
        .with_semantic_kind(err.kind)
        .with_args(err.args.clone())
        .with_source_context(file, source, err.span)
}

/// Associated converters with historical `Diagnostic::from_*` names.
pub trait DiagnosticConvert {
    /// Normalize a lexer error using the diagnostic code catalog.
    fn from_lex_error(file: &str, source: &str, err: &LexError) -> Diagnostic;

    /// Normalize a reader-locale spelling suggestion into a non-fatal warning.
    fn from_locale_suggestion(file: &str, source: &str, suggestion: &LocaleSuggestion) -> Diagnostic;

    /// Normalize a parser error using the diagnostic code catalog.
    fn from_parse_error(file: &str, source: &str, err: &ParseError) -> Diagnostic;

    /// Normalize a semantic error while preserving semantic warning policy.
    fn from_semantic_error(file: &str, source: &str, err: &SemanticError) -> Diagnostic;
}

impl DiagnosticConvert for Diagnostic {
    fn from_lex_error(file: &str, source: &str, err: &LexError) -> Diagnostic {
        convert_from_lex_error(file, source, err)
    }

    fn from_locale_suggestion(file: &str, source: &str, suggestion: &LocaleSuggestion) -> Diagnostic {
        convert_from_locale_suggestion(file, source, suggestion)
    }

    fn from_parse_error(file: &str, source: &str, err: &ParseError) -> Diagnostic {
        convert_from_parse_error(file, source, err)
    }

    fn from_semantic_error(file: &str, source: &str, err: &SemanticError) -> Diagnostic {
        convert_from_semantic_error(file, source, err)
    }
}

#[cfg(test)]
#[path = "convert_test.rs"]
mod tests;
