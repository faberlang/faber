//! Regression coverage for atomic intrinsic method typecheck rules.
//!
//! These tests prove that `atomic<T>` receivers accept load, store, exchange,
//! and compare_exchange with matching argument types, and that non-atomic
//! receivers or unsupported method names produce targeted diagnostics.

use radix_diagnostics::DiagnosticArg;

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted"
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

#[test]
fn atomic_integer_methods_typecheck() {
    accepts(
        r"
functio atomic_ops(in atomic<i32> cell, i32 value) → bivalens {
    sit i32 loaded ← cell.load()
    cell.store(value)
    sit i32 old ← cell.exchange(loaded)
    redde cell.compare_exchange(old, value)
}
",
    );
}

#[test]
fn atomic_methods_reject_argument_mismatch() {
    rejects_with_issue(
        r#"
functio atomic_ops(in atomic<u32> cell) → vacuum {
    cell.store("bad")
}
"#,
        "argument_type_mismatch",
    );
}

#[test]
fn atomic_method_names_reject_scalar_receivers() {
    rejects_with_issue(
        r"
functio bad(i32 value) → i32 {
    redde value.load()
}
",
        "atomic_method_requires_atomic_receiver",
    );
}

#[test]
fn unsupported_atomic_methods_reject_loudly() {
    rejects_with_issue(
        r"
functio bad(atomic<i32> cell, i32 value) → vacuum {
    cell.add(value)
}
",
        "unsupported_atomic_method",
    );
}
