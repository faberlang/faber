//! SGR-C1: `magnitudo` parameters name values only at `NumerusIndex` slots.
//!
//! A body-visible size parameter (`size d` / `magnitudo d`) may appear as a
//! bare value argument exactly where an intrinsic declares an index slot
//! (`rope_norm(position, d)`); every other expression position keeps the
//! `SEM001` not-a-value rejection, and lowering types the admitted parameter
//! as the rigid symbolic-size marker so misplaced uses fail unification.

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
}

fn accepts(source: &str) {
    let result = compile(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

const ROPE_Q_PROBE: &str = r"functio rope_q<T, magnitudo D, magnitudo d, magnitudo P>(tensor<T, [D, d]> input, tensor<T, [P, d, 3]> rota, in tensor<T, [D, d]> output) → vacuum {
    output ← input.rope_norm(0, d)
}";

#[test]
fn rope_norm_accepts_body_size_param_at_index_slot() {
    accepts(ROPE_Q_PROBE);
}

#[test]
fn size_param_in_arithmetic_still_rejects() {
    rejects_with_issue(
        r"functio bad<magnitudo d>(tensor<f32, [8, d]> input) → f32 {
    redde d + 1
}",
        "name_not_value",
    );
}

#[test]
fn size_param_at_non_index_intrinsic_slot_still_rejects() {
    // `addita` takes a TensorPeer operand, not an index: the size parameter
    // keeps the not-a-value rejection.
    rejects_with_issue(
        r"functio bad<T, magnitudo d>(tensor<T, [8, d]> input) → tensor<T, [8, d]> {
    redde input.addita(d)
}",
        "name_not_value",
    );
}
