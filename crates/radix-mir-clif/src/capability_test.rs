//! C2 proof: hand-built MIR fixtures drive the capability classifier
//! (goal `radix-mir-clif`, unit C2).

use crate::capability::{SLUG_CLIF_POLICY, SLUG_CLIF_SUBSET, classify_with_types};
use radix_lexer::{Span, Symbol};
use radix_mir::*;
use radix_types::Type;
use radix_types::vocabulary::Lowerability;
use radix_types::{InstansPrecision, Primitive, SemanticMutability, TypeTable};

fn sample_span() -> Span {
    Span::default()
}

fn ty(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

struct FunctionShape {
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    temps: Vec<MirTemp>,
    statements: Vec<MirStatement>,
    terminator: MirTerminatorKind,
    return_ty: MirType,
    error_ty: Option<MirType>,
    is_async: bool,
    is_generator: bool,
}

#[allow(clippy::too_many_lines)]
fn single_function(shape: FunctionShape) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: shape.params,
            locals: shape.locals,
            temps: shape.temps,
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: shape.statements,
                terminator: MirTerminator {
                    kind: shape.terminator,
                    span: sample_span(),
                },
                span: sample_span(),
            }],
            return_ty: shape.return_ty,
            error_ty: shape.error_ty,
            is_async: shape.is_async,
            is_generator: shape.is_generator,
            shader_stage: None,
            span: sample_span(),
        }],
    }
}

fn minimal(shape: FunctionShape) -> (MirProgram, TypeTable) {
    let types = TypeTable::new();
    let program = single_function(FunctionShape {
        return_ty: ty(&types, Primitive::Vacuum),
        ..shape
    });
    (program, types)
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: false,
        span: sample_span(),
    }
}

fn param(id: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(id),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: sample_span(),
    }
}

#[test]
fn scalar_core_is_capable() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let fractus = ty(&types, Primitive::Fractus);
    let bivalens = ty(&types, Primitive::Bivalens);
    let program = single_function(FunctionShape {
        params: vec![param(0, numerus)],
        locals: vec![local(1, numerus), local(2, fractus), local(3, bivalens)],
        temps: Vec::new(),
        statements: vec![
            MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Binary {
                            op: MirBinOp::Add,
                            lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            rhs: MirOperand::Constant(MirConstant::Int(1)),
                        },
                        ty: numerus,
                        span: sample_span(),
                    },
                },
                span: sample_span(),
            },
            MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: MirCallee::Function(MirFunctionId(0)),
                    args: vec![MirOperand::Place(MirPlace::local(MirLocalId(1)))],
                },
                span: sample_span(),
            },
        ],
        terminator: MirTerminatorKind::Branch {
            condition: MirOperand::Place(MirPlace::local(MirLocalId(3))),
            then_block: MirBlockId(0),
            else_block: MirBlockId(0),
        },
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert!(verdict.is_capable(), "scalar core must pass: {verdict:?}");
}

#[test]
fn text_local_is_capable() {
    let types = TypeTable::new();
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, ty(&types, Primitive::Textus))],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert!(
        verdict.is_capable(),
        "text is inside the subset: {verdict:?}"
    );
}

#[test]
fn littera_is_capable_and_a_map_of_letters_names_gap() {
    let mut types = TypeTable::new();
    let littera = types.primitive(Primitive::Littera);
    let numerus = types.primitive(Primitive::Numerus);
    let map = MirType::semantic(types.map(numerus, littera));
    let vacuum = ty(&types, Primitive::Vacuum);
    let shape = |local_ty: MirType| {
        single_function(FunctionShape {
            params: Vec::new(),
            locals: vec![local(0, local_ty)],
            temps: Vec::new(),
            statements: Vec::new(),
            terminator: MirTerminatorKind::Return(None),
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
        })
    };
    let program = shape(MirType::semantic(littera));
    assert!(classify_with_types(&program, &types).is_capable());

    let program = shape(map);
    let verdict = classify_with_types(&program, &types);
    let gaps = verdict.gaps();
    assert_eq!(gaps.len(), 1, "one gap expected: {gaps:?}");
    assert_eq!(gaps[0].shape(), "character in a map");
    assert_eq!(gaps[0].slug(), SLUG_CLIF_SUBSET);
}

#[test]
fn gaps_keep_storage_order() {
    let mut types = TypeTable::new();
    let numerus_id = types.primitive(Primitive::Numerus);
    let queue = MirType::semantic(types.intern(radix_types::Type::Queue(numerus_id)));
    let policy = MirType::semantic(types.intern(radix_types::Type::PolicyFloat(
        radix_types::FloatPolicy::Trapping,
        radix_types::NumericWidth::F64,
    )));
    let program = single_function(FunctionShape {
        params: vec![param(0, queue)],
        locals: vec![local(1, policy)],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    let gaps = verdict.gaps();
    assert_eq!(gaps.len(), 2, "two gaps expected: {gaps:?}");
    assert_eq!(
        gaps[0].shape(),
        "collection value",
        "param walks before local"
    );
    assert_eq!(gaps[1].shape(), "policy float value");
}

#[test]
fn list_type_is_capable_and_queue_type_names_gap() {
    let mut types = TypeTable::new();
    let numerus_id = types.primitive(Primitive::Numerus);
    let list = MirType::semantic(types.array(numerus_id));
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, list)],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    let verdict = classify_with_types(&program, &types);
    assert!(
        verdict.is_capable(),
        "a list of numbers is inside the subset: {verdict:?}"
    );

    let list = MirType::semantic(types.queue(numerus_id));
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, list)],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "collection value");
}

#[test]
fn unresolved_param_type_names_layout_gap() {
    let mut types = TypeTable::new();
    let param_ty = MirType::semantic(types.intern(Type::Param(Symbol(1))));
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, param_ty)],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "unresolved type");
    assert_eq!(verdict.gaps()[0].slug(), "missing-layout-fact");
}

#[test]
fn try_call_is_capable() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, numerus)],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::TryCall {
            destination: None,
            callee: MirCallee::Function(MirFunctionId(0)),
            args: Vec::new(),
            ok_block: MirBlockId(0),
            error_place: MirPlace::local(MirLocalId(0)),
            error_block: MirBlockId(0),
        },
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert!(
        verdict.is_capable(),
        "try_call is inside the subset: {verdict:?}"
    );
}

#[test]
fn error_typed_function_is_capable() {
    let types = TypeTable::new();
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: Some(ty(&types, Primitive::Numerus)),
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert!(
        verdict.is_capable(),
        "an error channel is inside the subset: {verdict:?}"
    );
}

#[test]
fn runtime_call_names_gap() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, numerus)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::ReadLine,
                    args: Vec::new(),
                    return_ty: ty(&types, Primitive::Vacuum),
                },
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "runtime call ReadLine");
}

#[test]
fn aggregate_construction_names_gap() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, numerus)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::local(MirLocalId(0)),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Tensor,
                    ty: numerus,
                    fields: MirAggregateFields::Ordered(Vec::new()),
                },
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "device construction");
}

#[test]
fn vector_value_and_construction_are_capable() {
    let mut types = TypeTable::new();
    let fractus = types.primitive(Primitive::Fractus);
    let width = types.intern_index(radix_types::IndexExpr::Literal(3));
    let vector = MirType::semantic(types.intern(Type::Vector(fractus, width)));
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, vector)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::local(MirLocalId(0)),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Vector,
                    ty: vector,
                    fields: MirAggregateFields::Ordered(vec![
                        MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Float(1.0))),
                        MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Float(2.0))),
                        MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Float(3.0))),
                    ]),
                },
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    assert!(classify_with_types(&program, &types).is_capable());
}

#[test]
fn place_projection_names_gap() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, numerus)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace {
                    base: MirPlaceBase::Local(MirLocalId(0)),
                    projections: vec![MirProjection::MatrixCell { row: 0, column: 0 }],
                },
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(1))),
                    ty: numerus,
                    span: sample_span(),
                },
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "place projection");
}

#[test]
fn closure_callee_is_capable() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, numerus)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::Call {
                destination: None,
                callee: MirCallee::Closure(MirClosureCallee {
                    function: MirFunctionId(0),
                    environment: MirOperand::Constant(MirConstant::Int(0)),
                    environment_id: MirClosureEnvironmentId(0),
                }),
                args: Vec::new(),
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert!(
        verdict.is_capable(),
        "a captured closure call is inside the subset: {verdict:?}"
    );
}

#[test]
fn device_constant_is_policy_gap() {
    let types = TypeTable::new();
    let fractus = ty(&types, Primitive::Fractus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, fractus)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::local(MirLocalId(0)),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::ShapeSize {
                        shape: radix_types::IndexId(0),
                        axis: None,
                    })),
                    ty: fractus,
                    span: sample_span(),
                },
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "device constant");
    assert_eq!(verdict.gaps()[0].slug(), SLUG_CLIF_POLICY);
}

#[test]
fn bit_operation_names_gap() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, numerus)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::local(MirLocalId(0)),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Binary {
                        op: MirBinOp::BitAnd,
                        lhs: MirOperand::Constant(MirConstant::Int(1)),
                        rhs: MirOperand::Constant(MirConstant::Int(2)),
                    },
                    ty: ty(&types, Primitive::Fractus),
                    span: sample_span(),
                },
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "bit operation");
}

fn cursor_stream_gaps(stepped: bool) -> Vec<String> {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let (program, types) = minimal(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, numerus)],
        temps: Vec::new(),
        statements: vec![MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::CursorStream(MirCursorStream {
                        callee: radix_hir::DefId(0),
                        stepped,
                    }),
                    args: Vec::new(),
                    return_ty: ty(&types, Primitive::Vacuum),
                },
            },
            span: sample_span(),
        }],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    classify_with_types(&program, &types)
        .gaps()
        .iter()
        .map(|gap| gap.shape().to_owned())
        .collect()
}

#[test]
fn stepped_cursor_stream_names_gap() {
    assert_eq!(cursor_stream_gaps(true), ["stepped cursor stream"]);
}

#[test]
fn unstepped_cursor_stream_is_capable() {
    assert!(cursor_stream_gaps(false).is_empty());
}

fn local_verdict(types: &TypeTable, local_ty: MirType) -> radix_types::vocabulary::Lowerability {
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, local_ty)],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    classify_with_types(&program, types)
}

#[test]
fn none_typed_local_is_capable() {
    let types = TypeTable::new();
    let verdict = local_verdict(&types, ty(&types, Primitive::Nihil));
    assert!(verdict.is_capable(), "the type of null: {verdict:?}");
    let verdict = local_verdict(&types, ty(&types, Primitive::Ignotum));
    assert!(verdict.is_capable(), "an unknown value: {verdict:?}");
}

#[test]
fn bounded_text_is_capable_and_bounded_bytes_are_not() {
    let mut types = TypeTable::new();
    let capacity = types.intern_index(radix_types::IndexExpr::Literal(8));
    let text = MirType::semantic(types.bounded_textus(capacity));
    let ascii = MirType::semantic(types.bounded_ascii(capacity));
    for ty in [text, ascii] {
        let verdict = local_verdict(&types, ty);
        assert!(verdict.is_capable(), "a bounded text: {verdict:?}");
    }
    let bytes = MirType::semantic(types.bounded_octeti(capacity));
    let verdict = local_verdict(&types, bytes);
    let gaps = verdict.gaps();
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(gaps[0].shape(), "byte buffer value");
}

#[test]
fn references_are_values_or_shared_handles() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let list = types.array(numerus);
    let read_only = MirType::semantic(types.reference(SemanticMutability::Immutable, numerus));
    let verdict = local_verdict(&types, read_only);
    assert!(verdict.is_capable(), "a read-only borrow: {verdict:?}");

    let shared = MirType::semantic(types.reference(SemanticMutability::Mutable, list));
    let verdict = local_verdict(&types, shared);
    assert!(verdict.is_capable(), "a mutable list handle: {verdict:?}");

    let scalar = MirType::semantic(types.reference(SemanticMutability::Mutable, numerus));
    let verdict = local_verdict(&types, scalar);
    let gaps = verdict.gaps();
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(gaps[0].shape(), "reference value");
}

/// `sermo` is a builtin frame struct; the classifier recognizes a conversation
/// handle by the `SermoOpen` result type, so the fixture only needs one builtin
/// struct identity to stand in for it.
fn sermo_handle(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.intern(Type::Struct(radix_hir::DefId(1))))
}

fn runtime_call(
    intrinsic: MirIntrinsic,
    args: Vec<MirOperand>,
    return_ty: MirType,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic,
                args,
                return_ty,
            },
        },
        span: sample_span(),
    }
}

fn explicit_conversion(source_ty: MirType, target_ty: MirType) -> MirIntrinsic {
    MirIntrinsic::Convert(MirConversion {
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty,
        target_ty,
        params: Vec::new(),
        recovery: None,
        struct_defaults: Vec::new(),
    })
}

fn sermo_materialization_program(handle: MirType, target: MirType, vacuum: MirType) -> MirProgram {
    single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle), local(1, target)],
        temps: Vec::new(),
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(Symbol(21)))],
                handle,
            ),
            runtime_call(
                explicit_conversion(handle, target),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                target,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    })
}

#[test]
fn sync_scalar_sermo_conversation_is_capable() {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let numerus = ty(&types, Primitive::Numerus);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle), local(1, numerus)],
        temps: Vec::new(),
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(Symbol(21)))],
                handle,
            ),
            runtime_call(
                MirIntrinsic::SermoSetOpener,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Constant(MirConstant::Int(1)),
                ],
                vacuum,
            ),
            runtime_call(
                explicit_conversion(handle, numerus),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                numerus,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    assert!(
        verdict.is_capable(),
        "the sync scalar conversation is inside the subset: {verdict:?}"
    );
}

#[test]
fn sermo_void_materialization_is_capable() {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = sermo_materialization_program(handle, vacuum, vacuum);

    assert_eq!(classify_with_types(&program, &types), Lowerability::Capable);
}

#[test]
fn sermo_instant_materialization_is_capable() {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let vacuum = ty(&types, Primitive::Vacuum);
    let instant = types.primitive(Primitive::Instans);
    let sized_instant = types.sized_instans(InstansPrecision::Nanos);
    let instant_alias = types.intern(Type::Alias(radix_hir::DefId(22), sized_instant));
    let nested_instant_alias = types.intern(Type::Alias(radix_hir::DefId(23), instant_alias));

    for target in [instant, sized_instant, instant_alias, nested_instant_alias] {
        let target = MirType::semantic(target);
        let program = sermo_materialization_program(handle, target, vacuum);
        assert_eq!(classify_with_types(&program, &types), Lowerability::Capable);
    }
}

#[test]
fn sermo_ops_still_name_the_gap_in_storage_order() {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let numerus = ty(&types, Primitive::Numerus);
    let bivalens = ty(&types, Primitive::Bivalens);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle), local(1, numerus)],
        temps: Vec::new(),
        statements: vec![
            // Admitted calls do not mask the directional operations, and the
            // gaps keep statement order.
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(Symbol(21)))],
                handle,
            ),
            runtime_call(
                explicit_conversion(handle, numerus),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                numerus,
            ),
            runtime_call(MirIntrinsic::Sermo(MirSermoOp::Send), Vec::new(), vacuum),
            runtime_call(MirIntrinsic::Sermo(MirSermoOp::Close), Vec::new(), bivalens),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    let gaps = verdict.gaps();
    assert_eq!(gaps.len(), 2, "two gaps expected: {gaps:?}");
    assert_eq!(gaps[0].shape(), "runtime call Sermo(Send)");
    assert_eq!(gaps[1].shape(), "runtime call Sermo(Close)");
    assert_eq!(gaps[0].slug(), SLUG_CLIF_SUBSET);
}

#[test]
fn sermo_text_materialization_is_still_declined() {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let textus = ty(&types, Primitive::Textus);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle), local(1, textus)],
        temps: Vec::new(),
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(Symbol(21)))],
                handle,
            ),
            runtime_call(
                explicit_conversion(handle, textus),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                textus,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let verdict = classify_with_types(&program, &types);
    let gaps = verdict.gaps();
    assert_eq!(gaps.len(), 1, "one gap expected: {gaps:?}");
    assert!(
        gaps[0]
            .shape()
            .contains("Convert Runtime explicit Struct->Textus"),
        "a text materialization has no CLIF symbol yet: {gaps:?}"
    );
}

#[test]
fn mut_union_parameter_names_the_box_writeback_gap_through_aliases() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let text = types.primitive(Primitive::Textus);
    let union = types.intern(Type::Union(vec![numerus, text]));
    let alias = types.intern(Type::Alias(radix_hir::DefId(51), union));
    for ty in [union, alias] {
        let mut parameter = param(0, MirType::semantic(ty));
        assert!(!crate::capability::is_mut_union(&types, &parameter));
        parameter.mode = MirParamMode::MutRef;
        let program = single_function(FunctionShape {
            params: vec![parameter],
            locals: Vec::new(),
            temps: Vec::new(),
            statements: Vec::new(),
            terminator: MirTerminatorKind::Return(None),
            return_ty: self::ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
        });
        let verdict = classify_with_types(&program, &types);
        assert_eq!(verdict.gaps()[0].shape(), "mutable union parameter");
    }
}

#[test]
fn b64_to_u64_is_a_compile_time_gap_even_for_a_small_pattern() {
    let mut types = TypeTable::new();
    let bits = MirType::semantic(types.intern(Type::Bits(radix_types::BitsWidth::B64)));
    let unsigned = MirType::semantic(types.intern(Type::SizedNumeric(
        Primitive::Numerus,
        radix_types::NumericWidth::U64,
    )));
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        statements: vec![runtime_call(
            explicit_conversion(bits, unsigned),
            vec![MirOperand::Constant(MirConstant::UInt(1))],
            unsigned,
        )],
        terminator: MirTerminatorKind::Return(None),
        return_ty: ty(&types, Primitive::Vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    let verdict = classify_with_types(&program, &types);
    assert_eq!(verdict.gaps()[0].shape(), "b64 to u64 conversion");
}

#[test]
fn i64_edge_guard_is_limited_to_cyclic_inclusive_shapes() {
    let types = TypeTable::new();
    let shape = |bound, op, terminator| {
        single_function(FunctionShape {
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::temp(MirTempId(0)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Binary {
                            op,
                            lhs: MirOperand::Constant(MirConstant::Int(0)),
                            rhs: MirOperand::Constant(MirConstant::Int(bound)),
                        },
                        ty: ty(&types, Primitive::Bivalens),
                        span: sample_span(),
                    },
                },
                span: sample_span(),
            }],
            terminator,
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
        })
    };
    let cycle = MirTerminatorKind::Goto(MirBlockId(0));
    for (program, expected) in [
        (shape(i64::MAX, MirBinOp::LtEq, cycle.clone()), true),
        (shape(10, MirBinOp::LtEq, cycle.clone()), false),
        (shape(i64::MAX, MirBinOp::Lt, cycle), false),
        (
            shape(i64::MAX, MirBinOp::LtEq, MirTerminatorKind::Return(None)),
            false,
        ),
    ] {
        assert_eq!(
            crate::capability::has_i64_edge_loop(&program.functions[0]),
            expected
        );
    }
}
