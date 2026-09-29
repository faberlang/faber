//! DEFER-064: tensor index intrinsics accept integer-width index lists.
//!
//! `accipe` / `ponde` (and the other `NumerusArray` slots `forma` / `crea` /
//! `structa`) accept integer lists that fit the runtime `i64` boundary, so
//! GPU/KIR source can pass `lista<u32>` index vectors without a bare-`numerus`
//! detour. `u64` and non-integer lists are rejected with a deliberate
//! diagnostic.

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
        diagnostics.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected a diagnostic issue `{issue}`, got: {diagnostics:?}"
    );
}

const REJECT_ISSUE: &str = "tensor_index_vector_integer_elements";

#[test]
fn accipe_accepts_lista_u32_bound_index() {
    accepts(
        r"functio check(tf32[16] input, u32 id) → f32 {
    fixum lista<u32> idx ← [id]
    redde input.accipe(idx) vel 0.0
}",
    );
}

#[test]
fn accipe_accepts_lu32_lista_sugar_index() {
    accepts(
        r"functio check(tf32[16] input, u32 id) → f32 {
    fixum lu32 idx ← [id]
    redde input.accipe(idx) vel 0.0
}",
    );
}

#[test]
fn accipe_accepts_inline_u32_index_literal() {
    accepts(
        r"functio check(tf32[16] input, u32 id) → f32 {
    redde input.accipe([id]) vel 0.0
}",
    );
}

#[test]
fn accipe_accepts_lista_numerus_u32_spelling() {
    accepts(
        r"functio check(tf32[16] input, u32 id) → f32 {
    fixum lista<numerus<u32>> idx ← [id]
    redde input.accipe(idx) vel 0.0
}",
    );
}

#[test]
fn accipe_accepts_lista_i32_index() {
    accepts(
        r"functio check(tf32[16] input, numerus<i32> id) → f32 {
    redde input.accipe([id]) vel 0.0
}",
    );
}

#[test]
fn ponde_accepts_lista_u32_index() {
    accepts(
        r"functio put(tf32[16] output, u32 id, f32 v) → vacuum {
    fixum lista<u32> idx ← [id]
    output.ponde(idx, v)
}",
    );
}

#[test]
fn accipe_rank2_accepts_lista_u32_origin() {
    accepts(
        r"functio pick(tf32[4, 4] grid, u32 r, u32 c) → f32 {
    fixum lista<u32> origin ← [r, c]
    redde grid.accipe(origin) vel 0.0
}",
    );
}

#[test]
fn forma_accepts_inline_shape_literal() {
    accepts(
        r"incipit {
    fixum tf32 seed ← vacua
    fixum tf32 reshaped ← seed.forma([2, 2])
    nota reshaped.longitudo()
}",
    );
}

#[test]
fn accipe_rejects_float_index_list() {
    rejects_with_issue(
        r"functio bad(tf32[16] input, lista<f32> idx) → f32 {
    redde input.accipe(idx) vel 0.0
}",
        REJECT_ISSUE,
    );
}

#[test]
fn accipe_rejects_textus_index_list() {
    rejects_with_issue(
        r"functio bad(tf32[16] input, lista<textus> idx) → f32 {
    redde input.accipe(idx) vel 0.0
}",
        REJECT_ISSUE,
    );
}

#[test]
fn accipe_rejects_scalar_non_list_index() {
    rejects_with_issue(
        r"functio bad(tf32[16] input, textus idx) → f32 {
    redde input.accipe(idx) vel 0.0
}",
        REJECT_ISSUE,
    );
}

#[test]
fn accipe_rejects_lista_u64_index() {
    rejects_with_issue(
        r"functio bad(tf32[16] input, u64 id) → f32 {
    fixum lista<u64> idx ← [id]
    redde input.accipe(idx) vel 0.0
}",
        REJECT_ISSUE,
    );
}

#[test]
fn accipe_accepts_empty_index_list() {
    accepts(
        r"functio check(tf32[16] input) → f32 {
    fixum lista<u32> idx ← []
    redde input.accipe(idx) vel 0.0
}",
    );
}

/// The empty literal is admitted inline: `accipe([])` is part of the shared
/// `NumerusArray` policy (an empty vector has no elements that could violate
/// the i64 boundary; its element type resolves to the slot's canonical
/// `numerus`).
#[test]
fn accipe_accepts_empty_inline_index_literal() {
    accepts(
        r"functio check(tf32[16] input) → f32 {
    redde input.accipe([]) vel 0.0
}",
    );
}

#[test]
fn accipe_rejects_bool_index_list() {
    rejects_with_issue(
        r"functio bad(tf32[16] input) → f32 {
    fixum lista<bivalens> idx ← [verum]
    redde input.accipe(idx) vel 0.0
}",
        REJECT_ISSUE,
    );
}

#[test]
fn ponde_rejects_float_index_list() {
    rejects_with_issue(
        r"functio bad(tf32[16] output, lista<f32> idx, f32 v) → vacuum {
    output.ponde(idx, v)
}",
        REJECT_ISSUE,
    );
}

/// WHY: the structural exception must not widen the global numeric lattice.
/// d11-6 U2: `u32` widens losslessly into `numerus`, so the binding is
/// accepted; `u64` has no implicit signed target and stays a type error.
#[test]
fn numerus_list_binding_accepts_u32_element() {
    accepts(
        r"functio check(tf32[16] input, u32 id) → f32 {
    fixum lista<numerus> idx ← [id]
    redde input.accipe(idx) vel 0.0
}",
    );
}

#[test]
fn numerus_list_binding_still_rejects_u64_element() {
    rejects_with_issue(
        r"functio check(tf32[16] input, u64 id) → f32 {
    fixum lista<numerus> idx ← [id]
    redde input.accipe(idx) vel 0.0
}",
        "initializer_annotation_mismatch",
    );
}

// ── CCI-1: the three common-case intrinsics typecheck ───────────────────

#[test]
fn rms_norm_typechecks_with_axis_epsilon_and_gamma_peer() {
    accepts(
        r"functio norm(tf32[2, 8] x, tf32[8] gamma) → tf32[2, 8] {
    redde x.rms_norm(1, 1e-5, gamma)
}",
    );
}

#[test]
fn rope_norm_typechecks_with_generic_dim_and_pos_argument() {
    accepts(
        r"functio rotate(tf32[2, 8] x, tf32[2, 4, 3] table) → tf32[2, 8] {
    redde x.rope_norm<8>(5, table)
}",
    );
}

#[test]
fn gather_typechecks_with_literal_and_bound_id_vectors() {
    accepts(
        r"functio select(tf32[4, 3] table) → tf32[4, 3] {
    fixum lista<numerus> ids ← [3, 0, 2, 0]
    redde table.gather(ids)
}",
    );
    accepts(
        r"functio select(tf32[4, 3] table) → tf32[4, 3] {
    redde table.gather([3, 0, 2, 0])
}",
    );
}

/// The empty id vector is spellable inline — `table.gather([])` typechecks
/// under the shared `NumerusArray` policy (the frozen contract's empty case).
#[test]
fn gather_accepts_empty_literal() {
    accepts(
        r"functio select(tf32[4, 3] table) → tf32[4, 3] {
    redde table.gather([])
}",
    );
}

#[test]
fn gather_result_shape_unifies_with_the_declared_t_by_d_result() {
    // The [T, last-of-receiver] result shape admits assignment to a
    // concrete [T, D] tensor at the type level; the static id-range contract
    // is enforced at the HIR→AIR construction site.
    accepts(
        r"functio select(tf32[4, 3] table) → tf32[4, 3] {
    fixum lista<numerus> ids ← [3, 0, 2, 0]
    fixum tensor<f32, [4, 3]> out ← table.gather(ids)
    redde out
}",
    );
}

#[test]
fn gather_rejects_non_list_id_argument() {
    rejects_with_issue(
        r"functio select(tf32[4, 3] table, numerus id) → tf32[4, 3] {
    redde table.gather(id)
}",
        REJECT_ISSUE,
    );
}
