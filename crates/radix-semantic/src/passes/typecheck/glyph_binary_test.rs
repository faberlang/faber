//! TENSOR-GLYPHS-1B rank-rule coverage for the glyph product operators
//! (`·` `×` `⊗` `⊙`). The glyphs typecheck on the built-in static-shape
//! families (vector / matrix / tensor); every mismatch fails closed with a
//! structured `issue` + `op` pair and never silently passes as scalar
//! arithmetic.

use radix_diagnostics::DiagnosticArg;

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source_la(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str, op: &str) {
    let result = crate::test_support::analyze_source_la(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted"
    );
    let diagnostics = result.diagnostics.clone();
    let issue_arg = DiagnosticArg::new("issue", issue);
    let op_arg = DiagnosticArg::new("op", op);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.args.contains(&issue_arg) && d.args.contains(&op_arg)),
        "expected a diagnostic with issue `{issue}` and op `{op}`, got: {diagnostics:?}"
    );
}

/// Rejection that surfaces the tensor matmul contract's own diagnostics
/// (`matmul_receiver_shape_must_be_tuple`, `incompatible_tensor_index`, …),
/// which — like the `matmul` intrinsic path — carry no glyph `op` arg.
fn rejects_with_contract_issue(source: &str, issue: &str) {
    let result = crate::test_support::analyze_source_la(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted"
    );
    let diagnostics = result.diagnostics.clone();
    let issue_arg = DiagnosticArg::new("issue", issue);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.args.contains(&issue_arg)),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

fn rejects_transpose_with_issue(source: &str, issue: &str) {
    rejects_with_contract_issue(source, issue);
}

// --- `·` inner product (dot / matvec / matmul by rank) ---

#[test]
fn glyph_dot_vector_dot_returns_scalar() {
    accepts(
        r"functio dot(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → f32 {
    redde a · b
}",
    );
}

#[test]
fn glyph_dot_matrix_matmul_returns_matrix() {
    accepts(
        r"functio product(matrix<f32, [2, 3]> a, matrix<f32, [3, 4]> b) → matrix<f32, [2, 4]> {
    redde a · b
}",
    );
}

// --- S0-G7: size-parametric glyph matmul (symbolic shape contract) ---

#[test]
fn glyph_dot_generic_matrix_matmul_returns_parametric_matrix() {
    accepts(
        r"functio multiply<magnitudo R, magnitudo K, magnitudo C>(matrix<f32, [R, K]> a, matrix<f32, [K, C]> b) → matrix<f32, [R, C]> {
    redde a · b
}",
    );
}

#[test]
fn glyph_dot_generic_matrix_matmul_inner_dim_mismatch_rejects() {
    rejects_with_issue(
        r"functio multiply<magnitudo R, magnitudo K, magnitudo C>(matrix<f32, [R, K]> a, matrix<f32, [C, C]> b) → matrix<f32, [R, C]> {
    redde a · b
}",
        "glyph_inner_dim_mismatch",
        "·",
    );
}

#[test]
fn glyph_dot_generic_vs_concrete_matrix_matmul_rejects() {
    rejects_with_issue(
        r"functio multiply<magnitudo R, magnitudo C>(matrix<f32, [R, 3]> a, matrix<f32, [4, C]> b) → matrix<f32, [R, C]> {
    redde a · b
}",
        "glyph_inner_dim_mismatch",
        "·",
    );
}

#[test]
fn glyph_dot_matvec_returns_vector() {
    accepts(
        r"functio apply(matrix<f32, [2, 3]> m, vector<f32, magnitudo 3> v) → vector<f32, magnitudo 2> {
    redde m · v
}",
    );
}

#[test]
fn glyph_dot_vector_matrix_returns_vector() {
    accepts(
        r"functio apply(vector<f32, magnitudo 3> v, matrix<f32, [3, 4]> m) → vector<f32, magnitudo 4> {
    redde v · m
}",
    );
}

#[test]
fn glyph_dot_vector_width_mismatch_rejects() {
    rejects_with_issue(
        r"functio dot(vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b) → f32 {
    redde a · b
}",
        "glyph_inner_dim_mismatch",
        "·",
    );
}

#[test]
fn glyph_dot_matmul_inner_dim_mismatch_rejects() {
    rejects_with_issue(
        r"functio product(matrix<f32, [2, 3]> a, matrix<f32, [4, 2]> b) → matrix<f32, [2, 2]> {
    redde a · b
}",
        "glyph_inner_dim_mismatch",
        "·",
    );
}

#[test]
fn glyph_dot_matvec_width_mismatch_rejects() {
    rejects_with_issue(
        r"functio apply(matrix<f32, [2, 3]> m, vector<f32, magnitudo 4> v) → vector<f32, magnitudo 2> {
    redde m · v
}",
        "glyph_inner_dim_mismatch",
        "·",
    );
}

#[test]
fn glyph_dot_scalar_operands_do_not_silently_pass() {
    rejects_with_issue(
        r"incipit {
  sit numerus a ← 1
  sit numerus b ← 2
  sit _ c ← a · b
}",
        "glyph_operands_unsupported",
        "·",
    );
}

#[test]
fn glyph_dot_non_numeric_element_rejects() {
    rejects_with_issue(
        r"functio dot(vector<textus, magnitudo 3> a, vector<textus, magnitudo 3> b) → textus {
    redde a · b
}",
        "glyph_op_numeric_element_required",
        "·",
    );
}

#[test]
fn glyph_dot_element_type_mismatch_rejects() {
    rejects_with_issue(
        r"functio dot(vector<f32, magnitudo 3> a, vector<f64, magnitudo 3> b) → f64 {
    redde a · b
}",
        "glyph_element_type_mismatch",
        "·",
    );
}

// --- `×` cross product (width 3 only) ---

#[test]
fn glyph_cross_width_three_typechecks() {
    accepts(
        r"functio cross(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → vector<f32, magnitudo 3> {
    redde a × b
}",
    );
}

#[test]
fn glyph_cross_non_width_three_rejects() {
    rejects_with_issue(
        r"functio cross(vector<f32, magnitudo 4> a, vector<f32, magnitudo 4> b) → vector<f32, magnitudo 4> {
    redde a × b
}",
        "glyph_cross_width_three_required",
        "×",
    );
}

#[test]
fn glyph_cross_width_mismatch_rejects() {
    rejects_with_issue(
        r"functio cross(vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b) → vector<f32, magnitudo 4> {
    redde a × b
}",
        "glyph_cross_width_mismatch",
        "×",
    );
}

#[test]
fn glyph_cross_matrix_operands_reject() {
    rejects_with_issue(
        r"functio cross(matrix<f32, [3, 3]> a, matrix<f32, [3, 3]> b) → matrix<f32, [3, 3]> {
    redde a × b
}",
        "glyph_operands_unsupported",
        "×",
    );
}

// --- `⊗` outer product (vector ⊗ vector → matrix) ---

#[test]
fn glyph_tensor_product_vector_outer_returns_matrix() {
    accepts(
        r"functio outer(vector<f32, magnitudo 3> a, vector<f32, magnitudo 2> b) → matrix<f32, [3, 2]> {
    redde a ⊗ b
}",
    );
}

#[test]
fn glyph_tensor_product_kronecker_on_matrices_deferred() {
    rejects_with_issue(
        r"functio kronecker(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [4, 4]> {
    redde a ⊗ b
}",
        "glyph_operands_unsupported",
        "⊗",
    );
}

// --- `⊙` Hadamard (identical shapes) ---

#[test]
fn glyph_hadamard_identical_vector_shapes_typecheck() {
    accepts(
        r"functio elementwise(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → vector<f32, magnitudo 3> {
    redde a ⊙ b
}",
    );
}

#[test]
fn glyph_hadamard_identical_matrix_shapes_typecheck() {
    accepts(
        r"functio elementwise(matrix<f32, [2, 3]> a, matrix<f32, [2, 3]> b) → matrix<f32, [2, 3]> {
    redde a ⊙ b
}",
    );
}

#[test]
fn glyph_hadamard_identical_tensor_shapes_typecheck() {
    accepts(
        r"functio elementwise(tensor<f32, [2, 3]> a, tensor<f32, [2, 3]> b) → tensor<f32, [2, 3]> {
    redde a ⊙ b
}",
    );
}

#[test]
fn glyph_hadamard_vector_width_mismatch_rejects() {
    rejects_with_issue(
        r"functio elementwise(vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b) → vector<f32, magnitudo 4> {
    redde a ⊙ b
}",
        "glyph_hadamard_shapes_mismatch",
        "⊙",
    );
}

#[test]
fn glyph_hadamard_matrix_shape_mismatch_rejects() {
    rejects_with_issue(
        r"functio elementwise(matrix<f32, [2, 3]> a, matrix<f32, [2, 4]> b) → matrix<f32, [2, 4]> {
    redde a ⊙ b
}",
        "glyph_hadamard_shapes_mismatch",
        "⊙",
    );
}

// --- `⊘` Hadamard divide (the exact `⊙` shape contract) ---

#[test]
fn glyph_hadamard_divide_identical_shapes_typecheck() {
    accepts(
        r"functio elementwise(tensor<f32, [2, 3]> a, tensor<f32, [2, 3]> b) → tensor<f32, [2, 3]> {
    redde a ⊘ b
}",
    );
}

#[test]
fn glyph_hadamard_divide_vector_shape_mismatch_rejects() {
    rejects_with_issue(
        r"functio elementwise(vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b) → vector<f32, magnitudo 4> {
    redde a ⊘ b
}",
        "glyph_hadamard_shapes_mismatch",
        "⊘",
    );
}

// --- `·` / `⊙` tensor arms over the symbolic contracts (shape-generics Phase 2) ---

#[test]
fn glyph_dot_rank2_tensor_symbolic_matmul_typechecks() {
    accepts(
        r"functio product<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [D, B]> b) → tensor<f32, [B, B]> {
    redde a · b
}",
    );
}

#[test]
fn glyph_dot_concrete_tensor_matmul_typechecks() {
    accepts(
        r"functio product(tensor<f32, [2, 3]> a, tensor<f32, [3, 4]> b) → tensor<f32, [2, 4]> {
    redde a · b
}",
    );
}

#[test]
fn glyph_dot_symbolic_transpose_chain_typechecks() {
    accepts(
        r"functio sdpa<magnitudo B, magnitudo D>(tensor<f32, [B, D]> qb, tensor<f32, [B, D]> kb, tensor<f32, [B, D]> vb) → tensor<f32, [B, D]> {
    redde (qb · kb.transpone() · vb)
}",
    );
}

// --- `ᵀ` postfix transpose (rank-2 v1 law) ---

#[test]
fn glyph_transpose_rank2_typechecks_and_swaps_shape() {
    accepts(
        r"functio transpose(tensor<f32, [2, 3]> grid) → tensor<f32, [3, 2]> {
    redde gridᵀ.materialize()
}",
    );
}

#[test]
fn glyph_transpose_rank1_is_permanently_declined() {
    rejects_transpose_with_issue(
        r"functio transpose(tensor<f32, [3]> vector) → tensor<f32, [3]> {
    redde vectorᵀ
}",
        "glyph_transpose_rank1_no_row_column_distinction",
    );
}

#[test]
fn glyph_transpose_rank3_typechecks_as_batched_transpose() {
    // U3 head-axis: the rank-3+ deferral is closed by the batched-transpose
    // consumer — leading batch axes stay, the trailing [M, N] pair swaps.
    accepts(
        r"functio transpose(tensor<f32, [2, 3, 4]> batch) → tensor<f32, [2, 4, 3]> {
    redde batchᵀ.materialize()
}",
    );
}

#[test]
fn glyph_transpose_scalar_is_not_a_tensor_operation() {
    rejects_transpose_with_issue(
        r"functio transpose(f32 value) → f32 {
    redde valueᵀ
}",
        "glyph_transpose_operands_unsupported",
    );
}

// --- S0-G9: `ᵀ` on matrix carriers (rank-2 matrix glyph law) ---

#[test]
fn glyph_transpose_matrix_typechecks_and_swaps_shape() {
    accepts(
        r"functio transpose(matrix<f32, [2, 3]> grid) → matrix<f32, [3, 2]> {
    redde gridᵀ
}",
    );
}

#[test]
fn glyph_transpose_matrix_parametric_swaps_shape_terms() {
    accepts(
        r"functio transpose<magnitudo R, magnitudo C>(matrix<f32, [R, C]> grid) → matrix<f32, [C, R]> {
    redde gridᵀ
}",
    );
}

#[test]
fn glyph_dot_tensor_inner_dim_mismatch_rejects() {
    rejects_with_contract_issue(
        r"functio product(tensor<f32, [2, 3]> a, tensor<f32, [4, 2]> b) → tensor<f32, [2, 2]> {
    redde a · b
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn glyph_dot_whole_shape_tensor_rejects() {
    rejects_with_contract_issue(
        r"functio product<magnitudo Figura>(tensor<f32, Figura> a, tensor<f32, Figura> b) → tensor<f32, Figura> {
    redde a · b
}",
        "matmul_receiver_shape_must_be_tuple",
    );
}

#[test]
fn glyph_dot_tensor_rank1_rhs_returns_rank1_tensor() {
    accepts(
        r"functio apply(tensor<f32, [2, 3]> weights, tensor<f32, [3]> input) → tensor<f32, [2]> {
    redde weights · input
}",
    );
}

#[test]
fn glyph_dot_tensor_rank1_rhs_inner_dim_mismatch_rejects() {
    rejects_with_contract_issue(
        r"functio apply(tensor<f32, [2, 3]> weights, tensor<f32, [4]> input) → tensor<f32, [2]> {
    redde weights · input
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn glyph_dot_rank1_tensor_rejects() {
    rejects_with_contract_issue(
        r"functio dot(tensor<f32, [3]> a, tensor<f32, [3]> b) → f32 {
    redde a · b
}",
        "matmul_receiver_rank_must_be_two",
    );
}

#[test]
fn glyph_dot_tensor_bf16_f32_returns_f32() {
    accepts(
        r"functio apply(tensor<bf16, [2, 3]> weights, tensor<f32, [3]> input) → tensor<f32, [2]> {
    redde weights · input
}",
    );
}

#[test]
fn glyph_dot_tensor_f32_bf16_stays_fail_closed() {
    rejects_with_issue(
        r"functio apply(tensor<f32, [2, 3]> weights, tensor<bf16, [3]> input) → tensor<f32, [2]> {
    redde weights · input
}",
        "glyph_element_type_mismatch",
        "·",
    );
}

#[test]
fn glyph_hadamard_symbolic_tuple_shapes_typecheck() {
    accepts(
        r"functio mask<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a ⊙ b
}",
    );
}

#[test]
fn glyph_hadamard_whole_shape_param_typechecks() {
    accepts(
        r"functio mask<magnitudo Figura>(tensor<f32, Figura> a, tensor<f32, Figura> b) → tensor<f32, Figura> {
    redde a ⊙ b
}",
    );
}

#[test]
fn glyph_hadamard_symbolic_shape_mismatch_rejects() {
    rejects_with_issue(
        r"functio mask<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, B]> b) → tensor<f32, [B, D]> {
    redde a ⊙ b
}",
        "glyph_hadamard_shapes_mismatch",
        "⊙",
    );
}

#[test]
fn glyph_hadamard_distinct_shape_params_reject() {
    // D5-conservative: distinct `magnitudo` params do not unify, so whole-shape
    // params that are not textually the same reject rather than silently merge.
    rejects_with_issue(
        r"functio mask<magnitudo Figura, magnitudo Forma>(tensor<f32, Figura> a, tensor<f32, Forma> b) → tensor<f32, Figura> {
    redde a ⊙ b
}",
        "glyph_hadamard_shapes_mismatch",
        "⊙",
    );
}

// --- S0-G8: size-parametric `⊗` outer product (symbolic shape contract) ---

#[test]
fn glyph_tensor_product_parametric_width_returns_symbolic_matrix() {
    accepts(
        r"functio scala<magnitudo N>(vector<f32, magnitudo N> v, vector<f32, magnitudo 1> w) → matrix<f32, [N, 1]> {
    redde v ⊗ w
}",
    );
}

#[test]
fn glyph_tensor_product_literal_widths_unchanged() {
    accepts(
        r"functio scala(vector<f32, magnitudo 3> v, vector<f32, magnitudo 2> w) → matrix<f32, [3, 2]> {
    redde v ⊗ w
}",
    );
}

#[test]
fn glyph_hadamard_parametric_width_still_fails_closed() {
    // Only `⊗` publishes a symbolic shape; same-shape `⊙` keeps the literal
    // demand so an unsolved parametric width cannot silently pass.
    rejects_with_issue(
        r"functio mask<magnitudo N>(vector<f32, magnitudo N> a, vector<f32, magnitudo N> b) → vector<f32, magnitudo N> {
    redde a ⊙ b
}",
        "glyph_static_shape_required",
        "⊙",
    );
}
