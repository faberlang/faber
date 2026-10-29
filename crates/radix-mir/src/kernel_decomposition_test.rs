use super::*;
use crate::elementwise_plan::{ElementwiseExpr, ElementwiseOp};
use crate::validate::ValidatedMir;
use crate::{
    MirBinOp, MirBlock, MirBlockId, MirCallee, MirConstant, MirFunctionId, MirLocal, MirLocalId,
    MirOperand, MirParam, MirPlace, MirProgram, MirStatement, MirTerminator, MirValue, MirValueId,
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

fn rank3_tensor_ty(types: &mut TypeTable, dims: [u64; 3]) -> MirType {
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

/// The gather ids operand (K7c): a rank-1 `tensor<u32, [count]>`.
fn gather_ids_tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let ids_element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let extent = types.intern_index(IndexExpr::Literal(count));
    let shape = types.intern_index(IndexExpr::Tuple(vec![extent]));
    MirType::semantic(types.intern(Type::Tensor(ids_element, shape)))
}

fn collection_statement(
    op: MirCollectionOp,
    args: &[u32],
    destination: u32,
    ty: MirType,
) -> MirStatement {
    collection_statement_operands(
        op,
        args.iter()
            .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
            .collect(),
        destination,
        ty,
    )
}

/// A collection statement built from explicit operands (the transformer
/// fixtures carry const trailing operands — axis / epsilon — that the
/// local-id helper cannot express).
fn collection_statement_operands(
    op: MirCollectionOp,
    args: Vec<MirOperand>,
    destination: u32,
    ty: MirType,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(destination))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                args,
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
/// diagnosed `mlp_loss` lane. Call destinations carry no statement-level
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
        if let (Some(local), Some(ty)) = (
            crate::read_analysis::statement_destination(statement),
            produced_ty,
        ) {
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
    let scalar = f32_ty(types);
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

/// The exact admitted staged MLP body: `gate = x @ wg; up = x @ wu;
/// swiglu = gate * up; down = swiglu @ wd`.
fn composed_mlp_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2), (1, r2), (2, r2), (3, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 4, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 2], 5, r2),
            collection_statement(MirCollectionOp::TensorMul, &[4, 5], 6, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[6, 3], 7, r2),
        ],
        Some(7),
        r2,
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

/// A body mixing two recipe ops — a matmul feeding a softmax:
/// `t = matmul(x, w); s = softmax(t)` — S6-P1 admits softmax as the
/// `RowSoftmax` recipe, so the decomposition splits the body into a
/// `TiledMatMul` subchain and a `RowSoftmax` subchain (compound
/// legalization, C5).
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

/// A body mixing a recipe op with a genuinely-unplannable op:
/// `t = matmul(x, w); u = crux_entropia(t, x)` — the decomposition must
/// fail with a typed diagnostic naming the unplannable op, never silently
/// drop it. (The former fixture used softmax, which S6-P1 admits.)
fn matmul_crux_entropia_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let scalar = f32_ty(types);
    device_function(
        &[(0, r2), (1, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            collection_statement(MirCollectionOp::TensorCruxEntropia, &[2, 0], 3, scalar),
        ],
        Some(3),
        scalar,
    )
}

/// A softmax-bearing body: `t = softmax(x)` over a rank-2 `[4,4]` tensor —
/// S6-P1 admits the op as the `RowSoftmax` recipe.
fn softmax_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2)],
        vec![collection_statement(
            MirCollectionOp::TensorSoftmax,
            &[0],
            1,
            r2,
        )],
        Some(1),
        r2,
    )
}

/// A layer-norm-bearing body: `t = layernorm(x, 1, 1e-5)` over `[4,4]` —
/// S6-P1 admits the op as the `LayerNormalization` recipe.
fn layer_norm_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2)],
        vec![collection_statement_operands(
            MirCollectionOp::TensorLayerNorm,
            vec![
                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                MirOperand::Constant(MirConstant::Int(1)),
                MirOperand::Constant(MirConstant::Float(1e-5)),
            ],
            1,
            r2,
        )],
        Some(1),
        r2,
    )
}

/// A sum-axis-bearing body: `t = sum_axis(x, 1)` over `[4,4]` — S6-P1
/// admits the op as the `AxisReduction` recipe.
fn sum_axis_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2)],
        vec![collection_statement_operands(
            MirCollectionOp::TensorSumAxis,
            vec![
                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                MirOperand::Constant(MirConstant::Int(1)),
            ],
            1,
            r2,
        )],
        Some(1),
        r2,
    )
}

/// A compound transformer body: `s = softmax(x); m = sum_axis(s, 1)` — two
/// transformer recipes decompose into two single-recipe subchain kernels
/// (the compound-op legalization, C5).
fn softmax_sum_axis_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorSoftmax, &[0], 1, r2),
            collection_statement_operands(
                MirCollectionOp::TensorSumAxis,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                    MirOperand::Constant(MirConstant::Int(1)),
                ],
                2,
                r2,
            ),
        ],
        Some(2),
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

/// An elementwise-only mul→add chain: `t = mul(x, w); r = add(t, b)`.
fn elementwise_mul_add_function(types: &mut TypeTable) -> MirFunction {
    let r1 = rank1_tensor_ty(types, 8);
    device_function(
        &[(0, r1), (1, r1), (2, r1)],
        vec![
            collection_statement(MirCollectionOp::TensorMul, &[0, 1], 3, r1),
            collection_statement(MirCollectionOp::TensorAdd, &[3, 2], 4, r1),
        ],
        Some(4),
        r1,
    )
}

/// Interleaved recipe: `t = add(x, y); m = matmul(t, w); r = mul(m, s)`.
/// The matmul is a fusion barrier — surrounding elementwise ops fold into
/// the recipe subchain; the body must not classify as elementwise.
fn interleaved_add_matmul_mul_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2), (1, r2), (3, r2), (5, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorAdd, &[0, 1], 2, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[2, 3], 4, r2),
            collection_statement(MirCollectionOp::TensorMul, &[4, 5], 6, r2),
        ],
        Some(6),
        r2,
    )
}

/// A tuple-`Construct` statement (the lowered `redde iuncta<...> [f0, f1]`):
/// destination local + ordered plain-local fields.
fn tuple_construct_statement(destination: u32, fields: &[u32], ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace::local(MirLocalId(destination)),
            aggregate: MirAggregate {
                kind: MirAggregateKind::Tuple,
                ty,
                fields: MirAggregateFields::Ordered(
                    fields
                        .iter()
                        .map(|&field| {
                            MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                MirLocalId(field),
                            )))
                        })
                        .collect(),
                ),
            },
        },
        span: test_span(),
    }
}

/// A gradient-companion-shaped tuple-return body (D-6): three recipe
/// subchains compute `xt`, the gradient locals `gw`/`gb`, and the loss; a
/// trailing `Construct` assembles the FULL result tuple `[loss, gw, gb]`
/// (the loss field 0 + the gradient fields 1 and 2 — the shape whose
/// full-tuple indices the caller's `gradient_slot` facts index).
fn gradient_companion_tuple_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let scalar = f32_ty(types);
    let tuple_ty = MirType::semantic(types.tuple(vec![
        scalar.semantic_id(),
        r2.semantic_id(),
        r2.semantic_id(),
    ]));
    device_function(
        &[(0, r2), (1, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorTranspose, &[0], 4, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[4, 1], 5, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[4, 0], 7, r2),
            collection_statement(MirCollectionOp::TensorMean, &[5], 6, scalar),
            tuple_construct_statement(8, &[6, 5, 7], tuple_ty),
        ],
        Some(8),
        tuple_ty,
    )
}

/// The `mlp_loss` GELU-lane shape (D-2a): `h1 = matmul(x, w1);
/// h1b = h1 + h1; a1 = gelu(h1b); h2 = matmul(a1, w2); loss = mean(h2)` —
/// subchain 0 folds the elementwise Add + GELU (plain Call) after its
/// matmul, and the GELU destination `a1` (NOT `h1b`) is read by the second
/// matmul's subchain.
fn mlp_loss_gelu_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let scalar = f32_ty(types);
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
// The staged MLP body decomposes into fork/join subchains — Gate and Up
// are independent roots, the activation join is its own elementwise child,
// and no edge exists that the MIR dataflow does not carry.
// -----------------------------------------------------------------------

#[test]
fn composed_mlp_body_decomposes_into_fork_join_subchains() {
    let mut types = TypeTable::new();
    let function = composed_mlp_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("composed MLP body decomposes by the dataflow rule");

    assert_eq!(
        decomposition.subchains.len(),
        4,
        "gate/up + join + down must split into four subchain kernels, got {:?}",
        decomposition.subchains
    );

    // Child 0: the gate matmul — an independent root.
    let gate = &decomposition.subchains[0];
    assert!(matches!(gate.plan, CollectionKernelPlan::TiledMatMul(_)));
    assert_eq!(gate.statements, vec![0]);
    assert!(gate.consumes.is_empty(), "gate must be a root");
    assert_eq!(gate.produced, Some(MirLocalId(4)));
    assert_eq!(gate.outputs, vec![MirLocalId(4)]);

    // Child 1: the up matmul — an independent root. The false Gate → Up
    // dependency the old source-order fold manufactured must stay absent.
    let up = &decomposition.subchains[1];
    assert!(matches!(up.plan, CollectionKernelPlan::TiledMatMul(_)));
    assert_eq!(up.statements, vec![1]);
    assert!(
        up.consumes.is_empty(),
        "up must not consume gate (no Gate → Up edge), got {:?}",
        up.consumes
    );
    assert_eq!(up.produced, Some(MirLocalId(5)));
    assert_eq!(up.outputs, vec![MirLocalId(5)]);

    // Child 2: the activation join — an elementwise-only child consuming
    // BOTH producer lineages.
    let join = &decomposition.subchains[2];
    assert!(matches!(join.plan, CollectionKernelPlan::Elementwise));
    assert_eq!(join.statements, vec![2]);
    assert_eq!(
        join.consumes,
        vec![0, 1],
        "the join must consume both roots, got {:?}",
        join.consumes
    );
    assert!(
        join.elementwise_plan.is_some(),
        "the join child must carry a typed elementwise expression plan"
    );
    assert_eq!(join.produced, Some(MirLocalId(6)));
    assert_eq!(join.outputs, vec![MirLocalId(6)]);

    // Child 3: the down matmul, consuming only the joined activation.
    let down = &decomposition.subchains[3];
    assert!(matches!(down.plan, CollectionKernelPlan::TiledMatMul(_)));
    assert_eq!(down.statements, vec![3]);
    assert_eq!(down.consumes, vec![2]);
    assert_eq!(down.produced, Some(MirLocalId(7)));
    assert_eq!(down.outputs, vec![MirLocalId(7)]);

    // The root set is exactly {gate, up}.
    let roots: Vec<usize> = decomposition
        .subchains
        .iter()
        .enumerate()
        .filter(|(_, subchain)| subchain.consumes.is_empty())
        .map(|(index, _)| index)
        .collect();
    assert_eq!(roots, vec![0, 1]);
}

/// The recognizer near-miss (non-MLP by statement kinds): the SwiGLU-shaped
/// mul sits AFTER the down matmul, so the composed recognizer has no stage
/// ordering to offer. The body must decompose by the ordinary
/// recipe-boundary rule — the recognizer's veto no longer gates this pass.
fn swiglu_after_down_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    device_function(
        &[(0, r2), (1, r2), (2, r2), (3, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 4, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 2], 5, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[4, 5], 6, r2),
            collection_statement(MirCollectionOp::TensorMul, &[4, 5], 7, r2),
        ],
        Some(6),
        r2,
    )
}

#[test]
fn recognizer_vetoed_body_decomposes_by_the_ordinary_rule() {
    let mut types = TypeTable::new();
    let function = swiglu_after_down_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("the near-miss body decomposes by the ordinary rule");

    assert_eq!(
        decomposition.subchains.len(),
        4,
        "three live recipe ops plus a two-lineage join must split into four \
         subchain kernels, got {:?}",
        decomposition.subchains
    );
    assert_eq!(decomposition.subchains[0].statements, vec![0]);
    assert_eq!(decomposition.subchains[1].statements, vec![1]);
    // The third matmul is its own subchain; the trailing mul reads BOTH
    // earlier matmul results, so it is its own elementwise join child.
    assert_eq!(decomposition.subchains[2].statements, vec![2]);
    let join = &decomposition.subchains[3];
    assert_eq!(join.statements, vec![3]);
    assert!(matches!(join.plan, CollectionKernelPlan::Elementwise));
    assert_eq!(
        join.consumes,
        vec![0, 1],
        "the join must consume both earlier lineages, got {:?}",
        join.consumes
    );
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
// S6-P1 transformer legalization (softmax / layer-norm / sum-axis bodies
// decompose into single-recipe subchain kernels — the C5 fix).
// -----------------------------------------------------------------------

#[test]
fn softmax_body_decomposes_into_one_row_softmax_subchain() {
    let mut types = TypeTable::new();
    let function = softmax_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("softmax body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a single softmax body carries exactly one subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::RowSoftmax(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "the softmax subchain must carry exactly one recipe (row-softmax), got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(plan.axis, 1, "row-wise over the last axis");
    assert_eq!(decomposition.subchains[0].statements, vec![0]);
    assert_eq!(decomposition.subchains[0].produced, Some(MirLocalId(1)));
    decomposition.subchains[0]
        .plan
        .validate()
        .expect("the carried row-softmax plan validates");
}

#[test]
fn layer_norm_body_decomposes_into_one_layer_normalization_subchain() {
    let mut types = TypeTable::new();
    let function = layer_norm_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("layer-norm body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a single layer-norm body carries exactly one subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::LayerNormalization(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "the layer-norm subchain must carry exactly one recipe (layer-normalization), got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(plan.axis, 1, "axis comes from the const operand");
    assert_eq!(decomposition.subchains[0].statements, vec![0]);
    decomposition.subchains[0]
        .plan
        .validate()
        .expect("the carried layer-normalization plan validates");
}

#[test]
fn sum_axis_body_decomposes_into_one_axis_reduction_subchain() {
    let mut types = TypeTable::new();
    let function = sum_axis_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("sum_axis body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a single sum_axis body carries exactly one subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::AxisReduction(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "the sum_axis subchain must carry exactly one recipe (axis-reduction), got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(plan.op, crate::kernel_plan::ReduceOp::Sum);
    assert_eq!(plan.axis, 1, "axis comes from the trailing const operand");
    decomposition.subchains[0]
        .plan
        .validate()
        .expect("the carried axis-reduction plan validates");
}

#[test]
fn softmax_then_sum_axis_decomposes_into_two_transformer_subchains() {
    // The compound legalization (C5): a body carrying softmax AND sum_axis
    // (the softmax VJP's reduction lane) splits into two single-recipe
    // subchain kernels with the explicit data-flow edge between them.
    let mut types = TypeTable::new();
    let function = softmax_sum_axis_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("softmax+sum_axis body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        2,
        "softmax + sum_axis must split into two subchains, got {:?}",
        decomposition.subchains
    );

    let softmax = &decomposition.subchains[0];
    let CollectionKernelPlan::RowSoftmax(softmax_plan) = &softmax.plan else {
        panic!(
            "subchain 0 must carry the row-softmax plan, got {:?}",
            softmax.plan
        );
    };
    assert_eq!(softmax_plan.axis, 1);
    assert_eq!(softmax.statements, vec![0]);
    assert!(softmax.consumes.is_empty());
    assert_eq!(softmax.produced, Some(MirLocalId(1)));

    let reduction = &decomposition.subchains[1];
    let CollectionKernelPlan::AxisReduction(reduction_plan) = &reduction.plan else {
        panic!(
            "subchain 1 must carry the axis-reduction plan, got {:?}",
            reduction.plan
        );
    };
    assert_eq!(reduction_plan.op, crate::kernel_plan::ReduceOp::Sum);
    assert_eq!(reduction_plan.axis, 1);
    assert_eq!(reduction.statements, vec![1]);
    assert_eq!(
        reduction.consumes,
        vec![0],
        "the axis-reduction subchain reads subchain 0's softmax output"
    );
    assert_eq!(reduction.produced, Some(MirLocalId(2)));
}

#[test]
fn matmul_softmax_body_decomposes_into_matmul_plus_softmax() {
    // The C5 legalization across recipe families: matmul feeding softmax
    // splits into a TiledMatMul subchain and a RowSoftmax subchain.
    let mut types = TypeTable::new();
    let function = matmul_softmax_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("matmul+softmax body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        2,
        "matmul + softmax must split into two subchains, got {:?}",
        decomposition.subchains
    );
    assert!(
        matches!(
            decomposition.subchains[0].plan,
            CollectionKernelPlan::TiledMatMul(_)
        ),
        "subchain 0 must carry the tiled-matmul plan, got {:?}",
        decomposition.subchains[0].plan
    );
    let CollectionKernelPlan::RowSoftmax(softmax_plan) = &decomposition.subchains[1].plan else {
        panic!(
            "subchain 1 must carry the row-softmax plan, got {:?}",
            decomposition.subchains[1].plan
        );
    };
    assert_eq!(softmax_plan.axis, 1);
    assert_eq!(
        decomposition.subchains[1].consumes,
        vec![0],
        "the softmax subchain reads the matmul output"
    );
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

/// OF-2: an elementwise-only subchain exposes the typed expression plan
/// (today `subchain.plan` is the payload-less `Elementwise` variant).
#[test]
fn elementwise_subchain_carries_explicit_expression_plan() {
    let mut types = TypeTable::new();
    let function = elementwise_mul_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("elementwise body decomposes");

    assert_eq!(decomposition.subchains.len(), 1);
    let subchain = &decomposition.subchains[0];
    assert_eq!(subchain.plan, CollectionKernelPlan::Elementwise);
    assert_eq!(subchain.statements, vec![0]);
    assert!(subchain.consumes.is_empty());
    assert_eq!(subchain.produced, Some(MirLocalId(2)));
    assert_eq!(subchain.outputs, vec![MirLocalId(2)]);

    let plan = subchain
        .expression_plan()
        .expect("elementwise-only subchain must carry a typed expression plan");
    assert_eq!(plan.output.local, MirLocalId(2));
    assert_eq!(plan.shape.dims, vec![4]);
    assert_eq!(plan.output.shape, vec![4]);
    match &plan.expression {
        ElementwiseExpr::Binary {
            op: ElementwiseOp::Mul,
            ..
        } => {}
        other => panic!("expected Mul expression, got {other:?}"),
    }
    assert_eq!(
        plan.inputs
            .iter()
            .map(|input| input.local)
            .collect::<Vec<_>>(),
        vec![MirLocalId(0), MirLocalId(1)]
    );
}

/// OF-2: a mul→add elementwise chain is one subchain with a source-order
/// composed expression; topology (slice / consume / produce / outputs)
/// stays a single elementwise kernel.
#[test]
fn elementwise_mul_add_chain_carries_composed_expression_plan() {
    let mut types = TypeTable::new();
    let function = elementwise_mul_add_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("mul+add body decomposes");

    assert_eq!(decomposition.subchains.len(), 1);
    let subchain = &decomposition.subchains[0];
    assert_eq!(subchain.plan, CollectionKernelPlan::Elementwise);
    assert_eq!(subchain.statements, vec![0, 1]);
    assert!(subchain.consumes.is_empty());
    assert_eq!(subchain.produced, Some(MirLocalId(4)));
    assert_eq!(subchain.outputs, vec![MirLocalId(4)]);

    let plan = subchain
        .expression_plan()
        .expect("elementwise chain must carry a typed expression plan");
    assert_eq!(plan.output.local, MirLocalId(4));
    match &plan.expression {
        ElementwiseExpr::Binary {
            op: ElementwiseOp::Add,
            lhs,
            ..
        } => match lhs.as_ref() {
            ElementwiseExpr::Binary {
                op: ElementwiseOp::Mul,
                ..
            } => {}
            other => panic!("expected inner Mul, got {other:?}"),
        },
        other => panic!("expected Add(Mul(..), ..), got {other:?}"),
    }
}

/// OF-2: an interleaved recipe remains a recipe barrier — surrounding
/// elementwise ops fold into the matmul subchain; the recipe is sliced
/// out, not reclassified as elementwise.
#[test]
fn interleaved_recipe_slices_out_and_is_not_elementwise() {
    let mut types = TypeTable::new();
    let function = interleaved_add_matmul_mul_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("interleaved add/matmul/mul decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        1,
        "elementwise ops fold into the adjacent recipe, got {:?}",
        decomposition.subchains
    );
    let subchain = &decomposition.subchains[0];
    assert!(
        matches!(subchain.plan, CollectionKernelPlan::TiledMatMul(_)),
        "the interleaved matmul is a recipe barrier, got {:?}",
        subchain.plan
    );
    assert_eq!(subchain.statements, vec![0, 1, 2]);
    assert!(
        subchain.expression_plan().is_none(),
        "a recipe subchain must not carry an elementwise expression plan"
    );
    assert!(subchain.consumes.is_empty());
    assert_eq!(subchain.produced, Some(MirLocalId(6)));
    assert_eq!(subchain.outputs, vec![MirLocalId(6)]);
}

/// A temp-forward `_dest = %temp` copy (the real lowering's shape: a
/// collection call writes a temp, then a plain Assign copies it into a
/// named local).
fn assign_from_temp(destination: u32, source: MirTempId, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(destination)),
            value: MirValue {
                id: MirValueId(destination),
                kind: MirValueKind::Operand(MirOperand::Temp(source)),
                ty,
                span: test_span(),
            },
        },
        span: test_span(),
    }
}

/// The `train_step_bert_linear` elementwise shape that trips the
/// per-kernel storage-buffer budget: 12 `[8]` current params + 12 `[8]`
/// gradients + an f32 scalar lr = 25 params, 12 scaled-grad muls, 12
/// subtract updates, and a 12-element tuple return — 37 storage buffers as
/// ONE kernel, above the 31-buffer binding maximum. Built with the real
/// lowering's temp shapes (collection calls write temps, the forwards copy
/// them into locals, and the terminator returns a tuple TEMP built by a
/// tuple `Construct`).
fn oversized_elementwise_train_step(types: &mut TypeTable) -> MirFunction {
    let tensor = rank1_tensor_ty(types, 8);
    let scalar = f32_ty(types);
    let span = test_span();
    let tuple_ty = MirType::semantic(types.intern(Type::Tuple(vec![tensor.semantic_id(); 12])));
    let mut params = Vec::with_capacity(25);
    for id in 0..24u32 {
        params.push((id, tensor));
    }
    params.push((24, scalar));
    let mut statements = Vec::new();
    let mut temps = Vec::new();
    // 12 scaled-grad muls: `%i = tensor_mul(grad_i, lr); s_i = %i`.
    for i in 0..12u32 {
        let temp = MirTempId(i);
        temps.push(MirTemp {
            id: temp,
            ty: tensor,
            span,
        });
        statements.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace {
                    base: MirPlaceBase::Temp(temp),
                    projections: Vec::new(),
                }),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMul),
                    args: vec![
                        MirOperand::Place(MirPlace::local(MirLocalId(12 + i))),
                        MirOperand::Place(MirPlace::local(MirLocalId(24))),
                    ],
                    return_ty: tensor,
                },
            },
            span,
        });
        statements.push(assign_from_temp(30 + i, temp, tensor));
    }
    // 12 update subs: `%j = tensor_sub(param_i, s_i); n_i = %j`.
    for i in 0..12u32 {
        let temp = MirTempId(12 + i);
        temps.push(MirTemp {
            id: temp,
            ty: tensor,
            span,
        });
        statements.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace {
                    base: MirPlaceBase::Temp(temp),
                    projections: Vec::new(),
                }),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSub),
                    args: vec![
                        MirOperand::Place(MirPlace::local(MirLocalId(i))),
                        MirOperand::Place(MirPlace::local(MirLocalId(30 + i))),
                    ],
                    return_ty: tensor,
                },
            },
            span,
        });
        statements.push(assign_from_temp(42 + i, temp, tensor));
    }
    // The 12-element return-tuple assembly (temp-based, the real shape).
    let tuple_temp = MirTempId(24);
    temps.push(MirTemp {
        id: tuple_temp,
        ty: tuple_ty,
        span,
    });
    statements.push(MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace {
                base: MirPlaceBase::Temp(tuple_temp),
                projections: Vec::new(),
            },
            aggregate: MirAggregate {
                kind: MirAggregateKind::Tuple,
                ty: tuple_ty,
                fields: MirAggregateFields::Ordered(
                    (42..54u32)
                        .map(|id| {
                            MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                MirLocalId(id),
                            )))
                        })
                        .collect(),
                ),
            },
        },
        span,
    });
    let mut locals = Vec::with_capacity(54);
    for id in 0..54u32 {
        locals.push(MirLocal {
            id: MirLocalId(id),
            name: None,
            ty: tensor,
            mutable: false,
            span,
        });
    }
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: params
            .into_iter()
            .map(|(id, ty)| MirParam {
                local: MirLocalId(id),
                name: None,
                ty,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals,
        temps,
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Temp(tuple_temp))),
                span,
            },
            span,
        }],
        return_ty: tuple_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// The per-kernel storage-buffer budget: an elementwise-only body whose
/// whole-function binding count exceeds the selected storage-buffer limit
/// splits into consecutive elementwise sub-slices, each at or under the
/// default 31-buffer budget, with the return assembly kept in the LAST
/// sub-slice.
#[test]
fn wider_lane_keeps_oversized_elementwise_body_whole() {
    let mut types = TypeTable::new();
    let function = oversized_elementwise_train_step(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function_with_limit(
        &function,
        &validation,
        crate::abi::StorageBufferLimitRecord::new("wide-test", 64),
    )
    .expect("the 64-buffer lane admits the whole elementwise body");

    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a 37-buffer body stays whole under the 64-buffer lane"
    );
    assert_eq!(
        decomposition.subchains[0].statements,
        (0..function.blocks[0].statements.len()).collect::<Vec<_>>()
    );
}

/// A deliberately out-of-window shape for the fixed candidate policy. The
/// extra pre-range statements put the first field boundary at 51. The
/// existing temp at position 0 is referenced again at position 25, so ends
/// 1..=25 are invalid; end 26 closes that temp and is valid under the former
/// unbounded scan. A second temp defined at position 26 is referenced by the
/// first field-range call, so ends 27..=51 are invalid. The guard must fire
/// before the 26th candidate (end 26) can be accepted.
fn oversized_elementwise_candidate_window(types: &mut TypeTable) -> MirFunction {
    let tensor = rank1_tensor_ty(types, 8);
    let mut function = oversized_elementwise_train_step(types);
    let mut extra = vec![
        assign_statement(54, 0, 1, tensor),
        assign_statement(55, 0, 1, tensor),
    ];
    let extra_temp = MirTempId(25);
    extra.push(MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace {
                base: MirPlaceBase::Temp(extra_temp),
                projections: Vec::new(),
            }),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                args: vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                ],
                return_ty: tensor,
            },
        },
        span: test_span(),
    });
    for destination in 56..80u32 {
        extra.push(assign_statement(destination, 0, 1, tensor));
    }
    function.blocks[0].statements.splice(24..24, extra);
    // The temp's forward must be in the field range, just beyond the largest
    // pre-range candidate, so every candidate above end 26 stays unclosed.
    let first_field_call = &mut function.blocks[0].statements[51];
    let MirStatementKind::RuntimeCall { call, .. } = &mut first_field_call.kind else {
        panic!("the first field-range statement must be the update call");
    };
    call.args[1] = MirOperand::Temp(extra_temp);
    function.temps.push(MirTemp {
        id: extra_temp,
        ty: tensor,
        span: test_span(),
    });
    for id in 54..80u32 {
        function.locals.push(MirLocal {
            id: MirLocalId(id),
            name: None,
            ty: tensor,
            mutable: false,
            span: test_span(),
        });
    }
    function
}

#[test]
fn oversized_elementwise_candidate_window_rejects_26th_candidate() {
    let mut types = TypeTable::new();
    let function = oversized_elementwise_candidate_window(&mut types);
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("the 26th candidate must fail closed before the old valid split");

    assert_eq!(
        error.message,
        "elementwise split candidate window exceeded (maximum 25 candidate ends)"
    );
}

#[test]
fn oversized_elementwise_body_splits_within_the_buffer_budget() {
    let mut types = TypeTable::new();
    let function = oversized_elementwise_train_step(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("oversized elementwise body decomposes into budget-fitting subchains");

    assert!(
        decomposition.subchains.len() > 1,
        "an elementwise body over the 31-buffer maximum must split, got {:?}",
        decomposition.subchains
    );
    // D-2b-2: pre-intern the multi-output subchain tuple return types (the
    // caller contract `subchain_function` requires, mirroring the faber
    // decomposition path).
    let mut subchain_types = TypeTable::from_snapshot(types.snapshot()).expect("live table");
    decomposition.intern_output_tuple_types(&function, &mut subchain_types);
    let subchain_validation = validation.with_types(&subchain_types);
    let subchain_count = decomposition.subchains.len();
    let tuple_arity = 12u32;
    for (position, subchain) in decomposition.subchains.iter().enumerate() {
        assert_eq!(
            subchain.plan,
            CollectionKernelPlan::Elementwise,
            "every split sub-slice stays elementwise-only"
        );
        let synthetic = decomposition.subchain_function(
            &function,
            position,
            subchain_validation.types.as_table(),
        );
        let outputs = if position + 1 == subchain_count {
            // The temp-based return keeps the source assembly: the signature
            // binds the FULL result-tuple count.
            usize::try_from(tuple_arity).unwrap_or(usize::MAX)
        } else {
            subchain.outputs.len()
        };
        let buffer_count = synthetic.params.len() + outputs;
        assert!(
            buffer_count <= crate::abi::DEFAULT_STORAGE_BUFFER_LIMIT.max_storage_buffers,
            "subchain {position} must fit the {}-buffer budget, \
             got {buffer_count} ({} params + {outputs} outputs)",
            crate::abi::DEFAULT_STORAGE_BUFFER_LIMIT.max_storage_buffers,
            synthetic.params.len()
        );
    }
    // The statements partition the source stream in order, and the return
    // assembly (the final tuple Construct) lives in the LAST sub-slice.
    let mut seen: Vec<usize> = Vec::new();
    for subchain in &decomposition.subchains {
        seen.extend(subchain.statements.iter().copied());
    }
    let expected: Vec<usize> = (0..function.blocks[0].statements.len()).collect();
    assert_eq!(
        seen, expected,
        "the split must partition the source statements in order"
    );
    let last = &decomposition.subchains[subchain_count - 1];
    assert_eq!(
        last.statements.last(),
        Some(&(function.blocks[0].statements.len() - 1)),
        "the LAST sub-slice keeps the return assembly"
    );
    // The data flow is carried: the last sub-slice consumes the pre and
    // field sub-slices' produced locals.
    assert!(
        !last.consumes.is_empty(),
        "the last sub-slice must consume the earlier sub-slices' outputs"
    );
}

/// `oversized_elementwise_train_step` with one dead `Sum` recipe statement
/// (its result local is never read) spliced in at statement `at`. The
/// slicer drops the dead recipe, so the elementwise slice's statement
/// indices are no longer contiguous from zero.
fn oversized_elementwise_train_step_with_dead_recipe(
    types: &mut TypeTable,
    at: usize,
) -> MirFunction {
    let tensor = rank1_tensor_ty(types, 8);
    let mut function = oversized_elementwise_train_step(types);
    function.locals.push(MirLocal {
        id: MirLocalId(54),
        name: None,
        ty: tensor,
        mutable: false,
        span: test_span(),
    });
    function.blocks[0].statements.insert(
        at,
        collection_statement(MirCollectionOp::Sum, &[0], 54, tensor),
    );
    function
}

/// A dropped dead recipe leaves a gap in the elementwise slice's statement
/// indices. The split indexes its per-position tables by position, not by
/// statement index, so the same body with a dead recipe at `at` must split
/// exactly as the body without it, shifted past the dead statement.
fn assert_dead_recipe_does_not_perturb_the_split(at: usize) {
    let mut types = TypeTable::new();
    let baseline_function = oversized_elementwise_train_step(&mut types);
    let function = oversized_elementwise_train_step_with_dead_recipe(&mut types, at);
    let validation = MirValidationContext::new(&types);
    let baseline = decompose_kernel_function(&baseline_function, &validation)
        .expect("the baseline oversized body decomposes");
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("a dead recipe must not break the oversized elementwise split");

    let shift = |statement: usize| {
        if statement >= at {
            statement + 1
        } else {
            statement
        }
    };
    let expected: Vec<Vec<usize>> = baseline
        .subchains
        .iter()
        .map(|subchain| subchain.statements.iter().copied().map(shift).collect())
        .collect();
    let actual: Vec<Vec<usize>> = decomposition
        .subchains
        .iter()
        .map(|subchain| subchain.statements.clone())
        .collect();
    assert_eq!(
        actual, expected,
        "the dead recipe at statement {at} must be dropped and the split unchanged"
    );
    assert!(
        actual.iter().flatten().all(|&statement| statement != at),
        "the dead recipe at statement {at} must belong to no subchain"
    );
}

#[test]
fn oversized_elementwise_split_survives_dead_recipe_at_statement_zero() {
    assert_dead_recipe_does_not_perturb_the_split(0);
}

#[test]
fn oversized_elementwise_split_survives_dead_recipe_inside_the_pre_range() {
    assert_dead_recipe_does_not_perturb_the_split(7);
}

#[test]
fn divergent_data_flow_consumes_multiple_prior_subchains() {
    // s = sum(x); m = mean(x); t = s + m: Sum and Mean are independent
    // roots, and the combine is its own elementwise join child consuming
    // BOTH prior subchains (the explicit data-flow edges model the
    // non-linear graph without serializing the roots).
    let mut types = TypeTable::new();
    let function = sum_mean_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("sum+mean body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        3,
        "sum/mean + join must split into three subchain kernels, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::TreeReduction(sum_plan) = &decomposition.subchains[0].plan else {
        panic!(
            "subchain 0 must be the sum reduction, got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(sum_plan.op, crate::kernel_plan::ReduceOp::Sum);
    assert!(
        decomposition.subchains[0].consumes.is_empty(),
        "sum must be a root"
    );
    let CollectionKernelPlan::TreeReduction(mean_plan) = &decomposition.subchains[1].plan else {
        panic!(
            "subchain 1 must be the mean reduction, got {:?}",
            decomposition.subchains[1].plan
        );
    };
    assert_eq!(mean_plan.op, crate::kernel_plan::ReduceOp::Mean);
    assert!(
        decomposition.subchains[1].consumes.is_empty(),
        "mean must not consume sum (no Sum → Mean edge), got {:?}",
        decomposition.subchains[1].consumes
    );
    let join = &decomposition.subchains[2];
    assert!(matches!(join.plan, CollectionKernelPlan::Elementwise));
    assert_eq!(join.statements, vec![2]);
    assert_eq!(
        join.consumes,
        vec![0, 1],
        "the combine must consume both roots, got {:?}",
        join.consumes
    );
}

// -----------------------------------------------------------------------
// The real Faber-lowered MLP body: fork/join over temps, forwards, and
// the expanded SiLU chain, ending in a MutRef output forward.
// -----------------------------------------------------------------------

#[test]
fn real_lowered_mlp_body_decomposes_into_fork_join_subchains() {
    let mut types = TypeTable::new();
    let function =
        crate::device_program_plans::tests::composed_mlp_real_lowered_function(&mut types);
    let validation = MirValidationContext::new(&types);

    // Drift check: the fixture must stay shape-faithful to the Faber
    // lowering — temp-destination matmuls + typed forwards, the expanded
    // SiLU, the separate multiply, the MutRef output forward, and the
    // vacuum return. A lowering change must break this fixture loudly
    // instead of silently re-baselining the oracle.
    let block = &function.blocks[0];
    let ops: Vec<Option<MirCollectionOp>> = block
        .statements
        .iter()
        .map(crate::collection_ops::statement_collection_op)
        .collect();
    let expected_ops = [
        Some(MirCollectionOp::TensorMatMul),
        None, // gate result forward
        Some(MirCollectionOp::TensorMatMul),
        None,                             // up result forward
        Some(MirCollectionOp::TensorNeg), // silu: -gate
        None,                             // exp (unary assign)
        Some(MirCollectionOp::TensorAdd), // +1
        Some(MirCollectionOp::TensorDiv), // gate / (1 + exp(-gate))
        None,                             // activated forward
        Some(MirCollectionOp::TensorMul), // fused = activated * up
        None,                             // fused forward
        Some(MirCollectionOp::TensorMatMul),
        None, // MutRef output forward
    ];
    assert_eq!(
        ops.len(),
        expected_ops.len(),
        "the real-lowered statement shape drifted: {ops:?}"
    );
    for (index, (op, expected)) in ops.iter().zip(expected_ops.iter()).enumerate() {
        assert_eq!(
            op, expected,
            "statement {index} drifted from the real lowered shape"
        );
    }
    // The mutable output parameter rides the last statement's forward.
    assert!(
        function
            .params
            .iter()
            .any(|param| { param.local == MirLocalId(4) && param.mode == MirParamMode::MutRef })
    );

    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("the real lowered body decomposes by the dataflow rule");

    let slices: Vec<Vec<usize>> = decomposition
        .subchains
        .iter()
        .map(|subchain| subchain.statements.clone())
        .collect();
    assert_eq!(
        slices,
        vec![
            vec![0, 1],
            vec![2, 3],
            vec![4, 5, 6, 7, 8, 9, 10],
            vec![11, 12],
        ],
        "the fork/join partition must match the dataflow, got {slices:?}"
    );
    assert!(matches!(
        decomposition.subchains[0].plan,
        CollectionKernelPlan::TiledMatMul(_)
    ));
    assert!(matches!(
        decomposition.subchains[1].plan,
        CollectionKernelPlan::TiledMatMul(_)
    ));
    assert!(matches!(
        decomposition.subchains[2].plan,
        CollectionKernelPlan::Elementwise
    ));
    assert!(matches!(
        decomposition.subchains[3].plan,
        CollectionKernelPlan::TiledMatMul(_)
    ));
    assert!(
        decomposition.subchains[0].consumes.is_empty(),
        "gate is a root"
    );
    assert!(
        decomposition.subchains[1].consumes.is_empty(),
        "up must not consume gate (no Gate → Up edge), got {:?}",
        decomposition.subchains[1].consumes
    );
    assert_eq!(
        decomposition.subchains[2].consumes,
        vec![0, 1],
        "the join consumes both roots, got {:?}",
        decomposition.subchains[2].consumes
    );
    assert_eq!(decomposition.subchains[3].consumes, vec![2]);
    // The MutRef output forward rides the down child's slice.
    assert_eq!(
        crate::read_analysis::statement_destination(&block.statements[12]),
        Some(MirLocalId(4)),
        "the down child must carry the mutable output forward"
    );
}

fn expect_binary(
    expr: &ElementwiseExpr,
    op: ElementwiseOp,
) -> (&ElementwiseExpr, &ElementwiseExpr) {
    match expr {
        ElementwiseExpr::Binary {
            op: found,
            lhs,
            rhs,
            ..
        } if *found == op => (lhs, rhs),
        other => panic!("expected Binary({op:?}), got {other:?}"),
    }
}

fn expect_unary(expr: &ElementwiseExpr, op: ElementwiseOp) -> &ElementwiseExpr {
    match expr {
        ElementwiseExpr::Unary {
            op: found, operand, ..
        } if *found == op => operand,
        other => panic!("expected Unary({op:?}), got {other:?}"),
    }
}

#[test]
// Comparing the embedded literal 1.0: exact by construction.
#[allow(clippy::float_cmp)]
fn real_lowered_mlp_join_child_carries_typed_expression_plan() {
    let mut types = TypeTable::new();
    let function =
        crate::device_program_plans::tests::composed_mlp_real_lowered_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("the real lowered body decomposes by the dataflow rule");

    // Unit-1A topology is unchanged by 1A-bis (F10): the same four-child
    // partition, the join as child 2, both roots consumed.
    let slices: Vec<Vec<usize>> = decomposition
        .subchains
        .iter()
        .map(|subchain| subchain.statements.clone())
        .collect();
    assert_eq!(
        slices,
        vec![
            vec![0, 1],
            vec![2, 3],
            vec![4, 5, 6, 7, 8, 9, 10],
            vec![11, 12]
        ],
        "the fork/join partition must be unchanged, got {slices:?}"
    );
    let join = &decomposition.subchains[2];
    assert_eq!(join.plan, CollectionKernelPlan::Elementwise);
    assert_eq!(join.consumes, vec![0, 1], "the join consumes both roots");

    let plan = join
        .elementwise_plan
        .as_ref()
        .expect("the join child must carry the typed SiLU+multiply expression plan");

    // Inputs in first-use order: gate, then up (F8).
    assert_eq!(plan.inputs.len(), 2);
    assert_eq!(plan.inputs[0].local, MirLocalId(6), "gate is read first");
    assert_eq!(plan.inputs[1].local, MirLocalId(7), "up is read second");

    // Output: fused, shape [2, 3] (F8).
    assert_eq!(plan.output.local, MirLocalId(9));
    assert_eq!(plan.output.shape, vec![2, 3]);

    // The exact lowered body tree, in MIR operand order — Exp is an
    // explicit operation node, never a collapsed SiLU (F8):
    // Mul(Div(gate, Add(Exp(Neg(gate)), 1.0)), up).
    let (mul_lhs, mul_rhs) = expect_binary(&plan.expression, ElementwiseOp::Mul);
    assert!(
        matches!(mul_rhs, ElementwiseExpr::Input { slot: 1, .. }),
        "the Mul rhs is the up input"
    );
    let (div_lhs, div_rhs) = expect_binary(mul_lhs, ElementwiseOp::Div);
    assert!(
        matches!(div_lhs, ElementwiseExpr::Input { slot: 0, .. }),
        "the Div lhs is the gate input"
    );
    let (add_lhs, add_rhs) = expect_binary(div_rhs, ElementwiseOp::Add);
    let exp_operand = expect_unary(add_lhs, ElementwiseOp::Exp);
    let neg_operand = expect_unary(exp_operand, ElementwiseOp::Neg);
    assert!(
        matches!(neg_operand, ElementwiseExpr::Input { slot: 0, .. }),
        "Neg reads the gate input"
    );
    assert!(
        matches!(add_rhs, ElementwiseExpr::Scalar(MirConstant::Float(value)) if *value == 1.0),
        "the Add rhs is the embedded 1.0 scalar, got {add_rhs:?}"
    );

    // Provenance operations in source order (F8).
    let ops: Vec<Option<ElementwiseOp>> = plan.provenance.iter().map(|fact| fact.op).collect();
    assert_eq!(
        ops,
        vec![
            Some(ElementwiseOp::Neg),
            Some(ElementwiseOp::Exp),
            Some(ElementwiseOp::Add),
            Some(ElementwiseOp::Div),
            Some(ElementwiseOp::Mul),
        ],
        "provenance must follow the source statement order"
    );

    // Complete coverage facts (F7): both local forwards and all five temp
    // producers belong to the expression — Unit 3 never reconstructs this.
    assert_eq!(
        plan.composed_locals,
        vec![MirLocalId(8), MirLocalId(9)],
        "activated and fused are the composed locals"
    );
    assert_eq!(
        plan.composed_temps,
        vec![
            crate::MirTempId(2),
            crate::MirTempId(3),
            crate::MirTempId(4),
            crate::MirTempId(5),
            crate::MirTempId(6),
        ],
        "the five SiLU/join temps are the composed temps"
    );
}

// -----------------------------------------------------------------------
// Generic topology controls: the rule is derived from reads, writes, and
// lineage — never from operation names, root counts, or join arity.
// -----------------------------------------------------------------------

#[test]
fn three_independent_roots_fan_in_through_chained_joins() {
    // sum(x) ∥ mean(x) ∥ max(x) → add → mul: more than two independently
    // ready roots, and the binary joins chain into ONE join child whose
    // lineage is the union of all three roots (arbitrary-arity fan-in via
    // the run union — never a root serialized behind a sibling).
    let mut types = TypeTable::new();
    let r1 = rank1_tensor_ty(&mut types, 16);
    let function = device_function(
        &[(0, r1)],
        vec![
            collection_statement(MirCollectionOp::Sum, &[0], 2, r1),
            collection_statement(MirCollectionOp::TensorMean, &[0], 3, r1),
            collection_statement(MirCollectionOp::TensorMax, &[0], 4, r1),
            collection_statement(MirCollectionOp::TensorAdd, &[2, 3], 5, r1),
            collection_statement(MirCollectionOp::TensorMul, &[5, 4], 6, r1),
        ],
        Some(6),
        r1,
    );
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("the three-root body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        4,
        "three roots plus one union join child, got {:?}",
        decomposition.subchains
    );
    let roots: Vec<usize> = decomposition
        .subchains
        .iter()
        .enumerate()
        .filter(|(_, subchain)| subchain.consumes.is_empty())
        .map(|(index, _)| index)
        .collect();
    assert_eq!(roots, vec![0, 1, 2], "the three recipes must all be roots");
    let join = &decomposition.subchains[3];
    assert!(matches!(join.plan, CollectionKernelPlan::Elementwise));
    assert_eq!(join.statements, vec![3, 4]);
    assert_eq!(
        join.consumes,
        vec![0, 1, 2],
        "the join child consumes all three roots, got {:?}",
        join.consumes
    );
}

#[test]
fn fan_out_feeds_two_consumers_without_invented_edges() {
    // matmul(x, w) → h; softmax(h) → s; mean(h) → m; add(s, m) → t: h fans
    // out to TWO child kernels; the consumers join, and neither carries an
    // edge the dataflow does not justify.
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 4]);
    let scalar = f32_ty(&mut types);
    let function = device_function(
        &[(0, r2), (1, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 2, r2),
            collection_statement(MirCollectionOp::TensorSoftmax, &[2], 3, r2),
            collection_statement(MirCollectionOp::TensorMean, &[2], 4, scalar),
            collection_statement(MirCollectionOp::TensorAdd, &[3, 4], 5, scalar),
        ],
        Some(5),
        scalar,
    );
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("the fan-out body decomposes");

    assert_eq!(
        decomposition.subchains.len(),
        4,
        "mm ∥ softmax ∥ mean → join must be four children, got {:#?}",
        decomposition.subchains
    );
    assert!(
        decomposition.subchains[0].consumes.is_empty(),
        "h's producer is the root"
    );
    assert_eq!(
        decomposition.subchains[1].consumes,
        vec![0],
        "the softmax child consumes h"
    );
    assert_eq!(
        decomposition.subchains[2].consumes,
        vec![0],
        "the mean child consumes the same h buffer"
    );
    assert!(
        !decomposition.subchains[1].consumes.contains(&2)
            && !decomposition.subchains[2].consumes.contains(&1),
        "fan-out: the two consumers must not depend on each other"
    );
    assert_eq!(
        decomposition.subchains[3].consumes,
        vec![1, 2],
        "the join consumes both consumers of h"
    );
}

#[test]
fn topology_control_uses_four_plan_kinds_without_position_logic() {
    // softmax(x) ∥ layernorm(x) → mul join → sum: four different plan kinds
    // in one fork/join graph; dependency discovery never inspects which kind
    // sits at which graph position.
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 4]);
    let scalar = f32_ty(&mut types);
    let function = device_function(
        &[(0, r2)],
        vec![
            collection_statement(MirCollectionOp::TensorSoftmax, &[0], 2, r2),
            collection_statement_operands(
                MirCollectionOp::TensorLayerNorm,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Constant(MirConstant::Int(1)),
                    MirOperand::Constant(MirConstant::Float(1e-5)),
                ],
                3,
                r2,
            ),
            collection_statement(MirCollectionOp::TensorMul, &[2, 3], 4, r2),
            collection_statement(MirCollectionOp::Sum, &[4], 5, scalar),
        ],
        Some(5),
        scalar,
    );
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("the four-kind body decomposes");

    assert_eq!(decomposition.subchains.len(), 4);
    assert!(matches!(
        decomposition.subchains[0].plan,
        CollectionKernelPlan::RowSoftmax(_)
    ));
    assert!(matches!(
        decomposition.subchains[1].plan,
        CollectionKernelPlan::LayerNormalization(_)
    ));
    assert!(matches!(
        decomposition.subchains[2].plan,
        CollectionKernelPlan::Elementwise
    ));
    assert!(matches!(
        decomposition.subchains[3].plan,
        CollectionKernelPlan::TreeReduction(_)
    ));
    assert!(decomposition.subchains[0].consumes.is_empty());
    assert!(decomposition.subchains[1].consumes.is_empty());
    assert_eq!(decomposition.subchains[2].consumes, vec![0, 1]);
    assert_eq!(decomposition.subchains[3].consumes, vec![2]);
}

// -----------------------------------------------------------------------
// Property cross-check: every carried consumes edge is justified by a
// read of the producing subchain's written locals, and every read of an
// earlier subchain's local produces an edge — on every fixture.
// -----------------------------------------------------------------------

fn assert_consumes_match_reads(
    name: &str,
    types: &mut TypeTable,
    function: &MirFunction,
    decomposition: &KernelDecomposition,
) {
    decomposition.intern_output_tuple_types(function, types);
    let block = &function.blocks[0];
    let written: Vec<std::collections::BTreeSet<MirLocalId>> = decomposition
        .subchains
        .iter()
        .map(|subchain| {
            subchain
                .statements
                .iter()
                .filter_map(|&index| {
                    crate::read_analysis::statement_destination(&block.statements[index])
                })
                .collect()
        })
        .collect();
    for (index, subchain) in decomposition.subchains.iter().enumerate() {
        let synthetic = decomposition.subchain_function(function, index, types);
        let inputs: BTreeSet<MirLocalId> =
            synthetic.params.iter().map(|param| param.local).collect();
        let expected: Vec<usize> = (0..index)
            .filter(|&prior| written[prior].iter().any(|local| inputs.contains(local)))
            .collect();
        assert_eq!(
            subchain.consumes, expected,
            "{name}: subchain {index} carries an edge no read justifies or misses one"
        );
    }
}

#[test]
fn every_emitted_edge_is_justified_by_a_read_across_fixtures() {
    type Fixture = fn(&mut TypeTable) -> MirFunction;
    let fixtures: &[(&str, Fixture)] = &[
        ("composed_mlp", composed_mlp_function),
        ("swiglu_after_down", swiglu_after_down_function),
        ("sum_mean", sum_mean_function),
        ("mlp_loss", mlp_loss_function),
        ("mlp_loss_gelu", mlp_loss_gelu_function),
        ("matmul_vjp", matmul_vjp_function),
        (
            "gradient_companion_tuple",
            gradient_companion_tuple_function,
        ),
        (
            "real_lowered_mlp",
            crate::device_program_plans::tests::composed_mlp_real_lowered_function,
        ),
    ];
    let mut types = TypeTable::new();
    for (name, build) in fixtures {
        let function = build(&mut types);
        let validation = MirValidationContext::new(&types);
        let decomposition = decompose_kernel_function(&function, &validation)
            .unwrap_or_else(|error| panic!("{name}: decomposes, got {error:?}"));
        assert_consumes_match_reads(name, &mut types, &function, &decomposition);
    }
}

// -----------------------------------------------------------------------
// Fail-closed (typed diagnostics, never silent).
// -----------------------------------------------------------------------

#[test]
fn unplannable_op_mixed_with_recipe_fails_closed() {
    let mut types = TypeTable::new();
    let function = matmul_crux_entropia_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("an unplannable op mixed with a recipe must fail");
    assert!(
        error.message.contains("TensorCruxEntropia"),
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
    let scalar = f32_ty(&mut types);
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
         destination a1 (read by subchain 1), varia that deterministic order"
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
    let scalar = f32_ty(&mut types);
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

/// The `mlp_loss` GELU-lane shape with the GELU as an ELEMENTWISE collection
/// op (a `TensorGelu` runtime call) instead of the plain Call: `h1 =
/// matmul(x, w1); h1b = h1 + h1; a1 = gelu(h1b); h2 = matmul(a1, w2);
/// loss = mean(h2)` — subchain 0 folds the Add + `TensorGelu` and exposes `a1`
/// as an additional output, and the whole body VALIDATES (the plain-Call
/// variant's callee has no resolvable signature in the isolated fixture, and
/// tensor addition is a collection op, not a binary arithmetic value).
fn mlp_loss_gelu_collection_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let scalar = f32_ty(types);
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
    let scalar = f32_ty(&mut types);
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

#[test]
fn intern_output_tuple_types_pre_interns_multi_output_subchain_returns() {
    // D-2b-2: `intern_output_tuple_types` pre-interns the tuple return of
    // every multi-output intermediate subchain into the mutable table, so
    // the emission call sites can satisfy `subchain_function`'s `&TypeTable`
    // seam without manually interning per subchain. Idempotent (interning is
    // hash-consed), a no-op for single-output and last subchains.
    let mut types = TypeTable::new();
    let function = mlp_loss_gelu_collection_function(&mut types);
    let local2_ty = source_local_ty(&function, MirLocalId(2));
    let local4_ty = source_local_ty(&function, MirLocalId(4));
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("gelu-lane body decomposes");
    assert_eq!(
        decomposition.subchains[0].outputs,
        vec![MirLocalId(2), MirLocalId(4)],
        "subchain 0 exposes the recipe result h1 and the gelu destination a1"
    );

    let mut subchain_types =
        TypeTable::from_snapshot(types.snapshot()).expect("a live table always restores");
    assert_eq!(
        subchain_types.find_tuple(&[local2_ty.semantic_id(), local4_ty.semantic_id()]),
        None,
        "the tuple is not interned before the helper runs"
    );
    decomposition.intern_output_tuple_types(&function, &mut subchain_types);
    decomposition.intern_output_tuple_types(&function, &mut subchain_types);

    let tuple_id = subchain_types
        .find_tuple(&[local2_ty.semantic_id(), local4_ty.semantic_id()])
        .expect("the helper interns the multi-output subchain's tuple return");
    assert_eq!(
        subchain_types
            .find_tuple(&[local2_ty.semantic_id(), local4_ty.semantic_id()])
            .expect("the tuple stays interned"),
        tuple_id,
        "re-running the helper is idempotent (same interned id)"
    );

    let synthetic = decomposition.subchain_function(&function, 0, &subchain_types);
    assert_eq!(
        synthetic.temps.len(),
        1,
        "one tuple temp is added once the tuple type is interned"
    );
    assert_eq!(synthetic.temps[0].ty, MirType::semantic(tuple_id));
}

// -----------------------------------------------------------------------
// D-6: full result-tuple → local field bindings + producer resolution
// (gradient-slot → buffer mapping for decomposed companions).
// -----------------------------------------------------------------------

#[test]
fn tuple_return_fields_map_full_result_tuple_indices_to_locals() {
    // The gradient-companion shape: the trailing Construct binds full-tuple
    // index 0 → loss (local 6), index 1 → gw (local 5), index 2 → gb
    // (local 7). The gradient-slot facts index these FULL indices (gw=1,
    // gb=2) — never any subchain's local output indices.
    let mut types = TypeTable::new();
    let function = gradient_companion_tuple_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation).expect("body decomposes");

    let fields = decomposition
        .return_tuple_fields(&function)
        .expect("a tuple-return function exposes its full result-tuple fields");
    assert_eq!(
        fields,
        vec![MirLocalId(6), MirLocalId(5), MirLocalId(7)],
        "full-tuple index i binds the local assembled into field i"
    );

    // Every gradient field resolves to the subchain kernel whose output
    // produces its local (the buffers the train_step gradient reads alias).
    // Field 0 (the loss) is written by the LAST subchain and — D-6 — the
    // tuple-return last subchain exposes its written return-tuple fields as
    // its output set (the return-tuple local itself is never a kernel
    // output: the ABI flattens tuple returns into per-field device-view
    // buffers). Fields 1 and 2 are produced by the intermediate subchains
    // that compute them.
    assert_eq!(
        decomposition.producer_position(fields[0]),
        Some((3, 0)),
        "the loss field (assembled inside the last subchain) is exposed as an \
         output of the last subchain"
    );
    assert_eq!(
        decomposition.producer_position(fields[1]),
        Some((1, 0)),
        "gw (field 1) is produced by subchain 1's first output slot"
    );
    assert_eq!(
        decomposition.producer_position(fields[2]),
        Some((2, 0)),
        "gb (field 2) is produced by subchain 2's first output slot"
    );
    assert_eq!(
        decomposition.subchains[3].outputs,
        vec![MirLocalId(6)],
        "the tuple-return last subchain's output set is the return-tuple field \
         it writes (the loss), varia full-tuple index order — never the nested \
         return-tuple local"
    );
}

/// A `dense_qkv`-shaped tuple-return body whose assembly lowers into a TEMP
/// (the real `return tuple<..> [...]` lowering): three matmul recipes each
/// forward their result temp into a stage local, the trailing `Construct`
/// builds the result tuple into a tuple temp, and the terminator returns that
/// temp. The D-6 field scan must resolve this shape exactly as it resolves the
/// local-return companion assembly, or the last subchain keeps a live source
/// tuple assembly the device emitters refuse.
fn dense_qkv_temp_tuple_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let tuple_ty =
        MirType::semantic(types.tuple(vec![r2.semantic_id(), r2.semantic_id(), r2.semantic_id()]));
    let span = test_span();
    let matmul_to_temp = |args: [u32; 2], temp: MirTempId| MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace {
                base: MirPlaceBase::Temp(temp),
                projections: Vec::new(),
            }),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                args: vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(args[0]))),
                    MirOperand::Place(MirPlace::local(MirLocalId(args[1]))),
                ],
                return_ty: r2,
            },
        },
        span,
    };
    let tuple_temp = MirTempId(3);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: [0, 1, 2, 3]
            .into_iter()
            .map(|id| MirParam {
                local: MirLocalId(id),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals: [4, 5, 6]
            .into_iter()
            .map(|id| MirLocal {
                id: MirLocalId(id),
                name: None,
                ty: r2,
                mutable: false,
                span,
            })
            .collect(),
        temps: vec![
            MirTemp {
                id: MirTempId(0),
                ty: r2,
                span,
            },
            MirTemp {
                id: MirTempId(1),
                ty: r2,
                span,
            },
            MirTemp {
                id: MirTempId(2),
                ty: r2,
                span,
            },
            MirTemp {
                id: tuple_temp,
                ty: tuple_ty,
                span,
            },
        ],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                matmul_to_temp([0, 1], MirTempId(0)),
                assign_from_temp(4, MirTempId(0), r2),
                matmul_to_temp([0, 2], MirTempId(1)),
                assign_from_temp(5, MirTempId(1), r2),
                matmul_to_temp([0, 3], MirTempId(2)),
                assign_from_temp(6, MirTempId(2), r2),
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Temp(tuple_temp),
                            projections: Vec::new(),
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple_ty,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(4),
                                ))),
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(5),
                                ))),
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(6),
                                ))),
                            ]),
                        },
                    },
                    span,
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Temp(tuple_temp))),
                span,
            },
            span,
        }],
        return_ty: tuple_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn temp_return_tuple_assembly_exposes_the_field_outputs() {
    // The dense_qkv shape: the return assembly builds the tuple into a temp
    // (the `return tuple<..> [...]` lowering), not into a stage local. The
    // D-6 field scan resolves the temp-return shape, the last subchain
    // exposes the one return-tuple field it writes, and the producer
    // resolution lands every field on the subchain kernel whose output
    // buffer carries it (the per-field result row the host assembles).
    let mut types = TypeTable::new();
    let function = dense_qkv_temp_tuple_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("the dense_qkv body decomposes");
    assert_eq!(
        decomposition.subchains.len(),
        3,
        "one matmul subchain per field"
    );

    let fields = decomposition
        .return_tuple_fields(&function)
        .expect("a temp-return tuple assembly exposes its full result-tuple fields");
    assert_eq!(
        fields,
        vec![MirLocalId(4), MirLocalId(5), MirLocalId(6)],
        "full-tuple index i binds the local assembled into field i"
    );
    assert_eq!(
        decomposition.producer_position(fields[0]),
        Some((0, 0)),
        "q (field 0) is produced by subchain 0's first output slot"
    );
    assert_eq!(
        decomposition.producer_position(fields[1]),
        Some((1, 0)),
        "k (field 1) is produced by subchain 1's first output slot"
    );
    assert_eq!(
        decomposition.producer_position(fields[2]),
        Some((2, 0)),
        "v (field 2) is produced by the last subchain's first output slot"
    );
    assert_eq!(
        decomposition.subchains[2].outputs,
        vec![MirLocalId(6)],
        "the last subchain's output set is the return-tuple field it writes"
    );

    // The emission synthetic replaces the kept source assembly with its own
    // output-set tuple (the 1-tuple the ABI's result-tuple contract
    // recognizes), so the kept source construct is dead — the shape the
    // device emitters accept.
    let mut subchain_types =
        TypeTable::from_snapshot(types.snapshot()).expect("a live table always restores");
    decomposition.intern_output_tuple_types(&function, &mut subchain_types);
    let synthetic = decomposition.subchain_function(&function, 2, &subchain_types);
    let subchain_validation = MirValidationContext::new(&subchain_types);
    assert_eq!(
        crate::abi::result_tuple_count(&synthetic, &subchain_validation),
        Some(1),
        "the synthetic returns the output-set tuple, not the kept source assembly"
    );
}

#[test]
fn scalar_return_function_has_no_tuple_fields() {
    // A scalar-return function (the mlp_loss forward) exposes no full
    // result-tuple fields — the caller's gradient-slot mapping is a no-op.
    let mut types = TypeTable::new();
    let function = mlp_loss_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("mlp_loss body decomposes");
    assert_eq!(
        decomposition.return_tuple_fields(&function),
        None,
        "a scalar return has no tuple Construct field bindings"
    );
    assert_eq!(
        decomposition.producer_position(MirLocalId(2)),
        Some((0, 0)),
        "the producer resolution still resolves forward-save locals"
    );
}

#[test]
fn tuple_return_with_non_plain_field_fails_closed() {
    // A tuple Construct with a non-plain (constant) field cannot bind that
    // full-tuple index to a local — the field mapping fails closed (None),
    // never a silent wrong local.
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 4]);
    let span = test_span();
    let tuple_ty = MirType::semantic(types.tuple(vec![r2.semantic_id()]));
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
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                collection_statement(MirCollectionOp::TensorTranspose, &[0], 1, r2),
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(MirLocalId(2)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple_ty,
                            fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(
                                MirOperand::Constant(MirConstant::Int(7)),
                            )]),
                        },
                    },
                    span,
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(2),
                )))),
                span,
            },
            span,
        }],
        return_ty: tuple_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    };
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation).expect("body decomposes");
    assert_eq!(
        decomposition.return_tuple_fields(&function),
        None,
        "a non-plain tuple field fails the local binding closed"
    );
}

// -----------------------------------------------------------------------
// GI3-1 inference-recipe legalization (rms_norm / rope / gather /
// causal-masked-softmax bodies decompose into single-recipe subchain
// kernels — the new recipes are recipe boundaries like the S6-P1
// transformer ops; a compound body splits at the recipe boundary).
// -----------------------------------------------------------------------

/// A RMSNorm-bearing body: `t = rms_norm(x, 1, 1e-5, gamma)` over `[2, 960]`
/// (the pinned hidden width) with gamma `[960]` — the CCI-1 const layout.
fn rms_norm_decomposition_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [2, 960]);
    let gamma = rank1_tensor_ty(types, 960);
    device_function(
        &[(0, r2), (1, gamma)],
        vec![collection_statement_operands(
            MirCollectionOp::TensorRmsNorm,
            vec![
                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                MirOperand::Constant(MirConstant::Int(1)),
                MirOperand::Constant(MirConstant::Float(1e-5)),
                MirOperand::Place(MirPlace::local(MirLocalId(1))),
            ],
            2,
            r2,
        )],
        Some(2),
        r2,
    )
}

/// A RoPE-bearing body: `t = rope_norm(x, 0, 64, rope_table)` over `[2, 64]`
/// with the explicit interleaved `[2, 32, 3]` table.
fn rope_decomposition_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [2, 64]);
    let table = rank3_tensor_ty(types, [2, 32, 3]);
    device_function(
        &[(0, r2), (1, table)],
        vec![collection_statement_operands(
            MirCollectionOp::TensorRope,
            vec![
                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                MirOperand::Constant(MirConstant::Int(0)),
                MirOperand::Constant(MirConstant::Int(64)),
                MirOperand::Place(MirPlace::local(MirLocalId(1))),
            ],
            2,
            r2,
        )],
        Some(2),
        r2,
    )
}

/// A row-gather-bearing body: `t = gather(table, ids)` over the pinned
/// `[960, 49152]` token-embedding table with 9 ids → `[9, 49152]`.
fn gather_decomposition_function(types: &mut TypeTable) -> MirFunction {
    let table = rank2_tensor_ty(types, [960, 49_152]);
    let output = rank2_tensor_ty(types, [9, 49_152]);
    let ids = gather_ids_tensor_ty(types, 9);
    device_function(
        &[(0, table), (1, ids)],
        vec![collection_statement_operands(
            MirCollectionOp::TensorGather,
            vec![
                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                MirOperand::Place(MirPlace::local(MirLocalId(1))),
            ],
            2,
            output,
        )],
        Some(2),
        output,
    )
}

/// A causal masked-softmax-bearing body: `t = causal_masked_softmax(x)`
/// over the pinned `[9, 9]` causal score matrix.
fn causal_masked_softmax_decomposition_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [9, 9]);
    device_function(
        &[(0, r2)],
        vec![collection_statement(
            MirCollectionOp::TensorCausalMaskedSoftmax,
            &[0],
            1,
            r2,
        )],
        Some(1),
        r2,
    )
}

/// A compound inference body: `n = rms_norm(x, 1, 1e-5, gamma); m = n @ w` —
/// the `RMSNorm` recipe followed by a matmul recipe splits into two
/// single-recipe subchain kernels.
fn rms_norm_then_matmul_decomposition_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [2, 960]);
    let gamma = rank1_tensor_ty(types, 960);
    let weight = rank2_tensor_ty(types, [960, 960]);
    let out = rank2_tensor_ty(types, [2, 960]);
    device_function(
        &[(0, r2), (1, gamma), (2, weight)],
        vec![
            collection_statement_operands(
                MirCollectionOp::TensorRmsNorm,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Constant(MirConstant::Int(1)),
                    MirOperand::Constant(MirConstant::Float(1e-5)),
                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                ],
                3,
                r2,
            ),
            collection_statement(MirCollectionOp::TensorMatMul, &[3, 2], 4, out),
        ],
        Some(4),
        out,
    )
}

#[test]
fn rms_norm_body_decomposes_into_one_rms_normalization_subchain() {
    let mut types = TypeTable::new();
    let function = rms_norm_decomposition_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("the rms_norm body decomposes");
    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a single rms_norm body carries exactly one subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::RmsNormalization(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "the rms_norm subchain must carry exactly one recipe (RMS-normalization), got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(plan.axis, 1, "axis comes from the const operand");
    assert_eq!(plan.width, 960, "the normalization width is the hidden dim");
    decomposition.subchains[0]
        .plan
        .validate()
        .expect("the carried RMS-normalization plan validates");
}

#[test]
fn rope_body_decomposes_into_one_rope_subchain() {
    let mut types = TypeTable::new();
    let function = rope_decomposition_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("the rope body decomposes");
    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a single rope body carries exactly one subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::Rope(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "the rope subchain must carry exactly one recipe (rope), got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(plan.pos, 0, "position comes from the const operand");
    assert_eq!(plan.dim, 64, "rotated width comes from the const operand");
    decomposition.subchains[0]
        .plan
        .validate()
        .expect("the carried rope plan validates");
}

#[test]
fn gather_body_decomposes_into_one_gather_subchain() {
    let mut types = TypeTable::new();
    let function = gather_decomposition_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        decompose_kernel_function(&function, &validation).expect("the gather body decomposes");
    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a single gather body carries exactly one subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::Gather(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "the gather subchain must carry exactly one recipe (gather), got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(
        plan.table_rows,
        crate::kernel_plan::PlanDim::Literal(960),
        "table rows from the operand dims"
    );
    assert_eq!(
        plan.table_cols,
        crate::kernel_plan::PlanDim::Literal(49_152),
        "table cols from the operand dims"
    );
    assert_eq!(
        plan.id_count,
        crate::kernel_plan::PlanDim::Literal(9),
        "id count from the return-type dims"
    );
    decomposition.subchains[0]
        .plan
        .validate()
        .expect("the carried gather plan validates");
}

#[test]
fn causal_masked_softmax_body_decomposes_into_one_causal_subchain() {
    let mut types = TypeTable::new();
    let function = causal_masked_softmax_decomposition_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("the causal masked-softmax body decomposes");
    assert_eq!(
        decomposition.subchains.len(),
        1,
        "a single causal masked-softmax body carries exactly one subchain, got {:?}",
        decomposition.subchains
    );
    let CollectionKernelPlan::CausalMaskedSoftmax(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "the causal masked-softmax subchain must carry exactly one recipe, got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(plan.rows, 9, "the row (query) extent");
    assert_eq!(plan.cols, 9, "the column (key) extent");
    decomposition.subchains[0]
        .plan
        .validate()
        .expect("the carried causal masked-softmax plan validates");
}

#[test]
fn rms_norm_then_matmul_decomposes_into_two_subchains() {
    // The compound legalization: a body carrying RMSNorm AND matmul splits
    // into two single-recipe subchain kernels at the recipe boundary.
    let mut types = TypeTable::new();
    let function = rms_norm_then_matmul_decomposition_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("the rms_norm+matmul body decomposes");
    assert_eq!(
        decomposition.subchains.len(),
        2,
        "rms_norm + matmul must split into two subchains, got {:?}",
        decomposition.subchains
    );
    assert!(
        matches!(
            decomposition.subchains[0].plan,
            CollectionKernelPlan::RmsNormalization(_)
        ),
        "the first subchain carries the RMS-normalization recipe, got {:?}",
        decomposition.subchains[0].plan
    );
    assert!(
        matches!(
            decomposition.subchains[1].plan,
            CollectionKernelPlan::TiledMatMul(_)
        ),
        "the second subchain carries the tiled-matmul recipe, got {:?}",
        decomposition.subchains[1].plan
    );
    for subchain in &decomposition.subchains {
        subchain
            .plan
            .validate()
            .expect("every carried subchain plan validates");
    }
}

// ── R-PACK-02A packed admission through decomposition ─────────────────────

fn quantized_matmul_decomposition_function(
    types: &mut TypeTable,
    ggml_type_id: i64,
) -> MirFunction {
    let left = rank2_tensor_ty(types, [8, 16]);
    let right = rank2_tensor_ty(types, [16, 16]);
    let out = rank2_tensor_ty(types, [8, 16]);
    device_function(
        &[(0, left), (1, right)],
        vec![collection_statement_operands(
            MirCollectionOp::TensorQuantizedMatMul,
            vec![
                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                MirOperand::Constant(MirConstant::Int(ggml_type_id)),
            ],
            2,
            out,
        )],
        Some(2),
        out,
    )
}

fn packed_gather_decomposition_function(types: &mut TypeTable, ggml_type_id: i64) -> MirFunction {
    let table = rank2_tensor_ty(types, [960, 64]);
    let output = rank2_tensor_ty(types, [9, 64]);
    let ids = gather_ids_tensor_ty(types, 9);
    device_function(
        &[(0, table), (1, ids)],
        vec![collection_statement_operands(
            MirCollectionOp::TensorGather,
            vec![
                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                MirOperand::Constant(MirConstant::Int(ggml_type_id)),
            ],
            2,
            output,
        )],
        Some(2),
        output,
    )
}

#[test]
fn q4k_quantized_matmul_decomposes_to_packed_plan() {
    let mut types = TypeTable::new();
    let function = quantized_matmul_decomposition_function(&mut types, 12);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("Q4_K quantized-matmul body decomposes");
    assert_eq!(decomposition.subchains.len(), 1);
    let CollectionKernelPlan::QuantizedMatMul(plan) = &decomposition.subchains[0].plan else {
        panic!(
            "Q4_K fixture must decompose to QuantizedMatMul, got {:?}",
            decomposition.subchains[0].plan
        );
    };
    assert_eq!(plan.layout, crate::abi::PackedStorageLayout::Q4_K);
    assert_eq!(
        plan.dequant_resolution().class,
        crate::kernel_plan::PackedDequantKind::Superblock
    );
}

#[test]
fn f32_quantized_matmul_decomposes_to_tiled_matmul() {
    let mut types = TypeTable::new();
    let function = quantized_matmul_decomposition_function(&mut types, 0);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("F32 quantized-matmul body decomposes via TiledMatMul fallback");
    assert!(
        matches!(
            decomposition.subchains[0].plan,
            CollectionKernelPlan::TiledMatMul(_)
        ),
        "F32 must fall back to TiledMatMul, got {:?}",
        decomposition.subchains[0].plan
    );
}

#[test]
fn packed_section5_layouts_decompose_to_quantized_matmul() {
    use crate::abi::PackedStorageLayout;
    let rows = [
        (PackedStorageLayout::F16, 1),
        (PackedStorageLayout::Bf16, 30),
        (PackedStorageLayout::Q8_0, 8),
        (PackedStorageLayout::Q4_K, 12),
        (PackedStorageLayout::Q5_K, 13),
        (PackedStorageLayout::Q6_K, 14),
        (PackedStorageLayout::Q5_0, 6),
        (PackedStorageLayout::MXFP4, 39),
    ];
    for (layout, id) in rows {
        let mut types = TypeTable::new();
        let function = quantized_matmul_decomposition_function(&mut types, i64::from(id));
        let validation = MirValidationContext::new(&types);
        let decomposition = decompose_kernel_function(&function, &validation)
            .unwrap_or_else(|error| panic!("{layout:?} must decompose, got {error}"));
        let CollectionKernelPlan::QuantizedMatMul(plan) = &decomposition.subchains[0].plan else {
            panic!("{layout:?} must admit QuantizedMatMul");
        };
        assert_eq!(plan.layout, layout);
    }
}

#[test]
fn unrepresentable_quantized_matmul_layout_fails_closed() {
    let mut types = TypeTable::new();
    let function = quantized_matmul_decomposition_function(&mut types, 99);
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("GGML type id 99 must fail closed");
    let message = error.to_string();
    assert!(
        message.contains("admitted GGML type id") || message.contains("unrepresentable"),
        "typed fail-closed diagnostic, got: {message}"
    );
}

#[test]
fn packed_gather_layout_admits_through_decomposition() {
    let mut types = TypeTable::new();
    let function = packed_gather_decomposition_function(&mut types, 12);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("Q4_K gather admits (carried plan stays Gather)");
    assert!(
        matches!(
            decomposition.subchains[0].plan,
            CollectionKernelPlan::Gather(_)
        ),
        "packed gather stays Gather on the wire-stable plan, got {:?}",
        decomposition.subchains[0].plan
    );
    let admitted = crate::kernel_plan::admit_quantized_gather(
        crate::abi::PackedStorageLayout::Q4_K,
        960,
        64,
        9,
    );
    assert!(
        matches!(admitted, crate::kernel_plan::AdmittedGather::Quantized(_)),
        "Q4_K embedding table admits QuantizedGather"
    );
}

#[test]
fn unrepresentable_gather_layout_fails_closed() {
    let mut types = TypeTable::new();
    let function = packed_gather_decomposition_function(&mut types, 99);
    let validation = MirValidationContext::new(&types);
    decompose_kernel_function(&function, &validation)
        .expect_err("unrepresentable gather layout must fail closed");
}

// -----------------------------------------------------------------------
// Explicit output parameter: the store `y ⇇ d` of an
// indexed-view kernel must reach the final subchain's output or fail closed.
// -----------------------------------------------------------------------

/// `y ⇇ d` — a plain copy of a local into a (possibly output) parameter.
fn copy_statement(destination: u32, source: u32, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(destination)),
            value: MirValue {
                id: MirValueId(destination),
                kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(MirLocalId(source)))),
                ty,
                span: test_span(),
            },
        },
        span: test_span(),
    }
}

/// An indexed-view kernel `(x, w, v, b, mut y, id) → vacuum`:
///
/// ```text
/// c = x · w;  e = c · v;  d = e + b;  y ⇇ d
/// ```
///
/// Locals: x0 w1 v2 b3 y4 id5, c6 e7 d8. The two matmuls split into two
/// subchains; the add folds into the second and the copy stores `y`.
fn two_matmul_output_parameter_function(types: &mut TypeTable) -> MirFunction {
    let x = rank2_tensor_ty(types, [4, 3]);
    let w = rank2_tensor_ty(types, [3, 2]);
    let v = rank2_tensor_ty(types, [2, 2]);
    let r2 = rank2_tensor_ty(types, [4, 2]);
    let id = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum = MirType::semantic(types.intern(Type::Primitive(Primitive::Vacuum)));
    let mut function = device_function(
        &[(0, x), (1, w), (2, v), (3, r2), (4, r2), (5, id)],
        vec![
            collection_statement(MirCollectionOp::TensorMatMul, &[0, 1], 6, r2),
            collection_statement(MirCollectionOp::TensorMatMul, &[6, 2], 7, r2),
            assign_statement(8, 7, 3, r2),
            copy_statement(4, 8, r2),
        ],
        None,
        vacuum,
    );
    function.params[4].mode = MirParamMode::MutRef;
    function
}

#[test]
fn final_subchain_binds_the_output_parameter() {
    let mut types = TypeTable::new();
    let function = two_matmul_output_parameter_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("a two-matmul body storing into an output parameter decomposes");

    assert_eq!(decomposition.subchains.len(), 2, "one subchain per matmul");
    assert_eq!(
        decomposition.subchains[0].output_param, None,
        "the first matmul's product is a data-flow buffer, not the output"
    );
    assert_eq!(
        decomposition.subchains[1].output_param,
        Some(MirLocalId(4)),
        "the final subchain stores into `y`"
    );
    assert!(
        decomposition.subchains[1].outputs.is_empty(),
        "the output parameter is not a consumed-output local: {:?}",
        decomposition.subchains[1].outputs
    );

    // The final subchain is the indexed-view kernel the source is: its inputs
    // in read order, then the output parameter, then the unread index.
    let synthetic = decomposition.subchain_function(&function, 1, validation.types.as_table());
    let params: Vec<(MirLocalId, MirParamMode)> = synthetic
        .params
        .iter()
        .map(|param| (param.local, param.mode))
        .collect();
    assert_eq!(
        params,
        vec![
            (MirLocalId(6), MirParamMode::Owned),
            (MirLocalId(2), MirParamMode::Owned),
            (MirLocalId(3), MirParamMode::Owned),
            (MirLocalId(4), MirParamMode::MutRef),
            (MirLocalId(5), MirParamMode::Owned),
        ],
        "c, v, b, then y and the index"
    );
    // The first subchain keeps the contract-shaped surface.
    let first = decomposition.subchain_function(&function, 0, validation.types.as_table());
    let first_params: Vec<MirLocalId> = first.params.iter().map(|param| param.local).collect();
    assert_eq!(first_params, vec![MirLocalId(0), MirLocalId(1)]);
}

#[test]
fn explicit_output_signature_binds_the_output_parameter_and_the_index() {
    let mut types = TypeTable::new();
    let function = two_matmul_output_parameter_function(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation).expect("decomposes");
    let synthetic = decomposition.subchain_function(&function, 1, validation.types.as_table());
    let interner = radix_lexer::Interner::new();

    let signature = crate::device_program_plans::explicit_output_subchain_signature(
        &synthetic,
        MirLocalId(4),
        &validation,
        &interner,
    )
    .expect("the final subchain reads as an indexed-view kernel");

    assert_eq!(signature.output.source_local, Some(MirLocalId(4)));
    assert_eq!(signature.output.element_count, 8);
    assert_eq!(signature.input.source_local, Some(MirLocalId(6)));
    let extra: Vec<Option<MirLocalId>> = signature
        .extra_inputs
        .iter()
        .map(|resource| resource.source_local)
        .collect();
    assert_eq!(
        extra,
        vec![Some(MirLocalId(2)), Some(MirLocalId(3))],
        "the right operand, then the fused tail's bias"
    );
    assert_eq!(
        signature.builtin_inputs.len(),
        1,
        "the index parameter is the invocation builtin the indexed output relies on"
    );
}

#[test]
fn a_store_to_the_output_parameter_before_the_final_subchain_fails_closed() {
    // `y ⇇ c` stores in the first subchain; the second matmul has nothing to
    // bind the store to, so the body must refuse rather than drop it.
    let mut types = TypeTable::new();
    let mut function = two_matmul_output_parameter_function(&mut types);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    function.blocks[0]
        .statements
        .insert(1, copy_statement(4, 6, r2));
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("an early store to the output parameter must not be dropped");
    assert!(
        error.message.contains("only the final subchain"),
        "named refusal: {}",
        error.message
    );
}

#[test]
fn reading_the_output_parameter_fails_closed() {
    // The second matmul consumes `y`: a write-only output cannot also be an
    // input of the same kernel.
    let mut types = TypeTable::new();
    let mut function = two_matmul_output_parameter_function(&mut types);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    function.blocks[0].statements[2] = assign_statement(8, 4, 3, r2);
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("a read of the output parameter must not decompose");
    assert!(
        error.message.contains("reads its output parameter"),
        "named refusal: {}",
        error.message
    );
}

#[test]
fn storing_to_two_output_parameters_fails_closed() {
    let mut types = TypeTable::new();
    let mut function = two_matmul_output_parameter_function(&mut types);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    // `b` becomes a second written output parameter.
    function.params[3].mode = MirParamMode::MutRef;
    function.blocks[0].statements.push(copy_statement(3, 8, r2));
    // `d = e + b` now reads the second output; use `e` alone instead.
    function.blocks[0].statements[2] = copy_statement(8, 7, r2);
    let validation = MirValidationContext::new(&types);
    let error = decompose_kernel_function(&function, &validation)
        .expect_err("two output parameters must not decompose into one final output");
    assert!(
        error.message.contains("more than one output parameter"),
        "named refusal: {}",
        error.message
    );
}

#[test]
fn a_recipe_result_copied_through_a_stage_local_binds_the_parameter() {
    // `e ← c·v; y ⇇ e` with no fused tail: the contract-shaped surface would
    // name the stage local `e` as the output (the synthetic does not even
    // declare `y`), so the copy to `y` must ride the indexed-view reading.
    let mut types = TypeTable::new();
    let mut function = two_matmul_output_parameter_function(&mut types);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    function.blocks[0].statements.truncate(2);
    function.blocks[0].statements.push(copy_statement(4, 7, r2));
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("a recipe result copied into the output parameter decomposes");
    assert_eq!(decomposition.subchains.len(), 2);
    assert_eq!(decomposition.subchains[1].output_param, Some(MirLocalId(4)));
}

#[test]
fn a_recipe_result_forwarded_straight_into_the_parameter_keeps_the_contract_surface() {
    // The lowering's `y ⇇ c.matmul(v)`: the recipe writes a result temp and
    // the next statement forwards it into `y`, which the contract-shaped
    // signature already binds as the recipe output.
    let mut types = TypeTable::new();
    let mut function = two_matmul_output_parameter_function(&mut types);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    function.temps.push(MirTemp {
        id: MirTempId(0),
        ty: r2,
        span: test_span(),
    });
    function.blocks[0].statements.truncate(1);
    function.blocks[0].statements.push(MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(MirTempId(0))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                args: vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(6))),
                    MirOperand::Place(MirPlace::local(MirLocalId(2))),
                ],
                return_ty: r2,
            },
        },
        span: test_span(),
    });
    function.blocks[0]
        .statements
        .push(assign_from_temp(4, MirTempId(0), r2));
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("a mut-out recipe tail decomposes");
    assert_eq!(decomposition.subchains.len(), 2);
    assert_eq!(
        decomposition.subchains[1].output_param, None,
        "the recipe result IS the output parameter: nothing to rebind"
    );
}

#[test]
fn a_returned_output_parameter_keeps_the_return_form_surface() {
    // A function that returns its output parameter already binds it as the
    // last subchain's output through the consumed-output pass.
    let mut types = TypeTable::new();
    let mut function = two_matmul_output_parameter_function(&mut types);
    let r2 = rank2_tensor_ty(&mut types, [4, 2]);
    function.return_ty = r2;
    function.blocks[0].terminator.kind =
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(4)))));
    let validation = MirValidationContext::new(&types);
    let decomposition = decompose_kernel_function(&function, &validation)
        .expect("a returned output parameter decomposes");
    assert_eq!(decomposition.subchains[1].output_param, None);
    assert_eq!(decomposition.subchains[1].outputs, vec![MirLocalId(4)]);
}
