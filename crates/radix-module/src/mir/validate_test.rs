// Validation contract tests for MIR programs.
//
// These tests prove that structural and type-level validation rejects
// malformed MIR and accepts well-formed programs. They cover atomic
// runtime calls, value operands, control flow edges, call signatures,
// and type consistency. Validation never repairs or interprets.

use super::*;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::lexer::Symbol;
use crate::mir::lower_analyzed_unit;
use crate::semantic::{
    FuncSig, IndexExpr, InferVar, NumericWidth, ParamType, Primitive, SemanticParamMode, Type, TypeTable,
};
use crate::test_support::test_span;
use radix_mir::validate::*;

fn ty(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

fn valid_number_program(types: &TypeTable) -> MirProgram {
    let number = ty(types, Primitive::Numerus);
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Int(1)))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: number,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

fn function_typed_value_callee_program(types: &mut TypeTable, args: Vec<MirOperand>) -> MirProgram {
    let number = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let callee = MirType::semantic(types.function(FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType { ty: number, mode: SemanticParamMode::Owned, optional: false, sponte: false }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    }));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: callee, mutable: false, span: test_span() }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::Value(MirOperand::Place(MirPlace::local(MirLocalId(0)))),
                        args,
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: MirType::semantic(vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

fn atomic_runtime_program(
    types: &mut TypeTable,
    op: MirAtomicOp,
    args: Vec<MirOperand>,
    return_ty: MirType,
    destination: Option<MirPlace>,
) -> MirProgram {
    let i32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I32));
    let atomic_ty = MirType::semantic(types.atomic(i32_ty.semantic_id()));
    let bool_ty = ty(types, Primitive::Bivalens);
    let vacuum_ty = ty(types, Primitive::Vacuum);
    let temp_ty = destination.as_ref().map_or(vacuum_ty, |_| return_ty);
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![
                MirLocal { id: MirLocalId(0), name: None, ty: atomic_ty, mutable: false, span: test_span() },
                MirLocal { id: MirLocalId(1), name: None, ty: i32_ty, mutable: false, span: test_span() },
                MirLocal { id: MirLocalId(2), name: None, ty: bool_ty, mutable: false, span: test_span() },
            ],
            temps: vec![MirTemp { id: MirTempId(0), ty: temp_ty, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination,
                        call: MirRuntimeCall { intrinsic: MirIntrinsic::Atomic(op), args, return_ty },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: vacuum_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

fn expect_validation_error(program: &MirProgram, types: &TypeTable, issue: &str) {
    let context = MirValidationContext::new(types);
    let errors = validate_program(program, &context).expect_err("MIR should be invalid");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == issue || error.issue.starts_with(issue)),
        "expected validation issue {issue:?}, got {errors:#?}"
    );
}

fn expect_validation_errors_with_context(program: &MirProgram, context: &MirValidationContext<'_>, issues: &[&str]) {
    let errors = validate_program(program, context).expect_err("MIR should be invalid");
    for issue in issues {
        assert!(
            errors
                .iter()
                .any(|error| error.issue == *issue || error.issue.starts_with(issue)),
            "expected validation issue {issue:?}, got {errors:#?}"
        );
    }
}

#[test]
fn validates_atomic_load_runtime_call() {
    let mut types = TypeTable::new();
    let i32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I32));

    let load = atomic_runtime_program(
        &mut types,
        MirAtomicOp::Load,
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        i32_ty,
        Some(MirPlace::temp(MirTempId(0))),
    );
    validate_program(&load, &MirValidationContext::new(&types)).expect("atomic load validates");
}

#[test]
fn validates_atomic_store_runtime_call() {
    let mut types = TypeTable::new();
    let vacuum_ty = ty(&types, Primitive::Vacuum);

    let store = atomic_runtime_program(
        &mut types,
        MirAtomicOp::Store,
        vec![
            MirOperand::Place(MirPlace::local(MirLocalId(0))),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        vacuum_ty,
        None,
    );
    validate_program(&store, &MirValidationContext::new(&types)).expect("atomic store validates");
}

#[test]
fn validates_atomic_compare_exchange_runtime_call() {
    let mut types = TypeTable::new();
    let bool_ty = ty(&types, Primitive::Bivalens);

    let compare_exchange = atomic_runtime_program(
        &mut types,
        MirAtomicOp::CompareExchange,
        vec![
            MirOperand::Place(MirPlace::local(MirLocalId(0))),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        bool_ty,
        Some(MirPlace::temp(MirTempId(0))),
    );
    validate_program(&compare_exchange, &MirValidationContext::new(&types)).expect("atomic compare_exchange validates");
}

#[test]
fn rejects_atomic_runtime_call_type_mismatch() {
    let mut types = TypeTable::new();
    let vacuum_ty = ty(&types, Primitive::Vacuum);
    let program = atomic_runtime_program(
        &mut types,
        MirAtomicOp::Store,
        vec![
            MirOperand::Place(MirPlace::local(MirLocalId(0))),
            MirOperand::Place(MirPlace::local(MirLocalId(2))),
        ],
        vacuum_ty,
        None,
    );

    expect_validation_error(&program, &types, "mir_validation_atomic_store_value_type_mismatch");
}

fn analyze(source: &str) -> crate::driver::AnalyzedModule {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

fn device_view_projection_program(types: &TypeTable, tensor_ty: MirType, element_ty: MirType) -> MirProgram {
    device_view_projection_program_at(types, tensor_ty, element_ty, MirConstant::Int(0))
}

fn device_view_projection_program_at(
    types: &TypeTable,
    tensor_ty: MirType,
    element_ty: MirType,
    index: MirConstant,
) -> MirProgram {
    let mut tensor_place = MirPlace::local(MirLocalId(0));
    tensor_place
        .projections
        .push(MirProjection::Index(MirOperand::Constant(index)));

    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: tensor_ty, mutable: false, span: test_span() }],
            temps: vec![MirTemp { id: MirTempId(0), ty: element_ty, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::temp(MirTempId(0)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(tensor_place)),
                            ty: element_ty,
                            span: test_span(),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

fn matrix_2x2_ty(types: &mut TypeTable) -> (MirType, MirType) {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let rows = types.intern_index(IndexExpr::Literal(2));
    let columns = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![rows, columns]));
    (
        MirType::semantic(types.intern(Type::Matrix(f32_ty, shape))),
        MirType::semantic(f32_ty),
    )
}

fn matrix_aggregate_program(
    types: &TypeTable,
    matrix_ty: MirType,
    element_ty: MirType,
    items: Vec<MirAggregateItem>,
) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: (0..4)
                .map(|index| MirLocal {
                    id: MirLocalId(index),
                    name: None,
                    ty: element_ty,
                    mutable: false,
                    span: test_span(),
                })
                .collect(),
            temps: vec![MirTemp { id: MirTempId(0), ty: matrix_ty, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Matrix,
                            ty: matrix_ty,
                            fields: MirAggregateFields::Ordered(items),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

#[test]
fn rejects_invalid_block_target() {
    let types = TypeTable::new();
    let mut program = valid_number_program(&types);
    program.functions[0].blocks[0].terminator.kind = MirTerminatorKind::Goto(MirBlockId(99));

    expect_validation_error(&program, &types, "mir_validation_block_bb99_does_not_exist");
}

#[test]
fn rejects_invalid_local_reference() {
    let types = TypeTable::new();
    let mut program = valid_number_program(&types);
    program.functions[0].blocks[0].terminator.kind =
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(99)))));

    expect_validation_error(&program, &types, "mir_validation_local_99_does_not_exist");
}

#[test]
fn accepts_rank1_device_view_index_projection() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, len)));
    let element_ty = MirType::semantic(f32_ty);
    let program = device_view_projection_program(&types, tensor_ty, element_ty);
    let context = MirValidationContext::new(&types);

    validate_program(&program, &context).expect("rank-1 device view projection validates");
}

#[test]
fn accepts_rank2_device_view_index_projection_with_one_linear_index() {
    // The view contract exists (85cd6cbd6): an N-element tensor index lowers to
    // one row-major integer index, so a rank-2 device view validates.
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim2, dim3]));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)));
    let element_ty = MirType::semantic(f32_ty);
    let program = device_view_projection_program(&types, tensor_ty, element_ty);

    validate_program(&program, &MirValidationContext::new(&types))
        .expect("rank-2 device view index projection validates");
}

#[test]
fn rejects_rank2_device_view_index_projection_with_non_integer_index() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim2, dim3]));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)));
    let element_ty = MirType::semantic(f32_ty);
    let program = device_view_projection_program_at(&types, tensor_ty, element_ty, MirConstant::Bool(true));

    let errors = validate_program(&program, &MirValidationContext::new(&types))
        .expect_err("a non-integer device view index should be invalid");
    assert!(
        errors
            .iter()
            .any(|error| error.issue.contains("device_view_index_is_not_integer")),
        "expected a non-integer device view index error, got {errors:#?}"
    );
}

#[test]
fn rejects_tensor_runtime_aggregate_index_projection() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.unspecified_shape();
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)));
    let element_ty = MirType::semantic(f32_ty);
    let program = device_view_projection_program(&types, tensor_ty, element_ty);

    expect_validation_error(
        &program,
        &types,
        "mir_validation_tensor_runtime_aggregate_has_no_device_view_projection_contract",
    );
}

#[test]
fn rejects_duplicate_local_declarations_with_same_type() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let mut program = valid_number_program(&types);
    program.functions[0].locals = vec![
        MirLocal { id: MirLocalId(0), name: None, ty: number, mutable: false, span: test_span() },
        MirLocal { id: MirLocalId(0), name: None, ty: number, mutable: false, span: test_span() },
    ];

    expect_validation_error(&program, &types, "mir_validation_duplicate_mir_local_id_0");
}

#[test]
fn rejects_invalid_temp_reference() {
    let types = TypeTable::new();
    let mut program = valid_number_program(&types);
    program.functions[0].blocks[0].terminator.kind = MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(99))));

    expect_validation_error(&program, &types, "mir_validation_temp_99_does_not_exist");
}

#[test]
fn rejects_value_operand_without_earlier_definition() {
    let types = TypeTable::new();
    let mut program = valid_number_program(&types);
    program.functions[0].blocks[0].terminator.kind = MirTerminatorKind::Return(Some(MirOperand::Value(MirValueId(99))));

    expect_validation_error(&program, &types, "mir_validation_value_v99_is_not_defined_earlier_in_mir");
}

#[test]
fn rejects_value_operand_from_prior_block() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![MirTemp { id: MirTempId(0), ty: number, span: test_span() }],
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::temp(MirTempId(0)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(1))),
                                ty: number,
                                span: test_span(),
                            },
                        },
                        span: test_span(),
                    }],
                    terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
                    span: test_span(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: Vec::new(),
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(Some(MirOperand::Value(MirValueId(0)))),
                        span: test_span(),
                    },
                    span: test_span(),
                },
            ],
            return_ty: number,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(&program, &types, "mir_validation_value_v0_is_not_defined_earlier_in_mir");
}

#[test]
fn rejects_return_type_mismatch() {
    let types = TypeTable::new();
    let mut program = valid_number_program(&types);
    program.functions[0].return_ty = ty(&types, Primitive::Textus);

    expect_validation_error(&program, &types, "mir_validation_return_type_mismatch");
}

#[test]
fn rejects_unresolved_infer_types() {
    let mut types = TypeTable::new();
    let infer = MirType::semantic(types.intern(Type::Infer(InferVar(0))));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: infer,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(&program, &types, "mir_validation_mir_type_is_unresolved_inference_variable");
}

#[test]
fn rejects_return_error_without_alternate_exit_type() {
    let types = TypeTable::new();
    let mut program = valid_number_program(&types);
    program.functions[0].blocks[0].terminator.kind =
        MirTerminatorKind::ReturnError(MirOperand::Constant(MirConstant::String(Symbol(1))));

    expect_validation_error(
        &program,
        &types,
        "mir_validation_return_error_in_function_without_alternate_exit_type",
    );
}

#[test]
fn rejects_non_bivalens_branch_condition() {
    let types = TypeTable::new();
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Branch {
                            condition: MirOperand::Constant(MirConstant::Int(1)),
                            then_block: MirBlockId(1),
                            else_block: MirBlockId(1),
                        },
                        span: test_span(),
                    },
                    span: test_span(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(&program, &types, "mir_validation_branch_condition_is_not_bivalens");
}

#[test]
fn rejects_malformed_try_call_edges() {
    let types = TypeTable::new();
    let text = ty(&types, Primitive::Textus);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: Vec::new(),
                locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: text, mutable: false, span: test_span() }],
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::TryCall {
                            destination: None,
                            callee: MirCallee::Function(MirFunctionId(1)),
                            args: Vec::new(),
                            ok_block: MirBlockId(0),
                            error_place: MirPlace::local(MirLocalId(0)),
                            error_block: MirBlockId(99),
                        },
                        span: test_span(),
                    },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: Some(text),
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
        ],
    };

    expect_validation_error(&program, &types, "mir_validation_block_bb99_does_not_exist");
}

#[test]
fn rejects_try_call_without_known_failable_signature() {
    let types = TypeTable::new();
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: vacuum, mutable: false, span: test_span() }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::TryCall {
                        destination: None,
                        callee: MirCallee::Value(MirOperand::Constant(MirConstant::Unit)),
                        args: Vec::new(),
                        ok_block: MirBlockId(0),
                        error_place: MirPlace::local(MirLocalId(0)),
                        error_block: MirBlockId(0),
                    },
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(
        &program,
        &types,
        "mir_validation_try_call_callee_does_not_have_a_known_failable_signature",
    );
}

#[test]
fn rejects_call_argument_count_and_type_mismatch() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Call {
                            destination: None,
                            callee: MirCallee::Function(MirFunctionId(1)),
                            args: vec![MirOperand::Constant(MirConstant::String(Symbol(1)))],
                        },
                        span: test_span(),
                    }],
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: None,
                params: vec![
                    MirParam {
                        local: MirLocalId(0),
                        name: None,
                        ty: number,
                        mode: MirParamMode::Owned,
                        span: test_span(),
                    },
                    MirParam {
                        local: MirLocalId(1),
                        name: None,
                        ty: number,
                        mode: MirParamMode::Owned,
                        span: test_span(),
                    },
                ],
                locals: vec![
                    MirLocal { id: MirLocalId(0), name: None, ty: number, mutable: false, span: test_span() },
                    MirLocal { id: MirLocalId(1), name: None, ty: number, mutable: false, span: test_span() },
                ],
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
        ],
    };

    let context = MirValidationContext::new(&types);
    expect_validation_errors_with_context(
        &program,
        &context,
        &[
            "mir_validation_call_argument_count_mismatch",
            "mir_validation_call_argument_type_mismatch",
        ],
    );
}

#[test]
fn mir_generic_call_rejects_conflicting_type_param_witnesses() {
    let mut types = TypeTable::new();
    let generic_t = MirType::semantic(types.intern(Type::Param(Symbol(1))));
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Call {
                            destination: None,
                            callee: MirCallee::Function(MirFunctionId(1)),
                            args: vec![
                                MirOperand::Constant(MirConstant::String(Symbol(2))),
                                MirOperand::Constant(MirConstant::Int(1)),
                            ],
                        },
                        span: test_span(),
                    }],
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: None,
                params: vec![
                    MirParam {
                        local: MirLocalId(0),
                        name: None,
                        ty: generic_t,
                        mode: MirParamMode::Owned,
                        span: test_span(),
                    },
                    MirParam {
                        local: MirLocalId(1),
                        name: None,
                        ty: generic_t,
                        mode: MirParamMode::Owned,
                        span: test_span(),
                    },
                ],
                locals: vec![
                    MirLocal { id: MirLocalId(0), name: None, ty: generic_t, mutable: false, span: test_span() },
                    MirLocal { id: MirLocalId(1), name: None, ty: generic_t, mutable: false, span: test_span() },
                ],
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
        ],
    };

    expect_validation_error(&program, &types, "mir_validation_call_argument_type_mismatch");
}

#[test]
fn value_callee_function_type_accepts_matching_argument() {
    let mut types = TypeTable::new();
    let program = function_typed_value_callee_program(&mut types, vec![MirOperand::Constant(MirConstant::Int(1))]);

    validate_program(&program, &MirValidationContext::new(&types)).expect("function-typed value callee validates");
}

#[test]
fn value_callee_function_type_rejects_argument_count_mismatch() {
    let mut types = TypeTable::new();
    let program = function_typed_value_callee_program(&mut types, Vec::new());

    expect_validation_error(&program, &types, "mir_validation_call_argument_count_mismatch");
}

#[test]
fn value_callee_function_type_rejects_argument_type_mismatch() {
    let mut types = TypeTable::new();
    let program = function_typed_value_callee_program(&mut types, vec![MirOperand::Constant(MirConstant::Bool(true))]);

    expect_validation_error(&program, &types, "mir_validation_call_argument_type_mismatch");
}

#[test]
fn rejects_ordinary_call_to_failable_function() {
    let types = TypeTable::new();
    let text = ty(&types, Primitive::Textus);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Call {
                            destination: None,
                            callee: MirCallee::Function(MirFunctionId(1)),
                            args: Vec::new(),
                        },
                        span: test_span(),
                    }],
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: Some(text),
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
        ],
    };

    expect_validation_error(&program, &types, "mir_validation_ordinary_call_callee_is_failable");
}

#[test]
fn rejects_runtime_call_destination_type_mismatch() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let text = ty(&types, Primitive::Textus);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: text, mutable: false, span: test_span() }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(0))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::Length),
                            args: vec![MirOperand::Constant(MirConstant::String(Symbol(1)))],
                            return_ty: number,
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(&program, &types, "mir_validation_destination_type_mismatch");
}

#[test]
fn rejects_sparsa_nonnihil_with_non_sparse_receiver() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let text = ty(&types, Primitive::Textus);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: text, mutable: false, span: test_span() }],
            temps: vec![MirTemp { id: MirTempId(0), ty: number, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::temp(MirTempId(0))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::SparseNonzeroCount),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                            return_ty: number,
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(&program, &types, "mir_validation_sparse_nonzero_count_receiver_is_not_sparsa");
}

#[test]
fn rejects_aggregate_payload_shape_mismatch() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![MirTemp { id: MirTempId(0), ty: number, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Map,
                            ty: number,
                            fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(MirOperand::Constant(
                                MirConstant::Int(1),
                            ))]),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0)))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: number,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(
        &program,
        &types,
        "mir_validation_aggregate_payload_shape_does_not_match_aggregate_kind",
    );
}

#[test]
fn rejects_typed_aggregate_mismatches() {
    let mut types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let struct_def = crate::hir::DefId(42);
    let struct_ty = MirType::semantic(types.intern(Type::Struct(struct_def)));
    let field = Symbol(7);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![MirTemp { id: MirTempId(0), ty: struct_ty, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Struct(struct_def),
                            ty: struct_ty,
                            fields: MirAggregateFields::Named(vec![
                                MirNamedOperand {
                                    name: field,
                                    value: MirOperand::Constant(MirConstant::String(Symbol(1))),
                                },
                                MirNamedOperand { name: field, value: MirOperand::Constant(MirConstant::Int(1)) },
                            ]),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0)))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: struct_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };
    let mut context = MirValidationContext::new(&types);
    let mut fields = rustc_hash::FxHashMap::default();
    fields.insert(field, number);
    context.struct_fields.insert(struct_def, fields);

    expect_validation_errors_with_context(
        &program,
        &context,
        &[
            "mir_validation_named_aggregate_field_type_mismatch",
            "mir_validation_named_aggregate_field_is_duplicated",
        ],
    );
}

#[test]
fn rejects_enum_variant_aggregate_parent_mismatch() {
    let mut types = TypeTable::new();
    let enum_a = crate::hir::DefId(10);
    let enum_b = crate::hir::DefId(11);
    let variant_a = crate::hir::DefId(12);
    let enum_b_ty = MirType::semantic(types.intern(Type::Enum(enum_b)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![MirTemp { id: MirTempId(0), ty: enum_b_ty, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::EnumVariant(variant_a),
                            ty: enum_b_ty,
                            fields: MirAggregateFields::Ordered(Vec::new()),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };
    let mut context = MirValidationContext::new(&types);
    context.variant_parents.insert(variant_a, enum_a);

    expect_validation_errors_with_context(
        &program,
        &context,
        &["mir_validation_enum_variant_does_not_belong_to_enum_type"],
    );
}

#[test]
fn rejects_variant_field_projection_parent_mismatch() {
    let mut types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let enum_a = crate::hir::DefId(20);
    let enum_b = crate::hir::DefId(21);
    let variant_a = crate::hir::DefId(22);
    let field = Symbol(1);
    let enum_b_ty = MirType::semantic(types.intern(Type::Enum(enum_b)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![
                MirTemp { id: MirTempId(0), ty: enum_b_ty, span: test_span() },
                MirTemp { id: MirTempId(1), ty: number, span: test_span() },
            ],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::temp(MirTempId(1)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Temp(MirTempId(0)),
                                projections: vec![MirProjection::VariantField { variant: variant_a, field }],
                            })),
                            ty: number,
                            span: test_span(),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };
    let mut context = MirValidationContext::new(&types);
    context.variant_parents.insert(variant_a, enum_a);
    let mut fields = rustc_hash::FxHashMap::default();
    fields.insert(field, number);
    context.variant_fields.insert(variant_a, fields);

    expect_validation_errors_with_context(
        &program,
        &context,
        &["mir_validation_enum_variant_does_not_belong_to_enum_type"],
    );
}

#[test]
fn validates_record_aggregate_named_fields() {
    let mut types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let boolean = ty(&types, Primitive::Bivalens);
    let mut record_fields = rustc_hash::FxHashMap::default();
    record_fields.insert(Symbol(1), number.semantic_id());
    record_fields.insert(Symbol(2), boolean.semantic_id());
    let record_ty = MirType::semantic(types.intern(Type::Record(record_fields)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![MirTemp { id: MirTempId(0), ty: record_ty, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Record,
                            ty: record_ty,
                            fields: MirAggregateFields::Named(vec![
                                MirNamedOperand {
                                    name: Symbol(2),
                                    value: MirOperand::Constant(MirConstant::Bool(true)),
                                },
                                MirNamedOperand { name: Symbol(1), value: MirOperand::Constant(MirConstant::Int(7)) },
                            ]),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    validate_program(&program, &MirValidationContext::new(&types)).expect("record aggregate validates");
}

#[test]
fn validates_matrix_aggregate_ordered_cells() {
    let mut types = TypeTable::new();
    let (matrix_ty, element_ty) = matrix_2x2_ty(&mut types);
    let items = (0..4)
        .map(|index| MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(MirLocalId(index)))))
        .collect();
    let program = matrix_aggregate_program(&types, matrix_ty, element_ty, items);

    validate_program(&program, &MirValidationContext::new(&types)).expect("matrix aggregate validates");
}

#[test]
fn rejects_matrix_aggregate_cell_count_mismatch() {
    let mut types = TypeTable::new();
    let (matrix_ty, element_ty) = matrix_2x2_ty(&mut types);
    let items = (0..3)
        .map(|index| MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(MirLocalId(index)))))
        .collect();
    let program = matrix_aggregate_program(&types, matrix_ty, element_ty, items);

    expect_validation_error(
        &program,
        &types,
        "mir_validation_matrix_aggregate_element_count_does_not_match_shape",
    );
}

#[test]
fn rejects_record_aggregate_field_mismatches() {
    let mut types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let mut record_fields = rustc_hash::FxHashMap::default();
    record_fields.insert(Symbol(1), number.semantic_id());
    record_fields.insert(Symbol(2), number.semantic_id());
    let record_ty = MirType::semantic(types.intern(Type::Record(record_fields)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![MirTemp { id: MirTempId(0), ty: record_ty, span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Record,
                            ty: record_ty,
                            fields: MirAggregateFields::Named(vec![
                                MirNamedOperand {
                                    name: Symbol(1),
                                    value: MirOperand::Constant(MirConstant::Bool(true)),
                                },
                                MirNamedOperand { name: Symbol(1), value: MirOperand::Constant(MirConstant::Int(7)) },
                                MirNamedOperand { name: Symbol(3), value: MirOperand::Constant(MirConstant::Int(8)) },
                            ]),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_errors_with_context(
        &program,
        &MirValidationContext::new(&types),
        &[
            "mir_validation_record_aggregate_field_type_mismatch",
            "mir_validation_record_aggregate_field_is_duplicated",
            "mir_validation_record_aggregate_references_unknown_field",
            "mir_validation_record_aggregate_is_missing_required_field",
        ],
    );
}

#[test]
fn rejects_runtime_intrinsic_argument_mismatch() {
    let types = TypeTable::new();
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: None,
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Convert(MirConversion {
                                store: None,
                                flavor: MirConversionFlavor::Runtime,
                                source_ty: vacuum,
                                target_ty: vacuum,
                                params: Vec::new(),
                                recovery: None,
                                struct_defaults: Vec::new(),
                            }),
                            args: Vec::new(),
                            return_ty: vacuum,
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(
        &program,
        &types,
        "mir_validation_conversion_runtime_call_expects_1_mir_arguments",
    );
}

#[test]
fn rejects_lista_to_tensor_runtime_convert() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let lista_numerus = types.array(numerus);
    let dim = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim]));
    let tensor_numerus = MirType::semantic(types.tensor_with_shape(numerus, shape));
    let lista_numerus = MirType::semantic(lista_numerus);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: lista_numerus,
                mutable: false,
                span: test_span(),
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: None,
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Convert(MirConversion {
                                store: None,
                                flavor: MirConversionFlavor::Runtime,
                                source_ty: lista_numerus,
                                target_ty: tensor_numerus,
                                params: Vec::new(),
                                recovery: None,
                                struct_defaults: Vec::new(),
                            }),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                            return_ty: tensor_numerus,
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(
        &program,
        &types,
        "mir_validation_eligible_lista_to_tensor_conversio_must_lower_to_tensor_from_flat_not_runtime_convert",
    );
}

#[test]
#[ignore = "standalone stepper capability-gap (octeti/unify.fab); runner_unsupported_mir_lowering_method_call_before_runtime_provider_mir_lowering"]
fn octeti_unify_exemplum_mir_validates() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../corpus/octeti/unify.fab");
    let source = std::fs::read_to_string(path).expect("read octeti unify exemplum");
    let unit = analyze(&source);
    lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("octeti unify exemplum validates in MIR");
}

#[test]
fn validates_valor_array_carrier_aggregate() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../corpus/conversio/valor-tensor.fab");
    let source = std::fs::read_to_string(path).expect("read valor tensor exemplum");
    let unit = analyze(&source);
    lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("valor array carrier validates in MIR");
}

#[ignore = "MIR validation — Stage 6 MIR lowering not yet complete"]
#[test]
fn representative_lowered_phase_3_to_7_mir_validates() {
    for source in [
        r"functio arithmetic(numerus a, numerus b) → numerus { redde a + b }",
        r"functio choose(bivalens ready) → numerus { si ready { redde 1 } secus { redde 2 } }",
        r#"functio maybe(textus ∪ nihil name) → textus { redde name vel "ignotus" }"#,
        r#"genus Persona { varia textus nomen varia numerus aetas = 0 } functio age() → numerus { sit Persona p ← Persona { nomen = "Ada" } redde p.aetas }"#,
        r"genus VacuumStruct {} functio empty_struct() → VacuumStruct { redde {} ∷ VacuumStruct }",
        r"discretio Status { Active } functio active() → Status { redde finge Active ∷ Status }",
        r"functio count(lista<numerus> xs) → numerus { redde xs.longitudo() }",
        r"functio parse(textus raw) → numerus { redde raw ↦ i32 via Hex ⊥ 0 }",
        r#"functio log(textus name) → vacuum { nota "salve" vide name mone "cave" }"#,
    ] {
        let unit = analyze(source);
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("lowered MIR validates");
    }
}

fn modular_array_program(types: &mut TypeTable, element: MirConstant) -> MirProgram {
    let modular = types.intern(Type::ModularWord(NumericWidth::U32));
    let array = types.array(modular);
    let array_ty = MirType::semantic(array);
    let vacuum_ty = ty(types, Primitive::Vacuum);
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: array_ty, mutable: false, span: test_span() }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(MirLocalId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Array,
                            ty: array_ty,
                            fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(MirOperand::Constant(
                                element,
                            ))]),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                span: test_span(),
            }],
            return_ty: vacuum_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

#[test]
fn accepts_fitting_constant_for_modular_element() {
    let mut types = TypeTable::new();
    let program = modular_array_program(&mut types, MirConstant::Int(4_294_967_295));
    validate_program(&program, &MirValidationContext::new(&types)).expect("u32::MAX fits modulus<u32>");
}

#[test]
fn rejects_out_of_range_constant_for_modular_element() {
    // The fitting-constant acceptance must not weaken validation: one past
    // u32::MAX does not fit `modulus<u32>` and is still an error.
    let mut types = TypeTable::new();
    let program = modular_array_program(&mut types, MirConstant::Int(4_294_967_296));
    expect_validation_error(&program, &types, "mir_validation_aggregate_element_type_mismatch");
}

#[test]
fn rejects_negative_constant_for_modular_element() {
    let mut types = TypeTable::new();
    let program = modular_array_program(&mut types, MirConstant::Int(-1));
    expect_validation_error(&program, &types, "mir_validation_aggregate_element_type_mismatch");
}

// --- Edge siblings for validation coverage ---

#[test]
fn rejects_duplicate_function_ids() {
    let types = TypeTable::new();
    let vacuum_ty = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum_ty,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
            MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum_ty,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
        ],
    };

    expect_validation_error(&program, &types, "mir_validation_duplicate_mir_function_id_f0");
}

#[test]
fn rejects_no_value_return_in_value_returning_function() {
    let types = TypeTable::new();
    let mut program = valid_number_program(&types);
    // Return a None terminator (no value) in a function with Numerus return type.
    program.functions[0].blocks[0].terminator.kind = MirTerminatorKind::Return(None);

    expect_validation_error(&program, &types, "mir_validation_no_value_return_in_value_returning_function");
}

#[test]
fn rejects_block_not_reachable_from_entry() {
    let types = TypeTable::new();
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![
                // Entry block terminates with Return — no successors.
                MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                },
                // bb1 is not reachable from entry (nothing targets it).
                // Its terminator references bb2, making bb2 a successor target.
                MirBlock {
                    id: MirBlockId(1),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(2)), span: test_span() },
                    span: test_span(),
                },
                // bb2 is referenced as a successor (by bb1) but not reachable
                // from entry — genuine orphan.
                MirBlock {
                    id: MirBlockId(2),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_error(&program, &types, "mir_validation_block_bb2_is_not_reachable_from_entry");
}

#[test]
fn rejects_block_that_does_not_reach_terminator() {
    let types = TypeTable::new();
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
                    span: test_span(),
                },
                // bb1 has Goto bb0, creating a loop that never reaches Return/ReturnError/Unreachable.
                MirBlock {
                    id: MirBlockId(1),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(0)), span: test_span() },
                    span: test_span(),
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    expect_validation_errors_with_context(
        &program,
        &MirValidationContext::new(&types),
        &[
            "mir_validation_block_bb0_does_not_reach_a_terminator",
            "mir_validation_block_bb1_does_not_reach_a_terminator",
        ],
    );
}

#[test]
fn rejects_call_where_non_vacuum_result_has_no_destination() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Call {
                            destination: None,
                            callee: MirCallee::Function(MirFunctionId(1)),
                            args: Vec::new(),
                        },
                        span: test_span(),
                    }],
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: number,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
        ],
    };

    expect_validation_error(&program, &types, "mir_validation_non_vacuum_result_has_no_destination");
}
