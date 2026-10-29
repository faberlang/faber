//! MIR lowering tests for the glyph product families (`·` `×` `⊗` `⊙`):
//! operand-family dispatch over the vector/matrix method machinery and the
//! collection-op seam, plus fail-closed diagnostics for every unsupported
//! shape/type combination (TENSOR-GLYPHS-3A).

use super::*;
use crate::codegen::Target;
use crate::driver::AnalyzedModule;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::hir::HirId;

fn analyze(source: &str) -> AnalyzedModule {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

fn dump_source(source: &str) -> String {
    let unit = analyze(source);
    dump_analyzed_unit(unit.application_view()).expect("MIR lowering succeeds")
}

// ===========================================================================
// Builder-level fixtures: typecheck rejects some glyph pairings (tensor `·`,
// Kronecker `⊗`), so those MIR arms are proven by constructing the HIR
// directly and lowering through `FunctionBuilder`.
// ===========================================================================

fn glyph_f32_tensor_ty(types: &mut TypeTable) -> TypeId {
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let row = types.intern_index(IndexExpr::Literal(2));
    let column = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![row, column]));
    types.intern(Type::Tensor(elem, shape))
}

fn glyph_f32_matrix_ty(types: &mut TypeTable) -> TypeId {
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let row = types.intern_index(IndexExpr::Literal(2));
    let column = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![row, column]));
    types.intern(Type::Matrix(elem, shape))
}

/// A builder whose params `a`/`b` (DefId 0/1) carry the given types, with the
/// entry block open.
fn glyph_builder(types: &TypeTable, lhs_ty: TypeId, rhs_ty: TypeId) -> FunctionBuilder<'_> {
    let mut builder = FunctionBuilder::new(types);
    builder.add_param(DefId(0), Symbol(0), lhs_ty, HirParamMode::Owned, Span::default());
    builder.add_param(DefId(1), Symbol(1), rhs_ty, HirParamMode::Owned, Span::default());
    let entry = builder.fresh_block(Span::default());
    builder.switch_to(entry);
    builder
}

fn glyph_operand(id: u32, ty: TypeId) -> HirExpression {
    HirExpression { id: HirId(id), kind: HirExpressionKind::Path(DefId(id)), ty: Some(ty), span: Span::default() }
}

fn glyph_binop(op: HirBinOp, lhs: &HirExpression, rhs: &HirExpression, ty: TypeId) -> HirExpression {
    HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Binary(op, Box::new(lhs.clone()), Box::new(rhs.clone())),
        ty: Some(ty),
        span: Span::default(),
    }
}

// ===========================================================================
// `·` dot family
// ===========================================================================

#[test]
fn lowers_glyph_transpose_to_tensor_transpose_collection_op() {
    let dump = dump_source(
        r"
functio transpose(tensor<f32, [2, 3]> input) → tensor<f32, [3, 2]> {
    redde inputᵀ.materialize()
}
",
    );

    assert!(dump.contains("runtime collection tensor_transpose"), "{dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
}

#[test]
fn lowers_glyph_vector_dot_to_lane_mul_add() {
    let dump = dump_source(
        r"
functio dot(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → f32 {
    redde a · b
}
",
    );

    assert!(dump.contains("_0.lane0 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.lane1 * _1.lane1"), "{dump}");
    assert!(dump.contains("_0.lane2 * _1.lane2"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn lowers_glyph_matrix_dot_to_cell_mul_add() {
    let dump = dump_source(
        r"
functio product(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> {
    redde a · b
}
",
    );

    assert!(dump.contains("_0.cell0_0 * _1.cell0_0"), "{dump}");
    assert!(dump.contains("_0.cell0_1 * _1.cell1_0"), "{dump}");
    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn lowers_glyph_matrix_applica_to_cell_lane_mul_add() {
    let dump = dump_source(
        r"
functio apply(matrix<f32, [2, 2]> m, vector<f32, magnitudo 2> v) → vector<f32, magnitudo 2> {
    redde m · v
}
",
    );

    assert!(dump.contains("_0.cell0_0 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.cell0_1 * _1.lane1"), "{dump}");
    assert!(dump.contains("construct vector"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn lowers_glyph_matrix_matmul_non_square_to_full_grid() {
    // Regression (tensor-glyphs P3, task): `[3,2]·[2,4]` must lower the
    // full [3,4] result (12 cells) by contracting over K=2. The pre-fix
    // square-subset lowering reused the receiver column count as the result
    // width and silently dropped RHS columns 2..=3.
    let dump = dump_source(
        r"
functio product(matrix<f32, [3, 2]> a, matrix<f32, [2, 4]> b) → matrix<f32, [3, 4]> {
    redde a · b
}
",
    );

    // Every one of the 12 result cells must materialize both K terms.
    for row in 0..3 {
        for column in 0..4 {
            assert!(
                dump.contains(&format!("_0.cell{row}_0 * _1.cell0_{column}")),
                "cell ({row},{column}) K=0 product missing:\n{dump}"
            );
            assert!(
                dump.contains(&format!("_0.cell{row}_1 * _1.cell1_{column}")),
                "cell ({row},{column}) K=1 product missing:\n{dump}"
            );
        }
    }
    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn lowers_glyph_tensor_dot_to_tensor_matmul_collection_op() {
    let mut types = TypeTable::new();
    let tensor_ty = glyph_f32_tensor_ty(&mut types);
    let lhs = glyph_operand(0, tensor_ty);
    let rhs = glyph_operand(1, tensor_ty);
    let binop = glyph_binop(HirBinOp::Dot, &lhs, &rhs, tensor_ty);
    let mut builder = glyph_builder(&types, tensor_ty, tensor_ty);

    assert!(
        builder
            .lower_binary(HirBinOp::Dot, &lhs, &rhs, &binop)
            .is_some()
    );
    assert!(builder.errors.is_empty(), "{:?}", builder.errors);
    let has_matmul = builder.blocks.iter().any(|block| {
        block.statements.iter().any(|stmt| {
            matches!(
                &stmt.kind,
                MirStatementKind::RuntimeCall {
                    call: MirRuntimeCall { intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul), .. },
                    ..
                }
            )
        })
    });
    assert!(has_matmul, "tensor · must lower to the TensorMatMul collection op");
}

// ===========================================================================
// `×` cross family
// ===========================================================================

#[test]
fn lowers_glyph_vector_cross_to_lane_mul_sub() {
    let dump = dump_source(
        r"
functio cross(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → vector<f32, magnitudo 3> {
    redde a × b
}
",
    );

    assert!(dump.contains("_0.lane1 * _1.lane2"), "{dump}");
    assert!(dump.contains("_0.lane2 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.lane0 * _1.lane1"), "{dump}");
    assert!(dump.contains("construct vector"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

// ===========================================================================
// `⊗` outer product family
// ===========================================================================

#[test]
fn lowers_glyph_vector_outer_to_matrix_cells() {
    let dump = dump_source(
        r"
functio outer(vector<f32, magnitudo 3> a, vector<f32, magnitudo 2> b) → matrix<f32, [3, 2]> {
    redde a ⊗ b
}
",
    );

    assert!(dump.contains("_0.lane0 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.lane0 * _1.lane1"), "{dump}");
    assert!(dump.contains("_0.lane1 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.lane1 * _1.lane1"), "{dump}");
    assert!(dump.contains("_0.lane2 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.lane2 * _1.lane1"), "{dump}");
    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn rejects_glyph_matrix_tensor_product_kronecker() {
    let mut types = TypeTable::new();
    let matrix_ty = glyph_f32_matrix_ty(&mut types);
    let lhs = glyph_operand(0, matrix_ty);
    let rhs = glyph_operand(1, matrix_ty);
    let binop = glyph_binop(HirBinOp::TensorProduct, &lhs, &rhs, matrix_ty);
    let mut builder = glyph_builder(&types, matrix_ty, matrix_ty);

    assert!(
        builder
            .lower_binary(HirBinOp::TensorProduct, &lhs, &rhs, &binop)
            .is_none()
    );
    assert_eq!(builder.errors.len(), 1, "{:?}", builder.errors);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_matrix_tensor_product_kronecker_is_not_supported_on_the_mir_target"
    );
}

#[test]
fn rejects_glyph_tensor_tensor_product_kronecker() {
    let mut types = TypeTable::new();
    let tensor_ty = glyph_f32_tensor_ty(&mut types);
    let lhs = glyph_operand(0, tensor_ty);
    let rhs = glyph_operand(1, tensor_ty);
    let binop = glyph_binop(HirBinOp::TensorProduct, &lhs, &rhs, tensor_ty);
    let mut builder = glyph_builder(&types, tensor_ty, tensor_ty);

    assert!(
        builder
            .lower_binary(HirBinOp::TensorProduct, &lhs, &rhs, &binop)
            .is_none()
    );
    assert_eq!(builder.errors.len(), 1, "{:?}", builder.errors);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_tensor_tensor_product_kronecker_is_not_supported_on_the_mir_target"
    );
}

// ===========================================================================
// `⊙` Hadamard family
// ===========================================================================

#[test]
fn lowers_glyph_vector_hadamard_to_lane_mul() {
    let dump = dump_source(
        r"
functio elementwise(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → vector<f32, magnitudo 3> {
    redde a ⊙ b
}
",
    );

    assert!(dump.contains("_0.lane0 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.lane1 * _1.lane1"), "{dump}");
    assert!(dump.contains("_0.lane2 * _1.lane2"), "{dump}");
    assert!(dump.contains("construct vector"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn lowers_glyph_matrix_hadamard_to_cell_mul() {
    let dump = dump_source(
        r"
functio elementwise(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> {
    redde a ⊙ b
}
",
    );

    assert!(dump.contains("_0.cell0_0 * _1.cell0_0"), "{dump}");
    assert!(dump.contains("_0.cell0_1 * _1.cell0_1"), "{dump}");
    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn lowers_glyph_tensor_hadamard_to_tensor_mul_collection_op() {
    let dump = dump_source(
        r"
functio elementwise(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    redde a ⊙ b
}
",
    );

    assert!(dump.contains("runtime collection tensor_mul"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
}

#[test]
fn lowers_glyph_tensor_hadamard_divide_to_tensor_div_collection_op() {
    let dump = dump_source(
        r"
functio elementwise(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    redde a ⊘ b
}
",
    );

    assert!(dump.contains("runtime collection tensor_div"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
}

// ===========================================================================
// `+` / `-` addita / subtrahe family
// ===========================================================================

#[test]
fn lowers_tensor_infix_plus_to_tensor_add_collection_op() {
    let dump = dump_source(
        r"
functio combine(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    redde a + b
}
",
    );

    assert!(dump.contains("runtime collection tensor_add"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
}

#[test]
fn lowers_tensor_infix_minus_to_tensor_sub_collection_op() {
    let dump = dump_source(
        r"
functio combine(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    redde a - b
}
",
    );

    assert!(dump.contains("runtime collection tensor_sub"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
}

// ===========================================================================
// Fail-closed combinations that still typecheck
// ===========================================================================

#[test]
fn lowers_glyph_vector_matrix_dot_to_lane_cell_mul_add() {
    // Row-vector convention: out[j] = sum_k v[k] * m[k, j]; [3] · [3, 2] gives
    // a width-2 vector, one 3-term sum per matrix column.
    let dump = dump_source(
        r"
functio vecmat(vector<f32, magnitudo 3> v, matrix<f32, [3, 2]> m) → vector<f32, magnitudo 2> {
    redde v · m
}
",
    );

    for column in 0..2 {
        for row in 0..3 {
            let term = format!("_0.lane{row} * _1.cell{row}_{column}");
            assert!(dump.contains(&term), "missing `{term}`: {dump}");
        }
    }
    assert!(dump.contains("construct vector"), "{dump}");
    assert!(!dump.contains("is not supported on the MIR target"), "{dump}");
    assert!(!dump.contains("glyph product binary operator"), "{dump}");
}

#[test]
fn rejects_glyph_vector_matrix_dot_inner_dimension_mismatch_at_typecheck() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let Err(errors) = crate::driver::analyze_source(
        &session,
        "test.fab",
        r"
functio vecmat(vector<f32, magnitudo 4> v, matrix<f32, [3, 2]> m) → vector<f32, magnitudo 2> {
    redde v · m
}
",
        AnalyzeRequest::default(),
    ) else {
        panic!("a [4] · [3, 2] product must be rejected");
    };
    assert!(
        format!("{errors:?}").contains("glyph_inner_dim_mismatch"),
        "expected a named inner-dimension diagnostic: {errors:?}"
    );
}

#[test]
fn rejects_glyph_vector_matrix_dot_width_mismatch_at_lowering() {
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Literal(3));
    let vector_ty = types.intern(Type::Vector(elem, width));
    let matrix_ty = glyph_f32_matrix_ty(&mut types);
    let lhs = glyph_operand(0, vector_ty);
    let rhs = glyph_operand(1, matrix_ty);
    let binop = glyph_binop(HirBinOp::Dot, &lhs, &rhs, vector_ty);
    let mut builder = glyph_builder(&types, vector_ty, matrix_ty);

    assert!(
        builder
            .lower_binary(HirBinOp::Dot, &lhs, &rhs, &binop)
            .is_none()
    );
    assert_eq!(builder.errors.len(), 1, "{:?}", builder.errors);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_vector_matrix_vector_width_must_match_matrix_rows"
    );
}
