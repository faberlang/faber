//! Tests for target-specific policy: unsupported construct detection and no-op warnings.
//!
//! Contract: each target rejects syntax it cannot represent; supported syntax passes without target diagnostics.

use super::*;
use crate::lexer::lex;
use crate::parser::parse;

/// Parse a source string into a program, or panic.
fn parse_program(source: &str) -> (Program, crate::lexer::Interner) {
    let result = parse(lex(source));
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
    let (program, _) = parse_program("functio f() → vacuum { fixum numerus x ← 1 }\n");
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
fn rust_si_with_cape_emits_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { si verum { } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .any(|e| e.issue() == Some("rust_target_cape_unsupported")),
        "expected rust_target_cape_unsupported error, got: {errors:?}"
    );
}

#[test]
fn rust_dum_with_cape_emits_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { dum verum { perge } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .any(|e| e.issue() == Some("rust_target_cape_unsupported")),
        "expected rust_target_cape_unsupported error, got: {errors:?}"
    );
}

#[test]
fn rust_elige_with_cape_emits_unsupported_error() {
    let (program, _) = parse_program("functio f() → vacuum { elige 1 { casu 1 { tacet } } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .any(|e| e.issue() == Some("rust_target_cape_unsupported")),
        "expected rust_target_cape_unsupported error, got: {errors:?}"
    );
}

#[test]
fn rust_mixed_supported_and_unsupported_constructs() {
    let (program, _) = parse_program("functio f() → vacuum { si verum { tacet } si verum { } cape err { tacet } }\n");
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    assert!(
        errors
            .iter()
            .any(|e| e.issue() == Some("rust_target_cape_unsupported")),
        "expected at least one unsupported error in mixed code, got: {errors:?}"
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

// ── Rust no-op warnings ─────────────────────────────────────────────────────

#[test]
fn rust_empty_program_has_no_noop_warnings() {
    let (program, _) = parse_program("");
    let warnings = collect_rust_noop_warnings(&program, "test.fab");
    assert!(warnings.is_empty(), "empty program should have no Rust no-op warnings");
}

#[test]
fn rust_simple_function_has_no_noop_warnings() {
    let (program, _) = parse_program("functio f() → vacuum { tacet }\n");
    let warnings = collect_rust_noop_warnings(&program, "test.fab");
    assert!(warnings.is_empty(), "simple function should have no Rust no-op warnings");
}

#[test]
fn rust_loop_without_cape_has_no_noop_warnings() {
    let (program, _) = parse_program("functio f() → vacuum { dum verum { perge } }\n");
    let warnings = collect_rust_noop_warnings(&program, "test.fab");
    assert!(warnings.is_empty(), "loop without cape should have no Rust no-op warnings");
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
fn rust_multiple_cape_constructs_each_reported() {
    // Two separate cape constructs: one in si, one in dum — both should be reported.
    let (program, _) = parse_program(
        "functio f() → vacuum { si verum { } cape err { tacet } dum verum { perge } cape err { tacet } }\n",
    );
    let errors = collect_rust_unsupported_errors(&program, "test.fab");
    let cape_count = errors
        .iter()
        .filter(|e| e.issue() == Some("rust_target_cape_unsupported"))
        .count();
    assert_eq!(
        cape_count, 2,
        "expected two cape unsupported errors, got {cape_count}: {errors:?}"
    );
}
