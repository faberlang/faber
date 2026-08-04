use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBinOp, MirBlock, MirBlockId, MirCallee, MirFunctionId, MirLocal, MirLocalId, MirOperand,
    MirParam, MirPlace, MirProgram, MirStatement, MirTerminator, MirValue, MirValueId,
    MirValueKind,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};
use std::collections::BTreeMap;

fn test_span() -> Span {
    Span::default()
}

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn scalar_f32_ty(types: &mut TypeTable) -> MirType {
    f32_ty(types)
}

/// A rank-2 `tf32[M, N]` tensor type with static literal dims (the matmul /
/// transpose operand shape the contract/plan passes read).
fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

/// A rank-1 `tf32[n]` tensor type.
fn rank1_tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let len = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.intern(Type::Tensor(f32, len)))
}

fn collection_statement(
    op: MirCollectionOp,
    args: &[u32],
    destination: u32,
    ty: MirType,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(destination))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                args: args
                    .iter()
                    .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                    .collect(),
                return_ty: ty,
            },
        },
        span: test_span(),
    }
}

fn assign_statement(destination: u32, lhs: u32, rhs: u32, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(destination)),
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Binary {
                    op: MirBinOp::Add,
                    lhs: MirOperand::Place(MirPlace::local(MirLocalId(lhs))),
                    rhs: MirOperand::Place(MirPlace::local(MirLocalId(rhs))),
                },
                ty,
                span: test_span(),
            },
        },
        span: test_span(),
    }
}

/// A plain (non-collection) function call statement — e.g. the GELU in the
/// diagnosed mlp_loss lane. Call destinations carry no statement-level
/// type; callers type the destination local in the locals table explicitly
/// (the plan resolver reads operand types from the preserved local tables).
fn call_statement(destination: u32, args: &[u32]) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination: Some(MirPlace::local(MirLocalId(destination))),
            callee: MirCallee::Function(MirFunctionId(0)),
            args: args
                .iter()
                .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                .collect(),
        },
        span: test_span(),
    }
}

/// Assemble a single-block device function from params + statements. The
/// statement list is built from `(op, args, destination, ty)` specs plus
/// optional raw statements; every referenced local appears in the locals
/// table typed from its producer (params use their declared type, statement
/// destinations their statement's return type, the return local the function
/// return type).
fn device_function(
    params: &[(u32, MirType)],
    statements: Vec<MirStatement>,
    return_local: Option<u32>,
    return_ty: MirType,
) -> MirFunction {
    let span = test_span();
    let mut type_by_local: std::collections::BTreeMap<u32, MirType> = BTreeMap::default();
    for &(id, ty) in params {
        type_by_local.insert(id, ty);
    }
    for statement in &statements {
        let produced_ty = match &statement.kind {
            MirStatementKind::RuntimeCall { destination, call } => {
                destination.as_ref().map(|_| call.return_ty)
            }
            MirStatementKind::Assign { value, .. } => Some(value.ty),
            _ => None,
        };
        if let (Some(local), Some(ty)) = (statement_destination(statement), produced_ty) {
            type_by_local.insert(local.0, ty);
        }
    }
    if let Some(return_local) = return_local {
        type_by_local.entry(return_local).or_insert(return_ty);
    }
    let locals = type_by_local
        .into_iter()
        .map(|(id, ty)| MirLocal {
            id: MirLocalId(id),
            name: None,
            ty,
            mutable: false,
            span,
        })
        .collect();

    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: params
            .iter()
            .map(|&(id, ty)| MirParam {
                local: MirLocalId(id),
                name: None,
                ty,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals,
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(
                    return_local.map(|id| MirOperand::Place(MirPlace::local(MirLocalId(id)))),
                ),
                span,
            },
            span,
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// The `mlp_loss` forward shape (S5-U1): `t1 = x @ w1; t2 = t1 @ w2;
/// loss = mean(t2)` over 4×4 tensors — two `TiledMatMul` recipes feeding a
/// `TreeReduction(Mean)` recipe in one body.
fn mlp_loss_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let scalar = scalar_f32_ty(types);
    device_function(
        &[(0, r2), (1, r2), (3, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[2, 3], 4, r2),
            collection_statement(MirCollectionOp::TensorMean, &[4], 5, scalar),
        ],
        Some(5),
        scalar,
    )
}

/// The matmul-VJP companion shape (S5-U1): `xt = transpose(x);
/// dw = xt @ grad` — a `Transpose` recipe feeding a `TiledMatMul` recipe.
fn matmul_vjp_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2), (1, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorTranspose, &[0], 2, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[2, 1], 3, r2),
        ],
        Some(3),
        r2,
    )
}

/// The S3-B1 forward shape: `product = mul(x, w); s = sum(product)` — the
/// elementwise mul folds into the reduction subchain.
fn mul_sum_function(types: &mut TypeTable) -> MirFunction {
    let r1 = rank1_tensor_ty(types, 1000);
    device_function(
        &[(0, r1), (1, r1)],
        vec![
            collection_statement(MirCollectionOp::TensorMul, &[0, 1], 2, r1),
            collection_statement(MirCollectionOp::Sum, &[2], 3, r1),
        ],
        Some(3),
        r1,
    )
}

/// A body whose two recipe results both feed one elementwise combine — the
/// non-linear data-flow case (subchains may consume more than one prior
/// subchain): `s = sum(x); m = mean(x); t = s + m`.
fn sum_mean_function(types: &mut TypeTable) -> MirFunction {
    let r1 = rank1_tensor_ty(types, 16);
    device_function(
        &[(0, r1)],
        vec![
            collection_statement(MirCollectionOp::Sum, &[0], 1, r1),
            collection_statement(MirCollectionOp::TensorMean, &[0], 2, r1),
            assign_statement(3, 1, 2, r1),
        ],
        Some(3),
        r1,
    )
}

/// A body mixing a recipe op with an unplannable op:
/// `t = matmul(x, w); s = softmax(t)` — the decomposition must fail with a
/// typed diagnostic naming the unplannable op, never silently drop it.
fn matmul_softmax_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2), (1, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            collection_statement(MirCollectionOp::TensorSoftmax, &[2], 3, r2),
        ],
        Some(3),
        r2,
    )
}

/// An elementwise-only body: `t = mul(x, w)` — one elementwise subchain.
fn elementwise_mul_function(types: &mut TypeTable) -> MirFunction {
    let r1 = rank1_tensor_ty(types, 4);
    device_function(
        &[(0, r1), (1, r1)],
        vec![collection_statement(
            MirCollectionOp::TensorMul,
            &[0, 1],
            2,
            r1,
        )],
        Some(2),
        r1,
    )
}

/// The mlp_loss GELU-lane shape (D-2a): `h1 = matmul(x, w1);
/// h1b = h1 + h1; a1 = gelu(h1b); h2 = matmul(a1, w2); loss = mean(h2)` —
/// subchain 0 folds the elementwise Add + GELU (plain Call) after its
/// matmul, and the GELU destination `a1` (NOT `h1b`) is read by the second
/// matmul's subchain.
fn mlp_loss_gelu_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let scalar = scalar_f32_ty(types);
    let mut function = device_function(
        &[(0, r2), (1, r2), (5, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            assign_statement(3, 2, 2, r2),
            call_statement(4, &[3]),
            collection_statement(MirCollectionOp::TensorMatMul, &[4, 5], 6, r2),
            collection_statement(MirCollectionOp::TensorMean, &[6], 7, scalar),
        ],
        Some(7),
        scalar,
    );
    // The plain Call's destination is not typed by the statement; type it in
    // the locals table (the plan resolver reads operand types from the
    // preserved source local tables).
    function.locals.push(MirLocal {
        id: MirLocalId(4),
        name: None,
        ty: r2,
        mutable: false,
        span: test_span(),
    });
    function
}

// -----------------------------------------------------------------------
// mlp_loss-shaped decomposition (2×MatMul + Mean).
// -----------------------------------------------------------------------

#[test]
fn mlp_loss_body_decomposes_into_two_matmul_plus_mean() {
    let mut types = TypeTable::new();
    let function = mlp_loss_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("mlp_loss body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        3,
        "2×MatMul + Mean must split into three subchain kernels, got {:?}",
        decomposition.subchains
    );

    // Kernel 0: the first matmul (x @ w1), with the gelu-free forward slice.
    let first = &decomposition.subchains[0];
    assert!(
        matches!(first.plan, CollectionKernelPlan::TiledMatMul(_)),
        "subchain 0 must carry the tiled-matmul plan, got {:?}",
        first.plan
    );
    assert_eq!(first.statements, vec![0]);
    assert!(
        first.consumes.is_empty(),
        "the first subchain consumes nothing"
    );
    assert_eq!(
        first.produced,
        Some(MirLocalId(2)),
        "subchain 0 produces t1"
    );

    // Kernel 1: the second matmul (t1 @ w2), consuming subchain 0's output.
    let second = &decomposition.subchains[1];
    assert!(
        matches!(second.plan, CollectionKernelPlan::TiledMatMul(_)),
        "subchain 1 must carry the tiled-matmul plan, got {:?}",
        second.plan
    );
    assert_eq!(second.statements, vec![1]);
    assert_eq!(
        second.consumes,
        vec![0],
        "subchain 1 consumes t1 from subchain 0"
    );
    assert_eq!(
        second.produced,
        Some(MirLocalId(4)),
        "subchain 1 produces t2"
    );

    // Kernel 2: the mean reduction, consuming subchain 1's output.
    let third = &decomposition.subchains[2];
    let CollectionKernelPlan::TreeReduction(reduction) = &third.plan else {
        panic!(
            "subchain 2 must carry the tree-reduction plan, got {:?}",
            third.plan
        );
    };
    assert_eq!(reduction.op, crate::kernel_plan::ReduceOp::Mean);
    assert_eq!(reduction.length, 16, "the 4×4 mean reduces 16 elements");
    assert_eq!(third.statements, vec![2]);
    assert_eq!(
        third.consumes,
        vec![1],
        "subchain 2 consumes t2 from subchain 1"
    );
    assert_eq!(
        third.produced,
        Some(MirLocalId(5)),
        "subchain 2 produces the loss"
    );
    third
        .plan
        .validate()
        .expect("the carried mean plan validates");
}

// -----------------------------------------------------------------------
// matmul-VJP-shaped decomposition (Transpose + MatMul).
// -----------------------------------------------------------------------

#[test]
fn matmul_vjp_body_decomposes_into_transpose_plus_matmul() {
    let mut types = TypeTable::new();
    let function = matmul_vjp_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("matmul-VJP body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        2,
        "Transpose + MatMul must split into two subchain kernels, got {:?}",
        decomposition.subchains
    );

    let transpose = &decomposition.subchains[0];
    let CollectionKernelPlan::Transpose(plan) = &transpose.plan else {
        panic!(
            "subchain 0 must carry the transpose plan, got {:?}",
            transpose.plan
        );
    };
    assert_eq!(
        (plan.m, plan.n),
        (4, 4),
        "the 4×4 transpose is rank-2 [4,4] → [4,4]"
    );
    assert_eq!(transpose.statements, vec![0]);
    assert!(transpose.consumes.is_empty());
    assert_eq!(
        transpose.produced,
        Some(MirLocalId(2)),
        "transpose subchain produces xt"
    );
    transpose
        .plan
        .validate()
        .expect("the carried transpose plan validates");

    let matmul = &decomposition.subchains[1];
    assert!(
        matches!(matmul.plan, CollectionKernelPlan::TiledMatMul(_)),
        "subchain 1 must carry the tiled-matmul plan, got {:?}",
        matmul.plan
    );
    assert_eq!(matmul.statements, vec![1]);
    assert_eq!(
        matmul.consumes,
        vec![0],
        "the matmul subchain consumes the transposed operand from subchain 0"
    );
    assert_eq!(matmul.produced, Some(MirLocalId(3)));
    matmul
        .plan
        .validate()
        .expect("the carried matmul plan validates");
}

// -----------------------------------------------------------------------
// Elementwise folding and non-linear data-flow.
// -----------------------------------------------------------------------

#[test]
fn mul_folds_into_reduction_subchain() {
    // The S3-B1 forward shape: mul(x, w) then sum(product) — ONE subchain
    // carrying the reduction recipe with the elementwise mul folded in.
    let mut types = TypeTable::new();
    let function = mul_sum_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("mul+sum body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        1,
        "mul + sum must fold into one reduction subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::TreeReduction(reduction) = &decomposition.subchains[0].plan else {
        panic!(
            "the folded body must carry the reduction plan, got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(reduction.op, crate::kernel_plan::ReduceOp::Sum);
    assert_eq!(reduction.length, 1000);
    assert_eq!(decomposition.subchains[0].statements, vec![0, 1]);
}

#[test]
fn elementwise_only_body_is_one_elementwise_subchain() {
    let mut types = TypeTable::new();
    let function = elementwise_mul_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("elementwise body decomposes");

    assert_eq!(decomposition.subchains.len(), 1);
    assert_eq!(
        decomposition.subchains[0].plan,
        CollectionKernelPlan::Elementwise
    );
    assert_eq!(decomposition.subchains[0].statements, vec![0]);
}

#[test]
fn divergent_data_flow_consumes_multiple_prior_subchains() {
    // s = sum(x); m = mean(x); t = s + m: the two recipe results feed one
    // elementwise combine, so the second subchain consumes BOTH prior
    // subchains' produced locals (the explicit data-flow edges model the
    // non-linear graph).
    let mut types = TypeTable::new();
    let function = sum_mean_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("sum+mean body decomposes");

    assert_eq!(decomposition.subchains.len(), 2);
    let CollectionKernelPlan::TreeReduction(sum_plan) = &decomposition.subchains[0].plan else {
        panic!(
            "subchain 0 must be the sum reduction, got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(sum_plan.op, crate::kernel_plan::ReduceOp::Sum);
    let CollectionKernelPlan::TreeReduction(mean_plan) = &decomposition.subchains[1].plan else {
        panic!(
            "subchain 1 must be the mean reduction, got {:?}",
            decomposition.subchains[1].plan
        );
    };
    assert_eq!(mean_plan.op, crate::kernel_plan::ReduceOp::Mean);
    assert_eq!(decomposition.subchains[1].consumes, vec![0]);
    assert_eq!(decomposition.subchains[1].statements, vec![1, 2]);
}

// -----------------------------------------------------------------------
// Fail-closed (typed diagnostics, never silent).
// -----------------------------------------------------------------------

#[test]
fn unplannable_op_mixed_with_recipe_fails_closed() {
    let mut types = TypeTable::new();
    let function = matmul_softmax_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("an unplannable op mixed with a recipe must fail");
    assert!(
        error.message.contains("TensorSoftmax"),
        "the diagnostic must name the unplannable op, got: {}",
        error.message
    );
    assert!(
        error.message.contains("no kernel plan"),
        "the diagnostic must say no kernel plan, got: {}",
        error.message
    );
}

#[test]
fn control_flow_body_fails_closed() {
    // A two-block body (a branch between a matmul and a reduction) cannot be
    // sliced into an ordered subchain chain — typed diagnostic, never a
    // silent Elementwise.
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 4]);
    let scalar = scalar_f32_ty(&mut types);
    let span = test_span();
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: r2,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![collection_statement(
                    MirCollectionOp::TensorMatMul,
                    &[0, 0],
                    1,
                    r2,
                )],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Goto(MirBlockId(1)),
                    span,
                },
                span,
            },
            MirBlock {
                id: MirBlockId(1),
                statements: vec![collection_statement(
                    MirCollectionOp::TensorMean,
                    &[1],
                    2,
                    scalar,
                )],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(2),
                    )))),
                    span,
                },
                span,
            },
        ],
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    };
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("a control-flow body must fail closed");
    assert!(
        error.message.contains("control flow"),
        "the diagnostic must name the control-flow condition, got: {}",
        error.message
    );
}

#[test]
fn dead_recipe_op_is_dropped_not_a_boundary() {
    // S3-A5: a recipe op whose result is never read is not part of the
    // kernel semantics — it must neither force a recipe boundary nor break
    // the decomposition. `t = matmul(x, w); s = sum(x)` where the matmul
    // result is dead: only the sum subchain survives.
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 4]);
    let r1 = rank1_tensor_ty(&mut types, 16);
    let function = device_function(
        &[(0, r2), (1, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            collection_statement(MirCollectionOp::Sum, &[0], 3, r1),
        ],
        Some(3),
        r1,
    );
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("dead recipe op must be dropped, not a boundary");
    assert_eq!(
        decomposition.subchains.len(),
        1,
        "the dead matmul must not create a subchain, got {:?}",
        decomposition.subchains
    );
    assert!(
        matches!(
            decomposition.subchains[0].plan,
            CollectionKernelPlan::TreeReduction(_)
        ),
        "the surviving sum must carry the reduction plan, got {:?}",
        decomposition.subchains[0].plan
    );
    assert_eq!(decomposition.subchains[0].statements, vec![1]);
}

// -----------------------------------------------------------------------
// Consumed-output sets (D-2a): locals a subchain writes that a later
// subchain of the same function reads (or the function returns) must be
// exposed as additional kernel outputs beside the recipe result.
// -----------------------------------------------------------------------

#[test]
fn folded_elementwise_destination_consumed_by_later_subchain_is_exposed() {
    // The GELU-lane shape: subchain 0 folds [MatMul → h1, Add → h1b,
    // GELU → a1]; the second matmul reads a1, NOT h1b. Subchain 0's output
    // set must expose a1 IN ADDITION to the recipe result h1 — the contract
    // output alone (h1) leaves a1 with no kernel producer for the later
    // subchain's input buffer.
    let mut types = TypeTable::new();
    let function = mlp_loss_gelu_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("gelu-lane body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        3,
        "MatMul + Add + GELU, MatMul, Mean must split into three subchains, got {:?}",
        decomposition.subchains
    );

    // Subchain 0: the matmul with its folded elementwise tail.
    let first = &decomposition.subchains[0];
    assert_eq!(first.statements, vec![0, 1, 2]);
    assert!(
        matches!(first.plan, CollectionKernelPlan::TiledMatMul(_)),
        "subchain 0 must carry the tiled-matmul plan, got {:?}",
        first.plan
    );
    assert_eq!(
        first.outputs,
        vec![MirLocalId(2), MirLocalId(4)],
        "subchain 0 must expose the recipe result h1 (primary) and the gelu \
         destination a1 (read by subchain 1), in that deterministic order"
    );
    assert!(
        !first.outputs.contains(&MirLocalId(3)),
        "h1b is consumed only inside subchain 0 and must NOT be exposed"
    );

    // Subchain 1: the second matmul reads a1 (local 4), so its recipe
    // result h2 is its only consumed output.
    let second = &decomposition.subchains[1];
    assert_eq!(second.statements, vec![3]);
    assert_eq!(
        second.outputs,
        vec![MirLocalId(6)],
        "subchain 1 exposes its recipe result h2"
    );

    // Subchain 2 (last): the output set is the function's return local,
    // unchanged.
    let third = &decomposition.subchains[2];
    assert_eq!(third.statements, vec![4]);
    assert_eq!(
        third.outputs,
        vec![MirLocalId(7)],
        "the last subchain's output set is the function's return local"
    );
}

#[test]
fn unconsumed_elementwise_destination_is_not_exposed() {
    // Same lane, but the second matmul reads h1 instead of a1: neither a1
    // nor h1b is read by a later subchain, so subchain 0 exposes ONLY its
    // recipe result h1.
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 4]);
    let scalar = scalar_f32_ty(&mut types);
    let mut function = device_function(
        &[(0, r2), (1, r2), (5, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            assign_statement(3, 2, 2, r2),
            call_statement(4, &[3]),
            collection_statement(MirCollectionOp::TensorMatMul, &[2, 5], 6, r2),
            collection_statement(MirCollectionOp::TensorMean, &[6], 7, scalar),
        ],
        Some(7),
        scalar,
    );
    function.locals.push(MirLocal {
        id: MirLocalId(4),
        name: None,
        ty: r2,
        mutable: false,
        span: test_span(),
    });
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation).expect("body decomposes");

    assert_eq!(decomposition.subchains.len(), 3);
    assert_eq!(
        decomposition.subchains[0].outputs,
        vec![MirLocalId(2)],
        "a1 and h1b are consumed only inside subchain 0 and must NOT be exposed; \
         only the recipe result h1 (read by the later matmul) is"
    );
    assert_eq!(
        decomposition.subchains[2].outputs,
        vec![MirLocalId(7)],
        "the last subchain still exposes the function's return local"
    );
}

#[test]
fn plain_fixture_outputs_are_the_recipe_results() {
    // The recipe-only fixtures fold no elementwise destinations, so every
    // intermediate subchain's output set is its recipe result and the last
    // subchain exposes the return local.
    let mut types = TypeTable::new();
    let mlp = mlp_loss_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&mlp, &validation).expect("mlp_loss body decomposes");
    let outputs: Vec<Vec<MirLocalId>> = decomposition
        .subchains
        .iter()
        .map(|subchain| subchain.outputs.clone())
        .collect();
    assert_eq!(
        outputs,
        vec![
            vec![MirLocalId(2)],
            vec![MirLocalId(4)],
            vec![MirLocalId(5)]
        ],
        "mlp_loss subchains expose t1, t2, and the loss return local"
    );

    let mut types = TypeTable::new();
    let vjp = matmul_vjp_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&vjp, &validation).expect("matmul-VJP body decomposes");
    let outputs: Vec<Vec<MirLocalId>> = decomposition
        .subchains
        .iter()
        .map(|subchain| subchain.outputs.clone())
        .collect();
    assert_eq!(
        outputs,
        vec![vec![MirLocalId(2)], vec![MirLocalId(3)]],
        "the VJP subchains expose xt and the dw return local"
    );
}

// -----------------------------------------------------------------------
// D-2b-1: the synthetic subchain function's return carries its output set.
// -----------------------------------------------------------------------

/// The mlp_loss GELU-lane shape with the GELU as an ELEMENTWISE collection
/// op (a `TensorGelu` runtime call) instead of the plain Call: `h1 =
/// matmul(x, w1); h1b = h1 + h1; a1 = gelu(h1b); h2 = matmul(a1, w2);
/// loss = mean(h2)` — subchain 0 folds the Add + TensorGelu and exposes `a1`
/// as an additional output, and the whole body VALIDATES (the plain-Call
/// variant's callee has no resolvable signature in the isolated fixture, and
/// tensor addition is a collection op, not a binary arithmetic value).
fn mlp_loss_gelu_collection_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let scalar = scalar_f32_ty(types);
    device_function(
        &[(0, r2), (1, r2), (5, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            collection_statement(MirCollectionOp::TensorAdd, &[2, 2], 3, r2),
            collection_statement(MirCollectionOp::TensorGelu, &[3], 4, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[4, 5], 6, r2),
            collection_statement(MirCollectionOp::TensorMean, &[6], 7, scalar),
        ],
        Some(7),
        scalar,
    )
}

#[test]
fn tuple_return_of_multi_output_subchain_validates_and_is_recognized() {
    // D-2b-1: an intermediate subchain with additional consumed outputs
    // returns a TUPLE of its output set (`[primary, extra_outputs...]` in
    // `outputs` order). The tuple element types come from the source
    // function's locals (the synthetic keeps the source local tables); the
    // tuple TYPE must be pre-interned — the seam holds only `&TypeTable`,
    // mirroring the reverse-AD `find_tuple` pattern.
    let mut types = TypeTable::new();
    let function = mlp_loss_gelu_collection_function(&mut types);
    // The tuple type is interned from the SOURCE LOCALS' types (the ids the
    // synthetic's `find_tuple` resolves — each `rank2_tensor_ty` call makes a
    // fresh tensor id, so the fixture's locals are the source of truth).
    let local2_ty = source_local_ty(&function, MirLocalId(2));
    let local4_ty = source_local_ty(&function, MirLocalId(4));
    let scalar = scalar_f32_ty(&mut types);
    types.tuple(vec![local2_ty.semantic_id(), local4_ty.semantic_id()]);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("gelu-lane body decomposes");
    assert_eq!(
        decomposition.subchains[0].outputs,
        vec![MirLocalId(2), MirLocalId(4)],
        "subchain 0 exposes the recipe result h1 and the gelu destination a1"
    );

    let synthetic = decomposition.subchain_function(&function, 0, &types);
    // The tuple form matches the ABI's multi-output contract exactly as the
    // signature layer recognizes it (result_tuple_count).
    assert_eq!(
        crate::abi::result_tuple_count(&synthetic, &validation),
        Some(2),
        "result_tuple_count must recognize the tuple return of the output set"
    );
    // The tuple is constructed into a fresh temp and returned.
    assert!(matches!(
        synthetic.blocks[0].terminator.kind,
        MirTerminatorKind::Return(Some(MirOperand::Temp(_)))
    ));
    let tuple_id = types
        .find_tuple(&[local2_ty.semantic_id(), local4_ty.semantic_id()])
        .expect("the tuple type is interned");
    assert_eq!(synthetic.temps.len(), 1, "one tuple temp is added");
    assert_eq!(synthetic.temps[0].ty, MirType::semantic(tuple_id));

    // The tuple-return synthetic validates as MIR.
    let program = MirProgram {
        functions: vec![synthetic],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the tuple-return subchain synthetic validates");
    assert_eq!(validated.program().functions.len(), 1);

    // The last subchain keeps the source return shape (scalar reduction);
    // single-output intermediates return nothing (unchanged shapes).
    let last = decomposition.subchain_function(&function, 2, &types);
    assert_eq!(last.return_ty, scalar);
    assert_eq!(last.temps.len(), 0);
    let second = decomposition.subchain_function(&function, 1, &types);
    assert!(matches!(
        second.blocks[0].terminator.kind,
        MirTerminatorKind::Return(None)
    ));
}
