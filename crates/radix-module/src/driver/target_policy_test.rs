//! Tests for target-specific policy: unsupported construct detection and no-op warnings.
//!
//! Contract: each target rejects syntax it cannot represent; supported syntax passes without target diagnostics.

use super::*;
use crate::lexer::lex_with_locale_pack;
use crate::parser::parse;

/// Parse a source string into a program, or panic.

/// The built-in Latin (`la`) reader pack: the canonical surface the parser
/// tests below are written in.
fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

fn parse_program(source: &str) -> (Program, crate::lexer::Interner) {
    let result = parse(lex_with_locale_pack(source, &la_pack()));
    assert!(result.success(), "parse errors: {:?}", result.errors);
    (result.program.expect("program"), result.interner)
}

// ── Rust unsupported construct detection ────────────────────────────────────

#[test]
fn rust_empty_program_has_no_unsupported_errors() {
    let (program, _) = parse_program("");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "empty program should have no Rust unsupported errors");
}

#[test]
fn rust_simple_function_has_no_unsupported_errors() {
    let (program, _) = parse_program("functio f() → vacuum { tacet }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "simple function should have no Rust unsupported errors");
}

#[test]
fn rust_variable_declaration_has_no_unsupported_errors() {
    let (program, _) = parse_program("functio f() → vacuum { sit numerus x ← 1 }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "variable declaration should have no Rust unsupported errors");
}

#[test]
fn rust_if_without_cape_has_no_unsupported_errors() {
    let (program, _) = parse_program("functio f() → vacuum { si verum { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "if without cape should have no Rust unsupported errors");
}

#[test]
fn rust_fac_cape_does_not_emit_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { fac { tacet } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "fac/cape should not produce Rust unsupported errors");
}

#[test]
fn rust_si_with_cape_has_no_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { si verum { } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .all(|e| e.issue() != Some("rust_target_cape_unsupported")),
        "si cape should emit; got: {errors:?}"
    );
}

#[test]
fn rust_dum_with_cape_has_no_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { dum verum { perge } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .all(|e| e.issue() != Some("rust_target_cape_unsupported")),
        "dum cape should emit; got: {errors:?}"
    );
}

#[test]
fn rust_itera_with_cape_has_no_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { itera ex items fixum n { tacet } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .all(|e| e.issue() != Some("rust_target_cape_unsupported")),
        "itera cape should emit; got: {errors:?}"
    );
}

#[test]
fn rust_elige_with_cape_has_no_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { elige 1 { casu 1 { tacet } } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .all(|e| e.issue() != Some("rust_target_cape_unsupported")),
        "elige cape should emit; got: {errors:?}"
    );
}

#[test]
fn rust_mixed_cape_forms_have_no_unsupported_error() {
    let (program, _) =
        parse_program("functio f() → vacuum { si verum { tacet } elige 1 { casu 1 { tacet } } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .all(|e| e.issue() != Some("rust_target_cape_unsupported")),
        "si+elige cape should emit; got: {errors:?}"
    );
}

#[test]
fn rust_fient_declarations_pass_target_policy() {
    let (program, _) = parse_program(
        r"functio stream() fient → numerus { cede 1 }

genus Source {
  functio stream() fient → numerus { cede 1 }
}

implendum SourceContract {
  functio stream() fient → numerus
}
",
    );
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "Rust fient declarations should pass: {errors:?}");
}

// ── Go unsupported construct detection ───────────────────────────────────────

#[test]
fn go_empty_program_has_no_unsupported_errors() {
    let (program, interner) = parse_program("");
    let errors = collect_go_unsupported_errors(&program, "test.fab", &interner);
    assert!(errors.is_empty(), "empty program should have no Go unsupported errors");
}

#[test]
fn go_simple_function_has_no_unsupported_errors() {
    let (program, interner) = parse_program("functio f() → vacuum { tacet }\n");
    let errors = collect_go_unsupported_errors(&program, "test.fab", &interner);
    assert!(errors.is_empty(), "simple function should have no Go unsupported errors");
}

#[test]
fn go_fac_handles_properly() {
    let (program, interner) = parse_program("functio f() → vacuum { fac { tacet } cape err { tacet } }\n");
    let errors = collect_go_unsupported_errors(&program, "test.fab", &interner);
    assert!(errors.is_empty(), "fac/cape should not produce Go errors");
}

#[test]
fn go_fient_declaration_fails_closed_until_async_cursor_backend_lands() {
    let (program, interner) = parse_program("functio stream() fient → numerus { cede 1 }\n");
    let errors = collect_go_unsupported_errors(&program, "test.fab", &interner);
    assert!(
        errors
            .iter()
            .any(|e| e.issue() == Some("go_target_async_stream_unsupported")),
        "expected Go async-stream unsupported error, got: {errors:?}"
    );
}

/// U3: the fail-closed async-stream policy applies uniformly across every
/// declaration surface — class methods and interface methods reject with the
/// same `go_target_async_stream_unsupported` diagnostic as free functions.
#[test]
fn go_fient_class_and_interface_methods_fail_closed() {
    let (program, interner) = parse_program(
        "genus Genus {\n  functio stream() fient → numerus { cede 1 }\n}\nimplendum Virtus {\n  functio stream() fient → numerus\n}\n",
    );
    let errors = collect_go_unsupported_errors(&program, "test.fab", &interner);
    let issues = errors
        .iter()
        .filter(|e| e.issue() == Some("go_target_async_stream_unsupported"))
        .count();
    assert_eq!(
        issues, 2,
        "class method and interface method must both reject async streams, got: {errors:?}"
    );
}

#[test]
fn rust_incipit_only_has_no_unsupported_errors() {
    let (program, _) = parse_program("incipit { tacet }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "bare incipit should have no Rust unsupported errors");
}

#[test]
fn rust_unsupported_errors_are_empty_on_empty_entry_program() {
    let (program, _) = parse_program("functio f() → vacuum { }\nincipit { }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(errors.is_empty(), "trivial entry should have no Rust unsupported errors");
}

#[test]
fn rust_multiple_cape_constructs_are_not_reported() {
    let (program, _) = parse_program("functio f() → vacuum { elige 1 { casu 1 { tacet } } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    let cape_count = errors
        .iter()
        .filter(|e| e.issue() == Some("rust_target_cape_unsupported"))
        .count();
    assert_eq!(cape_count, 0, "elige cape should emit; got {cape_count}: {errors:?}");
}

#[test]
fn rust_target_cape_unsupported_has_no_remaining_sites() {
    let source = include_str!("target_policy.rs");
    let count = source.matches("rust_target_cape_unsupported").count();
    assert_eq!(
        count, 0,
        "TARGET001 rust_target_cape_unsupported must be fully retired; {count} site(s) remain"
    );
}
