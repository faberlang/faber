//! Phase-error → [`Diagnostic`] conversion (monolith-owned; catalog specs come from `radix-semantic`).
//!
//! These converters depend on lexer/parser/semantic error enums and therefore
//! cannot live in `radix-diagnostics` without creating a Cargo cycle. Call sites
//! keep the historical inherent-method names via the inherent impl block below
//! once those types are co-located; until then free functions are used through
//! the façade re-exports.

use crate::lexer::{LexError, LocaleSuggestion, MixedScriptAdvisory};
use crate::parser::ParseError;
use crate::semantic::SemanticError;
use radix_diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase, PhaseErrorKind, Severity, diagnostic_identity};
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

/// Keep the LOCALE002 candidates that may be shown, given the file's other diagnostics.
///
/// This is the single filter for every consumption site of the lexer's
/// reader-locale suggestions. A candidate is kept when either:
///
/// - it is an accent/width match (phase 1): the folded spelling equals the
///   folded suggestion, so the source is the keyword in a variant form; or
/// - it is an edit-distance guess (phase 2) and an error in the same file
///   overlaps its span: a parse error, or a name-lookup failure
///   (`SEM001:unknown_identifier`, `SEM008:undefined_name`).
///
/// A valid identifier that is merely near a keyword therefore never draws a
/// guess; a misspelled keyword or name that the compiler already rejected does.
#[must_use]
pub fn convert_filter_locale_suggestions(candidates: Vec<Diagnostic>, diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    candidates
        .into_iter()
        .filter(|candidate| is_accent_match(candidate) || is_at_error_span(candidate, diagnostics))
        .collect()
}

fn is_accent_match(candidate: &Diagnostic) -> bool {
    match (
        radix_semantic::arg_value(&candidate.args, "spelling"),
        radix_semantic::arg_value(&candidate.args, "suggestion"),
    ) {
        (Some(spelling), Some(suggestion)) => {
            radix_semantic::locale::suggestion_key(spelling) == radix_semantic::locale::suggestion_key(suggestion)
        }
        _ => false,
    }
}

fn is_at_error_span(candidate: &Diagnostic, diagnostics: &[Diagnostic]) -> bool {
    let Some(span) = candidate.span else {
        return false;
    };
    diagnostics.iter().any(|other| {
        other.file == candidate.file
            && is_suggestion_error(other)
            && other
                .span
                .is_some_and(|other_span| spans_overlap(span, other_span))
    })
}

fn is_suggestion_error(diagnostic: &Diagnostic) -> bool {
    if !diagnostic.is_error() {
        return false;
    }
    match (diagnostic.phase, diagnostic.code) {
        (DiagnosticPhase::Parse, _) => true,
        (DiagnosticPhase::Analysis, Some("SEM001")) => {
            radix_semantic::arg_value(&diagnostic.args, "issue") == Some("unknown_identifier")
        }
        (DiagnosticPhase::Analysis, Some("SEM008")) => {
            radix_semantic::arg_value(&diagnostic.args, "issue") == Some("undefined_name")
        }
        _ => false,
    }
}

fn spans_overlap(left: radix_diagnostics::Span, right: radix_diagnostics::Span) -> bool {
    left == right || (left.start < right.end && right.start < left.end)
}

/// Normalize a mixed-script identifier advisory into a non-fatal warning.
#[must_use]
pub fn convert_from_mixed_script_advisory(file: &str, source: &str, advisory: &MixedScriptAdvisory) -> Diagnostic {
    let spec = radix_diagnostics::mixed_script_identifier_spec();

    Diagnostic::warning(diagnostic_identity(spec.code, &[]))
        .with_phase(DiagnosticPhase::Tool)
        .with_code(spec.code)
        .with_args(vec![
            DiagnosticArg::new("identifier", advisory.spelling.clone()),
            DiagnosticArg::new("scripts", advisory.scripts.join(", ")),
        ])
        .with_source_context(file, source, advisory.span)
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

    /// Normalize a mixed-script identifier advisory into a non-fatal warning.
    fn from_mixed_script_advisory(file: &str, source: &str, advisory: &MixedScriptAdvisory) -> Diagnostic;

    /// Keep the LOCALE002 candidates allowed by the shared suggestion filter.
    fn filter_locale_suggestions(candidates: Vec<Diagnostic>, diagnostics: &[Diagnostic]) -> Vec<Diagnostic>;

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

    fn from_mixed_script_advisory(file: &str, source: &str, advisory: &MixedScriptAdvisory) -> Diagnostic {
        convert_from_mixed_script_advisory(file, source, advisory)
    }

    fn filter_locale_suggestions(candidates: Vec<Diagnostic>, diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
        convert_filter_locale_suggestions(candidates, diagnostics)
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
