use super::*;
use crate::abi::MirKernelSignature;
use crate::device_program_plans::kernel_plan_for_function;
use crate::kernel_plan::CollectionKernelPlan;
use crate::layout::{MirDeviceViewLayout, MirLayoutKind, MirScalarLayout};
use crate::validate::ValidatedMir;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
    MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId, MirOperand,
    MirParam, MirParamMode, MirPlace, MirPlaceBase, MirRuntimeCall, MirStatement, MirStatementKind,
    MirTerminator, MirTerminatorKind, MirType,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, IndexVar, NumericWidth, Primitive, Type, TypeTable};

/// A validation context whose given tensor types carry device-handle layouts
/// (the kernel ABI's `require_kernel_device_view` gate).
fn device_validation<'a>(types: &'a TypeTable, tensors: &[MirType]) -> MirValidationContext<'a> {
    let mut validation = MirValidationContext::new(types);
    for ty in tensors {
        let Type::Tensor(element, shape) = types.get(ty.semantic_id()) else {
            continue;
        };
        let scalar = match types.get(*element) {
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => MirScalarLayout::F32,
            _ => continue,
        };
        let rank = match types.get_index(*shape) {
            IndexExpr::Tuple(dims) => u8::try_from(dims.len()).unwrap_or(0),
            IndexExpr::Literal(_) => 1,
            _ => continue,
        };
        validation.layouts.set_type_layout(
            ty.semantic_id(),
            MirLayoutKind::DeviceView(MirDeviceViewLayout::device_handle_row_major(scalar, rank)),
        );
    }
    validation
}

fn test_span() -> Span {
    Span::default()
}

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

/// A rank-2 `tf32[M, N]` tensor type with static literal dims (the MLP
/// operand shape the device path folds).
fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

/// A rank-0 `tf32[]` vacua seed template.
fn rank0_tensor_ty(types: &mut TypeTable) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let shape = types.intern_index(IndexExpr::Tuple(Vec::new()));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

/// A dynamic-shape tensor type (unfoldable `magnitudines()` receiver).
fn dynamic_tensor_ty(types: &mut TypeTable) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let shape = types.intern_index(IndexExpr::Infer(IndexVar(0)));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

/// The `lista<numerus>` shape-list type `magnitudines()` returns.
fn shape_lista_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.array(types.primitive(Primitive::Numerus)))
}

/// A `lista<f32>` flat-buffer type (the `TensorFromFlat` flat operand).
fn flat_lista_ty(types: &mut TypeTable) -> MirType {
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    MirType::semantic(types.array(f32))
}

/// A four-element tuple of one tensor type (the MLP result shape).
fn tuple_of_four_ty(types: &mut TypeTable, element: MirType) -> MirType {
    let element = element.semantic_id();
    MirType::semantic(types.intern(Type::Tuple(vec![element; 4])))
}

/// A collection runtime-call statement.
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

/// A construct statement (tensor vacua, shape lista, or the result tuple).
fn construct_statement(
    destination: u32,
    ty: MirType,
    kind: MirAggregateKind,
    fields: Vec<MirOperand>,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace::local(MirLocalId(destination)),
            aggregate: MirAggregate {
                kind,
                ty,
                fields: MirAggregateFields::Ordered(
                    fields.into_iter().map(MirAggregateItem::Operand).collect(),
                ),
            },
        },
        span: test_span(),
    }
}

/// The device-resident `train_step_4x4` body shape (S5-U4): for each of the
/// four params, `shape ← param.magnitudines()`, `lr_fill ← seed.crea(lr,
/// shape)`, `scaled ← grad.multiplica(lr_fill)`, `new ← param.subtrahe(scaled)`;
/// returns the four `new` tensors as a tuple.
///
/// Locals: 0-7 tensor params, 8 `lr` (f32), 9 seed (rank-0), 10-13 shape
/// lists, 14-17 lr fills, 18-21 scaled, 22-25 new params, 26 result tuple.
fn train_step_4x4_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let r0 = rank0_tensor_ty(types);
    let lista = shape_lista_ty(types);
    let tuple = tuple_of_four_ty(types, r2);
    let f32 = f32_ty(types);

    let params = (0..8)
        .map(|local| MirParam {
            local: MirLocalId(local),
            name: None,
            ty: r2,
            mode: MirParamMode::Owned,
            span: test_span(),
        })
        .chain([MirParam {
            local: MirLocalId(8),
            name: None,
            ty: f32,
            mode: MirParamMode::Owned,
            span: test_span(),
        }])
        .collect::<Vec<_>>();

    // Every local the body writes (9..26), all typed.
    let local_tys = [
        (9, r0),
        (10, lista),
        (11, lista),
        (12, lista),
        (13, lista),
        (14, r2),
        (15, r2),
        (16, r2),
        (17, r2),
        (18, r2),
        (19, r2),
        (20, r2),
        (21, r2),
        (22, r2),
        (23, r2),
        (24, r2),
        (25, r2),
        (26, tuple),
    ];
    let locals = local_tys
        .into_iter()
        .map(|(id, ty)| MirLocal {
            id: MirLocalId(id),
            name: None,
            ty,
            mutable: false,
            span: test_span(),
        })
        .collect();

    let statements = vec![
        // seed ← vacua
        construct_statement(9, r0, MirAggregateKind::Tensor, Vec::new()),
        // shape_wN ← weightN.magnitudines()
        collection_statement(MirCollectionOp::TensorShape, &[0], 10, lista),
        collection_statement(MirCollectionOp::TensorShape, &[1], 11, lista),
        collection_statement(MirCollectionOp::TensorShape, &[2], 12, lista),
        collection_statement(MirCollectionOp::TensorShape, &[3], 13, lista),
        // lr_fill_wN ← seed.crea(lr, shape_wN)
        collection_statement(MirCollectionOp::TensorCreate, &[9, 8, 10], 14, r2),
        collection_statement(MirCollectionOp::TensorCreate, &[9, 8, 11], 15, r2),
        collection_statement(MirCollectionOp::TensorCreate, &[9, 8, 12], 16, r2),
        collection_statement(MirCollectionOp::TensorCreate, &[9, 8, 13], 17, r2),
        // scaled_wN ← grad_wN.multiplica(lr_fill_wN)
        collection_statement(MirCollectionOp::TensorMul, &[4, 14], 18, r2),
        collection_statement(MirCollectionOp::TensorMul, &[5, 15], 19, r2),
        collection_statement(MirCollectionOp::TensorMul, &[6, 16], 20, r2),
        collection_statement(MirCollectionOp::TensorMul, &[7, 17], 21, r2),
        // new_wN ← weightN.subtrahe(scaled_wN)
        collection_statement(MirCollectionOp::TensorSub, &[0, 18], 22, r2),
        collection_statement(MirCollectionOp::TensorSub, &[1, 19], 23, r2),
        collection_statement(MirCollectionOp::TensorSub, &[2, 20], 24, r2),
        collection_statement(MirCollectionOp::TensorSub, &[3, 21], 25, r2),
        // result tuple
        construct_statement(
            26,
            tuple,
            MirAggregateKind::Tuple,
            (22..=25)
                .map(|local| MirOperand::Place(MirPlace::local(MirLocalId(local))))
                .collect(),
        ),
    ];

    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params,
        locals,
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(26),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tuple,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A minimal device-resident body with one `magnitudines()` on a dynamic
/// tensor — the unfoldable fail-closed case.
fn unfoldable_magnitudines_function(types: &mut TypeTable) -> MirFunction {
    let dynamic = dynamic_tensor_ty(types);
    let lista = shape_lista_ty(types);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: dynamic,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: dynamic,
            mutable: false,
            span: test_span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_statement(
                MirCollectionOp::TensorShape,
                &[0],
                1,
                lista,
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: lista,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A minimal device-resident body with `crea` on a dynamic destination — the
/// non-static-destination fail-closed case.
fn crea_on_dynamic_destination_function(types: &mut TypeTable) -> MirFunction {
    let dynamic = dynamic_tensor_ty(types);
    let r0 = rank0_tensor_ty(types);
    let lista = shape_lista_ty(types);
    let f32 = f32_ty(types);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: f32,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: f32,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: r0,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: lista,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                construct_statement(1, r0, MirAggregateKind::Tensor, Vec::new()),
                construct_statement(
                    2,
                    lista,
                    MirAggregateKind::Array,
                    vec![MirOperand::Constant(MirConstant::Int(2))],
                ),
                collection_statement(MirCollectionOp::TensorCreate, &[1, 0, 2], 3, dynamic),
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A device-resident body with `TensorFromFlat` on a static destination — the
/// fold records a shape fact and keeps the data movement.
fn from_flat_static_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let flat = flat_lista_ty(types);
    let lista = shape_lista_ty(types);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: flat,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: flat,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: r2,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: lista,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                // shape ← weights.magnitudines()
                collection_statement(MirCollectionOp::TensorShape, &[1], 2, lista),
                // grid ← seed.strue(flat, shape) — from-flat on a static dest
                collection_statement(MirCollectionOp::TensorFromFlat, &[2, 0, 2], 3, r2),
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(3),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: r2,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// The collection ops a function body carries, in statement order.
fn body_collection_ops(function: &MirFunction) -> Vec<MirCollectionOp> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => match call.intrinsic {
                MirIntrinsic::Collection(op) => Some(op),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn train_step_4x4_body_folds_shape_ops_to_elementwise_fills() {
    let mut types = TypeTable::new();
    let function = train_step_4x4_function(&mut types);
    let validation = MirValidationContext::new(&types);

    // The UNFOLDED body is not device-plannable: TensorShape/TensorCreate
    // have no kernel plan under the shared plan surface.
    let device = device_validation(&types, &[function.params[0].ty]);
    assert_eq!(
        device
            .layouts
            .layout_for_type(function.params[0].ty)
            .expect("r2 layout")
            .kind,
        MirLayoutKind::DeviceView(MirDeviceViewLayout::device_handle_row_major(
            MirScalarLayout::F32,
            2,
        )),
        "r2 must carry the device-handle layout for kernel admission"
    );
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &device)
        .expect("signature derivation precedes the plan scan");
    assert!(
        kernel_plan_for_function(&function, &signature, &device).is_err(),
        "unfolded shape ops must fail the plan pass (fail-closed), not plan silently"
    );

    let outcome = fold_static_shapes(&function, &validation).expect("fold succeeds");
    let folded = &outcome.function;

    // All shape ops are gone; only elementwise fill/mul/sub + the result
    // tuple tail remain.
    let ops = body_collection_ops(folded);
    assert_eq!(
        ops,
        vec![
            MirCollectionOp::TensorFill,
            MirCollectionOp::TensorFill,
            MirCollectionOp::TensorFill,
            MirCollectionOp::TensorFill,
            MirCollectionOp::TensorMul,
            MirCollectionOp::TensorMul,
            MirCollectionOp::TensorMul,
            MirCollectionOp::TensorMul,
            MirCollectionOp::TensorSub,
            MirCollectionOp::TensorSub,
            MirCollectionOp::TensorSub,
            MirCollectionOp::TensorSub,
        ],
        "folded body must carry only elementwise fill/mul/sub"
    );
    assert!(
        ops.iter()
            .all(|op| crate::device_program_plans::is_elementwise_collection_op(*op)),
        "every remaining op must be in the elementwise closed set"
    );

    // The dead fold artifacts (vacua seed, folded shape lists) are dropped:
    // the body keeps only the result tuple construct.
    let constructs: Vec<MirAggregateKind> = folded
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Construct { aggregate, .. } => Some(aggregate.kind.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(constructs, vec![MirAggregateKind::Tuple]);

    // The fill receivers are the shape-source tensors (the params whose
    // magnitudines() supplied the shape), not the vacua seed.
    let fills: Vec<&MirRuntimeCall> = folded
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => {
                if matches!(
                    call.intrinsic,
                    MirIntrinsic::Collection(MirCollectionOp::TensorFill)
                ) {
                    Some(call)
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    for (index, call) in fills.iter().enumerate() {
        assert_eq!(call.args.len(), 2, "folded fill carries (receiver, scalar)");
        let MirOperand::Place(receiver) = &call.args[0] else {
            panic!("folded fill receiver must be a local place");
        };
        let expected_receiver = [0u32, 1, 2, 3][index];
        assert_eq!(
            receiver.base,
            MirPlaceBase::Local(MirLocalId(expected_receiver)),
            "fill {index} must broadcast over its shape-source tensor (param {expected_receiver})"
        );
    }
}

#[test]
fn train_step_4x4_fold_reports_literal_shape_facts() {
    let mut types = TypeTable::new();
    let function = train_step_4x4_function(&mut types);
    let validation = MirValidationContext::new(&types);

    let outcome = fold_static_shapes(&function, &validation).expect("fold succeeds");
    let facts = &outcome.report.facts;

    assert_eq!(facts.len(), 8, "four magnitudines + four crea folds");
    for (index, fact) in facts.iter().enumerate() {
        assert_eq!(fact.dims, vec![4, 4], "fact {index} dims are literal 4×4");
        assert_eq!(fact.element_count, 16, "fact {index} flat element count");
        let expected_source = if index < 4 {
            ShapeFactSource::Magnitudines
        } else {
            ShapeFactSource::Crea
        };
        assert_eq!(fact.source, expected_source, "fact {index} source op");
    }
    // The magnitudines facts name the shape-list locals; the crea facts name
    // the fill destinations.
    assert_eq!(facts[0].local, MirLocalId(10));
    assert_eq!(facts[4].local, MirLocalId(14));
    assert_eq!(facts[7].local, MirLocalId(17));
}

#[test]
fn folded_train_step_plans_as_elementwise_kernel() {
    let mut types = TypeTable::new();
    let function = train_step_4x4_function(&mut types);
    let validation = MirValidationContext::new(&types);

    let outcome = fold_static_shapes(&function, &validation).expect("fold succeeds");
    let device = device_validation(&types, &[function.params[0].ty]);
    let signature = MirKernelSignature::storage_buffer_kernel(&outcome.function, &device)
        .expect("folded body derives a storage-buffer kernel signature");
    let plan = kernel_plan_for_function(&outcome.function, &signature, &device)
        .expect("folded body plans without a typed diagnostic");
    assert_eq!(plan, None, "elementwise-only body carries no recipe");
}

#[test]
fn admission_folds_and_admits_the_train_step_body() {
    let mut types = TypeTable::new();
    let function = train_step_4x4_function(&mut types);
    let device = device_validation(&types, &[function.params[0].ty]);

    let admission = admit_device_shape_materialization(&function, &device)
        .expect("train_step body is device-materializable after folding");
    assert_eq!(
        admission.plan,
        CollectionKernelPlan::Elementwise,
        "folded train_step resolves to an elementwise kernel"
    );
    assert_eq!(admission.facts.len(), 8);
    assert!(
        admission
            .facts
            .iter()
            .all(|fact| fact.dims == vec![4, 4] && fact.element_count == 16),
        "admission carries the literal 4×4 shape facts"
    );
}

#[test]
fn folded_train_step_revalidates_as_mir() {
    let mut types = TypeTable::new();
    let function = train_step_4x4_function(&mut types);
    let validation = MirValidationContext::new(&types);

    let outcome = fold_static_shapes(&function, &validation).expect("fold succeeds");
    let program = crate::MirProgram {
        functions: vec![outcome.function],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types));
    assert!(
        validated.is_ok(),
        "folded body must remain well-formed MIR: {validated:?}"
    );
}

#[test]
fn unfoldable_magnitudines_fails_closed_with_typed_diagnostic() {
    let mut types = TypeTable::new();
    let function = unfoldable_magnitudines_function(&mut types);
    let validation = MirValidationContext::new(&types);

    let error = fold_static_shapes(&function, &validation)
        .expect_err("dynamic magnitudines must fail closed");
    assert_eq!(error.kind, ShapeFoldErrorKind::UnfoldableMagnitudines);
    assert!(
        error.message.contains("no literal tensor shape"),
        "diagnostic names the missing literal shape: {}",
        error.message
    );
}

#[test]
fn crea_on_dynamic_destination_fails_closed_with_typed_diagnostic() {
    let mut types = TypeTable::new();
    let function = crea_on_dynamic_destination_function(&mut types);
    let validation = MirValidationContext::new(&types);

    let error = fold_static_shapes(&function, &validation)
        .expect_err("crea on a dynamic destination must fail closed");
    assert_eq!(error.kind, ShapeFoldErrorKind::NonStaticShapeDestination);
    assert!(
        error.message.contains("no literal tensor shape"),
        "diagnostic names the missing literal shape: {}",
        error.message
    );
}

#[test]
fn from_flat_on_static_destination_keeps_data_movement_and_records_shape() {
    let mut types = TypeTable::new();
    let function = from_flat_static_function(&mut types);
    let validation = MirValidationContext::new(&types);

    let outcome = fold_static_shapes(&function, &validation).expect("fold succeeds");
    let ops = body_collection_ops(&outcome.function);
    assert!(
        ops.contains(&MirCollectionOp::TensorFromFlat),
        "from-flat data movement survives the fold"
    );
    let from_flat_facts = outcome
        .report
        .facts
        .iter()
        .filter(|fact| fact.source == ShapeFactSource::FromFlat)
        .collect::<Vec<_>>();
    assert_eq!(from_flat_facts.len(), 1);
    assert_eq!(from_flat_facts[0].dims, vec![4, 4]);
    assert_eq!(from_flat_facts[0].element_count, 16);
}
