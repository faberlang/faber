//! The S3-A1 multi-output contract surface: the decomposition's result-tuple
//! TEMP aggregation is admitted (the ABI flattens the fields into distinct
//! output resources), while a genuine tuple temp outside the contract stays
//! fail-closed.

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
    MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId, MirOperand,
    MirParam, MirParamMode, MirPlace, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp,
    MirTempId, MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
};
use radix_types::TypeTable;

/// The D-6 decomposed subchain synthetic shape: a multi-output kernel
/// whose return-tuple fields are the subchain's output locals, with the
/// result TUPLE aggregated into a TEMP (`Return(Some(Operand::Temp(..)))`)
/// — exactly what `needs_synthetic_tuple_return` builds
/// (`kernel_decomposition.rs`). The tuple temp is the S3-A1 multi-output
/// contract — the ABI flattens its elements to DISTINCT output resources
/// — not a device aggregate value; the gate must admit it like the
/// result-tuple local (`visit_local` already has the allowance).
fn decomposed_multi_output_train_step(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let tuple = MirType::semantic(types.intern(Type::Tuple(vec![r2.semantic_id(); 2])));
    let span = test_span();
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: r2,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: r2,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: r2,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: r2,
                mutable: false,
                span,
            },
        ],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: tuple,
            span,
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                // t0 = x @ w, t1 = x @ w — the live tensor bodies.
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(2))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            ],
                            return_ty: r2,
                        },
                    },
                    span,
                },
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(3))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            ],
                            return_ty: r2,
                        },
                    },
                    span,
                },
                // result ← construct tuple(t0, t1) INTO THE TEMP — the
                // decomposed synthetic's result aggregation; the
                // multi-output ABI flattens the elements to distinct
                // output resources (the S3-A1 contract).
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: tuple,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(2),
                                ))),
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(3),
                                ))),
                            ]),
                        },
                    },
                    span,
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0)))),
                span,
            },
            span,
        }],
        return_ty: tuple,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// A genuine tuple-typed temp OUTSIDE a multi-output contract: the
/// function returns a single tensor, yet a tuple aggregate is built into
/// a temp. No `multi_output_result` allowance applies — the tuple temp
/// must stay fail-closed with the aggregate diagnostic.
fn single_output_tuple_temp_function(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let tuple = tuple_of_one_ty(types, r2);
    let span = test_span();
    MirFunction {
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
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: r2,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: r2,
                mutable: false,
                span,
            },
        ],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: tuple,
            span,
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Construct {
                    destination: MirPlace::temp(MirTempId(0)),
                    aggregate: MirAggregate {
                        kind: MirAggregateKind::Tuple,
                        ty: tuple,
                        fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        )]),
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span,
            },
            span,
        }],
        return_ty: r2,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn tuple_temp_multi_output_contract_passes_target_entry_gate() {
    // S3-A1 (U7): the D-6 decomposed subchain synthetics aggregate their
    // output tuple into a TEMP (`Return(Some(Operand::Temp(..)))`). The
    // temp is the multi-output contract — the ABI flattens the tuple
    // elements into DISTINCT output resources — so the gate must admit
    // it exactly like the result-tuple local. Pre-fix this body failed
    // with `device aggregate value type Tuple([...])` (the CUDA artifact
    // diagnostic for the MLP).
    let mut types = TypeTable::new();
    let function = decomposed_multi_output_train_step(&mut types);
    let program = crate::MirProgram {
        functions: vec![function],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the decomposed multi-output body must remain well-formed MIR");
    let device = kernel_device_context();

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    assert!(
        verdict.is_capable(),
        "a tuple-temp multi-output contract must pass the gate, got: {verdict:?}"
    );
}

#[test]
fn tuple_temp_outside_multi_output_contract_still_fails_target_entry_gate() {
    // The same allowance must NOT widen to a genuine tuple aggregate: a
    // tuple-typed temp in a single-output contract (no result-tuple
    // return) stays fail-closed with the existing aggregate diagnostic.
    let mut types = TypeTable::new();
    let function = single_output_tuple_temp_function(&mut types);
    let program = crate::MirProgram {
        functions: vec![function],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the tuple-temp body must remain well-formed MIR");
    let device = kernel_device_context();

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    let gaps = verdict.gaps();
    assert!(
        !gaps.is_empty(),
        "a tuple temp outside a multi-output contract must fail the target-entry gate"
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.shape().contains("device aggregate value type Tuple")),
        "the tuple temp must fail with the aggregate diagnostic, got: {gaps:?}"
    );
}
