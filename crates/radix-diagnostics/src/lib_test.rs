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
