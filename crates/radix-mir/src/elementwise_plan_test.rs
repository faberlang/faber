//! Design-doc §10 Unit-1 fixtures: a typed plan or a typed barrier reason.

use super::{
    plan_elementwise, ElementwiseBarrier, ElementwiseExpr, ElementwiseOp, ElementwisePlan,
    IndexMapping, ReassociationPolicy,
};
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase,
    MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirType, MirUnOp, MirValidationContext, MirValue, MirValueId, MirValueKind,
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

fn rank1_tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let len = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.intern(Type::Tensor(f32, len)))
}

fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

fn collection_statement(
    op: MirCollectionOp,
    args: &[u32],
    destination: u32,
    ty: MirType,
) -> MirStatement {
    collection_statement_place(
        op,
        args,
        MirPlace::local(MirLocalId(destination)),
        ty,
        test_span(),
    )
}

fn unary_assign_statement(
    op: MirUnOp,
    operand: u32,
    destination: u32,
    ty: MirType,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(destination)),
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Unary {
                    op,
                    operand: MirOperand::Place(MirPlace::local(MirLocalId(operand))),
                },
                ty,
                span: test_span(),
            },
        },
        span: test_span(),
    }
}

fn collection_statement_place(
    op: MirCollectionOp,
    args: &[u32],
    destination: MirPlace,
    ty: MirType,
    span: Span,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(destination),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                args: args
                    .iter()
                    .map(|&local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                    .collect(),
                return_ty: ty,
            },
        },
        span,
    }
}

fn device_function(
    params: &[(u32, MirType)],
    statements: Vec<MirStatement>,
    return_local: Option<u32>,
    return_ty: MirType,
) -> MirFunction {
    let span = test_span();
    let mut type_by_local: BTreeMap<u32, MirType> = BTreeMap::default();
    for &(id, ty) in params {
        type_by_local.insert(id, ty);
    }
    for statement in &statements {
        let produced_ty = match &statement.kind {
            MirStatementKind::RuntimeCall { destination, call } => {
                destination.as_ref().map(|_| call.return_ty)
            }
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

fn plan_of(function: &MirFunction, types: &TypeTable) -> ElementwisePlan {
    let validation = MirValidationContext::new(types);
    plan_elementwise(function, &validation).expect("fixture should yield a typed plan")
}

fn barrier_of(function: &MirFunction, types: &TypeTable) -> ElementwiseBarrier {
    let validation = MirValidationContext::new(types);
    plan_elementwise(function, &validation).expect_err("fixture should yield a typed barrier")
}

fn input_slot(plan: &ElementwisePlan, local: u32) -> usize {
    plan.inputs
        .iter()
        .position(|input| input.local == MirLocalId(local))
        .unwrap_or_else(|| panic!("plan is missing input local {local}"))
}

fn assert_input(expr: &ElementwiseExpr, slot: usize) -> &IndexMapping {
    match expr {
        ElementwiseExpr::Input {
            slot: found, index, ..
        } if *found == slot => index,
        other => panic!("expected Input slot {slot}, got {other:?}"),
    }
}

fn assert_binary(
    expr: &ElementwiseExpr,
    op: ElementwiseOp,
) -> (&ElementwiseExpr, &ElementwiseExpr) {
    match expr {
        ElementwiseExpr::Binary {
            op: found,
            lhs,
            rhs,
            numerical_policy,
            ..
        } if *found == op => {
            assert_eq!(
                numerical_policy.reassociation,
                ReassociationPolicy::Forbidden
            );
            (lhs.as_ref(), rhs.as_ref())
        }
        other => panic!("expected Binary({op:?}), got {other:?}"),
    }
}

fn assert_unary(expr: &ElementwiseExpr, op: ElementwiseOp) -> &ElementwiseExpr {
    match expr {
        ElementwiseExpr::Unary {
            op: found,
            operand,
            numerical_policy,
            ..
        } if *found == op => {
            assert_eq!(
                numerical_policy.reassociation,
                ReassociationPolicy::Forbidden
            );
            operand.as_ref()
        }
        other => panic!("expected Unary({op:?}), got {other:?}"),
    }
}

fn assert_no_target_syntax(plan: &ElementwisePlan) {
    let dump = format!("{plan:?}").to_ascii_lowercase();
    for needle in [
        "metal",
        "msl",
        "wgsl",
        "llvm",
        "nvvm",
        "amdgpu",
        "threadgroup",
        "addrspace",
        "[[buffer",
        "@group",
    ] {
        assert!(
            !dump.contains(needle),
            "plan must not carry target-specific syntax {needle:?}: {dump}"
        );
    }
}

/// Single add — design §10 Unit 1.
#[test]
fn single_add_yields_typed_plan() {
    let mut types = TypeTable::new();
    let r1 = rank1_tensor_ty(&mut types, 4);
    let function = device_function(
        &[(0, r1), (1, r1)],
        vec![collection_statement(
            MirCollectionOp::TensorAdd,
            &[0, 1],
            2,
            r1,
        )],
        Some(2),
        r1,
    );
    let plan = plan_of(&function, &types);
    assert_eq!(plan.output.local, MirLocalId(2));
    assert_eq!(plan.shape.dims, vec![4]);
    assert_eq!(
        plan.numerical_policy.reassociation,
        ReassociationPolicy::Forbidden
    );
    let x = input_slot(&plan, 0);
    let y = input_slot(&plan, 1);
    let (lhs, rhs) = assert_binary(&plan.expression, ElementwiseOp::Add);
    assert_eq!(*assert_input(lhs, x), IndexMapping::Identity);
    assert_eq!(*assert_input(rhs, y), IndexMapping::Identity);
    assert_eq!(plan.provenance.len(), 1);
    assert_no_target_syntax(&plan);
}

/// Multiply → add chain — design §10 Unit 1. Source order is preserved; the
/// mul is the inner node and the add is the root. No reassociation.
#[test]
fn multiply_add_chain_preserves_source_order() {
    let mut types = TypeTable::new();
    let r1 = rank1_tensor_ty(&mut types, 8);
    let mul_span = Span::new(10, 20);
    let add_span = Span::new(30, 40);
    let function = device_function(
        &[(0, r1), (1, r1), (2, r1)],
        vec![
            collection_statement_place(
                MirCollectionOp::TensorMul,
                &[0, 1],
                MirPlace::local(MirLocalId(3)),
                r1,
                mul_span,
            ),
            collection_statement_place(
                MirCollectionOp::TensorAdd,
                &[3, 2],
                MirPlace::local(MirLocalId(4)),
                r1,
                add_span,
            ),
        ],
        Some(4),
        r1,
    );
    let plan = plan_of(&function, &types);
    let x = input_slot(&plan, 0);
    let scale = input_slot(&plan, 1);
    let bias = input_slot(&plan, 2);
    let (add_lhs, add_rhs) = assert_binary(&plan.expression, ElementwiseOp::Add);
    let (mul_lhs, mul_rhs) = assert_binary(add_lhs, ElementwiseOp::Mul);
    assert_input(mul_lhs, x);
    assert_input(mul_rhs, scale);
    assert_input(add_rhs, bias);
    assert_eq!(
        plan.provenance
            .iter()
            .map(|fact| fact.span)
            .collect::<Vec<_>>(),
        vec![mul_span, add_span]
    );
    assert_eq!(
        plan.provenance
            .iter()
            .map(|fact| fact.op)
            .collect::<Vec<_>>(),
        vec![Some(ElementwiseOp::Mul), Some(ElementwiseOp::Add)]
    );
    assert_eq!(
        plan.numerical_policy.reassociation,
        ReassociationPolicy::Forbidden
    );
    assert_no_target_syntax(&plan);
}

/// Scalar fill + rank-1 broadcast — design §10 Unit 1.
#[test]
fn scalar_fill_and_rank1_broadcast_yields_typed_plan() {
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 8]);
    let r1 = rank1_tensor_ty(&mut types, 8);
    let scalar = f32_ty(&mut types);
    let function = device_function(
        &[(0, r2), (1, r1), (2, scalar)],
        vec![
            collection_statement(MirCollectionOp::TensorFill, &[0, 2], 3, r2),
            collection_statement(MirCollectionOp::TensorAdd, &[3, 1], 4, r2),
        ],
        Some(4),
        r2,
    );
    let plan = plan_of(&function, &types);
    assert_eq!(plan.shape.dims, vec![4, 8]);
    assert_eq!(plan.output.local, MirLocalId(4));
    let fill = input_slot(&plan, 2);
    let bias = input_slot(&plan, 1);
    assert!(
        plan.inputs.iter().all(|input| input.local != MirLocalId(0)),
        "TensorFill shape receiver is not a plan input"
    );
    let (lhs, rhs) = assert_binary(&plan.expression, ElementwiseOp::Add);
    let fill_operand = assert_unary(lhs, ElementwiseOp::Fill);
    assert_eq!(
        *assert_input(fill_operand, fill),
        IndexMapping::BroadcastScalar
    );
    assert_eq!(
        *assert_input(rhs, bias),
        IndexMapping::RankExtension {
            operand_shape: vec![8],
            result_shape: vec![4, 8],
        }
    );
    assert_eq!(plan.inputs[bias].shape, vec![8]);
    assert_eq!(
        plan.numerical_policy.reassociation,
        ReassociationPolicy::Forbidden
    );
    assert_no_target_syntax(&plan);
}

/// Multi-consumer intermediate — design §10 Unit 1 / §5.1 single-consumer.
#[test]
fn multi_consumer_intermediate_is_barrier() {
    let mut types = TypeTable::new();
    let r1 = rank1_tensor_ty(&mut types, 4);
    let function = device_function(
        &[(0, r1), (1, r1), (2, r1)],
        vec![
            collection_statement(MirCollectionOp::TensorMul, &[0, 1], 3, r1),
            collection_statement(MirCollectionOp::TensorAdd, &[3, 2], 4, r1),
            collection_statement(MirCollectionOp::TensorAdd, &[3, 4], 5, r1),
        ],
        Some(5),
        r1,
    );
    match barrier_of(&function, &types) {
        ElementwiseBarrier::MultiConsumer { local, .. } => {
            assert_eq!(local, MirLocalId(3));
        }
        other => panic!("expected MultiConsumer, got {other:?}"),
    }
}

/// Projected destination — design §10 Unit 1 / §5.1 base place only.
#[test]
fn projected_destination_is_barrier() {
    let mut types = TypeTable::new();
    let r1 = rank1_tensor_ty(&mut types, 4);
    let dest = MirPlace {
        base: MirPlaceBase::Local(MirLocalId(2)),
        projections: vec![MirProjection::Index(MirOperand::Constant(
            MirConstant::Int(0),
        ))],
    };
    let function = device_function(
        &[(0, r1), (1, r1)],
        vec![collection_statement_place(
            MirCollectionOp::TensorAdd,
            &[0, 1],
            dest,
            r1,
            test_span(),
        )],
        Some(2),
        r1,
    );
    assert!(
        matches!(
            barrier_of(&function, &types),
            ElementwiseBarrier::ProjectedDestination { .. }
        ),
        "projected destination must be a typed barrier"
    );
}

/// Unsupported / unplannable collection op, including recipe ops — design
/// §10 Unit 1 / §11.6. Recipe ops must not silently become elementwise.
#[test]
fn recipe_op_is_barrier_not_elementwise() {
    let mut types = TypeTable::new();
    let r2 = rank2_tensor_ty(&mut types, [4, 4]);
    let function = device_function(
        &[(0, r2), (1, r2)],
        vec![collection_statement(
            MirCollectionOp::TensorMatMul,
            &[0, 1],
            2,
            r2,
        )],
        Some(2),
        r2,
    );
    assert!(
        crate::device_program_plans::is_recipe_op(MirCollectionOp::TensorMatMul),
        "fixture must use the kernel_plan recipe-authority classifier"
    );
    match barrier_of(&function, &types) {
        ElementwiseBarrier::Recipe { op, .. } => {
            assert_eq!(op, MirCollectionOp::TensorMatMul);
        }
        other => panic!("expected Recipe barrier, got {other:?}"),
    }
}

#[test]
fn unsupported_collection_op_is_barrier() {
    let mut types = TypeTable::new();
    let r1 = rank1_tensor_ty(&mut types, 4);
    let function = device_function(
        &[(0, r1), (1, r1)],
        vec![collection_statement(
            MirCollectionOp::TensorCruxEntropia,
            &[0, 1],
            2,
            r1,
        )],
        Some(2),
        r1,
    );
    assert!(!crate::device_program_plans::is_recipe_op(
        MirCollectionOp::TensorCruxEntropia
    ));
    match barrier_of(&function, &types) {
        ElementwiseBarrier::UnsupportedOp { op, .. } => {
            assert_eq!(op, MirCollectionOp::TensorCruxEntropia);
        }
        other => panic!("expected UnsupportedOp barrier, got {other:?}"),
    }
}

/// Unary Relu/Gelu assign (`MirValueKind::Unary`) — the collection-op / unary
/// duality at `device_program_plans.rs` (`is_elementwise_collection_op` vs
/// `is_admitted_tensor_unary`).
///
/// `plan_elementwise` classifies only collection runtime calls. A live tensor
/// unary assign is therefore an [`ElementwiseBarrier::Effect`], even though
/// Relu/Gelu are admitted as both a collection op and a tensor unary.
#[test]
fn unary_relu_gelu_assign_is_effect_barrier() {
    for (unop, collection) in [
        (MirUnOp::Relu, MirCollectionOp::TensorRelu),
        (MirUnOp::Gelu, MirCollectionOp::TensorGelu),
    ] {
        assert!(
            crate::device_program_plans::is_admitted_tensor_unary(unop),
            "{unop:?} is an admitted tensor unary"
        );
        assert!(
            crate::device_program_plans::is_elementwise_collection_op(collection),
            "{collection:?} is an elementwise collection op — the other side of the duality"
        );
        let mut types = TypeTable::new();
        let r1 = rank1_tensor_ty(&mut types, 4);
        let function = device_function(
            &[(0, r1)],
            vec![unary_assign_statement(unop, 0, 1, r1)],
            Some(1),
            r1,
        );
        match barrier_of(&function, &types) {
            ElementwiseBarrier::Effect { .. } => {}
            other => panic!("expected Effect barrier for unary {unop:?}, got {other:?}"),
        }
    }
}
