//! C2 proof: hand-built MIR fixtures drive the capability classifier
//! (goal `radix-mir-clif`, unit C2).

use crate::capability::{SLUG_CLIF_POLICY, SLUG_CLIF_SUBSET, classify_with_types};
use radix_lexer::{Span, Symbol};
use radix_mir::*;
use radix_types::Type;
use radix_types::{Primitive, TypeTable};

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
fn string_local_names_gap() {
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
    let gaps = verdict.gaps();
    assert_eq!(gaps.len(), 1, "one gap expected: {gaps:?}");
    assert_eq!(gaps[0].shape(), "string value");
    assert_eq!(gaps[0].slug(), SLUG_CLIF_SUBSET);
}

#[test]
fn gaps_keep_storage_order() {
    let types = TypeTable::new();
    let string = ty(&types, Primitive::Textus);
    let instans = ty(&types, Primitive::Instans);
    let program = single_function(FunctionShape {
        params: vec![param(0, string)],
        locals: vec![local(1, instans)],
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
    assert_eq!(gaps[0].shape(), "string value", "param walks before local");
    assert_eq!(gaps[1].shape(), "instans value");
}

#[test]
fn collection_type_names_gap() {
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
fn try_call_names_gap() {
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
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "try_call");
}

#[test]
fn error_typed_function_names_gap() {
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
    assert!(!verdict.gaps().is_empty());
    assert_eq!(verdict.gaps()[0].shape(), "error-typed function");
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
                    intrinsic: MirIntrinsic::Assert,
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
    assert_eq!(verdict.gaps()[0].shape(), "runtime call");
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
                    kind: MirAggregateKind::Tuple,
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
    assert_eq!(verdict.gaps()[0].shape(), "tuple construction");
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
                    projections: vec![MirProjection::Field(Symbol(2))],
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
fn closure_callee_names_gap() {
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
    assert_eq!(verdict.gaps().len(), 1);
    assert_eq!(verdict.gaps()[0].shape(), "closure call");
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
    assert_eq!(verdict.gaps()[0].shape(), "bit operation");
}
