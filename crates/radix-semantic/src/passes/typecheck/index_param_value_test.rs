//! SGR-C1 → size-type Z1: a size parameter reads as a read-only int value.
//!
//! Per the size-type law (Z1 force, `docs/factory/size-type/goal.md`
//! §Proposal): a body-visible size parameter (`size`/`magnitudo`) names a
//! value — safe widening at use, so it reads as an `int` (la `numerus`)
//! anywhere in the body — and assignment to it is rejected. It stays legal
//! in the generic argument slot of `rope_norm<size d>(...)`; a type param `T`
//! still is not a value.

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

const ROPE_Q_PROBE: &str = r"functio rope_q<T, magnitudo D, magnitudo d, magnitudo P>(tensor<T, [D, d]> input, tensor<T, [P, d, 3]> rota, varia tensor<T, [D, d]> output) → vacuum {
    output ⇇ input.rope_norm<magnitudo d>(0, rota)
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
    // Z1 flip of the g12 pin: the size name now resolves as an int value, so
    // the failure moved from `name_not_value` to the dim slot's own named
    // rejection (the dim argument must be a literal dimension).
    rejects_with_issue(
        r"functio rotate<magnitudo d>(tf32[2, d] x) → tf32[2, d] {
    redde x.rope_norm(5, d)
}",
        "rope_norm_dim_required",
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
    // Z1 flip of the g12 pin: `d + 1` is the force at use — the size name
    // widens safely to int, so arithmetic accepts.
    accepts(
        r"functio bad<magnitudo d>(tensor<f32, [8, d]> input) → numerus {
    redde d + 1
}",
    );
}

#[test]
fn size_param_reads_as_int_value() {
    // Force: `W` reads as an int in the body (safe widening at use).
    accepts(
        r"functio width<magnitudo H, magnitudo W>(tensor<tf32, [H, W]> t) → numerus {
    redde W
}",
    );
    // Read-only: assignment to a size name rejects.
    rejects_with_issue(
        r"functio bad<magnitudo H, magnitudo W>(tensor<tf32, [H, W]> t) → vacuum {
    H ← 1
}",
        "assignment_to_size_param",
    );
    // A type param in value position still gets name_not_value.
    rejects_with_issue(
        r"functio bad<T>(tensor<T, [2]> x) → numerus {
    redde T
}",
        "name_not_value",
    );
}

#[test]
fn size_param_at_non_index_intrinsic_slot_still_rejects() {
    // Live `divisa` takes a TensorPeer operand, not an index: the size name
    // reads as an int, so the tensor slot rejects it on argument type.
    rejects_with_issue(
        r"functio bad<T, magnitudo d>(tensor<T, [8, d]> input) → tensor<T, [8, d]> {
    redde input.divisa(d)
}",
        "argument_type_mismatch",
    );
}
