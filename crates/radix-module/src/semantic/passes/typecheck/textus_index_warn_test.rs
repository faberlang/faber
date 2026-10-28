//! BB-9 pins: warn only for scalar indexing of unbounded `textus`.

use super::test_support::analyze_and_get_diagnostics_la;
use crate::diagnostics::{Diagnostic, DiagnosticArg};

const WARNING_CODE: &str = "WARN022";
const WARNING_ISSUE: &str = "unbounded_textus_index";

fn warning_for(source: &str) -> Diagnostic {
    let diagnostics = analyze_and_get_diagnostics_la(source);
    let warning = diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.code == Some(WARNING_CODE) && diagnostic.issue() == Some(WARNING_ISSUE))
        .unwrap_or_else(|| panic!("expected {WARNING_CODE}/{WARNING_ISSUE}, got diagnostics"));
    assert!(!warning.is_error(), "textus index guidance must remain a warning");
    assert_eq!(
        warning.args,
        vec![DiagnosticArg::new("issue", WARNING_ISSUE)],
        "warning must carry structured identity args, not rendered prose"
    );
    warning
}

fn assert_no_warning(source: &str) {
    let diagnostics = analyze_and_get_diagnostics_la(source);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == Some(WARNING_CODE) || diagnostic.issue() == Some(WARNING_ISSUE)),
        "did not expect {WARNING_CODE}/{WARNING_ISSUE}, got {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(Diagnostic::is_error),
        "fixture should remain valid, got {diagnostics:?}"
    );
}

#[test]
fn warns_on_unbounded_textus_scalar_index_with_structured_identity() {
    warning_for(
        r"functio probe(textus xs) → ascii<magnitudo 1> {
    redde xs[0]
}

incipit {
    nota 0
}",
    );
}

#[test]
fn does_not_warn_on_unbounded_ascii_scalar_index() {
    assert_no_warning(
        r"functio probe(ascii xs) → littera {
    redde xs[0]
}

incipit {
    nota 0
}",
    );
}

#[test]
fn does_not_warn_on_bounded_textus_or_ascii_scalar_index() {
    assert_no_warning(
        r"functio probe(textus<magnitudo 32> xs, ascii<magnitudo 32> ys) → ascii<magnitudo 1> {
    nota ys[0]
    redde xs[0]
}

incipit {
    nota 0
}",
    );
}

#[test]
fn does_not_warn_on_textus_range_slice() {
    assert_no_warning(
        r"functio probe(textus xs) → textus {
    redde xs[0‥1]
}

incipit {
    nota 0
}",
    );
}

#[test]
fn does_not_warn_when_iterating_textus_scalars() {
    assert_no_warning(
        r#"incipit {
    sit textus xs ← "ab"
    itera ex xs.cursor() sit scalar {
        nota scalar
    }
}"#,
    );
}
