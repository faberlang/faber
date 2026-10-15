//! `↦ regex` is a failable conversion (RX-2a, operator ruling RD-1).
//!
//! An invalid pattern fails to compile at run time, so the conversion follows
//! the ordinary text failure channel: outside `main` it needs a handler
//! (`fac`/`cape`, `capta`, or a function declaring `⇥ textus`); `main` is
//! exempt; `⊥` stays rejected because `regex` has no value to recover with.
//! Literal and dynamic sources take the same path (no literal fold, no
//! pattern length cap). Assertions are structured (code, issue, args).

use crate::test_support::{
    self, analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la,
};

const ISSUE: &str = "failable_conversio_requires_handler";

fn arg<'a>(err: &'a crate::SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn assert_unhandled(source: &str, expected_source: &str) {
    let result = analyze_source_la(source);
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
        .unwrap_or_else(|| panic!("expected {ISSUE}, got {:?}", result.diagnostics));
    assert_eq!(err.kind, crate::SemanticErrorKind::OperandTypesInvalid); // SEM011
    assert_eq!(arg(err, "source"), Some(expected_source));
    assert_eq!(arg(err, "target"), Some("regex"));
    assert!(test_support::args_have_no_legacy_message(&err.args));
}

#[test]
fn dynamic_pattern_in_plain_function_requires_handler() {
    assert_unhandled(
        "functio f(textus pat) → regex { redde pat ↦ regex }",
        "textus",
    );
}

#[test]
fn literal_pattern_in_plain_function_requires_handler() {
    assert_unhandled(r#"functio f() → regex { redde "\\d+" ↦ regex }"#, "textus");
}

#[test]
fn ascii_pattern_in_plain_function_requires_handler() {
    assert_rejects_with_issue_la(r"functio f() → regex { redde '\\d+' ↦ regex }", ISSUE);
}

#[test]
fn fac_cape_handles_dynamic_and_literal_patterns() {
    assert_accepts_la(
        r#"functio f(textus pat) → numerus {
    fac {
        sit a ← pat ↦ regex
        sit b ← "\\d+" ↦ regex
    } cape err {
        redde 0
    }
    redde 1
}"#,
    );
}

#[test]
fn textus_error_channel_handles_dynamic_and_literal_patterns() {
    assert_accepts_la(
        r#"functio f(textus pat) → regex ⇥ textus { redde pat ↦ regex }
functio g() → regex ⇥ textus { redde "\\d+" ↦ regex }"#,
    );
}

#[test]
fn incipit_frame_is_exempt() {
    assert_accepts_la(
        r#"incipit {
    sit textus pat ← "a+"
    sit a ← pat ↦ regex
    sit b ← "\\d+" ↦ regex
}"#,
    );
}

#[test]
fn recovery_slot_stays_rejected() {
    assert_rejects_with_issue_la(
        r#"incipit { sit textus p ← "a"
    sit r ← p ↦ regex ⊥ p ↦ regex }"#,
        "regex_recovery_unsupported",
    );
}
