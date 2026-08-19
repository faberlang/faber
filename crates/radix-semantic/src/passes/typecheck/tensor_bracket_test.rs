//! DEFER-063: tensor bracket access is `accipe` / `ponde` sugar.

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
            .any(|diagnostic| diagnostic.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

#[test]
fn tensor_bracket_read_accepts_rank1_scalar_index() {
    accepts(
        r"functio pick(tf32[16] input, numerus id) → f32 {
    redde input[id] vel 0.0
}",
    );
}

#[test]
fn tensor_bracket_write_accepts_rank1_scalar_index() {
    accepts(
        r"functio put(tf32[16] output, numerus id, f32 value) → vacuum {
    output[id] ← value
}",
    );
}

#[test]
fn tensor_bracket_write_rhs_is_element_not_option() {
    accepts(
        r"functio put(tf32[16] output, numerus id) → vacuum {
    output[id] ← 1.0
}",
    );
}

#[test]
fn tensor_bracket_read_remains_nullable_for_host_arithmetic() {
    rejects_with_issue(
        r"functio plus(tf32[16] input, numerus id) → f32 {
    redde input[id] + 1.0
}",
        "numeric_operands_required",
    );
}

#[test]
fn tensor_bracket_accepts_rank2_list_literal_index() {
    accepts(
        r"functio put(tf32[4, 4] grid, numerus r, numerus c, f32 value) → f32 {
    grid[[r, c]] ← value
    redde grid[[r, c]] vel 0.0
}",
    );
}

#[test]
fn tensor_bracket_accepts_bound_u32_index_list() {
    accepts(
        r"functio pick(tf32[4, 4] grid, u32 r, u32 c) → f32 {
    fixum lista<u32> origin ← [r, c]
    redde grid[origin] vel 0.0
}",
    );
}

#[test]
fn tensor_bracket_accepts_u32_scalar_on_rank1() {
    accepts(
        r"functio pick(tf32[16] input, u32 id) → f32 {
    redde input[id] vel 0.0
}",
    );
}

#[test]
fn tensor_bracket_rejects_scalar_on_rank2() {
    rejects_with_issue(
        r"functio bad(tf32[4, 4] grid, numerus id) → f32 {
    redde grid[id] vel 0.0
}",
        "tensor_scalar_index_rank",
    );
}

#[test]
fn tensor_bracket_rejects_float_index_list() {
    rejects_with_issue(
        r"functio bad(tf32[4, 4] grid, lista<f32> origin) → f32 {
    redde grid[origin] vel 0.0
}",
        "tensor_index_vector_integer_elements",
    );
}

#[test]
fn tensor_bracket_rejects_u64_scalar_index() {
    rejects_with_issue(
        r"functio bad(tf32[16] input, u64 id) → f32 {
    redde input[id] vel 0.0
}",
        "tensor_bracket_index_integer_or_lista",
    );
}
