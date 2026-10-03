//! SGR-C1: `magnitudo` parameters name values only at `NumerusIndex` slots.
//!
//! A body-visible size parameter is a type-level witness. It is legal in the
//! generic argument slot of `rope_norm<d>(position)`, but it is not a runtime
//! value and must not be admitted as one by intrinsic argument lowering.

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
    output ⇇ input.rope_norm<d>(0, rota)
}";

#[test]
fn rope_norm_accepts_body_size_param_as_generic_argument() {
    accepts(ROPE_Q_PROBE);
}

#[test]
fn rope_norm_rejects_literal_dimension_in_runtime_value_slot() {
    rejects_with_issue(
        r"functio rotate(tf32[2, 8] x) → tf32[2, 8] {
    redde x.rope_norm(5, 8)
}",
        "rope_norm_dim_required",
    );
}

#[test]
fn rope_norm_rejects_size_name_in_runtime_value_slot() {
    rejects_with_issue(
        r"functio rotate<magnitudo d>(tf32[2, d] x) → tf32[2, d] {
    redde x.rope_norm(5, d)
}",
        "name_not_value",
    );
}

#[test]
fn rope_norm_rejects_type_argument_in_size_slot() {
    rejects_with_issue(
        r"functio rotate<T>(tf32[2, 8] x) → tf32[2, 8] {
    redde x.rope_norm<T>(5)
}",
        "generic_type_arg_sort",
    );
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
