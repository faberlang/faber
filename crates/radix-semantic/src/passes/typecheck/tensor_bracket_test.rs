//! Tensor bracket access: a read yields `T` or traps (like a list index);
//! a write is `ponde` sugar.

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
            .any(|diagnostic| diagnostic.issue() == Some(issue)),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

#[test]
fn tensor_bracket_read_accepts_rank1_scalar_index() {
    accepts(
        r"functio pick(tf32[16] input, numerus id) → f32 {
    redde input[id]
}",
    );
}

#[test]
fn tensor_bracket_write_accepts_rank1_scalar_index() {
    accepts(
        r"functio put(varia tf32[16] output, numerus id, f32 value) → vacuum {
    output[id] ← value
}",
    );
}

#[test]
fn tensor_bracket_write_rhs_is_element_not_option() {
    accepts(
        r"functio put(varia tf32[16] output, numerus id) → vacuum {
    output[id] ← 1.0
}",
    );
}

#[test]
fn tensor_bracket_read_is_element_for_arithmetic() {
    accepts(
        r"functio plus(tf32[16] input, numerus id) → f32 {
    redde input[id] + 1.0
}",
    );
}

#[test]
fn tensor_accipe_method_is_failable_with_a_default() {
    accepts(
        r"functio pick(tf32[16] input, numerus id) → f32 {
    redde input.accipe([id]) ⊥ 0.0
}",
    );
}

#[test]
fn tensor_bracket_accepts_rank2_list_literal_index() {
    accepts(
        r"functio put(varia tf32[4, 4] grid, numerus r, numerus c, f32 value) → f32 {
    grid[[r, c]] ← value
    redde grid[[r, c]]
}",
    );
}

#[test]
fn tensor_bracket_accepts_bound_u32_index_list() {
    accepts(
        r"functio pick(tf32[4, 4] grid, u32 r, u32 c) → f32 {
    sit lista<u32> origin ← [r, c]
    redde grid[origin]
}",
    );
}

#[test]
fn tensor_bracket_accepts_u32_scalar_on_rank1() {
    accepts(
        r"functio pick(tf32[16] input, u32 id) → f32 {
    redde input[id]
}",
    );
}

#[test]
fn tensor_bracket_rejects_scalar_on_rank2() {
    rejects_with_issue(
        r"functio bad(tf32[4, 4] grid, numerus id) → f32 {
    redde grid[id]
}",
        "tensor_scalar_index_rank",
    );
}

#[test]
fn tensor_bracket_rejects_float_index_list() {
    rejects_with_issue(
        r"functio bad(tf32[4, 4] grid, lista<f32> origin) → f32 {
    redde grid[origin]
}",
        "tensor_index_vector_integer_elements",
    );
}

#[test]
fn tensor_bracket_rejects_u64_scalar_index() {
    rejects_with_issue(
        r"functio bad(tf32[16] input, u64 id) → f32 {
    redde input[id]
}",
        "tensor_bracket_index_integer_or_lista",
    );
}

#[test]
fn matrix_bracket_read_yields_element() {
    accepts(
        r"functio cell(matrix<f32, [2, 2]> m, numerus r, numerus c) → f32 {
    redde m[[r, c]]
}",
    );
}

#[test]
fn matrix_bracket_write_accepts_coordinate() {
    accepts(
        r"functio put(varia matrix<f32, [2, 2]> m, numerus r, numerus c, f32 v) → vacuum {
    m[[r, c]] ← v
}",
    );
}

#[test]
fn matrix_bracket_rejects_scalar_index() {
    rejects_with_issue(
        r"functio bad(matrix<f32, [2, 2]> m, numerus r) → f32 {
    redde m[r]
}",
        "tensor_scalar_index_rank",
    );
}

#[test]
fn matrix_bracket_rejects_float_coordinate() {
    rejects_with_issue(
        r"functio bad(matrix<f32, [2, 2]> m) → f32 {
    redde m[[1.5, 0]]
}",
        "tensor_index_vector_integer_elements",
    );
}

#[test]
fn tensor_bracket_range_literal_out_of_range_rejects() {
    rejects_with_issue(
        r"functio bad(tf32[4] t) → tf32[4] {
    redde t[3 ‥ 7]
}",
        "index_provably_out_of_bounds",
    );
}

#[test]
fn matrix_bracket_rejects_literal_out_of_range() {
    rejects_with_issue(
        r"functio bad(matrix<f32, [2, 2]> m) → f32 {
    redde m[[2, 0]]
}",
        "index_provably_out_of_bounds",
    );
}

#[test]
fn tensor_bracket_range_rank1_accepts() {
    accepts(
        r"functio lega(tf32[4] t) → tf32[2] {
    sit tf32[2] s ← t[1 ‥ 3]
    redde s
}",
    );
    accepts(
        r"functio lega(tf32[4] t) → tf32[2] {
    sit tf32[2] s ← t[1 … 2]
    redde s
}",
    );
}

#[test]
fn tensor_bracket_range_rank2_scalar_range_rejects() {
    rejects_with_issue(
        r"functio bad(tf32[2, 2] g) → tf32[1] {
    redde g[0 ‥ 1]
}",
        "tensor_scalar_index_rank",
    );
}

#[test]
fn tensor_bracket_range_step_accepted() {
    accepts(
        r"functio passus(tf32[4] t) → tf32[2] {
    sit tf32[2] s ← t[0 ‥ 4 per 2]
    redde s
}",
    );
}
