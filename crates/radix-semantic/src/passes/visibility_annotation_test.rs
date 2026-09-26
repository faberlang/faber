//! Coverage for visibility-annotation validation.
//!
//! These tests prove that the pass rejects the reserved `@ protecta`
//! annotation, accepts `@ publica`/`@ privata`/`@ interna`, errors on
//! conflicting visibility tiers on one declaration, rejects `@ privata` /
//! `@ interna` on `genus` members (privacy is not enforced yet, D5.5), and
//! warns on redundant member `@ publica` (WARN028).

use super::visibility_annotation::check;
use crate::test_support::args_have_no_legacy_message;
use crate::{SemanticError, SemanticErrorKind, WarningKind};
use radix_diagnostics::DiagnosticArg;

fn expect_structured_error<'a>(
    errors: &'a [SemanticError],
    kind: SemanticErrorKind,
    expected_args: &[(&str, &str)],
) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.kind == kind
                && expected_args.iter().all(|(name, value)| {
                    err.args
                        .iter()
                        .any(|arg| arg.name == *name && arg.value == *value)
                })
        })
        .expect("structured semantic diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

#[test]
fn rejects_protecta_function_annotation() {
    let source = r"
@ protecta
functio secretum() → vacuum {
}
";
    let result = crate::test_support::analyze_source(source);
    assert!(!result.is_ok(), "{:?}", result.diagnostics);
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diag| diag.is_error() && diag.issue() == Some("protecta_reserved"))
        .expect("protecta diagnostic");
    assert_eq!(
        diagnostic.args,
        vec![
            DiagnosticArg::new("issue", "protecta_reserved"),
            DiagnosticArg::new("annotation", "@ protecta"),
            DiagnosticArg::new("publica", "@ publica"),
            DiagnosticArg::new("privata", "@ privata")
        ]
    );
}

#[test]
fn top_level_visibility_is_non_warning_metadata() {
    let source = r"
@ privata
functio hidden() → vacuum {
}
";
    let result = crate::test_support::analyze_source(source);
    assert!(result.is_ok());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| !diag.is_error() && crate::semantic_spec(diag.kind).code == "WARN012")
    );
}

fn member_source(annotation: &str) -> String {
    format!(
        r"
genus Demo {{
    {annotation}
    textus nomen
    {annotation}
    functio open() → vacuum {{
    }}
}}
"
    )
}

fn member_diagnostics(annotation: &str) -> Vec<SemanticError> {
    let source = member_source(annotation);
    let lex_result = radix_lexer::lex(&source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    check(&program, &parse_result.interner)
        .err()
        .unwrap_or_default()
}

#[test]
fn member_privata_and_interna_are_errors_until_privacy_is_enforced() {
    for (annotation, spelling) in [("@ privata", "@ privata"), ("@ interna", "@ interna")] {
        let errors = member_diagnostics(annotation);
        let unenforced: Vec<_> = errors
            .iter()
            .filter(|err| err.issue() == Some("member_privacy_unenforced"))
            .collect();
        assert_eq!(
            unenforced.len(),
            2,
            "field and method each diagnosed: {errors:?}"
        );
        for err in unenforced {
            assert_eq!(err.kind, SemanticErrorKind::AnnotationInvalid);
            assert!(err.is_error());
            assert!(
                err.args
                    .iter()
                    .any(|arg| arg.name == "annotation" && arg.value == spelling)
            );
        }
    }
}

#[test]
fn member_publica_is_a_redundancy_warning() {
    let errors = member_diagnostics("@ publica");
    assert_eq!(errors.len(), 2, "field and method each warned: {errors:?}");
    for err in &errors {
        assert_eq!(
            err.kind,
            SemanticErrorKind::Warning(WarningKind::RedundantMemberVisibility)
        );
        assert!(!err.is_error());
        assert_eq!(err.issue(), Some("redundant_member_publica"));
        assert_eq!(crate::semantic_spec(err.kind).code, "WARN028");
    }
}

#[test]
fn member_publica_warning_survives_full_analysis_as_non_fatal() {
    let result = crate::test_support::analyze_source(&member_source("@ publica"));
    assert!(result.is_ok(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| !diag.is_error() && diag.issue() == Some("redundant_member_publica"))
    );
}

#[test]
fn member_privata_fails_full_analysis() {
    let result = crate::test_support::analyze_source(&member_source("@ privata"));
    assert!(!result.is_ok(), "{:?}", result.diagnostics);
}

#[test]
fn top_level_interna_is_non_warning_metadata() {
    let source = r"
@ interna
functio internal() → vacuum {
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    let interner = &parse_result.interner;
    let result = check(&program, interner);
    assert!(
        result.is_ok(),
        "@ interna on function should not error or warn"
    );
}

#[test]
fn accepts_publica_on_function() {
    let source = r"
@ publica
functio visible() → vacuum {
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    let interner = &parse_result.interner;
    let result = check(&program, interner);
    assert!(result.is_ok(), "@ publica on function should not error");
}

#[test]
fn accepts_privata_on_function() {
    let source = r"
@ privata
functio hidden() → vacuum {
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    let interner = &parse_result.interner;
    let result = check(&program, interner);
    assert!(result.is_ok(), "@ privata on function should not error");
}

#[test]
fn conflicting_visibility_annotations_error() {
    let source = r"
@ publica
@ privata
functio ambiguum() → vacuum {
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    let interner = &parse_result.interner;
    let errors = check(&program, interner).expect_err("conflicting visibility should error");
    expect_structured_error(
        &errors,
        SemanticErrorKind::ConflictingVisibility,
        &[
            ("issue", "conflicting_visibility"),
            ("first", "@ publica"),
            ("second", "@ privata"),
        ],
    );
}

#[test]
fn conflicting_visibility_annotations_compile_error() {
    let source = r"
@ publica
@ interna
functio ambiguum() → vacuum {
}
";
    let result = crate::test_support::analyze_source(source);
    assert!(!result.is_ok(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && crate::semantic_spec(diag.kind).code == "SEM019")
    );
}

#[test]
fn duplicate_same_tier_annotations_are_accepted() {
    let source = r"
@ publica
@ publica
functio duplicate() → vacuum {
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    let interner = &parse_result.interner;
    let result = check(&program, interner);
    assert!(
        result.is_ok(),
        "duplicate same-tier visibility should not error"
    );
}

#[test]
fn ast_pass_rejects_protecta_visibility_annotation() {
    let source = r"
@ protecta
functio secretum() → vacuum {
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    let interner = &parse_result.interner;
    let errors = check(&program, interner).expect_err("protecta annotation should be rejected");
    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "protecta_reserved"),
            ("annotation", "@ protecta"),
            ("publica", "@ publica"),
            ("privata", "@ privata"),
        ],
    );
}
