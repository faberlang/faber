//! Coverage for visibility-annotation validation.
//!
//! These tests prove that the pass rejects the reserved `@ protecta`
//! annotation, accepts `@ publica`/`@ privata`/`@ interna`, errors on
//! conflicting visibility tiers on one declaration, and no longer warns on
//! member visibility annotations (WARN012 retired for the enforced kinds).

use super::visibility_annotation::check;
use crate::test_support::args_have_no_legacy_message;
use crate::{SemanticError, SemanticErrorKind};
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

#[test]
fn member_visibility_annotations_are_accepted_without_diagnostic() {
    for annotation in ["@ publica", "@ interna", "@ privata"] {
        let source = format!(
            r"
genus Demo {{
    {annotation}
    functio open() → vacuum {{
    }}
}}
"
        );
        let lex_result = radix_lexer::lex(&source);
        let parse_result = radix_parser::parse(lex_result);
        let program = parse_result.program.expect("parse");
        let interner = &parse_result.interner;
        let result = check(&program, interner);
        assert!(
            result.is_ok(),
            "member {annotation} should not error or warn (WARN012 retired)"
        );
    }
}

#[test]
fn member_interna_is_accepted_without_diagnostic() {
    let source = r"
genus Demo {
    @ interna
    functio internal() → vacuum {
    }
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    let program = parse_result.program.expect("parse");
    let interner = &parse_result.interner;
    let result = check(&program, interner);
    assert!(result.is_ok(), "member @ interna should not error or warn");
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
