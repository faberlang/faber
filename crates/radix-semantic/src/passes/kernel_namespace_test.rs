//! Coverage for kernel-namespace import validation.
//!
//! These tests prove that known `faber:*` modules and glob imports pass,
//! while unknown modules, invalid paths, and glob re-exports are rejected.

use super::kernel_namespace::validate_kernel_imports;
use crate::test_support::args_have_no_legacy_message;
use crate::{SemanticError, SemanticErrorKind};
use radix_parser;

fn parse_program(source: &str) -> (radix_syntax::Program, radix_lexer::Interner) {
    let lex = radix_lexer::lex(source);
    let parse = radix_parser::parse(lex);
    let interner = parse.interner;
    let program = parse.program.expect("parse");
    (program, interner)
}

fn analyze_imports(source: &str) -> Vec<SemanticError> {
    let (program, interner) = parse_program(source);
    validate_kernel_imports(&program, &interner)
        .err()
        .unwrap_or_default()
}

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
fn known_kernel_imports_pass_validation() {
    let errors = analyze_imports(
        r#"
importa ex "faber:consolum" consolum
importa ex "faber:json" json
importa ex "faber:processus" processus
importa ex "faber:aleator" aleator
"#,
    );
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn kernel_glob_import_passes_validation() {
    let errors = analyze_imports(r#"importa ex "faber:*" faber"#);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn kernel_glob_import_rejects_re_export() {
    let errors = analyze_imports(r#"importa ex "faber:*" publica faber"#);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportAbsent,
        &[("issue", "kernel_glob_reexport"), ("path", "faber:*")],
    );
}

#[test]
fn unknown_kernel_import_emits_kernel_diagnostic() {
    let errors = analyze_imports(r#"importa ex "faber:nihil" knox"#);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportAbsent,
        &[("issue", "kernel_unknown_module"), ("module", "nihil")],
    );
}

#[test]
fn nested_faber_path_is_invalid() {
    let errors = analyze_imports(r#"importa ex "faber:hal/solum" solum"#);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportAbsent,
        &[
            ("issue", "kernel_invalid_path"),
            ("path", "faber:hal/solum"),
            (
                "reason",
                "kernel import paths are flat (`faber:<module>` only)",
            ),
        ],
    );
}
