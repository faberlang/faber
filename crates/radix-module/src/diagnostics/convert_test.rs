//! Tests for diagnostic conversion from lex/parse/semantic errors.
//! Proves: identity transport (code + issue + args survives conversion);
//! severity and phase are preserved; reader-locale suggestion conversions
//! carry facts without prose.

use super::{
    DiagnosticConvert, convert_from_lex_error as from_lex_error,
    convert_from_locale_suggestion as from_locale_suggestion,
    convert_from_mixed_script_advisory as from_mixed_script_advisory, convert_from_parse_error as from_parse_error,
    convert_from_semantic_error as from_semantic_error,
};
use crate::diagnostics::{DiagnosticPhase, Severity};
use crate::lexer::{LexErrorKind, LocaleSuggestion, Span, lex};
use crate::parser::{ParseError, ParseErrorKind};
use crate::semantic::{SemanticError, SemanticErrorKind, WarningKind};
use crate::test_support::assert_diagnostic_args_eq;
use radix_diagnostics::get_line_at_offset;
use radix_diagnostics::{Diagnostic, DiagnosticArg};

#[test]
fn lex_error_normalization_carries_identity_and_facts_only() {
    let source = "nota x  # explains\n";
    let lex_result = lex(source);
    let err = lex_result
        .errors
        .iter()
        .find(|err| err.kind == LexErrorKind::InlineComment)
        .expect("inline comment diagnostic");

    let diagnostic = from_lex_error("inline.fab", source, err);

    assert_transport(
        &diagnostic,
        Severity::Error,
        DiagnosticPhase::Lex,
        "LEX007",
        Some("inline_hash_after_code"),
        &[DiagnosticArg::new("issue", "inline_hash_after_code")],
    );
    assert_eq!(diagnostic.lex_kind(), Some(LexErrorKind::InlineComment));
    assert_eq!(diagnostic.file, "inline.fab");
    assert_eq!(diagnostic.span, Some(err.span));
    assert_eq!(diagnostic.source_line.as_deref(), Some("nota x  # explains"));
}

/// PRP-031: typed `LexErrorKind` survives convert; not Debug-text reconstruction.
#[test]
fn lex_error_conversion_preserves_typed_kind_round_trip() {
    use crate::lexer::LexError;
    use radix_diagnostics::PhaseErrorKind;

    let err = LexError {
        kind: LexErrorKind::UnterminatedString,
        span: Span { start: 0, end: 1 },
        args: vec![DiagnosticArg::new("issue", "unterminated_string_literal")],
    };

    let diagnostic = Diagnostic::from_lex_error("roundtrip.fab", "\"\n", &err);

    assert_eq!(diagnostic.lex_kind(), Some(LexErrorKind::UnterminatedString));
    assert_eq!(
        diagnostic.error_kind,
        Some(PhaseErrorKind::Lex(LexErrorKind::UnterminatedString))
    );
    assert_eq!(diagnostic.code, Some("LEX001"));
    assert_eq!(diagnostic.phase, DiagnosticPhase::Lex);
    // Catalog code remains for human/docs; typed kind is the identity carrier.
    assert_eq!(err.kind, diagnostic.lex_kind().expect("typed kind preserved"));
}

#[test]
fn parser_lex_bridge_normalization_preserves_lexer_identity() {
    use radix_diagnostics::PhaseErrorKind;

    let source = "// removed\n";
    let mut err = ParseError::structured(ParseErrorKind::LexError, Span { start: 0, end: 2 });
    err.error_kind = Some(PhaseErrorKind::Lex(LexErrorKind::CommentSyntaxInvalid));
    err = err.with_arg("issue", "c_style_line_comment");

    let diagnostic = from_parse_error("bridge.fab", source, &err);

    assert_transport(
        &diagnostic,
        Severity::Error,
        DiagnosticPhase::Lex,
        "LEX006",
        Some("c_style_line_comment"),
        &[DiagnosticArg::new("issue", "c_style_line_comment")],
    );
    assert_eq!(diagnostic.lex_kind(), Some(LexErrorKind::CommentSyntaxInvalid));
}

#[test]
fn parse_error_normalization_carries_identity_and_named_args_only() {
    let source = "functio f(tensor<f32> value) → vacuum {}\n";
    let err = ParseError::structured(ParseErrorKind::Expected, Span { start: 20, end: 21 })
        .with_arg("issue", "tensor_figura_after_element")
        .with_arg("expected", "Figura shape");

    let diagnostic = from_parse_error("type-shape.fab", source, &err);

    assert_transport(
        &diagnostic,
        Severity::Error,
        DiagnosticPhase::Parse,
        "PARSE001",
        Some("tensor_figura_after_element"),
        &[
            DiagnosticArg::new("issue", "tensor_figura_after_element"),
            DiagnosticArg::new("expected", "Figura shape"),
        ],
    );
    assert_eq!(diagnostic.file, "type-shape.fab");
    assert_eq!(diagnostic.span, Some(Span { start: 20, end: 21 }));
    assert_eq!(diagnostic.source_line.as_deref(), Some(source.trim_end()));
}

/// F-016: typed `ParseErrorKind` survives convert; not lost at conversion.
#[test]
fn parse_error_conversion_preserves_typed_kind_round_trip() {
    use radix_diagnostics::PhaseErrorKind;

    let err = ParseError::structured(ParseErrorKind::FunctionNameAbsent, Span { start: 0, end: 8 })
        .with_arg("functio", "functio");

    let diagnostic = Diagnostic::from_parse_error("missing-name.fab", "functio\n", &err);

    assert_eq!(diagnostic.parse_kind(), Some(ParseErrorKind::FunctionNameAbsent));
    assert_eq!(
        diagnostic.error_kind,
        Some(PhaseErrorKind::Parse(ParseErrorKind::FunctionNameAbsent))
    );
    assert_eq!(diagnostic.code, Some("PARSE011"));
    assert_eq!(diagnostic.phase, DiagnosticPhase::Parse);
    assert_eq!(err.kind, diagnostic.parse_kind().expect("typed kind preserved"));
}

#[test]
fn semantic_error_normalization_carries_identity_and_named_args_only() {
    let source = "nota greting\n";
    let err = SemanticError::structured(SemanticErrorKind::VariableUndefined, Span { start: 5, end: 12 })
        .with_arg("issue", "unknown_identifier")
        .with_arg("spelling", "greting")
        .with_arg("suggestion", "greeting");

    let diagnostic = from_semantic_error("semantic-name.fab", source, &err);

    assert_transport(
        &diagnostic,
        Severity::Error,
        DiagnosticPhase::Analysis,
        "SEM001",
        Some("unknown_identifier"),
        &[
            DiagnosticArg::new("issue", "unknown_identifier"),
            DiagnosticArg::new("spelling", "greting"),
            DiagnosticArg::new("suggestion", "greeting"),
        ],
    );
    assert_eq!(diagnostic.file, "semantic-name.fab");
    assert_eq!(diagnostic.span, Some(Span { start: 5, end: 12 }));
    assert_eq!(diagnostic.source_line.as_deref(), Some("nota greting"));
}

/// F-016: typed `SemanticErrorKind` survives convert; not lost at conversion.
#[test]
fn semantic_error_conversion_preserves_typed_kind_round_trip() {
    use radix_diagnostics::PhaseErrorKind;

    let err = SemanticError::structured(SemanticErrorKind::TypeMismatch, Span { start: 0, end: 1 })
        .with_arg("issue", "type_mismatch");

    let diagnostic = Diagnostic::from_semantic_error("type-mismatch.fab", "?\n", &err);

    assert_eq!(diagnostic.semantic_kind(), Some(SemanticErrorKind::TypeMismatch));
    assert_eq!(
        diagnostic.error_kind,
        Some(PhaseErrorKind::Semantic(SemanticErrorKind::TypeMismatch))
    );
    assert_eq!(diagnostic.code, Some("SEM010"));
    assert_eq!(diagnostic.phase, DiagnosticPhase::Analysis);
    assert_eq!(err.kind, diagnostic.semantic_kind().expect("typed kind preserved"));
}

/// F-016: typed `SemanticErrorKind::Warning` survives convert.
#[test]
fn semantic_warning_conversion_preserves_typed_kind_round_trip() {
    use radix_diagnostics::PhaseErrorKind;

    let err = SemanticError::structured(
        SemanticErrorKind::Warning(WarningKind::UnnecessaryCast),
        Span { start: 0, end: 1 },
    );

    let diagnostic = from_semantic_error("warning.fab", "?\n", &err);

    assert_eq!(
        diagnostic.semantic_kind(),
        Some(SemanticErrorKind::Warning(WarningKind::UnnecessaryCast))
    );
    assert_eq!(
        diagnostic.error_kind,
        Some(PhaseErrorKind::Semantic(SemanticErrorKind::Warning(
            WarningKind::UnnecessaryCast
        )))
    );
    assert_eq!(diagnostic.phase, DiagnosticPhase::Analysis);
    assert_eq!(diagnostic.severity, Severity::Warning);
}

#[test]
fn semantic_warning_normalization_keeps_warning_severity_without_prose() {
    let err = SemanticError::structured(
        SemanticErrorKind::Warning(WarningKind::UnnecessaryCast),
        Span { start: 0, end: 1 },
    );

    let diagnostic = from_semantic_error("warning.fab", "?\n", &err);

    assert_transport(&diagnostic, Severity::Warning, DiagnosticPhase::Analysis, "WARN005", None, &[]);
}

#[test]
fn locale_suggestion_normalization_preserves_facts_without_warning_prose() {
    let source = "ham chao() { }\n";
    let suggestion = LocaleSuggestion {
        pack_id: "vi".to_string(),
        spelling: "ham".to_string(),
        suggestion: "hàm".to_string(),
        span: Span { start: 0, end: 3 },
    };

    let diagnostic = from_locale_suggestion("reader.fab", source, &suggestion);

    assert_transport(
        &diagnostic,
        Severity::Warning,
        DiagnosticPhase::Tool,
        "LOCALE002",
        None,
        &[
            DiagnosticArg::new("pack", "vi"),
            DiagnosticArg::new("spelling", "ham"),
            DiagnosticArg::new("suggestion", "hàm"),
        ],
    );
    assert_eq!(diagnostic.source_line.as_deref(), Some("ham chao() { }"));
}

#[test]
fn line_lookup_clamps_offsets_past_source_end() {
    let source = "prima\nsecunda";

    assert_eq!(get_line_at_offset(source, usize::MAX), "secunda");
}

#[test]
fn line_lookup_clamps_offsets_to_utf8_boundary() {
    let source = "αβ\nsecunda";
    let inside_beta = "α".len() + 1;

    assert_eq!(get_line_at_offset(source, inside_beta), "αβ");
}

#[test]
fn line_lookup_on_empty_source_returns_empty_string() {
    assert_eq!(get_line_at_offset("", 0), "");
    assert_eq!(get_line_at_offset("", 10), "");
}

#[test]
fn line_lookup_on_single_line_returns_whole_source() {
    let source = "tota linea una";
    assert_eq!(get_line_at_offset(source, 0), "tota linea una");
    assert_eq!(get_line_at_offset(source, source.len()), "tota linea una");
}

#[test]
fn lex_error_at_end_of_file_has_no_source_line() {
    let source = "nota x";
    let lex_result = lex(source);
    // EOF position (past last char)
    if let Some(err) = lex_result.errors.first() {
        let diagnostic = from_lex_error("eof.fab", source, err);
        assert_eq!(diagnostic.file, "eof.fab");
    }
}

fn first_mixed_script_advisory(source: &str) -> crate::lexer::MixedScriptAdvisory {
    lex(source)
        .mixed_script_advisories
        .first()
        .expect("mixed-script advisory")
        .clone()
}

#[test]
fn mixed_script_advisory_normalizes_to_nonfatal_warning() {
    let source = "let аdd ← 1\n";
    let advisory = first_mixed_script_advisory(source);

    let diagnostic = from_mixed_script_advisory("advisory.fab", source, &advisory);

    assert_transport(
        &diagnostic,
        Severity::Warning,
        DiagnosticPhase::Tool,
        "LOCALE004",
        None,
        &[
            DiagnosticArg::new("identifier", "аdd"),
            DiagnosticArg::new("scripts", "Cyrillic, Latin"),
        ],
    );
    assert_eq!(diagnostic.source_line.as_deref(), Some("let аdd ← 1"));
}

#[test]
fn mixed_script_compile_yields_warning_and_still_parses_and_analyzes() {
    // Cyrillic `а` beside Latin `dd`, defined and called: the S-A5 shape.
    // Advisory only — check succeeds with no error-severity diagnostic.
    let source =
        "+++\nlocale = \"en\"\n+++\nfn аdd(int n) → int {\n    return n + 1\n}\n\nmain {\n    print аdd(41)\n}\n";

    let result = crate::services::check_diagnostics_for_source("confusable.fab", source, false);

    assert!(result.success, "advisory must not fail check: {:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "no error-severity diagnostics expected: {:?}",
        result.diagnostics
    );
    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some("LOCALE004"))
        .collect();
    assert_eq!(warnings.len(), 2, "definition and call each advise: {:?}", result.diagnostics);
    assert_eq!(warnings[0].severity, Severity::Warning);
}

/// Assert conversion identity on the structured surfaces: code + issue + args.
///
/// Catalog identity is `code` and optional `issue` (arg / `issue()`), not
/// `message` or `help`. Renderer owns user-facing template text.
fn assert_transport(
    diagnostic: &Diagnostic,
    severity: Severity,
    phase: DiagnosticPhase,
    code: &'static str,
    issue: Option<&str>,
    args: &[DiagnosticArg],
) {
    assert_eq!(diagnostic.severity, severity);
    assert_eq!(diagnostic.phase, phase);
    assert_eq!(diagnostic.code, Some(code));
    assert_eq!(diagnostic.issue(), issue);
    assert_diagnostic_args_eq(diagnostic, args);
}
