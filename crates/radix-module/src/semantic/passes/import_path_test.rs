//! Coverage for import-path validation.
//!
//! These tests prove that the pass accepts valid provider, relative, and
//! template-based import paths, and rejects parent-traversal (`..`) and
//! absolute paths.

use super::validate_import_paths;
use crate::parser;
use crate::parser::ParseErrorKind;
use crate::semantic::{SemanticError, SemanticErrorKind};
use crate::test_support::args_have_no_legacy_message;
use std::collections::BTreeMap;

fn parse_program(source: &str) -> (crate::syntax::Program, crate::lexer::Interner) {
    let lex = crate::lexer::lex(source);
    let parse = parser::parse(lex);
    let interner = parse.interner;
    let program = parse.program.expect("parse");
    (program, interner)
}

fn analyze(source: &str, templates: Option<&BTreeMap<String, String>>) -> Vec<SemanticError> {
    let (program, interner) = parse_program(source);
    validate_import_paths(&program, &interner, templates)
        .err()
        .unwrap_or_default()
}

fn make_templates() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("src".to_owned(), "src".to_owned());
    m.insert("gnu".to_owned(), "../common/gnu".to_owned());
    m
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

// ──── Valid forms ────

#[test]
fn valid_provider_import_passes() {
    let errors = analyze(r#"importa ex "norma:chorda" chorda"#, None);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn valid_relative_import_passes() {
    let errors = analyze(r#"importa ex "./sibling" sibling"#, None);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn valid_template_import_with_config_passes() {
    let templates = make_templates();
    let errors = analyze(r#"importa ex "§src/types" types"#, Some(&templates));
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn valid_default_src_template_passes() {
    // §src is a default even when templates map is present
    let templates = make_templates();
    let errors = analyze(r#"importa ex "§src/voxel" voxel"#, Some(&templates));
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

// ──── Parent traversal (`..`) ────

#[test]
fn reject_starting_dot_dot() {
    let errors = analyze(r#"importa ex "../src/voxel" voxel"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_parent_traversal")],
    );
}

#[test]
fn reject_mid_dot_dot() {
    let errors = analyze(r#"importa ex "a/../b" b"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_parent_traversal")],
    );
}

#[test]
fn reject_ending_dot_dot() {
    let errors = analyze(r#"importa { ex = "a/b/..", nomen = b }"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_parent_traversal")],
    );
}

#[test]
fn reject_bare_dot_dot() {
    let errors = analyze(r#"importa { ex = "..", nomen = parent }"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_parent_traversal")],
    );
}

// ──── Absolute paths ────

#[test]
fn reject_unix_absolute_path() {
    let errors = analyze(r#"importa ex "/usr/lib/foo" foo"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(&errors, SemanticErrorKind::ImportNotFound, &[("issue", "import_absolute_path")]);
}

#[test]
fn reject_windows_absolute_path() {
    let errors = analyze(r#"importa { ex = "C:\\Users\\foo", nomen = foo }"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(&errors, SemanticErrorKind::ImportNotFound, &[("issue", "import_absolute_path")]);
}

// ──── Unknown scheme ────

#[test]
fn reject_https_scheme() {
    let errors = analyze(r#"importa ex "https://example.com/lib" lib"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_unknown_scheme")],
    );
}

#[test]
fn reject_unknown_scheme() {
    let errors = analyze(r#"importa ex "ftp://files/pkg" pkg"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_unknown_scheme")],
    );
}

#[test]
fn reject_bare_name_without_prefix() {
    let errors = analyze(r#"importa ex "justaname" name"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_unknown_scheme")],
    );
}

// ──── Script mode (no templates) rejects all § forms ────

#[test]
fn reject_ssrc_in_script_mode() {
    // Script/bare mode has no templates map; §src must be rejected.
    let errors = analyze(r#"importa ex "§src/types" types"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_unknown_template")],
    );
}

#[test]
fn reject_section_in_script_mode() {
    // Any § form (not just §src) is rejected when templates is None.
    let errors = analyze(r#"importa ex "§gnu/common" common"#, None);
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_unknown_template")],
    );
}

// ──── Template validation ────

#[test]
fn reject_unknown_template_name() {
    let templates = make_templates();
    let errors = analyze(r#"importa ex "§unknown/types" types"#, Some(&templates));
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_unknown_template")],
    );
}

#[test]
fn reject_bare_glyph() {
    let templates = make_templates();
    let errors = analyze(r#"importa ex "§/rest" rest"#, Some(&templates));
    assert_eq!(errors.len(), 1);
    expect_structured_error(&errors, SemanticErrorKind::ImportNotFound, &[("issue", "import_bare_glyph")]);
}

#[test]
fn reject_bare_glyph_no_slash() {
    let templates = make_templates();
    let errors = analyze(r#"importa { ex = "§", nomen = bare }"#, Some(&templates));
    assert_eq!(errors.len(), 1);
    expect_structured_error(&errors, SemanticErrorKind::ImportNotFound, &[("issue", "import_bare_glyph")]);
}

#[test]
fn reject_multiple_holes() {
    let templates = make_templates();
    let errors = analyze(r#"importa ex "§a/§b/rest" rest"#, Some(&templates));
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_multiple_holes")],
    );
}

#[test]
fn reject_positional_hole() {
    let templates = make_templates();
    let errors = analyze(r#"importa ex "§0/something" something"#, Some(&templates));
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_positional_hole")],
    );
}

#[test]
fn reject_positional_hole_1() {
    let templates = make_templates();
    let errors = analyze(r#"importa ex "§1/something" something"#, Some(&templates));
    assert_eq!(errors.len(), 1);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_positional_hole")],
    );
}

// ──── Multiple errors ────

#[test]
fn multiple_errors_in_one_pass() {
    let source = r#"
importa ex "../src/bad" a
importa ex "/absolute/path" b
importa ex "https://evil.com/pkg" c
"#;
    let errors = analyze(source, None);
    // Three errors: parent traversal, absolute path, unknown scheme
    assert_eq!(errors.len(), 3);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_parent_traversal")],
    );
    expect_structured_error(&errors, SemanticErrorKind::ImportNotFound, &[("issue", "import_absolute_path")]);
    expect_structured_error(
        &errors,
        SemanticErrorKind::ImportNotFound,
        &[("issue", "import_unknown_scheme")],
    );
}

// ──── Mixed valid and invalid ────

#[test]
fn valid_imports_alongside_invalid() {
    let source = r#"
importa ex "norma:chorda" chorda
importa ex "./sibling" sibling
importa ex "../parent" parent
importa ex "https://cdn/pkg" pkg
"#;
    let errors = analyze(source, None);
    assert_eq!(errors.len(), 2);
}

// ──── Edge cases ────

#[test]
fn reject_empty_import_path() {
    let parse = parser::parse(crate::lexer::lex(r#"importa ex "" empty"#));
    assert!(
        parse
            .errors
            .iter()
            .any(|error| error.kind == ParseErrorKind::InvalidImport
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "import_name_inference_failed")),
        "empty import path should be rejected before semantic import-path validation: {:?}",
        parse.errors
    );
}

#[test]
fn reject_whitespace_only_import_path() {
    let errors = analyze(r#"importa { ex = "   ", nomen = spaces }"#, None);
    assert_eq!(errors.len(), 1);
}

#[test]
fn reject_tab_in_import_path() {
    let errors = analyze(r#"importa { ex = "\t", nomen = tab }"#, None);
    assert_eq!(errors.len(), 1);
}
