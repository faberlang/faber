// similar_names only honors module-level allows (fn/statement allows are
// ignored in this clippy); `callee`/`caller` below are the standard domain
// pair and the similarity is deliberate.
#![allow(clippy::similar_names)]

use super::*;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirCallee, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirGradientOp,
    MirIntrinsic, MirLocal, MirLocalId, MirNamedOperand, MirOperand, MirOptionChainLink,
    MirOptionOp, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProgram, MirProjection,
    MirRuntimeCall, MirStatement, MirStatementKind, MirSwitchCase, MirTerminator,
    MirTerminatorKind, MirType, MirUnOp, MirValue, MirValueId, MirValueKind,
};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{
    AppliedArg, FuncSig, IndexExpr, NumericWidth, ParamType, Primitive, RuntimeExtentId,
    SemanticParamMode, Type, TypeId, TypeTable,
};
use rustc_hash::FxHashMap;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Create a `tensor<element, [dim1, dim2, ...]>` type.
fn tensor_type(types: &mut TypeTable, element: Primitive, dims: &[u64]) -> TypeId {
    let elem_id = types.primitive(element);
    let dim_ids: Vec<_> = dims
        .iter()
        .map(|&d| types.intern_index(IndexExpr::Literal(d)))
        .collect();
    let shape = if dim_ids.is_empty() {
        types.rank0_shape()
    } else if dim_ids.len() == 1 {
        types.intern_index(IndexExpr::Literal(dims[0]))
    } else {
        types.intern_index(IndexExpr::Tuple(dim_ids))
    };
    types.tensor_with_shape(elem_id, shape)
}

/// Run validation on a program and return the error messages.
fn validate(program: &MirProgram, types: &TypeTable) -> Vec<String> {
    let context = MirValidationContext::new(types);
    validate_with(program, &context)
}

fn validate_with(program: &MirProgram, context: &MirValidationContext<'_>) -> Vec<String> {
    match validate_program(program, context) {
        Ok(()) => vec![],
        Err(errors) => errors.into_iter().map(|e| e.message).collect(),
    }
}

/// Run validation and return the structured errors (issue + message + span).
fn validate_structured(program: &MirProgram, types: &TypeTable) -> Vec<MirValidationError> {
    let context = MirValidationContext::new(types);
    match validate_program(program, &context) {
        Ok(()) => vec![],
        Err(errors) => errors,
    }
}

/// Build a one-function program whose block contains a single
/// `TensorLayerNorm` runtime call.
#[allow(clippy::too_many_arguments)]
fn layernorm_program(
    types: &mut TypeTable,
    input_dims: &[u64],
    axis: i64,
    epsilon: f64,
    has_gamma: bool,
    has_beta: bool,
    // Override types for negative testing (None = derive from dims).
    override_input_ty: Option<MirType>,
    override_gamma_ty: Option<MirType>,
    override_beta_ty: Option<MirType>,
    override_return_ty: Option<MirType>,
) -> MirProgram {
    let input_mty = override_input_ty.unwrap_or_else(|| {
        let tid = tensor_type(types, Primitive::Fractus, input_dims);
        MirType::semantic(tid)
    });
    let gamma_mty = override_gamma_ty.unwrap_or_else(|| {
        let tid = tensor_type(types, Primitive::Fractus, input_dims);
        MirType::semantic(tid)
    });
    let beta_mty = override_beta_ty.unwrap_or_else(|| {
        let tid = tensor_type(types, Primitive::Fractus, input_dims);
        MirType::semantic(tid)
    });
    let return_mty = override_return_ty.unwrap_or_else(|| {
        let tid = tensor_type(types, Primitive::Fractus, input_dims);
        MirType::semantic(tid)
    });

    let mut locals: Vec<MirLocal> = vec![
        MirLocal {
            id: MirLocalId(0),
            ty: input_mty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(1),
            ty: return_mty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
    ];
    let mut next_local = MirLocalId(2);

    let mut args: Vec<MirOperand> = vec![MirOperand::Place(MirPlace {
        base: MirPlaceBase::Local(MirLocalId(0)),
        projections: vec![],
    })];

    // Method-call order: input.laminatio(axis, epsilon, gamma?, beta?).
    args.push(MirOperand::Constant(MirConstant::Int(axis)));
    args.push(MirOperand::Constant(MirConstant::Float(epsilon)));

    let destination = Some(MirPlace {
        base: MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![],
    });

    if has_gamma {
        locals.push(MirLocal {
            id: next_local,
            ty: gamma_mty,
            name: None,
            mutable: false,
            span: Span::default(),
        });
        args.push(MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(next_local),
            projections: vec![],
        }));
        next_local.0 += 1;
    }

    if has_beta {
        locals.push(MirLocal {
            id: next_local,
            ty: beta_mty,
            name: None,
            mutable: false,
            span: Span::default(),
        });
        args.push(MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(next_local),
            projections: vec![],
        }));
    }

    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals,
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination,
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
                            args,
                            return_ty: return_mty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

// -----------------------------------------------------------------------
// Positive tests — structurally valid TensorLayerNorm calls
// -----------------------------------------------------------------------

#[test]
fn layernorm_valid_rank1_3args() {
    let mut types = TypeTable::new();
    let program = layernorm_program(
        &mut types,
        &[4],  // rank-1 [4]
        0,     // axis 0
        1e-5,  // epsilon
        false, // no gamma
        false, // no beta
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn layernorm_valid_rank2_3args() {
    let mut types = TypeTable::new();
    let program = layernorm_program(
        &mut types,
        &[8, 16], // rank-2 [8, 16]
        1,        // axis 1
        1e-5,
        false,
        false,
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn layernorm_valid_with_gamma() {
    let mut types = TypeTable::new();
    // gamma is rank-1 [4] matching the input.
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        1e-5,
        true,  // has gamma
        false, // no beta
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn layernorm_valid_with_gamma_beta() {
    let mut types = TypeTable::new();
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        1e-5,
        true, // has gamma
        true, // has beta
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn layernorm_valid_rank2_axis0() {
    let mut types = TypeTable::new();
    let program = layernorm_program(
        &mut types,
        &[8, 16],
        0, // axis 0 (normalize over dim 0)
        1e-5,
        false,
        false,
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

// -----------------------------------------------------------------------
// Negative tests — structurally invalid TensorLayerNorm calls
// -----------------------------------------------------------------------

#[test]
fn layernorm_invalid_not_tensor() {
    let mut types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        1e-5,
        false,
        false,
        Some(numerus), // input is numerus, not tensor
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("input is not tensor")),
        "expected 'input is not tensor' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_rank3() {
    let mut types = TypeTable::new();
    // Rank-3 tensor — validation limits layernorm to rank 1 or 2.
    let tid = tensor_type(&mut types, Primitive::Fractus, &[2, 4, 8]);
    let ty = MirType::semantic(tid);
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        1e-5,
        false,
        false,
        Some(ty), // input is rank-3
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("must have rank 1 or 2")),
        "expected rank error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_axis_out_of_range() {
    let mut types = TypeTable::new();
    let program = layernorm_program(
        &mut types,
        &[4], // rank-1
        2,    // axis 2 is out of range for rank 1
        1e-5,
        false,
        false,
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("out of range")),
        "expected 'out of range' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_axis_negative() {
    let mut types = TypeTable::new();
    let program = layernorm_program(
        &mut types,
        &[4],
        -1, // negative axis is invalid
        1e-5,
        false,
        false,
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("out of range")),
        "expected 'out of range' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_axis_not_int() {
    let mut types = TypeTable::new();
    // Only override axis: use float constant instead of int by constructing
    // the program manually — we need to replace the axis operand.
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let tid = tensor_type(&mut types, Primitive::Fractus, &[4]);
    let input_mty = MirType::semantic(tid);
    let out_tid = tensor_type(&mut types, Primitive::Fractus, &[4]);
    let out_mty = MirType::semantic(out_tid);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: input_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
                            args: vec![
                                MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                                MirOperand::Constant(MirConstant::Float(1.0)), // ← not Int
                                MirOperand::Constant(MirConstant::Float(1e-5)),
                            ],
                            return_ty: out_mty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("axis must be an int constant")),
        "expected 'axis must be an int constant' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_epsilon_not_float() {
    let mut types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let tid = tensor_type(&mut types, Primitive::Fractus, &[4]);
    let input_mty = MirType::semantic(tid);
    let out_tid = tensor_type(&mut types, Primitive::Fractus, &[4]);
    let out_mty = MirType::semantic(out_tid);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: input_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
                            args: vec![
                                MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                                MirOperand::Constant(MirConstant::Int(0)),
                                MirOperand::Constant(MirConstant::Int(1)), // ← Int, not float
                            ],
                            return_ty: out_mty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("epsilon must be a float constant")),
        "expected 'epsilon must be a float constant' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_epsilon_non_positive() {
    let mut types = TypeTable::new();
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        -1.0, // epsilon must be positive
        false,
        false,
        None,
        None,
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("epsilon must be positive")),
        "expected 'epsilon must be positive' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_gamma_not_tensor() {
    let mut types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        1e-5,
        true, // has gamma
        false,
        None,
        Some(numerus), // gamma is numerus, not tensor
        None,
        None,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("gamma is not tensor")),
        "expected 'gamma is not tensor' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_beta_not_tensor() {
    let mut types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        1e-5,
        true, // has gamma
        true, // has beta
        None,
        None,          // gamma is fine
        Some(numerus), // beta is numerus
        None,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("beta is not tensor")),
        "expected 'beta is not tensor' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_result_not_tensor() {
    let mut types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = layernorm_program(
        &mut types,
        &[4],
        0,
        1e-5,
        false,
        false,
        None,
        None,
        None,
        Some(numerus), // return type is numerus
    );
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("result is not tensor")),
        "expected 'result is not tensor' error, got: {errors:?}"
    );
}

#[test]
fn layernorm_invalid_arity_too_few() {
    // Only one arg — should not panic from axis_idx underflow.
    let mut types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let f32_id = types.primitive(Primitive::Fractus);
    let shape1 = types.intern_index(IndexExpr::Literal(4));
    let input_tid = types.tensor_with_shape(f32_id, shape1);
    let input_mty = MirType::semantic(input_tid);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: input_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
                            args: vec![MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![],
                            })],
                            return_ty: input_mty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    // Validate should NOT panic from usize underflow on axis_idx = n - 2.
    let errors = validate(&program, &types);
    // At minimum the arity check should have fired.
    assert!(
        !errors.is_empty(),
        "expected at least one arity error, got none"
    );
}

// -----------------------------------------------------------------------
// Stage 1B — tuple aggregate construction
// -----------------------------------------------------------------------

#[test]
fn tuple_aggregate_valid() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let tuple_tid = types.tuple(vec![bivalens, bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: tuple_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple_mty,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Bool(
                                    true,
                                ))),
                                MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Bool(
                                    false,
                                ))),
                            ]),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: vec![],
                    }))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: tuple_mty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn return_accepts_runtime_dependent_tensor_shape_relation() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Fractus);
    let runtime = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarExpression {
        expression: 7,
    }));
    let from = MirType::semantic(types.tensor_with_shape(element, runtime));
    let literal = types.intern_index(IndexExpr::Literal(3));
    let to = MirType::semantic(types.tensor_with_shape(element, literal));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: from,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: vec![],
                    }))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: to,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        !errors
            .iter()
            .any(|error| error.contains("return type mismatch")),
        "unexpected return mismatch: {errors:?}"
    );
}

#[test]
fn tuple_aggregate_element_count_mismatch() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    // Tuple arity 2 but we provide 1 element
    let tuple_tid = types.tuple(vec![bivalens, bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: tuple_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple_mty,
                            fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(
                                MirOperand::Constant(MirConstant::Bool(true)),
                            )]),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: vec![],
                    }))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: tuple_mty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("element count")),
        "expected 'element count' error, got: {errors:?}"
    );
}

#[test]
fn tuple_aggregate_element_type_mismatch() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let tuple_tid = types.tuple(vec![bivalens, bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: tuple_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple_mty,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Bool(
                                    true,
                                ))),
                                // Second element should be bivalens but is numerus
                                MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(
                                    42,
                                ))),
                            ]),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: vec![],
                    }))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: tuple_mty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("element type mismatch")),
        "expected 'element type mismatch' error, got: {errors:?}"
    );
}

#[test]
fn tuple_aggregate_spread_rejected() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let tuple_tid = types.tuple(vec![bivalens, bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: tuple_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: tuple_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple_mty,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Bool(
                                    true,
                                ))),
                                MirAggregateItem::Spread(MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(1)),
                                    projections: vec![],
                                })),
                            ]),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: vec![],
                    }))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: tuple_mty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("tuple aggregate spread is unsupported")),
        "expected 'tuple aggregate spread is unsupported' error, got: {errors:?}"
    );
}

/// Program constructing `[0, spread source]` into an array of `numerus`,
/// where local 1 (the spread source) has type `source_ty`.
fn array_spread_program(array_ty: TypeId, source_ty: TypeId) -> MirProgram {
    let array_mty = MirType::semantic(array_ty);
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: array_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: MirType::semantic(source_ty),
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Array,
                            ty: array_mty,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(
                                    0,
                                ))),
                                MirAggregateItem::Spread(MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(1)),
                                    projections: vec![],
                                })),
                            ]),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: vec![],
                    }))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: array_mty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

#[test]
fn array_aggregate_accepts_spread_of_bounded_array() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let array_tid = types.array(numerus);
    let cap = types.intern_index(IndexExpr::Literal(2));
    let bounded_tid = types.bounded_array(numerus, cap);

    let program = array_spread_program(array_tid, bounded_tid);
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn array_aggregate_rejects_spread_of_mismatched_element() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let bivalens = types.primitive(Primitive::Bivalens);
    let array_tid = types.array(numerus);
    let cap = types.intern_index(IndexExpr::Literal(2));
    let bounded_tid = types.bounded_array(bivalens, cap);

    let program = array_spread_program(array_tid, bounded_tid);
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("aggregate spread type mismatch")),
        "expected 'aggregate spread type mismatch' error, got: {errors:?}"
    );
}

#[test]
fn tuple_aggregate_empty() {
    let mut types = TypeTable::new();
    // Empty tuple () is the unit type.
    let unit_tid = types.unit();
    let unit_mty = MirType::semantic(unit_tid);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: unit_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: unit_mty,
                            fields: MirAggregateFields::Ordered(vec![]),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "expected no errors for empty tuple, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// Stage 1B — tuple index projection
// -----------------------------------------------------------------------

#[test]
fn tuple_index_projection_valid() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let tuple_tid = types.tuple(vec![bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);
    let bivalens_mty = MirType::semantic(bivalens);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: tuple_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::Index(MirOperand::Constant(
                                    MirConstant::Int(0),
                                ))],
                            })),
                            ty: bivalens_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn tuple_index_projection_out_of_bounds() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let tuple_tid = types.tuple(vec![bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);
    let bivalens_mty = MirType::semantic(bivalens);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: tuple_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::Index(MirOperand::Constant(
                                    MirConstant::Int(5),
                                ))],
                            })),
                            // The out-of-bounds index means project_index returns None,
                            // so the value ty won't be checked against the projection result.
                            // Use a placeholder type since the value is never fully resolved.
                            ty: bivalens_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("out of bounds")),
        "expected 'out of bounds' error, got: {errors:?}"
    );
}

#[test]
fn tuple_index_projection_non_constant() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let numerus = types.primitive(Primitive::Numerus);
    let tuple_tid = types.tuple(vec![bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);
    let numerus_mty = MirType::semantic(numerus);

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: tuple_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: numerus_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::Index(
                                    // Non-constant index: a place operand
                                    MirOperand::Place(MirPlace {
                                        base: MirPlaceBase::Local(MirLocalId(1)),
                                        projections: vec![],
                                    }),
                                )],
                            })),
                            ty: numerus_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("compile-time constant index")),
        "expected 'compile-time constant index' error, got: {errors:?}"
    );
}

#[test]
fn tuple_index_projection_rejects_negative_index() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let tuple_tid = types.tuple(vec![bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);
    let bivalens_mty = MirType::semantic(bivalens);

    // Negative index -1 on 1-element tuple must be rejected.
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: tuple_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::Index(MirOperand::Constant(
                                    MirConstant::Int(-1),
                                ))],
                            })),
                            ty: bivalens_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("out of bounds")),
        "expected 'out of bounds' error for negative index, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// Stage 1 — CFG reachability + termination invariants (PRP-021)
// -----------------------------------------------------------------------

#[test]
fn rejects_function_with_no_blocks() {
    let types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![],
            temps: vec![],
            blocks: vec![], // empty — no entry block
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("no entry block")),
        "expected 'no entry block' error, got: {errors:?}"
    );
}

#[test]
fn rejects_orphan_block_not_reachable_from_entry() {
    let types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![],
            temps: vec![],
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                // Block 1 is dead code — no terminator references it, so
                // it is not flagged as unreachable (the lowerer creates
                // dead blocks during lowering).
                MirBlock {
                    id: MirBlockId(1),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Goto(MirBlockId(2)),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                // Block 2 is referenced by block 1 but not reachable from
                // entry block 0 — a genuine orphan.
                MirBlock {
                    id: MirBlockId(2),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("not reachable from entry")),
        "expected 'not reachable from entry' error, got: {errors:?}"
    );
}

#[test]
fn accepts_detached_conversion_recovery_region() {
    let types = TypeTable::new();
    let int_ty = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![],
            temps: vec![
                crate::MirTemp {
                    id: crate::MirTempId(0),
                    ty: int_ty,
                    span: Span::default(),
                },
                crate::MirTemp {
                    id: crate::MirTempId(1),
                    ty: int_ty,
                    span: Span::default(),
                },
            ],
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::temp(crate::MirTempId(0))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Convert(crate::MirConversion {
                                    store: None,
                                    flavor: crate::MirConversionFlavor::Runtime,
                                    source_ty: int_ty,
                                    target_ty: int_ty,
                                    params: vec![],
                                    recovery: Some(MirOperand::Temp(crate::MirTempId(1))),
                                    struct_defaults: vec![],
                                }),
                                args: vec![MirOperand::Constant(MirConstant::Int(42))],
                                return_ty: int_ty,
                            },
                        },
                        span: Span::default(),
                    }],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Goto(MirBlockId(2)),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::temp(crate::MirTempId(1)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(
                                    MirConstant::Int(7),
                                )),
                                ty: int_ty,
                                span: Span::default(),
                            },
                        },
                        span: Span::default(),
                    }],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Goto(MirBlockId(2)),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                MirBlock {
                    id: MirBlockId(2),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(Some(MirOperand::Temp(crate::MirTempId(
                            0,
                        )))),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
            ],
            return_ty: int_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };

    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "expected valid recovery region, got: {errors:?}"
    );
}

/// Build a runtime `convert` lowering a flat `lista<f64>` to
/// `tensor<f64, [2, 8]>`, with an optional `⊥` recovery operand.
fn lista_to_tensor_convert_program(recovery: Option<MirOperand>) -> (TypeTable, MirProgram) {
    let mut types = TypeTable::new();
    let f64_id = types.primitive(Primitive::Fractus);
    let lista_ty = MirType::semantic(types.array(f64_id));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let tensor_ty = MirType::semantic(tensor_type(&mut types, Primitive::Fractus, &[2, 8]));

    let mut temps = vec![
        crate::MirTemp {
            id: crate::MirTempId(0),
            ty: tensor_ty,
            span: Span::default(),
        },
        crate::MirTemp {
            id: crate::MirTempId(1),
            ty: lista_ty,
            span: Span::default(),
        },
    ];
    if recovery.is_some() {
        temps.push(crate::MirTemp {
            id: crate::MirTempId(2),
            ty: tensor_ty,
            span: Span::default(),
        });
    }

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![],
            temps,
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::temp(crate::MirTempId(0))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Convert(crate::MirConversion {
                                store: None,
                                flavor: crate::MirConversionFlavor::Runtime,
                                source_ty: lista_ty,
                                target_ty: tensor_ty,
                                params: vec![],
                                recovery,
                                struct_defaults: vec![],
                            }),
                            args: vec![MirOperand::Temp(crate::MirTempId(1))],
                            return_ty: tensor_ty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    (types, program)
}

#[test]
fn recovered_lista_to_tensor_conversio_validates() {
    let (types, program) =
        lista_to_tensor_convert_program(Some(MirOperand::Temp(crate::MirTempId(2))));
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "a recovered lista ↦ tensor conversio may ride the runtime convert — \
         tensor_from_flat has no failure channel, got: {errors:?}"
    );
}

#[test]
fn unrecovered_lista_to_tensor_conversio_rejected() {
    let (types, program) = lista_to_tensor_convert_program(None);
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("must lower to tensor_from_flat")),
        "expected the recovery-free lista ↦ tensor route rejection, got: {errors:?}"
    );
}

#[test]
fn rejects_function_with_only_goto_cycle() {
    let types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![],
            temps: vec![],
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Goto(MirBlockId(1)),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Goto(MirBlockId(0)),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not reach a terminator")),
        "expected 'does not reach a terminator' error, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// F-025 – unary/binary operand type checks
// -----------------------------------------------------------------------

/// Neg on a bool operand should be rejected — Neg requires numeric.
#[test]
fn unary_neg_on_bivalens_rejected() {
    let types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let bivalens_mty = MirType::semantic(bivalens);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Unary {
                                op: MirUnOp::Neg,
                                operand: MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                            },
                            ty: bivalens_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("not numeric")),
        "expected 'not numeric' error for Neg on bivalens, got: {errors:?}"
    );
}

/// Sqrt on a numerus operand should be rejected — float unary ops require fractus.
#[test]
fn unary_sqrt_on_numerus_rejected() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let numerus_mty = MirType::semantic(numerus);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: numerus_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: numerus_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Unary {
                                op: MirUnOp::Sqrt,
                                operand: MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                            },
                            ty: numerus_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("not fractus")),
        "expected 'not fractus' error for Sqrt on numerus, got: {errors:?}"
    );
}

/// Build a one-function program with a single unary statement:
/// `local1 <- unary(op, local0)`, where local0 has `operand_ty` and the
/// value/local1 have `result_ty`.
fn unary_op_program(
    types: &TypeTable,
    op: MirUnOp,
    operand_ty: MirType,
    result_ty: MirType,
) -> MirProgram {
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: operand_ty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: result_ty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Unary {
                                op,
                                operand: MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                            },
                            ty: result_ty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

/// R-W1 regression: float-unary ops on `tensor<f32,…>` must validate.
/// AIR lowers `AirTensorOp::Gelu/Tanh/Exp/Log/Relu/Sqrt` to `MirUnOp` on the
/// tensor operand (MLP fmir path); the F-035/F-036 scalar-only float check
/// rejected them as "not fractus".
#[test]
fn unary_float_ops_on_tensor_f32_accepted() {
    for op in [
        MirUnOp::Sqrt,
        MirUnOp::Relu,
        MirUnOp::Gelu,
        MirUnOp::Tanh,
        MirUnOp::Exp,
        MirUnOp::Log,
    ] {
        let mut types = TypeTable::new();
        let tensor_f32 = MirType::semantic(tensor_type(&mut types, Primitive::Fractus, &[4, 4]));
        let errors = validate(
            &unary_op_program(&types, op, tensor_f32, tensor_f32),
            &types,
        );
        assert!(
            errors.is_empty(),
            "expected no errors for {op:?} on tensor<f32,[4,4]>, got: {errors:?}"
        );
    }
}

/// R-W1 hardening-preservation: a tensor whose ELEMENT type is non-float is
/// still rejected — the F-035/F-036 intent extends to tensor element types.
#[test]
fn unary_sqrt_on_tensor_numerus_rejected() {
    let mut types = TypeTable::new();
    let tensor_numerus = MirType::semantic(tensor_type(&mut types, Primitive::Numerus, &[4, 4]));
    let errors = validate(
        &unary_op_program(&types, MirUnOp::Sqrt, tensor_numerus, tensor_numerus),
        &types,
    );
    assert!(
        errors.iter().any(|e| e.contains("not fractus")),
        "expected 'not fractus' error for Sqrt on tensor<numerus>, got: {errors:?}"
    );
}

/// Binary Add with a bivalens lhs should be rejected — arithmetic needs numeric.
#[test]
fn binary_add_non_numeric_operand_rejected() {
    let types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let bivalens_mty = MirType::semantic(bivalens);
    let numerus_mty = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: numerus_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Binary {
                                op: MirBinOp::Add,
                                lhs: MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                                rhs: MirOperand::Constant(MirConstant::Int(1)),
                            },
                            ty: numerus_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("not numeric")),
        "expected 'not numeric' error for Add with bivalens lhs, got: {errors:?}"
    );
}

/// Binary Eq should produce bivalens result, not numerus.
#[test]
fn binary_eq_result_not_bivalens_rejected() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let numerus_mty = MirType::semantic(numerus);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: numerus_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: numerus_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Binary {
                                op: MirBinOp::Eq,
                                lhs: MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                                rhs: MirOperand::Constant(MirConstant::Int(1)),
                            },
                            ty: numerus_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("not bivalens")),
        "expected 'not bivalens' error for Eq result, got: {errors:?}"
    );
}

/// Binary And with non-bivalens operands should be rejected.
#[test]
fn binary_and_non_bivalens_rejected() {
    let types = TypeTable::new();
    let _numerus_mty = MirType::semantic(types.primitive(Primitive::Numerus));
    let bivalens_mty = MirType::semantic(types.primitive(Primitive::Bivalens));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: bivalens_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Binary {
                                op: MirBinOp::And,
                                lhs: MirOperand::Constant(MirConstant::Int(1)),
                                rhs: MirOperand::Constant(MirConstant::Int(2)),
                            },
                            ty: bivalens_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("bivalens")),
        "expected bivalens operand error for And, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// F-026 – Symbol ID as tuple field index
// -----------------------------------------------------------------------

/// Field projection on a tuple uses Symbol.0 as raw index. This must be
/// rejected — tuples have no named fields.
#[test]
fn tuple_field_projection_rejected() {
    let mut types = TypeTable::new();
    let bivalens = types.primitive(Primitive::Bivalens);
    let tuple_tid = types.tuple(vec![bivalens, bivalens]);
    let tuple_mty = MirType::semantic(tuple_tid);
    let bivalens_mty = MirType::semantic(bivalens);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: tuple_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::Field(Symbol(0))],
                            })),
                            ty: bivalens_mty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("not a struct or record")),
        "expected field-projection-on-tuple rejection, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// F-027 – non-function Value callee unchecked
// -----------------------------------------------------------------------

/// Calling a Value whose type is not a function must produce an error.
#[test]
fn value_callee_not_callable_rejected() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let numerus_mty = MirType::semantic(numerus);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: numerus_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::Value(MirOperand::Place(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        })),
                        args: vec![],
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("not callable")),
        "expected 'not callable' error for Value callee, got: {errors:?}"
    );
}

/// The lowering shape for a stepped-cursor settle: the cursor-stream temp was
/// retyped to its array type (`mark_stepped_cursor_stream`) and is invoked
/// with no arguments as a `TryCall`. The cursor runtime carries the failable
/// contract, so no signature exists for the callee.
fn array_value_callee_fixture(types: &mut TypeTable, try_call: bool) -> MirProgram {
    let element = types.primitive(Primitive::Numerus);
    let array = MirType::semantic(types.array(element));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let textus = MirType::semantic(types.primitive(Primitive::Textus));
    let cursor = || MirCallee::Value(MirOperand::Temp(crate::MirTempId(0)));
    let mut blocks = vec![MirBlock {
        id: MirBlockId(0),
        statements: if try_call {
            Vec::new()
        } else {
            vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: cursor(),
                    args: vec![],
                },
                span: Span::default(),
            }]
        },
        terminator: MirTerminator {
            kind: if try_call {
                MirTerminatorKind::TryCall {
                    destination: Some(MirPlace::temp(crate::MirTempId(1))),
                    callee: cursor(),
                    args: vec![],
                    ok_block: MirBlockId(1),
                    error_place: MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: Vec::new(),
                    },
                    error_block: MirBlockId(2),
                }
            } else {
                MirTerminatorKind::Return(None)
            },
            span: Span::default(),
        },
        span: Span::default(),
    }];
    if try_call {
        for id in [MirBlockId(1), MirBlockId(2)] {
            blocks.push(MirBlock {
                id,
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            });
        }
    }
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: vacuum,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![
                crate::MirTemp {
                    id: crate::MirTempId(0),
                    ty: array,
                    span: Span::default(),
                },
                crate::MirTemp {
                    id: crate::MirTempId(1),
                    ty: vacuum,
                    span: Span::default(),
                },
            ],
            blocks,
            return_ty: vacuum,
            error_ty: Some(textus),
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

/// The array-typed cursor temp settles through `TryCall` without a signature;
/// the error place only has to be a valid place.
#[test]
fn try_call_on_array_value_callee_is_accepted_as_cursor_settle() {
    let mut types = TypeTable::new();
    let program = array_value_callee_fixture(&mut types, true);
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "expected cursor-settle TryCall acceptance, got: {errors:?}"
    );
}

/// Only the cursor-settle `TryCall` shape is exempt: a plain `Call` on the
/// same array value is still not callable.
#[test]
fn plain_call_on_array_value_callee_is_still_rejected() {
    let mut types = TypeTable::new();
    let program = array_value_callee_fixture(&mut types, false);
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("not callable")),
        "expected 'not callable' error for array Value callee, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// F-028 – gradient arity-only → add minimal type/handle checks
// -----------------------------------------------------------------------

/// Gradient Read with a non-pointer-type handle should be rejected.
#[test]
fn gradient_read_handle_not_opaque_rejected() {
    let types = TypeTable::new();
    let numerus_mty = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    // Gradient Read with a numerus handle (should be an opaque pointer).
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: numerus_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Gradient(MirGradientOp::Read),
                            args: vec![MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![],
                            })],
                            return_ty: numerus_mty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("gradient handle")),
        "expected gradient handle type error, got: {errors:?}"
    );
}

/// Gradient Create with non-numerus rank should be rejected.
#[test]
fn gradient_create_rank_not_numerus_rejected() {
    let types = TypeTable::new();
    let bivalens_mty = MirType::semantic(types.primitive(Primitive::Bivalens));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(2),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(3),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(4),
                    ty: bivalens_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Gradient(MirGradientOp::Create),
                            args: vec![
                                MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(0)),
                                    projections: vec![],
                                }),
                                MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(1)),
                                    projections: vec![],
                                }),
                                MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(2)),
                                    projections: vec![],
                                }),
                                MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(3)),
                                    projections: vec![],
                                }),
                                MirOperand::Place(MirPlace {
                                    base: MirPlaceBase::Local(MirLocalId(4)),
                                    projections: vec![],
                                }),
                            ],
                            return_ty: vacuum,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("gradient") && e.contains("rank")),
        "expected gradient rank type error, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// F-080 – switch cases untyped (empty / duplicate unchecked)
// -----------------------------------------------------------------------

/// Switch with an empty cases vector must be rejected.
#[test]
fn switch_empty_cases_rejected() {
    let types = TypeTable::new();
    let bivalens_mty = MirType::semantic(types.primitive(Primitive::Bivalens));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: bivalens_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Switch {
                            value: MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![],
                            }),
                            cases: vec![], // empty — must be rejected
                            default: MirBlockId(1),
                        },
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("switch") && e.contains("no cases")),
        "expected switch empty cases error, got: {errors:?}"
    );
}

/// Switch with duplicate case constant values must be rejected.
#[test]
fn switch_duplicate_cases_rejected() {
    let types = TypeTable::new();
    let numerus_mty = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: numerus_mty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Switch {
                            value: MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![],
                            }),
                            cases: vec![
                                MirSwitchCase {
                                    value: MirConstant::Int(1),
                                    target: MirBlockId(1),
                                },
                                MirSwitchCase {
                                    value: MirConstant::Int(1), // duplicate!
                                    target: MirBlockId(2),
                                },
                            ],
                            default: MirBlockId(3),
                        },
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                MirBlock {
                    id: MirBlockId(2),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
                MirBlock {
                    id: MirBlockId(3),
                    statements: vec![],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span: Span::default(),
                    },
                    span: Span::default(),
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("duplicate") && e.contains("switch case")),
        "expected duplicate switch case error, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// F-081 – cede on non-Promissum identity-accepts
// -----------------------------------------------------------------------

/// Cede on a non-Promissum type must be rejected.
#[test]
fn cede_non_promissum_rejected() {
    let types = TypeTable::new();
    let numerus_mty = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: numerus_mty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: numerus_mty,
                    name: None,
                    mutable: true,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(1)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Cede,
                            args: vec![MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![],
                            })],
                            return_ty: numerus_mty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cede") && e.contains("promissum")),
        "expected cede promissum error, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// CCI-1: TensorRmsNorm / TensorRope / TensorGather MIR validation
// -----------------------------------------------------------------------

/// A single collection-op program: one tensor-typed receiver local at
/// local 0, then the given call args (the receiver place is prepended).
fn cci_collection_program(
    types: &mut TypeTable,
    op: MirCollectionOp,
    args: Vec<MirOperand>,
    receiver_and_return_ty: MirType,
) -> MirProgram {
    let mut locals = vec![
        MirLocal {
            id: MirLocalId(0),
            ty: receiver_and_return_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        // Destination local for the non-vacuum collection result.
        MirLocal {
            id: MirLocalId(2),
            ty: receiver_and_return_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
    ];
    if matches!(
        op,
        MirCollectionOp::TensorRope | MirCollectionOp::TensorRopeHalf
    ) {
        locals.insert(
            1,
            MirLocal {
                id: MirLocalId(1),
                ty: receiver_and_return_ty,
                name: None,
                mutable: false,
                span: Span::default(),
            },
        );
    }
    let mut call_args = args;
    call_args.insert(
        0,
        MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(0)),
            projections: vec![],
        }),
    );
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals,
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(op),
                            args: call_args,
                            return_ty: receiver_and_return_ty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

fn tensor_mty(types: &mut TypeTable, dims: &[u64]) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32);
    let dim_ids: Vec<_> = dims
        .iter()
        .map(|&d| types.intern_index(IndexExpr::Literal(d)))
        .collect();
    let shape = if dim_ids.len() == 1 {
        dim_ids[0]
    } else {
        types.intern_index(IndexExpr::Tuple(dim_ids))
    };
    MirType::semantic(types.tensor_with_shape(f32_ty, shape))
}

fn array_mty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.array(types.primitive(Primitive::Numerus)))
}

/// A single unary scalar collection-op program: one scalar receiver local at
/// local 0 and a destination local typed with the call's result type (the
/// rounding ops map fractus-family receivers to plain `numerus`; the
/// absolute/signum ops return the receiver type unchanged).
fn unary_scalar_collection_program(
    types: &mut TypeTable,
    op: MirCollectionOp,
    receiver_ty: MirType,
    return_ty: MirType,
) -> MirProgram {
    let locals = vec![
        MirLocal {
            id: MirLocalId(0),
            ty: receiver_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        // Destination local for the non-vacuum rounding result.
        MirLocal {
            id: MirLocalId(2),
            ty: return_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
    ];
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals,
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(op),
                            args: vec![MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![],
                            })],
                            return_ty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

/// A three-operand scalar collection-op program for `approximata`: receiver,
/// comparison value, and tolerance all share the selected scalar type.
fn approximate_collection_program(
    types: &mut TypeTable,
    receiver_ty: MirType,
    argument_ty: MirType,
) -> MirProgram {
    let return_ty = MirType::semantic(types.primitive(Primitive::Bivalens));
    let locals = vec![
        MirLocal {
            id: MirLocalId(0),
            ty: receiver_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(1),
            ty: argument_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(2),
            ty: argument_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(3),
            ty: return_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
    ];
    let place = |id| {
        MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(id)),
            projections: vec![],
        })
    };
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals,
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(3)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::Approximate),
                            args: vec![place(0), place(1), place(2)],
                            return_ty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

#[test]
fn rms_norm_valid_rank2_consts_and_gamma() {
    let mut types = TypeTable::new();
    let gamma_ty = tensor_mty(&mut types, &[8]);
    let return_ty = tensor_mty(&mut types, &[2, 8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRmsNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
            MirOperand::Place(MirPlace {
                base: MirPlaceBase::Local(MirLocalId(1)),
                projections: vec![],
            }),
        ],
        return_ty,
    );
    let mut program = program;
    program.functions[0].locals.push(MirLocal {
        id: MirLocalId(1),
        ty: gamma_ty,
        name: None,
        mutable: false,
        span: Span::default(),
    });
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn rms_norm_rejects_out_of_range_axis_const() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[2, 8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRmsNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(5)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rms_norm") && e.contains("axis")),
        "expected rms_norm axis error, got: {errors:?}"
    );
}

#[test]
fn rope_rejects_odd_dim_const() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(3)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rope") && e.contains("even")),
        "expected rope parity error, got: {errors:?}"
    );
}

#[test]
fn rope_rejects_negative_pos_const() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(-1)),
            MirOperand::Constant(MirConstant::Int(2)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("tensor rope pos -1 must be non-negative")),
        "expected rope position bound error, got: {errors:?}"
    );
}

#[test]
fn rope_rejects_non_int_pos_const() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Float(1.0)),
            MirOperand::Constant(MirConstant::Int(2)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("tensor rope pos must be an int constant or temp")),
        "expected rope position constant-kind error, got: {errors:?}"
    );
}

#[test]
fn rope_accepts_symbolic_shape_size_dim() {
    // Generic templates carry `Param(d)` dims as symbolic ShapeSize; the
    // validator must admit them like a temp and wait for instantiate fold.
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let param_shape = types.intern_index(IndexExpr::Param(Symbol(900)));
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::ShapeSize {
                shape: param_shape,
                axis: None,
            }),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "symbolic rope dim must pass validation, got: {errors:?}"
    );
}

#[test]
fn rope_rejects_fractional_float_dim_const() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1.5)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rope dim must be an int constant or temp")),
        "expected rope dim constant-kind error, got: {errors:?}"
    );
}

#[test]
fn rope_accepts_whole_number_float_dim() {
    // Literal shape sizes fold to Float at lowering; whole-number dims
    // must clear the same checks as Int, not the constant-kind error.
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(4.0)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "whole-number float rope dim must pass validation, got: {errors:?}"
    );
}

#[test]
fn rope_accepts_uint_dim() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::UInt(4)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "uint rope dim must pass validation, got: {errors:?}"
    );
}

#[test]
fn rope_rejects_odd_whole_number_float_dim() {
    // Whole-number floats run the Int checks, so parity still fires.
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(3.0)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rope") && e.contains("even")),
        "expected rope parity error, got: {errors:?}"
    );
}

#[test]
fn rope_rejects_odd_uint_dim() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[8]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::UInt(3)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rope") && e.contains("even")),
        "expected rope parity error, got: {errors:?}"
    );
}

#[test]
fn rms_norm_rejects_non_tensor_receiver() {
    let mut types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRmsNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(0)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
            MirOperand::Constant(MirConstant::Int(1)),
        ],
        numerus,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rms_norm") && e.contains("not tensor")),
        "expected rms_norm non-tensor receiver error, got: {errors:?}"
    );
}

#[test]
fn rope_rejects_non_tensor_receiver() {
    let mut types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorRope,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(2)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
        numerus,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rope") && e.contains("not tensor")),
        "expected rope non-tensor receiver error, got: {errors:?}"
    );
}

/// K7c: the gather ids operand is a rank-1 `tensor<u32, [n]>`.
fn gather_ids_mty(
    types: &mut TypeTable,
    element: radix_types::NumericWidth,
    dims: &[u64],
) -> MirType {
    let element_ty = types.sized_numeric(Primitive::Numerus, element);
    let dim_ids: Vec<_> = dims
        .iter()
        .map(|&d| types.intern_index(IndexExpr::Literal(d)))
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.tensor_with_shape(element_ty, shape))
}

fn gather_program_with_ids(types: &mut TypeTable, ids_ty: MirType) -> MirProgram {
    let return_ty = tensor_mty(types, &[2, 3]);
    let mut program = cci_collection_program(
        types,
        MirCollectionOp::TensorGather,
        vec![MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(1)),
            projections: vec![],
        })],
        return_ty,
    );
    program.functions[0].locals.push(MirLocal {
        id: MirLocalId(1),
        ty: ids_ty,
        name: None,
        mutable: false,
        span: Span::default(),
    });
    program
}

#[test]
fn gather_valid_ids_tensor_operand() {
    let mut types = TypeTable::new();
    let ids = gather_ids_mty(&mut types, radix_types::NumericWidth::U32, &[2]);
    let program = gather_program_with_ids(&mut types, ids);
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn gather_rejects_a_lista_ids_operand() {
    let mut types = TypeTable::new();
    let ids = array_mty(&mut types);
    let program = gather_program_with_ids(&mut types, ids);
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("gather ids is not a tensor<u32, [n]>")),
        "expected the gather ids tensor error, got: {errors:?}"
    );
}

#[test]
fn gather_rejects_non_u32_or_non_rank_one_ids() {
    let mut types = TypeTable::new();
    let signed = gather_ids_mty(&mut types, radix_types::NumericWidth::I32, &[2]);
    let program = gather_program_with_ids(&mut types, signed);
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("gather ids is not a tensor<u32, [n]>")),
        "expected the gather ids element error, got: {errors:?}"
    );

    let mut types = TypeTable::new();
    let rank2 = gather_ids_mty(&mut types, radix_types::NumericWidth::U32, &[2, 1]);
    let program = gather_program_with_ids(&mut types, rank2);
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("gather ids must be a rank-1 tensor")),
        "expected the gather ids rank error, got: {errors:?}"
    );
}

// ── Rounding intrinsics: fractus-family receivers ──────────────────────
// The registry deliberately maps sized-fractus receivers (f32/f64) to the
// `Fractus` receiver slot for the rounding helpers; MIR validation must
// accept a `SizedNumeric(Fractus, _)` receiver (sibling to the
// Minimum/Maximum arm) while keeping the plain `numerus` result.

const ROUNDING_OPS: [MirCollectionOp; 4] = [
    MirCollectionOp::Round,
    MirCollectionOp::Floor,
    MirCollectionOp::Ceiling,
    MirCollectionOp::Truncate,
];

fn f32_receiver_mty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32))
}

fn numerus_mty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.primitive(Primitive::Numerus))
}

#[test]
fn rounding_f32_receiver_valid_for_all_rounding_ops() {
    for op in ROUNDING_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = f32_receiver_mty(&mut types);
        let return_ty = numerus_mty(&mut types);
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "expected a valid f32 receiver for {op:?}, got: {errors:?}"
        );
    }
}

#[test]
fn rounding_plain_fractus_receiver_valid_for_all_rounding_ops() {
    for op in ROUNDING_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = MirType::semantic(types.primitive(Primitive::Fractus));
        let return_ty = numerus_mty(&mut types);
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "expected a valid fractus receiver for {op:?}, got: {errors:?}"
        );
    }
}

#[test]
fn rounding_rejects_numerus_receiver() {
    for op in ROUNDING_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = numerus_mty(&mut types);
        let return_ty = numerus_mty(&mut types);
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("numeric rounding receiver is not fractus")),
            "expected the fractus-receiver diagnostic for {op:?}, got: {errors:?}"
        );
    }
}

// The frontend types a rounding expression by its destination (contextual
// slot typing), so an explicit signed slot carries a `SizedNumeric(Numerus, _)`
// result into MIR. The runtime handles that result, so validation must accept
// every signed width while still rejecting a `fractus` return.
#[test]
fn rounding_sized_signed_result_valid_for_all_rounding_ops() {
    for op in ROUNDING_OPS {
        for width in [
            radix_types::NumericWidth::I8,
            radix_types::NumericWidth::I16,
            radix_types::NumericWidth::I32,
            radix_types::NumericWidth::I64,
        ] {
            let mut types = TypeTable::new();
            let receiver_ty = f32_receiver_mty(&mut types);
            let return_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, width));
            let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
            let errors = validate(&program, &types);
            assert!(
                errors.is_empty(),
                "expected a valid sized signed result for {op:?} {width:?}, got: {errors:?}"
            );
        }
    }
}

#[test]
fn rounding_fractus_result_is_still_rejected() {
    for op in ROUNDING_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = f32_receiver_mty(&mut types);
        let return_ty = MirType::semantic(types.primitive(Primitive::Fractus));
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("numeric rounding result is not numerus")),
            "expected the numerus-result diagnostic for {op:?}, got: {errors:?}"
        );
    }
}

// ── Approximate comparison: fractus-family receivers ───────────────────

#[test]
fn approximate_f32_receiver_valid() {
    let mut types = TypeTable::new();
    let receiver_ty = f32_receiver_mty(&mut types);
    let program = approximate_collection_program(&mut types, receiver_ty, receiver_ty);
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "expected a valid f32 approximate receiver, got: {errors:?}"
    );
}

#[test]
fn approximate_plain_fractus_receiver_valid() {
    let mut types = TypeTable::new();
    let receiver_ty = fractus_mty(&mut types);
    let program = approximate_collection_program(&mut types, receiver_ty, receiver_ty);
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "expected a valid fractus approximate receiver, got: {errors:?}"
    );
}

#[test]
fn approximate_policy_float_receiver_valid() {
    // A `saturating`/`trapping` cell is the bare float of its width: width
    // limits apply only at the store, so the comparison receiver is valid.
    for policy in [
        radix_types::FloatPolicy::Saturating,
        radix_types::FloatPolicy::Trapping,
    ] {
        let mut types = TypeTable::new();
        let receiver_ty = MirType::semantic(
            types.intern(Type::PolicyFloat(policy, radix_types::NumericWidth::F32)),
        );
        let program = approximate_collection_program(&mut types, receiver_ty, receiver_ty);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "expected a valid {policy:?} approximate receiver, got: {errors:?}"
        );
    }
}

#[test]
fn approximate_rejects_numerus_receiver() {
    let mut types = TypeTable::new();
    let receiver_ty = numerus_mty(&mut types);
    let program = approximate_collection_program(&mut types, receiver_ty, receiver_ty);
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("approximate comparison receiver is not fractus")),
        "expected the fractus-receiver diagnostic, got: {errors:?}"
    );
}

// ── Absolute/Signum intrinsics: fractus-family receivers ───────────────
// `absolutum`/`signum` live on the registry's shared Numeric slot (plain
// `numerus` + `fractus` surface) with a `SameAsReceiver` return kind. The
// registry deliberately maps sized-fractus receivers (f32/f64) to the
// `Fractus` family; MIR validation must accept a `SizedNumeric(Fractus, _)`
// receiver (sibling to the Minimum/Maximum arm) while keeping the plain
// result checks. Sized-numerus receivers are accepted too.

const ABSOLUTE_SIGNUM_OPS: [MirCollectionOp; 2] =
    [MirCollectionOp::Absolute, MirCollectionOp::Signum];

fn fractus_mty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.primitive(Primitive::Fractus))
}

fn i32_receiver_mty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::I32))
}

#[test]
fn absolute_signum_f32_receiver_valid_for_all_ops() {
    for op in ABSOLUTE_SIGNUM_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = f32_receiver_mty(&mut types);
        // SameAsReceiver: the result type mirrors the receiver type.
        let return_ty = receiver_ty;
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "expected a valid f32 receiver for {op:?}, got: {errors:?}"
        );
    }
}

#[test]
fn absolute_signum_plain_fractus_receiver_valid_for_all_ops() {
    for op in ABSOLUTE_SIGNUM_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = fractus_mty(&mut types);
        let return_ty = receiver_ty;
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "expected a valid fractus receiver for {op:?}, got: {errors:?}"
        );
    }
}

#[test]
fn absolute_signum_plain_numerus_receiver_valid_for_all_ops() {
    // The corpus (`intrinseca/numerus-methodi.fab`) exercises int
    // `absolutum`/`signum`; the plain `numerus` surface must keep validating.
    for op in ABSOLUTE_SIGNUM_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = numerus_mty(&mut types);
        let return_ty = receiver_ty;
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "expected a valid numerus receiver for {op:?}, got: {errors:?}"
        );
    }
}

#[test]
fn absolute_signum_sized_numerus_receiver_valid_for_all_ops() {
    // A sized integer receiver is admitted (the runner used to crash on it).
    for op in ABSOLUTE_SIGNUM_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = i32_receiver_mty(&mut types);
        let return_ty = receiver_ty;
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "expected a valid sized-numerus receiver for {op:?}, got: {errors:?}"
        );
    }
}

#[test]
fn absolute_of_sized_numerus_takes_a_range_typed_integer_result() {
    // `|int<i32>|` is typed by the range rule, so the result width may differ
    // from the receiver's; a non-integer result is still rejected.
    let mut types = TypeTable::new();
    let receiver_ty = i32_receiver_mty(&mut types);
    let u32_ty =
        MirType::semantic(types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::U32));
    let program =
        unary_scalar_collection_program(&mut types, MirCollectionOp::Absolute, receiver_ty, u32_ty);
    assert!(validate(&program, &types).is_empty());

    let fractus_ty = fractus_mty(&mut types);
    let program = unary_scalar_collection_program(
        &mut types,
        MirCollectionOp::Absolute,
        receiver_ty,
        fractus_ty,
    );
    assert!(
        validate(&program, &types)
            .iter()
            .any(|e| e.contains("numeric intrinsic result type mismatch"))
    );
}

// ── ScalarMath (A2-LIFT G01): float-only scalar math functions ─────────

#[test]
fn scalar_math_float_receivers_are_valid_for_every_function() {
    for function in radix_types::MirMathFunction::SCALAR {
        let op = MirCollectionOp::ScalarMath(function);
        let mut types = TypeTable::new();
        for receiver_ty in [f32_receiver_mty(&mut types), fractus_mty(&mut types)] {
            let program = unary_scalar_collection_program(&mut types, op, receiver_ty, receiver_ty);
            let errors = validate(&program, &types);
            assert!(errors.is_empty(), "{op:?}: {errors:?}");
        }
    }
}

#[test]
fn scalar_math_rejects_an_integer_receiver() {
    for function in radix_types::MirMathFunction::SCALAR {
        let op = MirCollectionOp::ScalarMath(function);
        let mut types = TypeTable::new();
        for receiver_ty in [numerus_mty(&mut types), i32_receiver_mty(&mut types)] {
            let program = unary_scalar_collection_program(&mut types, op, receiver_ty, receiver_ty);
            assert!(
                validate(&program, &types)
                    .iter()
                    .any(|e| e.contains("scalar math receiver is not fractus")),
                "{op:?}"
            );
        }
    }
}

#[test]
fn scalar_math_result_must_be_the_receiver_type() {
    let mut types = TypeTable::new();
    let receiver_ty = f32_receiver_mty(&mut types);
    let wider = fractus_mty(&mut types);
    let program = unary_scalar_collection_program(
        &mut types,
        MirCollectionOp::ScalarMath(radix_types::MirMathFunction::Sqrt),
        receiver_ty,
        wider,
    );
    assert!(
        validate(&program, &types)
            .iter()
            .any(|e| e.contains("scalar math result type mismatch"))
    );
}

// -----------------------------------------------------------------------
// GI3-1: TensorCausalMaskedSoftmax MIR validation (the causal masked-row
// softmax op the CausalMaskedSoftmax recipe resolves from).
// -----------------------------------------------------------------------

#[test]
fn causal_masked_softmax_valid_rank2_receiver() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[2, 2]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorCausalMaskedSoftmax,
        vec![],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "expected no errors for a valid causal masked-softmax call, got: {errors:?}"
    );
}

#[test]
fn causal_masked_softmax_rejects_wrong_arity() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[2, 2]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorCausalMaskedSoftmax,
        vec![MirOperand::Constant(MirConstant::Int(1))],
        return_ty,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("TensorCausalMaskedSoftmax") && e.contains("expects 1")),
        "expected the arity diagnostic for a 2-arg causal masked-softmax call, got: {errors:?}"
    );
}

// ── DDCP1-U3: access-mode axis — role/access pairs fail closed ───────────

#[test]
fn access_mode_axis_rejects_incoherent_role_access_pairs() {
    // Contract §Resource axes rule 2: access mode is declared per
    // resource/launch (read, write, read-write) and validated independently.
    // An access outside the declared mode fails closed at the ABI→program
    // role mapping: writing an input role or reading an output role has no
    // coherent program role and yields `None`.
    use crate::abi::{MirKernelResourceAccess, MirKernelResourceRole};
    use crate::device_program::BufferRole;

    assert_eq!(
        BufferRole::from_abi_role(MirKernelResourceRole::Input, MirKernelResourceAccess::Read),
        Some(BufferRole::Input)
    );
    assert_eq!(
        BufferRole::from_abi_role(
            MirKernelResourceRole::Output,
            MirKernelResourceAccess::Write
        ),
        Some(BufferRole::Output)
    );
    assert_eq!(
        BufferRole::from_abi_role(
            MirKernelResourceRole::Input,
            MirKernelResourceAccess::ReadWrite
        ),
        Some(BufferRole::InOut)
    );
    // Incoherent pairs: writing an input, reading an output — an access
    // outside the declared mode, rejected (no silent role collapse).
    assert_eq!(
        BufferRole::from_abi_role(MirKernelResourceRole::Input, MirKernelResourceAccess::Write),
        None
    );
    assert_eq!(
        BufferRole::from_abi_role(MirKernelResourceRole::Output, MirKernelResourceAccess::Read),
        None
    );
}

// -----------------------------------------------------------------------
// Named aggregate fitting-int (Family B / triga Material.side)
// -----------------------------------------------------------------------

/// Struct `{ int<u32> side }` constructed with a single named field.
fn named_int_u32_field_program(
    types: &mut TypeTable,
    field_value: MirOperand,
) -> (MirProgram, DefId, Symbol) {
    let def_id = DefId(1);
    let field = Symbol(0);
    let _int_u32 = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let struct_ty = MirType::semantic(types.intern(Type::Struct(def_id)));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: struct_ty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Struct(def_id),
                            ty: struct_ty,
                            fields: MirAggregateFields::Named(vec![MirNamedOperand {
                                name: field,
                                value: field_value,
                            }]),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    (program, def_id, field)
}

fn named_int_u32_context(
    types: &TypeTable,
    def_id: DefId,
    field: Symbol,
) -> MirValidationContext<'_> {
    let int_u32 = types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
        .expect("u32 interned");
    let mut context = MirValidationContext::new(types);
    let mut fields = FxHashMap::default();
    fields.insert(field, MirType::semantic(int_u32));
    context.struct_fields.insert(def_id, fields);
    context
}

#[test]
fn named_aggregate_accepts_fitting_int_literal() {
    // HIR already accepts `Material { side = 0 }` for `int<u32> side`.
    // MIR constants carry plain `numerus`; collections honor the fitting-int
    // exception, named aggregates must too.
    let mut types = TypeTable::new();
    let (program, def_id, field) =
        named_int_u32_field_program(&mut types, MirOperand::Constant(MirConstant::Int(0)));
    let context = named_int_u32_context(&types, def_id, field);
    let errors = validate_with(&program, &context);
    assert!(
        errors.is_empty(),
        "expected fitting numerus literal into u32 named field, got: {errors:?}"
    );
}

#[test]
fn named_aggregate_rejects_out_of_range_int_literal() {
    let mut types = TypeTable::new();
    let (program, def_id, field) = named_int_u32_field_program(
        &mut types,
        MirOperand::Constant(MirConstant::Int(i64::from(u32::MAX) + 1)),
    );
    let context = named_int_u32_context(&types, def_id, field);
    let errors = validate_with(&program, &context);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("named aggregate field type mismatch")),
        "expected out-of-range literal still rejected, got: {errors:?}"
    );
}

#[test]
fn named_aggregate_rejects_non_integer_field_mismatch() {
    let mut types = TypeTable::new();
    let (program, def_id, field) =
        named_int_u32_field_program(&mut types, MirOperand::Constant(MirConstant::Bool(false)));
    let context = named_int_u32_context(&types, def_id, field);
    let errors = validate_with(&program, &context);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("named aggregate field type mismatch")),
        "expected bool into u32 field still rejected, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// MLC-1: octeti<N> carrier validation row
// -----------------------------------------------------------------------

/// Minimal valid program with one local of `octeti<N>` — the bounded byte
/// carrier is a validation leaf and must validate cleanly.
#[test]
fn bounded_octeti_local_validates_as_leaf_carrier() {
    let mut types = TypeTable::new();
    let capacity = types.intern_index(IndexExpr::Literal(8));
    let octeti8 = MirType::semantic(types.intern(Type::BoundedOcteti { capacity }));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: octeti8,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

// -----------------------------------------------------------------------
// MMG-D1 — surface 4, first boundary: run-depth shape witness red
// -----------------------------------------------------------------------

/// Build the smallest direct-MIR call that matches the CTP-3 run-depth red:
/// six concrete `[4, 4]` tensors are passed to a generic `[N, N]` signature,
/// and the concrete destination is checked against the same symbolic return.
fn mlp_shape_mismatch_program(types: &mut TypeTable) -> MirProgram {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let n = types.intern_index(IndexExpr::Param(Symbol(900)));
    let symbolic_shape = types.intern_index(IndexExpr::Tuple(vec![n, n]));
    let expected = MirType::semantic(types.tensor_with_shape(element, symbolic_shape));
    let four = types.intern_index(IndexExpr::Literal(4));
    let actual_shape = types.intern_index(IndexExpr::Tuple(vec![four, four]));
    let actual = MirType::semantic(types.tensor_with_shape(element, actual_shape));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    // `callee`/`caller` are the standard domain pair; the similarity is deliberate.
    #[allow(clippy::similar_names)]
    let callee = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: (0..6)
            .map(|index| MirParam {
                local: MirLocalId(index),
                name: None,
                ty: expected,
                mode: MirParamMode::Owned,
                span: Span::default(),
            })
            .collect(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Unreachable,
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: expected,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };

    #[allow(clippy::similar_names)]
    let caller = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: (0..7)
            .map(|index| MirLocal {
                id: MirLocalId(index),
                name: None,
                ty: actual,
                mutable: false,
                span: Span::default(),
            })
            .collect(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: Some(MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(6)),
                        projections: Vec::new(),
                    }),
                    callee: MirCallee::Function(MirFunctionId(1)),
                    args: (0..6)
                        .map(|index| {
                            MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(index)),
                                projections: Vec::new(),
                            })
                        })
                        .collect(),
                },
                span: Span::default(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };

    MirProgram {
        functions: vec![caller, callee],
    }
}

fn mlp_shape_try_call_program(types: &mut TypeTable) -> MirProgram {
    let mut program = mlp_shape_mismatch_program(types);
    let statement = program.functions[0].blocks[0]
        .statements
        .pop()
        .expect("shape fixture has a call statement");
    let MirStatementKind::Call {
        destination,
        callee,
        args,
    } = statement.kind
    else {
        panic!("shape fixture statement is not a call");
    };
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    program.functions[1].error_ty = Some(vacuum);
    program.functions[0].locals.push(MirLocal {
        id: MirLocalId(7),
        ty: vacuum,
        name: None,
        mutable: false,
        span: Span::default(),
    });
    program.functions[0].blocks[0].terminator.kind = MirTerminatorKind::TryCall {
        destination,
        callee,
        args,
        ok_block: MirBlockId(1),
        error_place: MirPlace {
            base: MirPlaceBase::Local(MirLocalId(7)),
            projections: Vec::new(),
        },
        error_block: MirBlockId(2),
    };
    for id in [MirBlockId(1), MirBlockId(2)] {
        program.functions[0].blocks.push(MirBlock {
            id,
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        });
    }
    program
}

#[test]
fn mlp_shape_witness_binds_through_ordinary_call_args_and_destination() {
    let mut types = TypeTable::new();
    let program = mlp_shape_mismatch_program(&mut types);
    let table_size = (types.type_count(), types.index_count());
    let messages = validate(&program, &types);
    assert!(
        messages.is_empty(),
        "expected shape witness binding, got: {messages:?}"
    );
    assert_eq!(
        (types.type_count(), types.index_count()),
        table_size,
        "validation must not intern shape types or indices"
    );
}

#[test]
fn mlp_shape_witness_binds_through_try_call_args_and_destination() {
    let mut types = TypeTable::new();
    let messages = validate(&mlp_shape_try_call_program(&mut types), &types);
    assert!(
        messages.is_empty(),
        "expected shape witness binding, got: {messages:?}"
    );
}

#[test]
fn mlp_shape_wrong_shape_call_preserves_argument_mismatch() {
    let mut types = TypeTable::new();
    let mut program = mlp_shape_mismatch_program(&mut types);
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let eight = types.intern_index(IndexExpr::Literal(8));
    let wrong_shape = types.intern_index(IndexExpr::Tuple(vec![eight, eight]));
    let wrong = MirType::semantic(types.tensor_with_shape(element, wrong_shape));
    program.functions[1]
        .params
        .iter_mut()
        .for_each(|param| param.ty = wrong);
    program.functions[1].return_ty = wrong;

    let messages = validate(&program, &types);
    let expected = (0..6)
        .map(|index| {
            format!(
                "call argument type mismatch (arg {index}: expected tensor<f32, [8, 8]>, got tensor<f32, [4, 4]>)"
            )
        })
        .chain(std::iter::once("destination type mismatch".to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(messages, expected);
}

#[test]
fn mlp_shape_destination_mismatch_preserves_structured_issue() {
    let mut types = TypeTable::new();
    let mut program = mlp_shape_mismatch_program(&mut types);
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let eight = types.intern_index(IndexExpr::Literal(8));
    let wrong_shape = types.intern_index(IndexExpr::Tuple(vec![eight, eight]));
    program.functions[0].locals[6].ty =
        MirType::semantic(types.tensor_with_shape(element, wrong_shape));

    let messages = validate(&program, &types);
    assert_eq!(
        messages,
        vec!["destination type mismatch", "destination type mismatch"]
    );
}

fn runtime_shape_call_program(
    types: &mut TypeTable,
    guard: bool,
    relation_axis: u32,
    second_static_extent: Option<u64>,
    try_call: bool,
) -> MirProgram {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let formal_extent =
        types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
            parameter: 0,
            axis: 0,
        }));
    let actual_first = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(
        DefId(700),
    )));
    let actual_second = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(
        DefId(701),
    )));
    let second_extent = types.intern_index(IndexExpr::Literal(second_static_extent.unwrap_or(2)));
    let static_two = types.intern_index(IndexExpr::Literal(2));
    let shape = |types: &mut TypeTable, first, second| {
        let shape = types.intern_index(IndexExpr::Tuple(vec![first, second]));
        MirType::semantic(types.tensor_with_shape(element, shape))
    };
    let formal = shape(types, formal_extent, static_two);
    let first = shape(types, actual_first, static_two);
    let second = shape(types, actual_second, second_extent);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let text = MirType::semantic(types.primitive(Primitive::Textus));
    let args = vec![
        MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(0)),
            projections: Vec::new(),
        }),
        MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(1)),
            projections: Vec::new(),
        }),
    ];
    let mut statements = Vec::new();
    if guard {
        let carrier = crate::MirShapeExpr::TensorAxis {
            argument: 0,
            axis: 0,
        };
        let repeated = crate::MirShapeExpr::TensorAxis {
            argument: 1,
            axis: relation_axis,
        };
        statements.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::TensorShapeCheck(crate::MirShapeCheck {
                        relations: vec![crate::MirShapeRelation {
                            left: carrier.clone(),
                            right: repeated,
                        }],
                        size_bindings: vec![crate::MirShapeBinding {
                            parameter: 0,
                            witness: carrier,
                        }],
                    }),
                    args: args.clone(),
                    return_ty: vacuum,
                },
            },
            span: Span::default(),
        });
    }
    if !try_call {
        statements.push(MirStatement {
            kind: MirStatementKind::Call {
                destination: None,
                callee: MirCallee::Function(MirFunctionId(1)),
                args: args.clone(),
            },
            span: Span::default(),
        });
    }
    let caller_terminator = if try_call {
        MirTerminatorKind::TryCall {
            destination: None,
            callee: MirCallee::Function(MirFunctionId(1)),
            args,
            ok_block: MirBlockId(1),
            error_place: MirPlace {
                base: MirPlaceBase::Local(MirLocalId(2)),
                projections: Vec::new(),
            },
            error_block: MirBlockId(2),
        }
    } else {
        MirTerminatorKind::Return(None)
    };
    let mut caller_blocks = vec![MirBlock {
        id: MirBlockId(0),
        statements,
        terminator: MirTerminator {
            kind: caller_terminator,
            span: Span::default(),
        },
        span: Span::default(),
    }];
    let mut caller_locals = vec![
        MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: first,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(1),
            name: None,
            ty: second,
            mutable: false,
            span: Span::default(),
        },
    ];
    if try_call {
        caller_locals.push(MirLocal {
            id: MirLocalId(2),
            name: None,
            ty: text,
            mutable: false,
            span: Span::default(),
        });
        for id in [MirBlockId(1), MirBlockId(2)] {
            caller_blocks.push(MirBlock {
                id,
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            });
        }
    }
    let caller = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: caller_locals,
        temps: Vec::new(),
        blocks: caller_blocks,
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let callee = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: formal,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: formal,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: vacuum,
        error_ty: try_call.then_some(text),
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    MirProgram {
        functions: vec![caller, callee],
    }
}

#[test]
fn runtime_shape_check_authorizes_only_adjacent_matching_call_axes() {
    let mut types = TypeTable::new();
    let program = runtime_shape_call_program(&mut types, true, 0, None, false);
    assert!(validate(&program, &types).is_empty());

    let mut types = TypeTable::new();
    let program = runtime_shape_call_program(&mut types, true, 0, None, true);
    assert!(
        validate(&program, &types).is_empty(),
        "TryCall consumes its immediately preceding shape check"
    );

    let mut types = TypeTable::new();
    let program = runtime_shape_call_program(&mut types, false, 0, None, false);
    assert!(
        validate(&program, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch"))
    );

    let mut types = TypeTable::new();
    let program = runtime_shape_call_program(&mut types, true, 1, None, false);
    assert!(
        validate(&program, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch"))
    );

    let mut types = TypeTable::new();
    let program = runtime_shape_call_program(&mut types, true, 0, Some(3), false);
    assert!(
        validate(&program, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch"))
    );

    let mut types = TypeTable::new();
    let mut program = runtime_shape_call_program(&mut types, true, 0, None, false);
    let MirStatementKind::RuntimeCall { call, .. } =
        &mut program.functions[0].blocks[0].statements[0].kind
    else {
        unreachable!();
    };
    call.args.swap(0, 1);
    assert!(
        validate(&program, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch")),
        "a guard for different operands cannot authorize this call"
    );

    let mut types = TypeTable::new();
    let mut program = runtime_shape_call_program(&mut types, true, 0, None, false);
    let block = &mut program.functions[0].blocks[0];
    block.statements.push(block.statements[1].clone());
    assert!(
        validate(&program, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch")),
        "the first call consumes adjacency; a later call needs its own guard"
    );
}

#[test]
fn runtime_affine_call_and_return_require_the_matching_derived_witness() {
    let mut types = TypeTable::new();
    let mut program = runtime_shape_call_program(&mut types, true, 0, None, false);
    let first = program.functions[0].locals[0].ty;
    let Type::Tensor(element, shape) = types.get(first.semantic_id()).clone() else {
        unreachable!()
    };
    let IndexExpr::Tuple(mut dims) = types.get_index(shape).clone() else {
        unreachable!()
    };
    dims[0] = types.intern_index(IndexExpr::Affine {
        base: dims[0],
        offset: -1,
    });
    let shape = types.intern_index(IndexExpr::Tuple(dims));
    let affine = MirType::semantic(types.tensor_with_shape(element, shape));
    program.functions[0].locals[0].ty = affine;
    program.functions[0].locals.push(MirLocal {
        id: MirLocalId(2),
        name: None,
        ty: affine,
        mutable: false,
        span: Span::default(),
    });
    let destination = MirPlace {
        base: MirPlaceBase::Local(MirLocalId(2)),
        projections: Vec::new(),
    };
    let MirStatementKind::Call {
        destination: call_destination,
        ..
    } = &mut program.functions[0].blocks[0].statements[1].kind
    else {
        unreachable!()
    };
    *call_destination = Some(destination);
    let formal = program.functions[1].params[0].ty;
    program.functions[1].return_ty = formal;
    program.functions[1].blocks[0].terminator.kind =
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(0)),
            projections: Vec::new(),
        })));
    let MirStatementKind::RuntimeCall { call, .. } =
        &mut program.functions[0].blocks[0].statements[0].kind
    else {
        unreachable!()
    };
    call.args.push(MirOperand::Constant(MirConstant::Int(4)));
    let MirIntrinsic::TensorShapeCheck(check) = &mut call.intrinsic else {
        unreachable!()
    };
    let witness = crate::MirShapeExpr::ScalarArgument { argument: 2 };
    check.size_bindings[0].witness = witness.clone();
    check.relations.push(crate::MirShapeRelation {
        left: crate::MirShapeExpr::TensorAxis {
            argument: 0,
            axis: 0,
        },
        right: crate::MirShapeExpr::Affine {
            base: Box::new(witness),
            offset: -1,
        },
    });
    assert!(validate(&program, &types).is_empty());

    let mut unguarded = program.clone();
    unguarded.functions[0].blocks[0].statements.remove(0);
    assert!(
        validate(&unguarded, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch"))
    );

    let mut wrong_destination = program.clone();
    wrong_destination.functions[0].locals[2].ty = wrong_destination.functions[0].locals[1].ty;
    assert!(
        validate(&wrong_destination, &types)
            .iter()
            .any(|message| message.contains("destination type mismatch"))
    );

    let mut wrong_witness = program;
    let MirStatementKind::RuntimeCall { call, .. } =
        &mut wrong_witness.functions[0].blocks[0].statements[0].kind
    else {
        unreachable!()
    };
    call.args.push(MirOperand::Constant(MirConstant::Int(5)));
    let MirIntrinsic::TensorShapeCheck(check) = &mut call.intrinsic else {
        unreachable!()
    };
    check.size_bindings[0].witness = crate::MirShapeExpr::ScalarArgument { argument: 3 };
    assert!(
        validate(&wrong_witness, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch"))
    );
}

// -----------------------------------------------------------------------
// Call destination with an expression-origin runtime axis
// -----------------------------------------------------------------------

/// Caller `f0` stores the result of `f1(x: tensor<f32, [3]>) -> tensor<f32, [3]>`
/// (a monomorphized affine instance) into a local of `destination_axis`.
fn expression_axis_destination_program(
    types: &mut TypeTable,
    destination_axis: IndexExpr,
) -> MirProgram {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let three = types.intern_index(IndexExpr::Literal(3));
    let destination_axis = types.intern_index(destination_axis);
    let concrete = MirType::semantic(types.tensor_with_shape(element, three));
    let destination = MirType::semantic(types.tensor_with_shape(element, destination_axis));
    let place = |local: u32| MirPlace {
        base: MirPlaceBase::Local(MirLocalId(local)),
        projections: Vec::new(),
    };
    let local = |id: u32, ty| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: false,
        span: Span::default(),
    };
    let block = |statements| MirBlock {
        id: MirBlockId(0),
        statements,
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    };
    let function = |id: u32, params, locals, statements, return_ty| MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params,
        locals,
        temps: Vec::new(),
        blocks: vec![block(statements)],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let caller = function(
        0,
        Vec::new(),
        vec![local(0, concrete), local(1, destination)],
        vec![MirStatement {
            kind: MirStatementKind::Call {
                destination: Some(place(1)),
                callee: MirCallee::Function(MirFunctionId(1)),
                args: vec![MirOperand::Place(place(0))],
            },
            span: Span::default(),
        }],
        vacuum,
    );
    let mut callee = function(
        1,
        vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: concrete,
            mode: MirParamMode::Owned,
            span: Span::default(),
        }],
        vec![local(0, concrete)],
        Vec::new(),
        concrete,
    );
    callee.blocks[0].terminator.kind = MirTerminatorKind::Return(Some(MirOperand::Place(place(0))));
    MirProgram {
        functions: vec![caller, callee],
    }
}

#[test]
fn call_destination_with_expression_axis_accepts_a_concrete_callee_extent() {
    let mut types = TypeTable::new();
    let program = expression_axis_destination_program(
        &mut types,
        IndexExpr::Runtime(RuntimeExtentId::ExpressionAxis {
            expression: 21,
            axis: 0,
        }),
    );
    assert_eq!(validate(&program, &types), Vec::<String>::new());
}

#[test]
fn call_destination_with_other_runtime_or_static_axis_stays_a_mismatch() {
    for axis in [
        IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(700))),
        IndexExpr::Literal(2),
    ] {
        let mut types = TypeTable::new();
        let program = expression_axis_destination_program(&mut types, axis);
        assert_eq!(
            validate(&program, &types),
            vec!["destination type mismatch"]
        );
    }
}

// -----------------------------------------------------------------------
// N7E-3 – policy-float cells are numeric float types in MIR
// -----------------------------------------------------------------------

/// `trapping<W>` and `saturating<W>` slots validate: a MIR type that is a cell
/// is accepted, arithmetic and negation over it are numeric, and a float
/// constant stands for a cell value (semantic analysis already checked it).
#[test]
fn policy_float_cell_arithmetic_validates() {
    for policy in [
        radix_types::FloatPolicy::Trapping,
        radix_types::FloatPolicy::Saturating,
    ] {
        let mut types = TypeTable::new();
        let cell = types.intern(Type::PolicyFloat(policy, NumericWidth::F32));
        let cell_mty = MirType::semantic(cell);
        let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
        let local = |id: u32| MirLocal {
            id: MirLocalId(id),
            ty: cell_mty,
            name: None,
            mutable: true,
            span: Span::default(),
        };
        let place = |id: u32| MirPlace {
            base: MirPlaceBase::Local(MirLocalId(id)),
            projections: vec![],
        };
        let assign = |value_id: u32, target: u32, kind: MirValueKind| MirStatement {
            kind: MirStatementKind::Assign {
                place: place(target),
                value: MirValue {
                    id: MirValueId(value_id),
                    kind,
                    ty: cell_mty,
                    span: Span::default(),
                },
            },
            span: Span::default(),
        };
        let program = MirProgram {
            functions: vec![MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: vec![],
                locals: vec![local(0), local(1)],
                temps: vec![],
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![
                        assign(
                            0,
                            0,
                            MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(1.5))),
                        ),
                        assign(
                            1,
                            1,
                            MirValueKind::Binary {
                                op: MirBinOp::Mul,
                                lhs: MirOperand::Place(place(0)),
                                rhs: MirOperand::Place(place(0)),
                            },
                        ),
                        assign(
                            2,
                            1,
                            MirValueKind::Unary {
                                op: MirUnOp::Neg,
                                operand: MirOperand::Place(place(1)),
                            },
                        ),
                    ],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span: Span::default(),
                    },
                    span: Span::default(),
                }],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: Span::default(),
            }],
        };
        let errors = validate(&program, &types);
        assert!(errors.is_empty(), "{policy:?}: {errors:?}");
    }
}

// -----------------------------------------------------------------------
// `⇇` copy-into: TensorCopyInto(dest, src) -> vacuum
// -----------------------------------------------------------------------

/// One function with a destination local (0) and a source local (1) and a
/// single `TensorCopyInto(dest, src)` call returning `return_ty`.
fn copy_into_program(
    dest_ty: MirType,
    src_ty: MirType,
    dest: MirOperand,
    return_ty: MirType,
    function_return: MirType,
) -> MirProgram {
    let local = |id: u32, ty: MirType| MirLocal {
        id: MirLocalId(id),
        ty,
        name: None,
        mutable: true,
        span: Span::default(),
    };
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![local(0, dest_ty), local(1, src_ty)],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: None,
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCopyInto),
                            args: vec![dest, MirOperand::Place(MirPlace::local(MirLocalId(1)))],
                            return_ty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: function_return,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

fn dest_place() -> MirOperand {
    MirOperand::Place(MirPlace::local(MirLocalId(0)))
}

#[test]
fn copy_into_accepts_a_tensor_place_and_an_equal_shape_tensor() {
    let mut types = TypeTable::new();
    let dest = tensor_mty(&mut types, &[2, 3]);
    let src = tensor_mty(&mut types, &[2, 3]);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let errors = validate(
        &copy_into_program(dest, src, dest_place(), vacuum, vacuum),
        &types,
    );
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn copy_into_rejects_a_shape_mismatch() {
    let mut types = TypeTable::new();
    let dest = tensor_mty(&mut types, &[2, 3]);
    let src = tensor_mty(&mut types, &[3, 2]);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let errors = validate(
        &copy_into_program(dest, src, dest_place(), vacuum, vacuum),
        &types,
    );
    assert!(
        errors.iter().any(|e| e.contains("copy-into shapes differ")),
        "expected a shape error, got: {errors:?}"
    );
}

#[test]
fn copy_into_rejects_an_element_type_mismatch() {
    let mut types = TypeTable::new();
    let dest = tensor_mty(&mut types, &[2, 3]);
    let src = MirType::semantic(tensor_type(&mut types, Primitive::Numerus, &[2, 3]));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let errors = validate(
        &copy_into_program(dest, src, dest_place(), vacuum, vacuum),
        &types,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("copy-into element types differ")),
        "expected an element-type error, got: {errors:?}"
    );
}

#[test]
fn copy_into_rejects_a_non_tensor_destination_or_source() {
    let mut types = TypeTable::new();
    let tensor = tensor_mty(&mut types, &[2, 3]);
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let errors = validate(
        &copy_into_program(numerus, tensor, dest_place(), vacuum, vacuum),
        &types,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("copy-into destination is not tensor")),
        "expected a destination error, got: {errors:?}"
    );
    let errors = validate(
        &copy_into_program(tensor, numerus, dest_place(), vacuum, vacuum),
        &types,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("copy-into source is not tensor")),
        "expected a source error, got: {errors:?}"
    );
}

#[test]
fn copy_into_rejects_a_destination_that_is_not_a_place() {
    let mut types = TypeTable::new();
    let tensor = tensor_mty(&mut types, &[2, 3]);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let program = copy_into_program(
        tensor,
        tensor,
        MirOperand::Constant(MirConstant::Int(0)),
        vacuum,
        vacuum,
    );
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("copy-into destination is not a place")),
        "expected a place error, got: {errors:?}"
    );
}

#[test]
fn copy_into_rejects_a_non_vacuum_result() {
    let mut types = TypeTable::new();
    let tensor = tensor_mty(&mut types, &[2, 3]);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let errors = validate(
        &copy_into_program(tensor, tensor, dest_place(), tensor, vacuum),
        &types,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("copy-into result is not vacuum")),
        "expected a result error, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// Device tensor view: one integer index into row-major storage of any rank
// -----------------------------------------------------------------------

/// One function reading `tensor[index]` where `index` is a numerus local.
fn device_view_linear_read_program(types: &mut TypeTable, dims: &[u64]) -> MirProgram {
    let tensor_ty = MirType::semantic(tensor_type(types, Primitive::Fractus, dims));
    let element_ty = MirType::semantic(types.primitive(Primitive::Fractus));
    device_view_linear_read_program_typed(types, tensor_ty, element_ty)
}

/// Same program with an explicit tensor type and the read's result type.
fn device_view_linear_read_program_typed(
    types: &mut TypeTable,
    tensor_ty: MirType,
    element_ty: MirType,
) -> MirProgram {
    let index_ty = MirType::semantic(types.primitive(Primitive::Numerus));
    let local = |id: u32, ty: MirType| MirLocal {
        id: MirLocalId(id),
        ty,
        name: None,
        mutable: false,
        span: Span::default(),
    };
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                local(0, tensor_ty),
                local(1, index_ty),
                local(2, element_ty),
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(2)),
                            projections: vec![],
                        },
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::Index(MirOperand::Place(
                                    MirPlace {
                                        base: MirPlaceBase::Local(MirLocalId(1)),
                                        projections: vec![],
                                    },
                                ))],
                            })),
                            ty: element_ty,
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

#[test]
fn device_view_linear_index_validates_for_rank2_and_rank3_storage() {
    for dims in [&[4u64, 6][..], &[2, 3, 5][..]] {
        let mut types = TypeTable::new();
        let program = device_view_linear_read_program(&mut types, dims);
        let errors = validate(&program, &types);
        assert!(
            errors.is_empty(),
            "rank {} linear device index must validate: {errors:?}",
            dims.len()
        );
    }
}

/// `tensor<element, [4, 6]>` and its element type id.
fn tensor_of_element(types: &mut TypeTable, element: TypeId) -> MirType {
    let d0 = types.intern_index(IndexExpr::Literal(4));
    let d1 = types.intern_index(IndexExpr::Literal(6));
    let shape = types.intern_index(IndexExpr::Tuple(vec![d0, d1]));
    MirType::semantic(types.tensor_with_shape(element, shape))
}

#[test]
fn device_view_index_on_vector_element_view_yields_the_vector_layout() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Literal(4));
    let vector = types.intern(Type::Vector(f32_ty, width));
    let tensor_ty = tensor_of_element(&mut types, vector);
    let program =
        device_view_linear_read_program_typed(&mut types, tensor_ty, MirType::semantic(vector));
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "vector-element view index must validate: {errors:?}"
    );
}

#[test]
fn device_view_index_on_opaque_tensor_runtime_is_still_rejected() {
    let mut types = TypeTable::new();
    let f64_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let tuple = types.intern(Type::Tuple(vec![f64_ty]));
    let tensor_ty = tensor_of_element(&mut types, tuple);
    let program =
        device_view_linear_read_program_typed(&mut types, tensor_ty, MirType::semantic(tuple));
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("tensor runtime aggregate has no device-view projection contract")),
        "expected the opaque-tensor rejection, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// TensorExtent
// -----------------------------------------------------------------------

/// One-function program: `local1 = TensorExtent { local0, axis }` where
/// local 0 has `operand_ty` and local 1 has `result_ty`.
fn tensor_extent_program(
    types: &TypeTable,
    operand_ty: MirType,
    axis: u32,
    result_ty: MirType,
) -> MirProgram {
    let mut program = unary_op_program(types, MirUnOp::Neg, operand_ty, result_ty);
    let MirStatementKind::Assign { value, .. } =
        &mut program.functions[0].blocks[0].statements[0].kind
    else {
        unreachable!("fixture statement is an assign");
    };
    value.kind = MirValueKind::TensorExtent {
        tensor: MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(0)),
            projections: vec![],
        }),
        axis,
    };
    program
}

#[test]
fn tensor_extent_valid_axis_passes() {
    let mut types = TypeTable::new();
    let tensor = MirType::semantic(tensor_type(&mut types, Primitive::Fractus, &[4, 6]));
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    for axis in 0..2 {
        let errors = validate(
            &tensor_extent_program(&types, tensor, axis, numerus),
            &types,
        );
        assert!(errors.is_empty(), "axis {axis}: {errors:?}");
    }
}

#[test]
fn tensor_extent_runtime_shaped_tensor_passes() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Fractus);
    let runtime = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarExpression {
        expression: 3,
    }));
    let tensor = MirType::semantic(types.tensor_with_shape(element, runtime));
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let errors = validate(&tensor_extent_program(&types, tensor, 0, numerus), &types);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn tensor_extent_axis_at_or_past_rank_is_rejected() {
    let mut types = TypeTable::new();
    let tensor = MirType::semantic(tensor_type(&mut types, Primitive::Fractus, &[4, 6]));
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let errors = validate(&tensor_extent_program(&types, tensor, 2, numerus), &types);
    assert!(
        errors.iter().any(|e| e.contains("out of range for rank 2")),
        "expected an axis range error, got: {errors:?}"
    );
}

#[test]
fn tensor_extent_non_tensor_operand_is_rejected() {
    let types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let errors = validate(&tensor_extent_program(&types, numerus, 0, numerus), &types);
    assert!(
        errors.iter().any(|e| e.contains("operand is not a tensor")),
        "expected a non-tensor operand error, got: {errors:?}"
    );
}

#[test]
fn tensor_extent_non_numerus_result_is_rejected() {
    let mut types = TypeTable::new();
    let tensor = MirType::semantic(tensor_type(&mut types, Primitive::Fractus, &[4]));
    let fractus = MirType::semantic(types.primitive(Primitive::Fractus));
    let errors = validate(&tensor_extent_program(&types, tensor, 0, fractus), &types);
    assert!(
        errors.iter().any(|e| e.contains("result is not numerus")),
        "expected a result type error, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// Applied (instantiated generic class) receiver for a Struct `self`
// -----------------------------------------------------------------------

/// `caller` holds a value of type `receiver` and calls `callee(self: Struct(def))`.
fn applied_receiver_call_program(types: &mut TypeTable, base_def: DefId) -> MirProgram {
    let class_def = DefId(1);
    let struct_ty = types.intern(Type::Struct(class_def));
    let base = types.intern(Type::Struct(base_def));
    let size = types.intern_index(IndexExpr::Literal(3));
    let applied = types.intern(Type::Applied(base, vec![AppliedArg::Size(size)]));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let block = |statements| MirBlock {
        id: MirBlockId(0),
        statements,
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    };
    let caller = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: MirType::semantic(applied),
            mutable: false,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: vec![block(vec![MirStatement {
            kind: MirStatementKind::Call {
                destination: None,
                callee: MirCallee::Function(MirFunctionId(1)),
                args: vec![MirOperand::Place(MirPlace {
                    base: MirPlaceBase::Local(MirLocalId(0)),
                    projections: Vec::new(),
                })],
            },
            span: Span::default(),
        }])],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let callee = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: MirType::semantic(struct_ty),
            mode: MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![block(Vec::new())],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    MirProgram {
        functions: vec![caller, callee],
    }
}

#[test]
fn applied_receiver_of_the_same_class_satisfies_a_struct_self_parameter() {
    let mut types = TypeTable::new();
    let program = applied_receiver_call_program(&mut types, DefId(1));
    assert!(validate(&program, &types).is_empty());

    let mut types = TypeTable::new();
    let program = applied_receiver_call_program(&mut types, DefId(2));
    assert!(
        validate(&program, &types)
            .iter()
            .any(|message| message.contains("call argument type mismatch")),
        "an Applied receiver of a different class is still a mismatch"
    );
}

// -----------------------------------------------------------------------
// Interval aggregate window step
// -----------------------------------------------------------------------

/// One function constructing an interval of `interval_ty` from `items`.
fn interval_program_with_ty(
    types: &mut TypeTable,
    interval_ty: MirType,
    items: Vec<MirAggregateItem>,
) -> MirProgram {
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: interval_ty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Interval { inclusive: false },
                            ty: interval_ty,
                            fields: MirAggregateFields::Ordered(items),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

/// One function constructing `intervallum<numerus>` from `items`.
fn interval_program(types: &mut TypeTable, items: Vec<MirAggregateItem>) -> MirProgram {
    let numerus = types.primitive(Primitive::Numerus);
    let interval_ty = MirType::semantic(types.intervallum(numerus));
    interval_program_with_ty(types, interval_ty, items)
}

fn interval_items(step: Option<MirAggregateItem>) -> Vec<MirAggregateItem> {
    let mut items = vec![
        MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(0))),
        MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(6))),
    ];
    items.extend(step);
    items
}

#[test]
fn interval_aggregate_accepts_numeric_step() {
    let mut types = TypeTable::new();
    let step = MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(2)));
    let program = interval_program(&mut types, interval_items(Some(step)));
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn interval_aggregate_rejects_undefined_step_operand() {
    let mut types = TypeTable::new();
    let step = MirAggregateItem::Operand(MirOperand::Value(MirValueId(9)));
    let program = interval_program(&mut types, interval_items(Some(step)));
    let errors = validate(&program, &types);
    assert!(
        errors.iter().any(|e| e.contains("value v9 is not defined")),
        "expected the step operand to be validated, got: {errors:?}"
    );
}

#[test]
fn interval_aggregate_rejects_non_numeric_step() {
    let mut types = TypeTable::new();
    let step = MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Bool(true)));
    let program = interval_program(&mut types, interval_items(Some(step)));
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("interval aggregate step must be numeric")),
        "expected non-numeric step rejected, got: {errors:?}"
    );
}

#[test]
fn interval_aggregate_accepts_stepped_type_and_third_field() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let step_index = types.intern_index(IndexExpr::Literal(2));
    let interval_ty = MirType::semantic(types.intervallum_with_step(numerus, step_index));
    let step = MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(2)));
    let program = interval_program_with_ty(&mut types, interval_ty, interval_items(Some(step)));
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn interval_aggregate_rejects_spread_step() {
    let mut types = TypeTable::new();
    let step = MirAggregateItem::Spread(MirOperand::Constant(MirConstant::Int(2)));
    let program = interval_program(&mut types, interval_items(Some(step)));
    let errors = validate(&program, &types);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("interval aggregate step cannot spread")),
        "expected spread step rejected, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// Enum variant positional payload aggregate
// -----------------------------------------------------------------------

/// One function constructing variant `variant` of `enum_def` from positional
/// `items`.
fn ordered_variant_program(
    types: &mut TypeTable,
    enum_def: DefId,
    variant: DefId,
    items: Vec<MirAggregateItem>,
) -> MirProgram {
    let enum_ty = MirType::semantic(types.intern(Type::Enum(enum_def)));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: enum_ty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        },
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::EnumVariant(variant),
                            ty: enum_ty,
                            fields: MirAggregateFields::Ordered(items),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

/// Validation context registering `variant` under `enum_def` with payload
/// fields in declaration order.
fn ordered_variant_context(
    types: &TypeTable,
    enum_def: DefId,
    variant: DefId,
    fields: Vec<(Symbol, MirType)>,
) -> MirValidationContext<'_> {
    let mut context = MirValidationContext::new(types);
    context.variant_parents.insert(variant, enum_def);
    context.enum_variants.insert(enum_def, vec![variant]);
    let mut field_map = FxHashMap::default();
    let mut order = Vec::new();
    for (name, ty) in fields {
        field_map.insert(name, ty);
        order.push(name);
    }
    context.variant_fields.insert(variant, field_map);
    context.variant_field_order.insert(variant, order);
    context
}

fn numerus_and_bivalens(types: &mut TypeTable) -> (MirType, MirType) {
    (
        MirType::semantic(types.primitive(Primitive::Numerus)),
        MirType::semantic(types.primitive(Primitive::Bivalens)),
    )
}

#[test]
fn ordered_variant_aggregate_accepts_declared_payload() {
    let mut types = TypeTable::new();
    let (numerus, bivalens) = numerus_and_bivalens(&mut types);
    let enum_def = DefId(1);
    let variant = DefId(2);
    let program = ordered_variant_program(
        &mut types,
        enum_def,
        variant,
        vec![
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(7))),
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Bool(true))),
        ],
    );
    let context = ordered_variant_context(
        &types,
        enum_def,
        variant,
        vec![(Symbol(0), numerus), (Symbol(1), bivalens)],
    );
    let errors = validate_with(&program, &context);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn ordered_variant_aggregate_rejects_missing_payload_operand() {
    let mut types = TypeTable::new();
    let (numerus, bivalens) = numerus_and_bivalens(&mut types);
    let enum_def = DefId(1);
    let variant = DefId(2);
    let program = ordered_variant_program(
        &mut types,
        enum_def,
        variant,
        vec![MirAggregateItem::Operand(MirOperand::Constant(
            MirConstant::Int(7),
        ))],
    );
    let context = ordered_variant_context(
        &types,
        enum_def,
        variant,
        vec![(Symbol(0), numerus), (Symbol(1), bivalens)],
    );
    let errors = validate_with(&program, &context);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not match variant payload arity")),
        "expected the missing payload operand rejected, got: {errors:?}"
    );
}

#[test]
fn ordered_variant_aggregate_rejects_extra_payload_operand() {
    let mut types = TypeTable::new();
    let (numerus, bivalens) = numerus_and_bivalens(&mut types);
    let enum_def = DefId(1);
    let variant = DefId(2);
    let program = ordered_variant_program(
        &mut types,
        enum_def,
        variant,
        vec![
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(7))),
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Bool(true))),
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(9))),
        ],
    );
    let context = ordered_variant_context(
        &types,
        enum_def,
        variant,
        vec![(Symbol(0), numerus), (Symbol(1), bivalens)],
    );
    let errors = validate_with(&program, &context);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not match variant payload arity")),
        "expected the extra payload operand rejected, got: {errors:?}"
    );
}

#[test]
fn ordered_variant_aggregate_rejects_payload_type_mismatch() {
    let mut types = TypeTable::new();
    let (numerus, _) = numerus_and_bivalens(&mut types);
    let enum_def = DefId(1);
    let variant = DefId(2);
    let program = ordered_variant_program(
        &mut types,
        enum_def,
        variant,
        vec![MirAggregateItem::Operand(MirOperand::Constant(
            MirConstant::Bool(true),
        ))],
    );
    let context = ordered_variant_context(&types, enum_def, variant, vec![(Symbol(0), numerus)]);
    let errors = validate_with(&program, &context);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("enum variant aggregate element type mismatch")),
        "expected the payload type mismatch rejected, got: {errors:?}"
    );
}

#[test]
fn ordered_variant_aggregate_rejects_payload_spread() {
    let mut types = TypeTable::new();
    let (numerus, _) = numerus_and_bivalens(&mut types);
    let enum_def = DefId(1);
    let variant = DefId(2);
    let program = ordered_variant_program(
        &mut types,
        enum_def,
        variant,
        vec![MirAggregateItem::Spread(MirOperand::Constant(
            MirConstant::Int(7),
        ))],
    );
    let context = ordered_variant_context(&types, enum_def, variant, vec![(Symbol(0), numerus)]);
    let errors = validate_with(&program, &context);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("enum variant aggregate spread is unsupported")),
        "expected the payload spread rejected, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// Context rehoming keeps contract conformance
// -----------------------------------------------------------------------

#[test]
fn with_types_carries_struct_contracts() {
    let types = TypeTable::new();
    let mut context = MirValidationContext::new(&types);
    let mut contracts = rustc_hash::FxHashSet::default();
    contracts.insert(DefId(2));
    context.struct_contracts.insert(DefId(3), contracts);

    let rehomed_types = types.clone();
    let rehomed = context.with_types(&rehomed_types);

    assert_eq!(rehomed.struct_contracts, context.struct_contracts);
}

// -----------------------------------------------------------------------
// Ruling 2026-10-15 – matrix receivers on the tensor coordinate ops
// -----------------------------------------------------------------------

/// A `matrix<f32, [2, 2]>` receiver on `TensorGet` (return `Option<f32>`) and
/// on `TensorSet` (return `vacuum`) validates: the coordinate read/write is
/// admitted on the matrix path, not only on tensors.
#[test]
fn matrix_receiver_coordinate_get_and_set_validate() {
    let mut types = TypeTable::new();
    let f32_element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let rows = types.intern_index(IndexExpr::Literal(2));
    let columns = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![rows, columns]));
    let matrix = MirType::semantic(types.matrix_with_shape(f32_element, shape));
    let index = MirType::semantic(types.array(types.primitive(Primitive::Numerus)));
    let optional = MirType::semantic(types.option(f32_element));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));

    let receiver = MirOperand::Place(MirPlace::local(MirLocalId(0)));
    let coordinates = MirOperand::Place(MirPlace::local(MirLocalId(1)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    name: None,
                    ty: matrix,
                    mutable: true,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    name: None,
                    ty: index,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![crate::MirTemp {
                id: crate::MirTempId(0),
                ty: optional,
                span: Span::default(),
            }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::temp(crate::MirTempId(0))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGet),
                                args: vec![receiver.clone(), coordinates.clone()],
                                return_ty: optional,
                            },
                        },
                        span: Span::default(),
                    },
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: None,
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSet),
                                args: vec![
                                    receiver,
                                    coordinates,
                                    MirOperand::Constant(MirConstant::Float(1.0)),
                                ],
                                return_ty: vacuum,
                            },
                        },
                        span: Span::default(),
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "a matrix receiver must admit the tensor coordinate read/write, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// Optional-chain call links
// -----------------------------------------------------------------------

/// Build a one-function program whose block assigns an optional chain call to
/// a result local: `result ← base?.call(args)`.
fn option_chain_call_program(
    types: &mut TypeTable,
    base_ty: MirType,
    result_ty: MirType,
    callee: MirCallee,
    args: Vec<MirOperand>,
) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![
                MirLocal {
                    id: MirLocalId(0),
                    ty: base_ty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
                MirLocal {
                    id: MirLocalId(1),
                    ty: result_ty,
                    name: None,
                    mutable: false,
                    span: Span::default(),
                },
            ],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(MirLocalId(1)),
                        value: MirValue {
                            id: MirValueId(0),
                            ty: result_ty,
                            kind: MirValueKind::Option(MirOptionOp::Chain {
                                base: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                link: MirOptionChainLink::Call { callee, args },
                            }),
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

/// `?.` on an `(int) → int` value must unwrap the option before resolving the
/// callee's signature; the old path looked the signature up on `Option<Func>`
/// and reported "value callee is not callable".
#[test]
fn option_chain_call_value_callee_unwraps_option() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let fn_ty = types.intern(Type::Func(FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: int,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: int,
        err: None,
        is_async: false,
        is_generator: false,
    }));
    let option_fn = MirType::semantic(types.option(fn_ty));
    let option_int = MirType::semantic(types.option(int));
    let program = option_chain_call_program(
        &mut types,
        option_fn,
        option_int,
        MirCallee::Value(MirOperand::Place(MirPlace::local(MirLocalId(0)))),
        vec![MirOperand::Constant(MirConstant::Int(5))],
    );
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "an optional function value must be callable through ?., got: {errors:?}"
    );
}

/// A method called through `?.` gets its receiver from the chain base, so the
/// self-first MIR signature must drop the receiver before argument checking.
#[test]
fn option_chain_call_method_callee_skips_receiver_param() {
    let mut types = TypeTable::new();
    let string = types.primitive(Primitive::Textus);
    let int = types.primitive(Primitive::Numerus);
    let option_string = MirType::semantic(types.option(string));
    let option_int = MirType::semantic(types.option(int));
    let program = option_chain_call_program(
        &mut types,
        option_string,
        option_int,
        MirCallee::definition(DefId(7)),
        vec![MirOperand::Constant(MirConstant::Int(5))],
    );
    let mut context = MirValidationContext::new(&types);
    context.functions.insert(
        DefId(7),
        MirFunctionSignature {
            params: vec![
                MirParamSignature {
                    ty: MirType::semantic(string),
                    optional: false,
                },
                MirParamSignature {
                    ty: MirType::semantic(int),
                    optional: false,
                },
            ],
            return_ty: MirType::semantic(int),
            error_ty: None,
        },
    );
    context.method_defs.insert(DefId(7));
    let errors = validate_with(&program, &context);
    assert!(
        errors.is_empty(),
        "a method called through ?. must not count the chain base as a receiver argument, got: {errors:?}"
    );
}

/// The receiver exemption must not swallow arity checking: with no arguments
/// the remaining `int` parameter is still required.
#[test]
fn option_chain_call_method_callee_arity_still_checked() {
    let mut types = TypeTable::new();
    let string = types.primitive(Primitive::Textus);
    let int = types.primitive(Primitive::Numerus);
    let option_string = MirType::semantic(types.option(string));
    let option_int = MirType::semantic(types.option(int));
    let program = option_chain_call_program(
        &mut types,
        option_string,
        option_int,
        MirCallee::definition(DefId(7)),
        vec![],
    );
    let mut context = MirValidationContext::new(&types);
    context.functions.insert(
        DefId(7),
        MirFunctionSignature {
            params: vec![
                MirParamSignature {
                    ty: MirType::semantic(string),
                    optional: false,
                },
                MirParamSignature {
                    ty: MirType::semantic(int),
                    optional: false,
                },
            ],
            return_ty: MirType::semantic(int),
            error_ty: None,
        },
    );
    context.method_defs.insert(DefId(7));
    let errors = validate_with(&program, &context);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("call argument count mismatch")),
        "a method call through ?. must still require its non-receiver arguments, got: {errors:?}"
    );
}

#[test]
fn dependent_parameter_axes_use_exact_actual_argument_witnesses() {
    for try_call in [false, true] {
        let mut types = TypeTable::new();
        let mut program = runtime_shape_call_program(&mut types, false, 0, None, try_call);
        assert!(
            validate(&program, &types)
                .iter()
                .any(|message| message.contains("call argument type mismatch"))
        );
        program.functions[0].locals[1].ty = program.functions[0].locals[0].ty;
        assert!(
            validate(&program, &types).is_empty(),
            "equal actual axes prove the dependent parameter without a runtime guard"
        );
    }
}

/// Build a one-function program whose block holds a single `TensorSliceNd`
/// runtime call: `[receiver, starts, ends, steps, keeps, inclusives]`.
fn tensor_slice_nd_program(
    types: &mut TypeTable,
    int_list_len: u64,
    keeps_list_len: u64,
    inclusives_list_len: u64,
    arity: usize,
) -> MirProgram {
    let receiver_ty = MirType::semantic(tensor_type(types, Primitive::Fractus, &[2, 3]));
    let numerus = types.primitive(Primitive::Numerus);
    let starts_capacity = types.intern_index(IndexExpr::Literal(int_list_len));
    let int_list_ty = MirType::semantic(types.bounded_array(numerus, starts_capacity));
    let bivalens = types.primitive(Primitive::Bivalens);
    let keeps_capacity = types.intern_index(IndexExpr::Literal(keeps_list_len));
    let keeps_ty = MirType::semantic(types.bounded_array(bivalens, keeps_capacity));
    let inclusives_capacity = types.intern_index(IndexExpr::Literal(inclusives_list_len));
    let inclusives_ty = MirType::semantic(types.bounded_array(bivalens, inclusives_capacity));
    let locals = vec![
        MirLocal {
            id: MirLocalId(0),
            ty: receiver_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(1),
            ty: int_list_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(2),
            ty: keeps_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(3),
            ty: inclusives_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
        MirLocal {
            id: MirLocalId(4),
            ty: receiver_ty,
            name: None,
            mutable: false,
            span: Span::default(),
        },
    ];

    let mut args = vec![local_operand(0)];
    for index in 1..arity {
        args.push(local_operand(match index {
            1..=3 => 1,
            4 => 2,
            _ => 3,
        }));
    }

    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals,
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(4)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSliceNd),
                            args,
                            return_ty: receiver_ty,
                        },
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

/// Operand reading a program local by index.
fn local_operand(id: u32) -> MirOperand {
    MirOperand::Place(MirPlace {
        base: MirPlaceBase::Local(MirLocalId(id)),
        projections: vec![],
    })
}

/// `TensorSliceNd` with one entry per axis on all five lists is valid.
#[test]
fn tensor_slice_nd_validates_six_arg_form() {
    let mut types = TypeTable::new();
    let program = tensor_slice_nd_program(&mut types, 2, 2, 2, 6);
    let errors = validate(&program, &types);
    assert!(
        errors.is_empty(),
        "six in-range axis lists must validate, got: {errors:?}"
    );
}

/// Both the operand count and the shared axis length are enforced.
#[test]
fn tensor_slice_nd_rejects_arity_and_length_mismatch() {
    let mut types = TypeTable::new();
    let program = tensor_slice_nd_program(&mut types, 2, 2, 2, 5);
    let errors = validate_structured(&program, &types);
    assert_eq!(
        errors.len(),
        1,
        "five operands must report exactly one arity error, got: {errors:?}"
    );
    assert_eq!(
        errors[0].issue, "mir_validation_collection_tensorslicend_expects_6_mir_arguments",
        "unexpected arity diagnostic: {errors:?}"
    );

    let mut types = TypeTable::new();
    let program = tensor_slice_nd_program(&mut types, 2, 3, 2, 6);
    let errors = validate_structured(&program, &types);
    assert_eq!(
        errors.len(),
        1,
        "a keeps list longer than the starts list must report exactly one length error, got: {errors:?}"
    );
    assert_eq!(
        errors[0].issue, "mir_validation_tensor_slice_nd_lists_do_not_share_one_axis_length",
        "unexpected length diagnostic: {errors:?}"
    );
}
