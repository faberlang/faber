//! Regression coverage for tensor indexing, arithmetic, reduction, and matmul typecheck rules.
//!
//! These tests lock the typecheck behaviour for tensor element access (`accipe`),
//! shape inference (`structa` / `crea`), elementwise arithmetic, broadcast
//! unification, reductions (`summa` / `media`), and matrix multiplication.

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
            .any(|d| d.is_error() && d.issue() == Some(issue)),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

#[test]
fn tensor_structa_infers_shape_from_literal_shape_binding() {
    accepts(
        r"incipit {
  fixum lista<fractus<f32>> flat ← [1.0, 2.0]
  fixum lista<numerus> shape ← [2, 1]
  fixum tensor<fractus<f32>, _> seed ← vacua
  fixum tensor<fractus<f32>, _> grid ← seed.strue(flat, shape)
  nota grid.longitudo()
}",
    );
}

#[test]
fn tensor_forma_infers_output_shape() {
    accepts(
        r"incipit {
  fixum tensor<fractus<f32>, [2, 2]> grid ← vacua
  fixum tensor<fractus<f32>, _> reshaped ← grid.forma([4, 1])
  nota reshaped.longitudo()
}",
    );
}

#[test]
fn tensor_sugar_tf32_typechecks_with_inferred_shape() {
    accepts(
        r"incipit {
  fixum lista<fractus<f32>> flat ← [1.0, 2.0]
  fixum lista<numerus> shape ← [2, 1]
  fixum tf32 seed ← vacua
  sit grid ← seed.strue(flat, shape)
  nota grid.longitudo()
}",
    );
}

#[test]
fn tensor_shape_with_nested_infer_hole_rejects() {
    rejects_with_issue(
        r"incipit {
  fixum tensor<textus, [_, 3]> grid ← vacua
  nota grid.longitudo()
}",
        "cannot_infer_tensor_shape",
    );
}

#[test]
fn sparsa_sugar_typechecks_with_intrinsics() {
    accepts(
        r"incipit {
  varia sf32[2, 3] grid ← vacua
  grid.ponde([0, 1], 4.0)
  fixum f32 stored ← grid.accipe([0, 1])
  fixum numerus count ← grid.nonnihil()
  fixum tf32[2, 3] dense ← grid.densata()
  nota stored, count, dense.longitudo()
}",
    );
}

#[test]
fn standalone_f32_width_typechecks() {
    accepts(
        r"incipit {
  fixum f32 y ← 1.0
  nota y
}",
    );
}

#[test]
fn lista_f32_literal_uses_declared_element_width() {
    accepts(
        r"incipit {
  fixum lista<f32> flat ← [1.0, 2.0]
  fixum tf32[] seed ← vacua
  fixum tf32[1, 2] grid ← seed.strue(flat, [1, 2])
  fixum f32 first ← grid.accipe([0, 0]) vel 0.0
  nota first
}",
    );
}

#[test]
fn tensor_sectio_infers_axis_zero_slice_shape() {
    accepts(
        r"incipit {
  fixum tensor<numerus, [3, 2]> grid ← vacua
  fixum tensor<numerus, _> slice ← grid.sectio(1, 3)
  nota slice.longitudo()
}",
    );
}

// --- tensor arithmetic family (Commit A: elementwise broadcast) ---

#[test]
fn tensor_arithmetic_addita_same_shape_typechecks() {
    accepts(
        r"incipit {
  fixum tf32[2] a ← vacua
  fixum tf32[2] b ← vacua
  fixum tf32[2] c ← a.addita(b)
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_arithmetic_subtrahe_multiplica_typecheck() {
    accepts(
        r"incipit {
  fixum tf32[2] a ← vacua
  fixum tf32[2] b ← vacua
  fixum tf32[2] c ← a.subtrahe(b)
  fixum tf32[2] d ← c.multiplica(b)
  nota d.longitudo()
}",
    );
}

#[test]
fn tensor_arithmetic_broadcast_unifies_result_shape() {
    // [2] + [2, 1] must resolve to a [2, 2] result shape via broadcast unification.
    accepts(
        r"incipit {
  fixum tf32[2] a ← vacua
  fixum tf32[2, 1] b ← vacua
  fixum tf32[2, 2] c ← a.addita(b)
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_arithmetic_broadcast_incompatible_shape_rejects() {
    // [3] + [2] has no broadcast merge; result shape must be rejected.
    rejects_with_issue(
        r"incipit {
  fixum tf32[3] a ← vacua
  fixum tf32[2] b ← vacua
  fixum tf32[3] c ← a.addita(b)
  nota c.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_arithmetic_textus_element_rejects() {
    rejects_with_issue(
        r"incipit {
  fixum tensor<textus, [2]> a ← vacua
  fixum tensor<textus, [2]> b ← vacua
  fixum tensor<textus, [2]> c ← a.addita(b)
  nota c.longitudo()
}",
        "tensor_arithmetic_numeric_element_required",
    );
}

#[test]
fn tensor_arithmetic_mixed_width_element_rejects() {
    // numerus<i32> + numerus<i64> is a ↦ problem, not silent promotion.
    rejects_with_issue(
        r"incipit {
  fixum tensor<numerus<i32>, [2]> a ← vacua
  fixum tensor<numerus<i64>, [2]> b ← vacua
  fixum tensor<numerus<i32>, [2]> c ← a.addita(b)
  nota c.longitudo()
}",
        "argument_type_mismatch",
    );
}

// --- tensor reductions (Commit B: summa / media) ---

#[test]
fn tensor_reduction_summa_returns_element_type() {
    accepts(
        r"incipit {
  fixum tf32[2, 3] grid ← vacua
  fixum f32 total ← grid.summa()
  nota total
}",
    );
}

#[test]
fn tensor_reduction_summa_integer_tensor_returns_numerus() {
    accepts(
        r"incipit {
  fixum tensor<numerus, [3]> grid ← vacua
  fixum numerus total ← grid.summa()
  nota total
}",
    );
}

#[test]
fn tensor_reduction_summa_on_numerus_i32_tensor() {
    // EDGE: explicit-width integer tensor summa keeps the element width.
    accepts(
        r"incipit {
  fixum tensor<numerus<i32>, [3]> grid ← vacua
  fixum numerus<i32> total ← grid.summa()
  nota total
}",
    );
}

#[test]
fn tensor_reduction_media_numerus_family_returns_f64() {
    // WHY: media over an integer tensor must reduce to fractus<f64> so integer
    // division never silently truncates the mean.
    accepts(
        r"incipit {
  fixum tensor<numerus, [3]> grid ← vacua
  fixum f64 mean ← grid.media()
  nota mean
}",
    );
}

#[test]
fn tensor_reduction_media_fractus_f32_keeps_width() {
    accepts(
        r"incipit {
  fixum tf32[2, 3] grid ← vacua
  fixum f32 mean ← grid.media()
  nota mean
}",
    );
}

#[test]
fn tensor_reduction_media_numerus_rejects_integer_return() {
    // media never returns an integer type; binding to numerus must fail.
    rejects_with_issue(
        r"incipit {
  fixum tensor<numerus, [3]> grid ← vacua
  fixum numerus mean ← grid.media()
  nota mean
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn tensor_reduction_textus_element_rejects() {
    rejects_with_issue(
        r"incipit {
  fixum tensor<textus, [2]> grid ← vacua
  fixum textus total ← grid.summa()
  nota total
}",
        "tensor_arithmetic_numeric_element_required",
    );
}

// --- tensor matmul (Commit C: linear algebra) ---

#[test]
fn tensor_matmul_same_inner_dim_typechecks() {
    // [2,3] × [3,4] → [2,4]. All dims static.
    accepts(
        r"incipit {
  fixum tensor<numerus, [2, 3]> a ← vacua
  fixum tensor<numerus, [3, 4]> b ← vacua
  fixum tensor<numerus, [2, 4]> c ← a.matmul(b)
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_matmul_static_result_shape() {
    // Verify the result shape is [M,N] concretely, not a fresh hole.
    accepts(
        r"incipit {
  fixum tf32[2, 3] a ← vacua
  fixum tf32[3, 4] b ← vacua
  fixum tf32[2, 4] c ← a.matmul(b)
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_matmul_rank_mismatch_rejects() {
    // [2] × [2,3] — receiver is rank-1, rejected.
    rejects_with_issue(
        r"incipit {
  fixum tensor<numerus, [2]> a ← vacua
  fixum tensor<numerus, [2, 3]> b ← vacua
  fixum tensor<numerus, [1, 3]> c ← a.matmul(b)
  nota c.longitudo()
}",
        "matmul_receiver_rank_must_be_two",
    );
}

#[test]
fn tensor_matmul_rejects_rhs_rank_mismatch() {
    // [2,3] × [3] — RHS is rank-1, rejected.
    rejects_with_issue(
        r"incipit {
  fixum tensor<numerus, [2, 3]> a ← vacua
  fixum tensor<numerus, [3]> b ← vacua
  fixum tensor<numerus, [2, 1]> c ← a.matmul(b)
  nota c.longitudo()
}",
        "matmul_argument_rank_must_be_two",
    );
}

#[test]
fn tensor_matmul_inner_dim_mismatch_rejects() {
    // [2,3] × [4,5] — inner dims 3 ≠ 4, rejected.
    rejects_with_issue(
        r"incipit {
  fixum tensor<numerus, [2, 3]> a ← vacua
  fixum tensor<numerus, [4, 5]> b ← vacua
  fixum tensor<numerus, [2, 5]> c ← a.matmul(b)
  nota c.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_matmul_textus_element_rejects() {
    rejects_with_issue(
        r"incipit {
  fixum tensor<textus, [2, 3]> a ← vacua
  fixum tensor<textus, [3, 4]> b ← vacua
  fixum tensor<textus, [2, 4]> c ← a.matmul(b)
  nota c.longitudo()
}",
        "tensor_arithmetic_numeric_element_required",
    );
}

// --- symbolic shape contracts (Phase 2A: magnitudo shape params) ---
// These pin the shape-changing contract surface consuming `IndexExpr::Param`
// dims in generic bodies (the `sdpa-full` / `broadcast-symbolic` /
// `reduce-symbolic` repro shapes).

#[test]
fn tensor_matmul_symbolic_params_contract_inner_dimension() {
    // [M,K] × [K,N] → [M,N] with magnitudo Param dims in a generic body.
    accepts(
        r#"functio matmul_sym<magnitudo M, magnitudo K, magnitudo N>(
    tensor<f32, [M, K]> a, tensor<f32, [K, N]> b
) → tensor<f32, [M, N]> {
    redde a.matmul(b)
}

nota "symbolic matmul body ok"
"#,
    );
}

#[test]
fn tensor_transpone_symbolic_swaps_rank_two_dims() {
    // [R,C] → [C,R] with Param dims.
    accepts(
        r#"functio transpose_sym<magnitudo R, magnitudo C>(tensor<f32, [R, C]> grid) → tensor<f32, [C, R]> {
    redde grid.transpone()
}

nota "symbolic transpose body ok"
"#,
    );
}

#[test]
fn tensor_transpone_rank_one_symbolic_is_identity() {
    // Rank-1 transpose is identity: [R] → [R].
    accepts(
        r#"functio transpose_rank1_sym<magnitudo R>(tensor<f32, [R]> v) → tensor<f32, [R]> {
    redde v.transpone()
}

nota "rank-one transpose body ok"
"#,
    );
}

#[test]
fn tensor_broadcast_symbolic_stretches_size_one_dim() {
    // [B,1] + [B,D] → [B,D] via broadcast merge with Param dims.
    accepts(
        r#"functio bias_add_sym<magnitudo B, magnitudo D>(tensor<f32, [B, D]> grid, tensor<f32, [B, 1]> bias) → tensor<f32, [B, D]> {
    redde grid.addita(bias)
}

nota "symbolic broadcast body ok"
"#,
    );
}

#[test]
fn tensor_elementwise_symbolic_same_param_shapes() {
    // Same-param elementwise: [B,D] multiplica [B,D] → [B,D].
    accepts(
        r#"functio scale_sym<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a.multiplica(b)
}

nota "same-param elementwise body ok"
"#,
    );
}

#[test]
fn tensor_reduction_symbolic_returns_element_type() {
    // Scalar-return reduce over a rank-agnostic symbolic shape.
    accepts(
        r#"functio totum_sym<magnitudo Figura>(tensor<f32, Figura> grid) → f32 {
    redde grid.summa()
}

nota "symbolic reduce body ok"
"#,
    );
}

#[test]
fn tensor_reduction_media_symbolic_returns_float() {
    accepts(
        r#"functio media_sym<magnitudo Figura>(tensor<f32, Figura> grid) → f32 {
    redde grid.media()
}

nota "symbolic media body ok"
"#,
    );
}

#[test]
fn tensor_matmul_rank_agnostic_param_rejects() {
    // A whole-shape param into matmul has no rank-2 tuple: clean rejection.
    rejects_with_issue(
        r#"functio bad<magnitudo Figura>(tensor<f32, Figura> a, tensor<f32, Figura> b) → tensor<f32, Figura> {
    redde a.matmul(b)
}

nota "rankagnostic ok"
"#,
        "matmul_receiver_shape_must_be_tuple",
    );
}
