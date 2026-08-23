use super::*;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirCallee, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirGradientOp,
    MirIntrinsic, MirLocal, MirLocalId, MirNamedOperand, MirOperand, MirPlace, MirPlaceBase,
    MirProgram, MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirSwitchCase,
    MirTerminator, MirTerminatorKind, MirType, MirUnOp, MirValue, MirValueId, MirValueKind,
};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};
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
    let locals = vec![
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

#[test]
fn gather_valid_ids_array_operand() {
    let mut types = TypeTable::new();
    let return_ty = tensor_mty(&mut types, &[2, 3]);
    let program = cci_collection_program(
        &mut types,
        MirCollectionOp::TensorGather,
        vec![MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(1)),
            projections: vec![],
        })],
        return_ty,
    );
    let mut program = program;
    program.functions[0].locals.push(MirLocal {
        id: MirLocalId(1),
        ty: array_mty(&mut types),
        name: None,
        mutable: false,
        span: Span::default(),
    });
    let errors = validate(&program, &types);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
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
// result checks. The corpus (`intrinseca/numerus-methodi.fab`) relies on the
// plain `numerus` surface, so it stays accepted; sized-numerus receivers
// (`i32`) stay rejected.

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
fn absolute_signum_rejects_sized_numerus_receiver() {
    for op in ABSOLUTE_SIGNUM_OPS {
        let mut types = TypeTable::new();
        let receiver_ty = i32_receiver_mty(&mut types);
        let return_ty = receiver_ty;
        let program = unary_scalar_collection_program(&mut types, op, receiver_ty, return_ty);
        let errors = validate(&program, &types);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("numeric intrinsic receiver is not numerus or fractus")),
            "expected the numerus-or-fractus receiver diagnostic for {op:?}, got: {errors:?}"
        );
    }
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

fn named_int_u32_context<'a>(
    types: &'a TypeTable,
    def_id: DefId,
    field: Symbol,
) -> MirValidationContext<'a> {
    let int_u32 = types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
        .expect("int<u32> interned");
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
        "expected fitting numerus literal into int<u32> named field, got: {errors:?}"
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
        "expected bool into int<u32> field still rejected, got: {errors:?}"
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
