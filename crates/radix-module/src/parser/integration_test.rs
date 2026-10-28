//! Integration tests for parser → diagnostic conversion.
//!
//! These tests cover invalid-form rejection and structured-diagnostic conversion
//! paths that depend on `DiagnosticConvert` (defined in `radix_module::diagnostics`)
//! and the driver. They stayed in the radix crate when the parser core moved
//! to the `radix-parser` sibling crate because the diagnostic-conversion trait
//! is a radix-owned integration surface, not a parser-leaf concern.

use crate::diagnostics::Diagnostic;
use crate::diagnostics::DiagnosticConvert;
use crate::driver::AnalyzeRequest;
use crate::lexer::INLINE_COMMENT_ISSUE;
use crate::lexer::lex_with_locale_pack;
use crate::parser::{ParseErrorKind, ParseResult, parse};
use crate::test_support::args_have_no_legacy_message;

/// The built-in Latin (`la`) reader pack: the canonical surface the parser
/// tests below are written in.
fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

/// Convenience wrapper: parse source that is expected to succeed.
#[allow(dead_code)]
fn parse_ok(source: &str) -> ParseResult {
    let result = parse(lex_with_locale_pack(source, &la_pack()));
    assert!(result.success(), "expected clean parse, got errors: {:?}", result.errors);
    result
}

// =============================================================================
// Conversio vel migration diagnostic
// =============================================================================

#[test]
fn conversio_vel_fallback_reports_migration_diagnostic() {
    let source = r#"sit _ n ← "bad" ↦ numerus vel 0"#;
    let result = parse(lex_with_locale_pack(source, &la_pack()));
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
// Prefix predicate diagnostics (literal-prefix guard)
// =============================================================================

fn assert_retired_prefix_predicate(source: &str, predicate: &str, replacement: &str) {
    let result = parse(lex_with_locale_pack(source, &la_pack()));
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

fn assert_semantic_rejection(source: &str) {
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let result = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default());
    let Some(diags) = result.err() else {
        panic!("expected semantic rejection, got ok for: {source}")
    };
    assert!(
        diags.iter().any(|d| d.code == Some("SEM001")),
        "expected SEM001 name-resolution rejection for: {source}, got: {diags:?}"
    );
}

fn assert_call_form_constructor_rejected(source: &str) {
    // The type-expression head is claimed by the parser as a typed-zero
    // constructor, so the retired call form is refused at parse time: a
    // constructor is not callable.
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let result = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default());
    let Some(diags) = result.err() else {
        panic!("expected rejection, got ok for: {source}")
    };
    assert!(
        diags
            .iter()
            .any(|d| d.code == Some("PARSE030") && d.issue() == Some("constructor_store_position_only")),
        "expected PARSE030 constructor_store_position_only for: {source}, got: {diags:?}"
    );
}

#[test]
fn retired_prefix_predicates_emit_rewrite_diagnostics() {
    assert_retired_prefix_predicate("sit _ x ← falsum disabled", "falsum", "value ≡ falsum");
    assert_retired_prefix_predicate("sit _ x ← verum enabled", "verum", "value ≡ verum");
    // D23.2/D23.4: `nulla` is the null value literal, so it is a
    // literal-prefix form. `nihil` is no longer a value spelling.
    assert_retired_prefix_predicate("sit _ x ← nulla maybe", "nulla", "value est nihil");
}

#[test]
fn required_prefix_predicates_stay_rejected() {
    assert_semantic_rejection("incipit { sit _ x ← nonnihil maybe }");
    assert_semantic_rejection("incipit { sit _ x ← nonnulla maybe }");
    assert_semantic_rejection("incipit { sit _ x ← negativum count }");
    assert_semantic_rejection("incipit { sit _ x ← positivum count }");
}

#[test]
fn postfix_array_variable_type_is_rejected() {
    assert_semantic_rejection("incipit { sit textus[] words ← vacua }");
}

// =============================================================================
// Expression ad block rejection
// =============================================================================

#[test]
fn expression_ad_rejects_block_form() {
    let source = r"sit _ s ← ad 'runtime:echo' { }";
    let result = parse(lex_with_locale_pack(source, &la_pack()));
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
    let source = r"sit _ v ← { name: 1 }";
    let result = parse(lex_with_locale_pack(source, &la_pack()));
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
    let source = r#"sit _ v ← { "x": verum }"#;
    let result = parse(lex_with_locale_pack(source, &la_pack()));
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
// Type-call constructor rejection (semantic name resolution)
// =============================================================================

#[test]
fn scalar_type_call_constructor_is_rejected() {
    assert_semantic_rejection(r#"incipit { sit _ n ← numerus("42") }"#);
}

#[test]
fn lista_type_call_constructor_is_rejected() {
    assert_semantic_rejection("incipit { sit _ xs ← lista<numerus>([1, 2]) }");
}

#[test]
fn tensor_long_constructor_is_rejected() {
    assert_call_form_constructor_rejected("incipit { sit _ t ← tensor<f32, [2]>([1.0, 2.0]) }");
}

#[test]
fn vector_long_constructor_is_rejected() {
    assert_call_form_constructor_rejected("incipit { sit _ v ← vector<f32, magnitudo 4>(1.0, 2.0, 3.0, 4.0) }");
}

#[test]
fn matrix_long_constructor_is_rejected() {
    assert_call_form_constructor_rejected("incipit { sit _ m ← matrix<f32, [2, 2]>(1.0, 0.0, 0.0, 1.0) }");
}

fn assert_list_bracket_constructor_accepted(source: &str) {
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let analyzed = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default())
        .unwrap_or_else(|diags| panic!("expected the stated bracket constructor to be accepted, got: {diags:?}"));
    assert!(
        analyzed.diagnostics.is_empty(),
        "expected no diagnostics for the stated bracket constructor, got: {:?}",
        analyzed.diagnostics
    );
}

#[test]
fn list_bracket_constructor_as_call_argument_is_accepted() {
    // The stated bracket constructor is a value expression at any depth: a
    // call argument builds the `verte` over the array like an initializer.
    assert_list_bracket_constructor_accepted(
        "functio f(lista<numerus> xs) → vacuum { nota xs }\nincipit { f(lista<numerus>[1, 2]) }",
    );
}

#[test]
fn list_bracket_constructor_parenthesized_is_accepted() {
    assert_list_bracket_constructor_accepted("incipit { sit lista<numerus> xs ← (lista<numerus>[1, 2])\n nota xs }");
}

#[test]
fn list_bracket_constructor_bound_to_a_name_is_accepted() {
    assert_list_bracket_constructor_accepted("incipit { sit lista<numerus> xs ← lista<numerus>[1, 2]\n nota xs }");
}

// =============================================================================
// Vector width issue diagnostic
// =============================================================================

#[test]
fn vector_long_constructor_without_width_is_rejected() {
    assert_semantic_rejection("incipit { sit _ v ← vector<f32>(1.0) }");
}

// =============================================================================
// Retired sugar constructors
// =============================================================================

#[test]
fn tensor_sugar_constructor_is_rejected() {
    assert_call_form_constructor_rejected("incipit { sit _ t ← tf32[2]([1.0, 2.0]) }");
}

#[test]
fn vector_sugar_constructor_is_rejected() {
    assert_call_form_constructor_rejected("incipit { sit _ v ← vf32[4](1.0, 2.0, 3.0, 4.0) }");
}

#[test]
fn matrix_sugar_constructor_is_rejected() {
    assert_call_form_constructor_rejected("incipit { sit _ m ← mf32[2, 2](1.0, 0.0, 0.0, 1.0) }");
}

// =============================================================================
// Inline comment lex diagnostic bridge
// =============================================================================

const BRACE_INLINE_FIXTURE: &str = "functio f() → vacuum { sit numerus x ← 1  # inline }\n";

#[test]
fn parser_bridge_normalizes_inline_comment_as_lex_diagnostic() {
    let parse_result = parse(lex_with_locale_pack(BRACE_INLINE_FIXTURE, &la_pack()));
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
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let result = crate::driver::analyze_source(
        &session,
        "bare-sparsa.fab",
        "incipit { sit sparsa x ← vacua }",
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

// =============================================================================
// Retired `sic` conditional: migration diagnostic keyed on structure
// =============================================================================

#[test]
fn retired_sic_conditional_reports_conditional_sic_removed() {
    let source = "sit _ v ← c sic 1 secus 2";
    let result = parse(lex_with_locale_pack(source, &la_pack()));
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "conditional_sic_removed")
        })
        .expect("conditional_sic_removed diagnostic");
    let diagnostic = Diagnostic::from_parse_error("test.fab", source, err);
    assert_eq!(diagnostic.code, Some("PARSE030"));
    assert_eq!(diagnostic.issue(), Some("conditional_sic_removed"));
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
}
