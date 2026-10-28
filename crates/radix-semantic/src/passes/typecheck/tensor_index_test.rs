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
fn bounded_tensor_literal_indices_reject_provable_oob_and_negative() {
    rejects_with_issue(
        r"incipit {
  sit tf32[3] grid ← vacua
  sit _ bad ← grid.accipe([3])
  nota bad
}",
        "index_provably_out_of_bounds",
    );
    rejects_with_issue(
        r"incipit {
  sit tf32[3] grid ← vacua
  sit _ bad ← grid[-1]
  nota bad
}",
        "index_provably_out_of_bounds",
    );
}

#[test]
fn bounded_tensor_literal_indices_in_bounds_still_accept() {
    accepts(
        r"incipit {
  sit tf32[3] grid ← vacua
  sit _ good ← grid.accipe([2])
  sit _ sugar ← grid[1]
}",
    );
}

#[test]
fn tensor_param_and_dynamic_shapes_decline_static_oob_check() {
    accepts(
        r"functio read<magnitudo N>(tensor<f32, [N]> param, tf32[] dynamic) → vacuum {
  sit _ a ← param.accipe([9]) ⊥ 0.0
  sit _ b ← dynamic.accipe([9]) ⊥ 0.0
}",
    );
}

#[test]
fn tensor_structa_infers_shape_from_literal_shape_binding() {
    accepts(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0]
  sit lista<numerus> shape ← [2, 1]
  sit tensor<f32, _> grid ← flat ↦ tensor<f32, shape>
  nota grid.longitudo()
}",
    );
}

#[test]
fn tensor_forma_infers_output_shape() {
    accepts(
        r"incipit {
  sit tensor<f32, [2, 2]> grid ← vacua
  sit tensor<f32, _> reshaped ← grid.forma([4, 1])
  nota reshaped.longitudo()
}",
    );
}

#[test]
fn expression_shape_hole_is_admitted_in_reshape_shape_slot() {
    accepts(
        r"incipit {
  sit tensor<f32, [2, 2]> grid ← vacua
  sit tensor<f32, _> reshaped ← grid.forma([_, 4])
  nota reshaped.longitudo()
}",
    );
}

#[test]
fn expression_shape_hole_is_rejected_outside_shape_argument_slots() {
    rejects_with_issue(
        r"incipit {
  sit _ values ← [_, 4]
}",
        "tensor_shape_hole_requires_shape_argument",
    );
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [2]> grid ← vacua
  sit _ value ← grid.accipe([_])
}",
        "tensor_shape_hole_requires_shape_argument",
    );
}

#[test]
fn tensor_sugar_tf32_typechecks_with_inferred_shape() {
    accepts(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0]
  sit lista<numerus> shape ← [2, 1]
  sit _ grid ← flat ↦ tensor<f32, shape>
  nota grid.longitudo()
}",
    );
}

#[test]
fn tensor_shape_with_nested_infer_hole_rejects() {
    rejects_with_issue(
        r"incipit {
  sit tensor<textus, [_, 3]> grid ← vacua
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
  sit f32 stored ← grid.accipe([0, 1])
  sit numerus count ← grid.nonnihil()
  sit tf32[2, 3] dense ← grid.densata()
  nota stored, count, dense.longitudo()
}",
    );
}

#[test]
fn standalone_f32_width_typechecks() {
    accepts(
        r"incipit {
  sit f32 y ← 1.0
  nota y
}",
    );
}

#[test]
fn lista_f32_literal_uses_declared_element_width() {
    accepts(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0]
  sit tf32[1, 2] grid ← flat ↦ tf32[1, 2]
  sit f32 first ← grid.accipe([0, 0])
  nota first
}",
    );
}

#[test]
fn tensor_sectio_infers_axis_zero_slice_shape() {
    accepts(
        r"incipit {
  sit tensor<numerus, [3, 2]> grid ← vacua
  sit tensor<numerus, _> slice ← grid.sectio(1, 3)
  nota slice.longitudo()
}",
    );
}

#[test]
fn tensor_sectio_step_infers_strided_axis_zero_shape() {
    accepts(
        r"incipit {
  sit tensor<numerus, [8, 2]> grid ← vacua
  sit tensor<numerus, [4, 2]> every_other ← grid.sectio(0, 8, 2)
  nota every_other.longitudo()
}",
    );
}

#[test]
fn tensor_expande_allows_inserted_and_singleton_stretch_axes() {
    accepts(
        r"incipit {
  sit tensor<numerus, [3]> row ← vacua
  sit tensor<numerus, [1, 3]> inserted ← row.expande([1, 3])
  sit tensor<numerus, [4, 3]> broadcast ← inserted.expande([4, 3])
  nota broadcast.longitudo()
}",
    );
}

#[test]
fn tensor_expande_rejects_incompatible_non_singleton_axis() {
    rejects_with_issue(
        r"incipit {
  sit tensor<numerus, [2, 3]> grid ← vacua
  sit tensor<numerus, [4, 3]> invalid ← grid.expande([4, 3])
  nota invalid.longitudo()
}",
        "incompatible_tensor_index",
    );
}

// --- tensor arithmetic glyphs (exact shape, explicit expansion) ---

#[test]
fn tensor_arithmetic_plus_same_shape_typechecks() {
    accepts(
        r"incipit {
  sit tf32[2] a ← vacua
  sit tf32[2] b ← vacua
  sit tf32[2] c ← a + b
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_arithmetic_minus_hadamard_typecheck() {
    accepts(
        r"incipit {
  sit tf32[2] a ← vacua
  sit tf32[2] b ← vacua
  sit tf32[2] c ← a - b
  sit tf32[2] d ← c ⊙ b
  nota d.longitudo()
}",
    );
}

#[test]
fn tensor_arithmetic_expanded_operands_preserve_result_shape() {
    // Expand both [2] and [2, 1] explicitly before exact-shape addition.
    accepts(
        r"incipit {
  sit tf32[2] a ← vacua
  sit tf32[2, 1] b ← vacua
  sit tf32[2, 2] c ← a.expande([2, 2]) + b.expande([2, 2])
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_arithmetic_incompatible_expansion_rejects() {
    // [2] cannot expand to [3]; the attempted shape repair must reject.
    rejects_with_issue(
        r"incipit {
  sit tf32[3] a ← vacua
  sit tf32[2] b ← vacua
  sit tf32[3] c ← a + b.expande([3])
  nota c.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_arithmetic_textus_element_rejects() {
    rejects_with_issue(
        r"incipit {
  sit tensor<textus, [2]> a ← vacua
  sit tensor<textus, [2]> b ← vacua
  sit tensor<textus, [2]> c ← a + b
  nota c.longitudo()
}",
        "tensor_arithmetic_numeric_element_required",
    );
}

#[test]
fn tensor_arithmetic_mixed_width_result_narrows_at_the_store() {
    // Glyph addition joins i32 and i64 elements to i64. Under ruling Q-X3
    // (2026-10-15, need; numeric-model.md section 5) the
    // tensor<i32, [2]> slot is a checked element-narrowing store (implicit
    // store node, run-time trap out of range) — the container twin of the
    // scalar store rule — not a compile error.
    accepts(
        r"incipit {
  sit tensor<i32, [2]> a ← vacua
  sit tensor<i64, [2]> b ← vacua
  sit tensor<i32, [2]> c ← a + b
  nota c.longitudo()
}",
    );
}

// --- tensor reductions (Commit B: summa / media) ---

#[test]
fn tensor_reduction_summa_returns_element_type() {
    accepts(
        r"incipit {
  sit tf32[2, 3] grid ← vacua
  sit f32 total ← grid.summa()
  nota total
}",
    );
}

#[test]
fn tensor_reduction_summa_integer_tensor_returns_numerus() {
    accepts(
        r"incipit {
  sit tensor<numerus, [3]> grid ← vacua
  sit numerus total ← grid.summa()
  nota total
}",
    );
}

#[test]
fn tensor_reduction_summa_on_numerus_i32_tensor() {
    // EDGE: explicit-width integer tensor summa keeps the element width.
    accepts(
        r"incipit {
  sit tensor<i32, [3]> grid ← vacua
  sit i32 total ← grid.summa()
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
  sit tensor<numerus, [3]> grid ← vacua
  sit f64 mean ← grid.media()
  nota mean
}",
    );
}

#[test]
fn tensor_reduction_media_fractus_f32_keeps_width() {
    accepts(
        r"incipit {
  sit tf32[2, 3] grid ← vacua
  sit f32 mean ← grid.media()
  nota mean
}",
    );
}

#[test]
fn tensor_reduction_media_numerus_rejects_integer_return() {
    // media never returns an integer type; binding to numerus must fail.
    rejects_with_issue(
        r"incipit {
  sit tensor<numerus, [3]> grid ← vacua
  sit numerus mean ← grid.media()
  nota mean
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn tensor_reduction_textus_element_rejects() {
    rejects_with_issue(
        r"incipit {
  sit tensor<textus, [2]> grid ← vacua
  sit textus total ← grid.summa()
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
  sit tensor<numerus, [2, 3]> a ← vacua
  sit tensor<numerus, [3, 4]> b ← vacua
  sit tensor<numerus, [2, 4]> c ← a.matmul(b)
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_matmul_static_result_shape() {
    // Verify the result shape is [M,N] concretely, not a fresh hole.
    accepts(
        r"incipit {
  sit tf32[2, 3] a ← vacua
  sit tf32[3, 4] b ← vacua
  sit tf32[2, 4] c ← a.matmul(b)
  nota c.longitudo()
}",
    );
}

#[test]
fn tensor_matmul_rank_mismatch_rejects() {
    // [2] × [2,3] — receiver is rank-1, rejected.
    rejects_with_issue(
        r"incipit {
  sit tensor<numerus, [2]> a ← vacua
  sit tensor<numerus, [2, 3]> b ← vacua
  sit tensor<numerus, [1, 3]> c ← a.matmul(b)
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
  sit tensor<numerus, [2, 3]> a ← vacua
  sit tensor<numerus, [3]> b ← vacua
  sit tensor<numerus, [2, 1]> c ← a.matmul(b)
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
  sit tensor<numerus, [2, 3]> a ← vacua
  sit tensor<numerus, [4, 5]> b ← vacua
  sit tensor<numerus, [2, 5]> c ← a.matmul(b)
  nota c.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_matmul_textus_element_rejects() {
    rejects_with_issue(
        r"incipit {
  sit tensor<textus, [2, 3]> a ← vacua
  sit tensor<textus, [3, 4]> b ← vacua
  sit tensor<textus, [2, 4]> c ← a.matmul(b)
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
    redde grid.transpone().materialize()
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
    redde v.transpone().materialize()
}

nota "rank-one transpose body ok"
"#,
    );
}

#[test]
fn batched_transpose_decode_shape_typechecks() {
    // U3 head-axis decode fixture: K [5,76,64] → Kᵀ [5,64,76] — the
    // leading head axis stays, the trailing [M, N] pair swaps.
    accepts(
        r#"functio key_transpose_decode(tensor<f32, [5, 76, 64]> keys) → tensor<f32, [5, 64, 76]> {
    redde keys.transpone().materialize()
}

nota "batched transpose decode body ok"
"#,
    );
}

#[test]
fn batched_transpose_prefill_shape_typechecks() {
    // U3 head-axis prefill fixture: K [5,36,64] → Kᵀ [5,64,36].
    accepts(
        r#"functio key_transpose_prefill(tensor<f32, [5, 36, 64]> keys) → tensor<f32, [5, 64, 36]> {
    redde keys.transpone().materialize()
}

nota "batched transpose prefill body ok"
"#,
    );
}

#[test]
fn batched_transpose_wrong_result_shape_rejects() {
    // A result that keeps the un-swapped trailing pair is not the batched
    // transpose law — the shape unifier rejects typed.
    rejects_with_issue(
        r#"functio bad_batched_transpose(tensor<f32, [5, 76, 64]> keys) → tensor<f32, [5, 76, 64]> {
    redde keys.transpone()
}

nota "bad batched transpose body"
"#,
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_broadcast_symbolic_stretches_size_one_dim() {
    // Expand [B,1] to the grid's [B,D] explicitly before exact-shape addition.
    // Size params are type witnesses, not runtime values: query the grid shape.
    accepts(
        r#"functio bias_add_sym<magnitudo B, magnitudo D>(tensor<f32, [B, D]> grid, tensor<f32, [B, 1]> bias) → tensor<f32, [B, D]> {
    redde grid + bias.expande(grid.magnitudines())
}

nota "symbolic broadcast body ok"
"#,
    );
}

#[test]
fn tensor_elementwise_symbolic_same_param_shapes() {
    // Same-param elementwise: [B,D] ⊙ [B,D] → [B,D].
    accepts(
        r#"functio scale_sym<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a ⊙ b
}

nota "same-param elementwise body ok"
"#,
    );
}

#[test]
fn tensor_divide_en_surface_preserves_same_generic_shape() {
    // Match Gradus math.fab's English surface and the real non-la intrinsic
    // canonicalization path, not registry-direct Latin spelling.
    let pack =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("installed English locale pack should load");
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    let result = crate::test_support::analyze_source_with_config(
        r#"@ kernel
@ public
fn divide_sym<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a.divided(b)
}
"#,
        config,
    );
    assert!(
        result.is_ok(),
        "Gradus' en-surface generic divide must preserve [M,N]: {:?}",
        result.diagnostics
    );
}

#[test]
fn generic_tensor_call_keeps_immutable_scalar_shape_witness_and_carrier_guards() {
    let pack =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("installed English locale pack should load");
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    let result = crate::test_support::analyze_source_with_config(
        r#"fn keep_first<size M, size N>(tensor<f32, [M, N]> left, tensor<f32, [M, N]> right) → tensor<f32, [M, N]> {
    return left
}

main {
    let list<f32> data ← [1.0, 2.0, 3.0, 4.0]
    let int n ← data.length() / 2
    let int m ← data.length() / 2
    let tensor<f32, [size n, size n]> source ← data ↦ tensor<f32, [size n, size n]>
    let tensor<f32, [size m, size n]> other ← data ↦ tensor<f32, [size m, size n]>
    let tensor<f32, [size n, size n]> result ← keep_first(source, other)
}
"#,
        config,
    );
    assert!(
        result.is_ok(),
        "generic result shape should retain immutable scalar identity: {:?}",
        result.diagnostics
    );

    let guarded_axes: std::collections::BTreeSet<_> = result
        .shape_obligations
        .calls
        .values()
        .flatten()
        .filter_map(|relation| {
            let crate::index::IndexExpr::Runtime(radix_types::RuntimeExtentId::CallArgumentAxis {
                argument,
                axis,
            }) = result.types.get_index(relation.left)
            else {
                return None;
            };
            matches!(
                result.types.get_index(relation.right),
                crate::index::IndexExpr::Runtime(radix_types::RuntimeExtentId::ScalarBinding(_))
            )
            .then_some((*argument, *axis))
        })
        .collect();
    assert_eq!(
        guarded_axes,
        std::collections::BTreeSet::from([(0, 0), (0, 1), (1, 0), (1, 1)]),
        "each runtime carrier axis must still be checked against the stable size witness"
    );

    let second_argument_axis_zero_witnesses: std::collections::BTreeSet<_> = result
        .shape_obligations
        .calls
        .values()
        .flatten()
        .filter_map(|relation| {
            let crate::index::IndexExpr::Runtime(radix_types::RuntimeExtentId::CallArgumentAxis {
                argument: 1,
                axis: 0,
            }) = result.types.get_index(relation.left)
            else {
                return None;
            };
            match result.types.get_index(relation.right) {
                crate::index::IndexExpr::Runtime(radix_types::RuntimeExtentId::ScalarBinding(
                    def_id,
                )) => Some(def_id.0),
                _ => None,
            }
        })
        .collect();
    assert_eq!(
        second_argument_axis_zero_witnesses.len(),
        2,
        "the second argument's physical extent and repeated generic extent remain separate runtime checks"
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

#[test]
fn tensor_matmul_rank_agnostic_argument_rejects() {
    // A rank-2 receiver with a whole-shape argument has no argument tuple to
    // contract; the argument rank check reports the typed issue.
    rejects_with_issue(
        r#"functio bad<magnitudo Figura>(tensor<f32, [2, 3]> a, tensor<f32, Figura> b) → tensor<f32, [2, 3]> {
    redde a.matmul(b)
}

nota "rankagnostic argument ok"
"#,
        "matmul_argument_shape_must_be_tuple",
    );
}

// --- U1 head-axis: batched rank-3/4 matmul admission (leading head axes) ---

#[test]
fn batched_matmul_decode_shape_typechecks() {
    // GQA decode: Q [5,3,1,64] · Kᵀ [5,64,76] → scores [5,3,1,76]. The
    // shared leading kv-head axis 5 is the prefix share; query_seq is 1
    // (rectangular decode, never square).
    accepts(
        r"incipit {
  sit tensor<f32, [5, 3, 1, 64]> q ← vacua
  sit tensor<f32, [5, 64, 76]> kt ← vacua
  sit tensor<f32, [5, 3, 1, 76]> scores ← q · kt
  nota scores.longitudo()
}",
    );
}

#[test]
fn batched_matmul_prefill_shape_typechecks() {
    // Prefill: [5,3,36,64] · [5,64,36] → [5,3,36,36].
    accepts(
        r"incipit {
  sit tensor<f32, [5, 3, 36, 64]> q ← vacua
  sit tensor<f32, [5, 64, 36]> kt ← vacua
  sit tensor<f32, [5, 3, 36, 36]> scores ← q · kt
  nota scores.longitudo()
}",
    );
}

#[test]
fn batched_matmul_wrong_result_shape_rejects() {
    // The result must carry the receiver's batch dims over [M, N] — a
    // [76]-major result shape contradicts the constraint.
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [5, 3, 1, 64]> q ← vacua
  sit tensor<f32, [5, 64, 76]> kt ← vacua
  sit tensor<f32, [5, 3, 76, 1]> scores ← q · kt
  nota scores.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn batched_matmul_prefix_mismatch_rejects() {
    // The argument's leading batch dims must be a prefix of the
    // receiver's (kv-head share): 6 ≠ 5.
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [5, 3, 1, 64]> q ← vacua
  sit tensor<f32, [6, 64, 76]> kt ← vacua
  sit tensor<f32, [5, 3, 1, 76]> scores ← q · kt
  nota scores.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn batched_matmul_inner_dim_mismatch_rejects() {
    // K tail must unify: 64 ≠ 63.
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [5, 3, 1, 64]> q ← vacua
  sit tensor<f32, [5, 63, 76]> kt ← vacua
  sit tensor<f32, [5, 3, 1, 76]> scores ← q · kt
  nota scores.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn batched_matmul_rank1_argument_rejects() {
    // The rank-1 GEMV promotion stays rank-2-only: a batched contraction
    // requires a trailing [K, N] matrix on the argument.
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [5, 3, 1, 64]> q ← vacua
  sit tensor<f32, [64]> k ← vacua
  sit tensor<f32, [5, 3, 1]> scores ← q · k
  nota scores.longitudo()
}",
        "matmul_argument_rank_must_be_two",
    );
}

#[test]
fn batched_matmul_rank5_receiver_still_rejects() {
    // Admission is rank-3/4 (the settled GEA3 shapes); rank-5+ fails
    // closed with the existing receiver rank issue.
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [2, 2, 2, 2, 2]> a ← vacua
  sit tensor<f32, [2, 2, 2]> b ← vacua
  sit tensor<f32, [2, 2, 2, 2, 2]> c ← a · b
  nota c.longitudo()
}",
        "matmul_receiver_rank_must_be_two",
    );
}

#[test]
fn batched_matmul_argument_rank_above_receiver_rejects() {
    // The argument's batch rank cannot exceed the receiver's (a longer
    // batch prefix than the receiver has is not a share).
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [5, 64, 76]> kt ← vacua
  sit tensor<f32, [3, 5, 64, 76]> q ← vacua
  sit tensor<f32, [3, 5, 64, 76]> scores ← kt · q
  nota scores.longitudo()
}",
        "matmul_argument_rank_must_be_two",
    );
}

// --- S0-G8: vector lane access ---

// A lane yields the element at literal and parametric widths alike, constant
// or run-time (the run-time lane is the checked lane select, VEC-5b).
#[test]
fn vector_bracket_index_yields_element_at_literal_width() {
    accepts(
        r"functio lane(vector<f32, magnitudo 3> v) → f32 {
    redde v[2]
}",
    );
}

#[test]
fn vector_bracket_index_yields_element_at_parametric_width() {
    accepts(
        r"functio lane<magnitudo N>(vector<f32, magnitudo N> v) → f32 {
    redde v[0]
}",
    );
    accepts(
        r"functio lane<magnitudo N>(vector<f32, magnitudo N> v, numerus i) → f32 {
    redde v[i]
}",
    );
}

#[test]
fn vector_bracket_index_requires_integer() {
    rejects_with_issue(
        r"functio lane(vector<f32, magnitudo 3> v, fractus i) → f32 {
    redde v[i]
}",
        "array_index_must_be_numerus",
    );
}

// --- U2 head-axis: batched additive-masked softmax (rank-3/4) ---

#[test]
fn batched_masked_softmax_decode_shape_typechecks() {
    // Decode: explicitly expand the [1,76] length mask over batch/head axes
    // before exact-shape addition and last-axis softmax. The [1,76] query/key
    // axes remain rectangular, never required square.
    accepts(
        r"incipit {
    sit tensor<f32, [5, 3, 1, 76]> scores ← vacua
    sit tensor<f32, [1, 76]> length_mask ← vacua
    sit tensor<f32, [5, 3, 1, 76]> expanded_mask ← length_mask.expande([5, 3, 1, 76])
    sit tensor<f32, [5, 3, 1, 76]> probabilities ← (scores + expanded_mask).activatio_softmax()
    nota probabilities.longitudo()
}",
    );
}

#[test]
fn batched_masked_softmax_prefill_shape_typechecks() {
    // Prefill: explicitly expand the [36,36] causal mask over batch/head
    // axes; exact-shape addition and last-axis softmax preserve [5,3,36,36].
    accepts(
        r"incipit {
    sit tensor<f32, [5, 3, 36, 36]> scores ← vacua
    sit tensor<f32, [36, 36]> causal_mask ← vacua
    sit tensor<f32, [5, 3, 36, 36]> expanded_mask ← causal_mask.expande([5, 3, 36, 36])
    sit tensor<f32, [5, 3, 36, 36]> probabilities ← (scores + expanded_mask).activatio_softmax()
    nota probabilities.longitudo()
}",
    );
}

#[test]
fn batched_masked_softmax_non_broadcast_mask_rejects() {
    // A mask dim that contradicts its aligned score dim (38 against 76)
    // cannot expand to the score shape — the expansion fails typed.
    rejects_with_issue(
        r"incipit {
    sit tensor<f32, [5, 3, 1, 76]> scores ← vacua
    sit tensor<f32, [2, 38]> bad_mask ← vacua
    sit tensor<f32, [5, 3, 1, 76]> probabilities ← (scores + bad_mask.expande([5, 3, 1, 76])).activatio_softmax()
    nota probabilities.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn vector_constant_lane_index_accepts_in_bounds() {
    accepts(
        r"incipit {
  sit vector<f32, magnitudo 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
  sit f32 a ← v[2]
  nota a
}",
    );
}

#[test]
fn vector_constant_lane_index_rejects_provable_oob_and_negative() {
    for index in ["3", "-1"] {
        rejects_with_issue(
            &format!(
                r"incipit {{
  sit vector<f32, magnitudo 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
  nota v[{index}]
}}"
            ),
            "index_provably_out_of_bounds",
        );
    }
}

#[test]
fn vector_runtime_lane_index_is_a_checked_lane_select() {
    accepts(
        r"functio lane(vector<f32, magnitudo 3> v, numerus i) → f32 {
  redde v[i]
}",
    );
}

#[test]
fn tensor_constructor_shape_affine_scalar_expression_matches_annotation() {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("English locale pack"),
    );
    let result = crate::test_support::analyze_source_with_config(
        r#"fn runtime_size(int value) → int {
    return value
}

main {
    let list<f32> flat ← [1.0, 2.0, 3.0]
    let int n ← runtime_size(4)
    let tensor<f32, [size (n - 1)]> dynamic ← flat ↦ tensor<f32, [size (n - 1)]>
}
"#,
        config,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn tensor_shape_of_static_rank_is_a_bounded_list_of_the_rank() {
    // K14: rank is static even when an extent is a run-time value, so the
    // shape list has a compile-time length. The capacity hole infers from it.
    accepts(
        r"functio runtime_extent(numerus value) → numerus {
    redde value
}

incipit {
  sit tf32[2, 3] grid ← vacua
  sit numerus n ← runtime_extent(3)
  sit tf32[2, magnitudo n] dynamic ← vacua
  sit lista<numerus, magnitudo 2> exact ← grid.magnitudines()
  sit lista<numerus, _> inferred ← dynamic.magnitudines()
  sit numerus second ← grid.magnitudines()[1] vel 0
  nota exact.longitudo(), inferred.longitudo(), second
}",
    );
}

#[test]
fn tensor_shape_literal_index_past_the_rank_rejects() {
    rejects_with_issue(
        r"incipit {
  sit tf32[2, 3] grid ← vacua
  sit _ bad ← grid.magnitudines()[5]
  nota bad
}",
        "index_provably_out_of_bounds",
    );
}

#[test]
fn tensor_shape_does_not_widen_to_an_unbounded_list_implicitly() {
    rejects_with_issue(
        r"incipit {
  sit tf32[2, 3] grid ← vacua
  sit lista<numerus> dims ← grid.magnitudines()
  nota dims.longitudo()
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn tensor_shape_widens_to_an_unbounded_list_through_explicit_conversio() {
    accepts(
        r"incipit {
  sit tf32[2, 3] grid ← vacua
  sit lista<numerus> dims ← grid.magnitudines() ↦ lista<numerus>
  nota dims.longitudo()
}",
    );
}

#[test]
fn tensor_shape_without_a_static_rank_stays_an_unbounded_list() {
    // A bare `magnitudo` parameter in shape position binds a whole shape of
    // any rank at instantiation, and `tf32[]` is the shapeless seed; neither
    // fixes a rank, so both keep the unbounded list (and `[1]` is not judged
    // against a guessed length).
    accepts(
        r#"functio second_extent<magnitudo S>(tensor<f32, S> value) → numerus {
    sit lista<numerus> dims ← value.magnitudines()
    redde dims[1] vel 0
}

nota "generic shape keeps the unbounded list"
"#,
    );
}

#[test]
fn tensor_sectio_on_reduction_output_literal_shape_constrains_result() {
    rejects_with_issue(
        r"incipit {
  sit tensor<f32, [3, 2]> grid ← vacua
  sit tensor<f32, [3]> reduced ← grid.summa(axis 1)
  sit tensor<f32, [3]> sliced ← reduced.sectio(0, 2)
  nota sliced
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_sectio_on_inferred_reduction_output_slices_the_kept_axis() {
    // WHY: `summa(axis 1)` over `[3, 2]` keeps axis 0 as the result shape
    // itself (a rank-1 `Literal`, not a singleton tuple). `sectio(0, 2)` must
    // read that shape as one dim and constrain the result to `[2]`; before the
    // literal arm existed the result shape stayed unbound and this correct
    // slice was rejected as uninferable.
    accepts(
        r"incipit {
  sit tensor<f32, [3, 2]> grid ← vacua
  sit _ reduced ← grid.summa(axis 1)
  sit tensor<f32, [2]> sliced ← reduced.sectio(0, 2)
  nota sliced
}",
    );
}
