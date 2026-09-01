//! Integration tests for parser → diagnostic conversion.
//!
//! These tests cover retired-form rejection and structured-diagnostic conversion
//! paths that depend on `DiagnosticConvert` (defined in `radix_module::diagnostics`)
//! and the driver. They stayed in the radix crate when the parser core moved
//! to the `radix-parser` sibling crate because the diagnostic-conversion trait
//! is a radix-owned integration surface, not a parser-leaf concern.

use crate::diagnostics::Diagnostic;
use crate::diagnostics::DiagnosticConvert;
use crate::driver::AnalyzeRequest;
use crate::lexer::INLINE_COMMENT_ISSUE;
use crate::lexer::lex;
use crate::parser::{ParseErrorKind, ParseResult, parse};
use crate::test_support::args_have_no_legacy_message;

/// Convenience wrapper: parse source that is expected to succeed.
#[allow(dead_code)]
fn parse_ok(source: &str) -> ParseResult {
    let result = parse(lex(source));
    assert!(result.success(), "expected clean parse, got errors: {:?}", result.errors);
    result
}

// =============================================================================
// Conversio vel migration diagnostic
// =============================================================================

#[test]
fn conversio_vel_fallback_reports_migration_diagnostic() {
    let source = r#"fixum _ n ← "bad" ↦ numerus vel 0"#;
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "conversio_recovery_uses_vel")
        })
        .expect("structured conversio vel migration diagnostic");
    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE030"));
    assert_eq!(diagnostic.issue(), Some("conversio_recovery_uses_vel"));
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
}

// =============================================================================
// Retired prefix predicate diagnostics
// =============================================================================

fn assert_retired_prefix_predicate(source: &str, predicate: &str, replacement: &str) {
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "retired_prefix_predicate")
        })
        .expect("retired prefix predicate diagnostic");
    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE001"));
    assert_eq!(diagnostic.issue(), Some("retired_prefix_predicate"));
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "predicate" && arg.value == predicate)
    );
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "replacement" && arg.value == replacement)
    );
}

#[test]
fn retired_prefix_predicates_emit_rewrite_diagnostics() {
    assert_retired_prefix_predicate("fixum _ x ← falsum disabled", "falsum", "value est falsum");
    assert_retired_prefix_predicate("fixum _ x ← verum enabled", "verum", "value est verum");
    assert_retired_prefix_predicate("fixum _ x ← nihil maybe", "nihil", "value est nihil");
    assert_retired_prefix_predicate("fixum _ x ← nonnihil maybe", "nonnihil", "value non est nihil");
    assert_retired_prefix_predicate("fixum _ x ← nulla maybe", "nulla", "value est nihil");
    assert_retired_prefix_predicate("fixum _ x ← nonnulla maybe", "nonnulla", "value non est nihil");
    assert_retired_prefix_predicate("fixum _ x ← negativum count", "negativum", "value < 0");
    assert_retired_prefix_predicate("fixum _ x ← positivum count", "positivum", "value > 0");
}

// =============================================================================
// Expression ad block rejection
// =============================================================================

#[test]
fn expression_ad_rejects_block_form() {
    let source = r"fixum _ s ← ad 'runtime:echo' { }";
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "expression_ad_block")
        })
        .expect("structured expression ad block diagnostic");
    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE030"));
    assert_eq!(diagnostic.issue(), Some("expression_ad_block"));
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
}

// =============================================================================
// JSON literal rejection diagnostics
// =============================================================================

#[test]
fn json_literal_rejects_bare_identifier_key() {
    let source = r"fixum _ v ← { name: 1 }";
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "json_quoted_key")
        })
        .expect("structured quoted-key diagnostic");
    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE001"));
    assert_eq!(diagnostic.issue(), Some("json_quoted_key"));
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
}

#[test]
fn json_literal_rejects_faber_keywords_as_values() {
    // verum/falsum/nihil are Faber keywords; JSON wants true/false/null.
    let source = r#"fixum _ v ← { "x": verum }"#;
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "json_faber_keyword_value")
        })
        .expect("structured JSON keyword diagnostic");
    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE002"));
    assert_eq!(diagnostic.issue(), Some("json_faber_keyword_value"));
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
}

// =============================================================================
// Retired type-call constructor diagnostics
// =============================================================================

fn assert_retired_type_call_constructor(source: &str) {
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "retired_type_call_constructor")
        })
        .expect("retired type-call constructor diagnostic");
    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE001"));
    assert_eq!(diagnostic.issue(), Some("retired_type_call_constructor"));
}

#[test]
fn scalar_type_call_constructor_is_rejected() {
    assert_retired_type_call_constructor(r#"fixum _ n ← numerus("42")"#);
}

#[test]
fn lista_type_call_constructor_is_rejected() {
    assert_retired_type_call_constructor("fixum _ xs ← lista<numerus>([1, 2])");
}

#[test]
fn tensor_long_constructor_is_rejected() {
    assert_retired_type_call_constructor("fixum _ t ← tensor<f32, [2]>([1.0, 2.0])");
}

#[test]
fn vector_long_constructor_is_rejected() {
    assert_retired_type_call_constructor("fixum _ v ← vector<f32, 4>(1.0, 2.0, 3.0, 4.0)");
}

#[test]
fn matrix_long_constructor_is_rejected() {
    assert_retired_type_call_constructor("fixum _ m ← matrix<f32, [2, 2]>(1.0, 0.0, 0.0, 1.0)");
}

// =============================================================================
// Vector width issue diagnostic
// =============================================================================

#[test]
fn vector_long_constructor_requires_width_issue() {
    let source = "fixum _ v ← vector<f32>(1.0)";
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "vector_width_after_element")
        })
        .expect("vector width diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );

    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE001"));
    assert_eq!(diagnostic.issue(), Some("vector_width_after_element"));
}

// =============================================================================
// Retired sugar constructors
// =============================================================================

#[test]
fn tensor_sugar_constructor_is_rejected() {
    assert_retired_type_call_constructor("fixum _ t ← tf32[2]([1.0, 2.0])");
}

#[test]
fn vector_sugar_constructor_is_rejected() {
    assert_retired_type_call_constructor("fixum _ v ← vf32[4](1.0, 2.0, 3.0, 4.0)");
}

#[test]
fn matrix_sugar_constructor_is_rejected() {
    assert_retired_type_call_constructor("fixum _ m ← mf32[2, 2](1.0, 0.0, 0.0, 1.0)");
}

// =============================================================================
// Inline comment lex diagnostic bridge
// =============================================================================

const BRACE_INLINE_FIXTURE: &str = "functio f() → vacuum { fixum numerus x ← 1  # inline }\n";

#[test]
fn parser_bridge_normalizes_inline_comment_as_lex_diagnostic() {
    let parse_result = parse(lex(BRACE_INLINE_FIXTURE));
    assert!(!parse_result.success());

    let diagnostic = Diagnostic::from_parse_error("inline.fab", BRACE_INLINE_FIXTURE, &parse_result.errors[0]);

    assert_eq!(diagnostic.phase, crate::diagnostics::DiagnosticPhase::Lex);
    assert_eq!(diagnostic.code, Some("LEX007"));
    assert_eq!(diagnostic.issue(), Some(INLINE_COMMENT_ISSUE));
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == INLINE_COMMENT_ISSUE),
        "expected inline comment issue fact, got {:?}",
        diagnostic.args
    );
}

// =============================================================================
// Semantic-level sparsa rejection (driver integration)
// =============================================================================

#[test]
fn rejects_bare_sparsa() {
    let session = crate::driver::Session::new(crate::driver::Config::default());
    let result = crate::driver::analyze_source(
        &session,
        "bare-sparsa.fab",
        "incipit { fixum sparsa x ← vacua }",
        AnalyzeRequest::default(),
    );
    let Some(diags) = result.err() else {
        panic!("expected sparsa type error, got ok");
    };
    assert!(
        diags.iter().any(|d| {
            d.code == Some("SEM010")
                && d.args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "sparsa_incomplete")
        }),
        "expected sparsa incomplete diagnostic fact, got: {diags:?}"
    );
}
