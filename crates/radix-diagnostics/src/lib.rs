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
    diagnostic_identity, get_line_at_offset, nearest_char_boundary, offset_to_line,
    parse_arg_value, Diagnostic, DiagnosticArg, DiagnosticPhase, LexErrorKind, ParseErrorKind,
    PhaseErrorKind, SemanticErrorKind, Severity, WarningKind,
};
pub use span::Span;
pub use spec::{
    locale_suggestion_spec, locale_unknown_spec, unresolved_intrinsic_spec,
    unresolved_keyword_spec, unresolved_library_member_spec, DiagnosticSpec,
};

#[cfg(test)]
#[path = "diagnostic_test.rs"]
mod diagnostic_tests;

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn span_merge_covers_both() {
        let a = Span::new(0, 3);
        let b = Span::new(2, 9);
        assert_eq!(a.merge(b), Span::new(0, 9));
    }

    #[test]
    fn diagnostic_builder_attaches_code_and_span() {
        let d = Diagnostic::error("boom")
            .with_phase(DiagnosticPhase::Lex)
            .with_code("LEX001")
            .with_lex_kind(LexErrorKind::UnterminatedString)
            .with_span(Span::new(1, 2))
            .with_arg("issue", "sample");
        assert!(d.is_error());
        assert_eq!(d.code, Some("LEX001"));
        assert_eq!(d.lex_kind(), Some(LexErrorKind::UnterminatedString));
        assert_eq!(
            d.error_kind,
            Some(PhaseErrorKind::Lex(LexErrorKind::UnterminatedString))
        );
        assert_eq!(d.issue(), Some("sample"));
        assert_eq!(d.span, Some(Span::new(1, 2)));
    }
}
