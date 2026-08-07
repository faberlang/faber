//! The S5A-U4 fail-closed surface: only the decomposition's result
//! aggregation tuples are admitted; a live unrelated tuple aggregate in a
//! tuple-returning source stays rejected with the aggregate diagnostics.

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
    MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProjection, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirValue, MirValueId, MirValueKind,
};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

/// A tuple-returning source whose UNRELATED aggregate is a LIVE tuple TEMP
/// (the rule-3 fail-closed case): the result aggregation local (built by
/// a Tuple `Construct` and returned) is the multi-output contract, while
/// the unrelated tuple temp(0) — constructed and then READ via a projected
/// place `Local(3) ← Temp(0)[0]` (a direct, non-pure-forward consumer; a
/// pure `Local ← Temp` forward is excluded from `temp_directly_read`
/// because its liveness flows to the local) — is a genuine device
/// aggregate value the temp admission (rule 3) must NOT admit. Local 3
/// is a tensor (the projected field), so the ONLY tuple slot the gate
/// should reject is the live tuple temp.
fn tuple_returning_source_with_live_unrelated_tuple_temp(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let one_tuple = tuple_of_one_ty(types, r2);
    let pair_tuple = MirType::semantic(types.intern(Type::Tuple(vec![r2.semantic_id(); 2])));
    let span = test_span();
    MirFunction {
        id: MirFunctionId(0),
        source: Some(radix_hir::DefId(1)),
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
            MirLocal {
                id: MirLocalId(4),
                name: None,
                ty: one_tuple,
                mutable: false,
                span,
            },
        ],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: pair_tuple,
            span,
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                // t = x @ w — the live tensor body.
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
                // temp(0) ← construct tuple(t, t) — the live unrelated
                // tuple temp.
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::temp(MirTempId(0)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: pair_tuple,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(2),
                                ))),
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(2),
                                ))),
                            ]),
                        },
                    },
                    span,
                },
                // t_field ← temp(0)[0] — a projected read: temp(0) is a
                // DIRECT (non-pure-forward) consumer, so the live tuple
                // temp is not a kept aggregation.
                MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(MirLocalId(3)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Temp(MirTempId(0)),
                                projections: vec![MirProjection::Index(MirOperand::Constant(
                                    MirConstant::Int(0),
                                ))],
                            })),
                            ty: r2,
                            span,
                        },
                    },
                    span,
                },
                // result ← construct tuple(t) — the multi-output result
                // aggregation.
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(MirLocalId(4)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: one_tuple,
                            fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(
                                MirOperand::Place(MirPlace::local(MirLocalId(2))),
                            )]),
                        },
                    },
                    span,
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(4),
                )))),
                span,
            },
            span,
        }],
        return_ty: one_tuple,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn live_unrelated_tuple_temp_in_tuple_returning_source_fails_target_entry_gate() {
    // S5A-U4 (rule 3 fail-closed): the temp admission admits ONLY the
    // decomposition's kept result-aggregation tuple temp. A live
    // unrelated tuple temp — constructed and then READ via a projected
    // place — is a genuine device aggregate value and must fail closed
    // with the typed aggregate diagnostic. This locks the rule-3
    // exclusion: without the `temp_directly_read` guard the live tuple
    // temp (written by a Tuple `Construct`) would be over-admitted.
    let mut types = TypeTable::new();
    let function = tuple_returning_source_with_live_unrelated_tuple_temp(&mut types);
    let program = crate::MirProgram {
        functions: vec![function],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the tuple-returning source must remain well-formed MIR");
    let device = MirDeviceContext::from_hir_roles(FxHashMap::from_iter([(
        radix_hir::DefId(1),
        MirDeviceRole::Kernel,
    )]));

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    let gaps = verdict.gaps();
    assert!(
        !gaps.is_empty(),
        "a live unrelated tuple temp in a tuple-returning source must fail the target-entry gate"
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.shape().contains("device aggregate value type Tuple")),
        "the live unrelated tuple temp must fail with the aggregate diagnostic, got: {gaps:?}"
    );
}

/// A tuple-returning source that ALSO carries a live unrelated tuple
/// aggregate (S5A-U4): the return aggregation local (built by a Tuple
/// `Construct` and returned) is the multi-output contract, while the
/// unrelated tuple local `u` — constructed and then READ by a plain
/// `Assign` — is a genuine device aggregate value. The pre-fix
/// program-level multi-output source set (radix ce36c1285) admitted any
/// tuple-typed slot sharing the source `DefId`; the decomposition-fact
/// gate must admit ONLY the result-aggregation tuples, so `u` stays
/// fail-closed with the typed aggregate diagnostic.
fn tuple_returning_source_with_live_unrelated_tuple(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let one_tuple = tuple_of_one_ty(types, r2);
    let pair_tuple = MirType::semantic(types.intern(Type::Tuple(vec![r2.semantic_id(); 2])));
    let span = test_span();
    MirFunction {
        id: MirFunctionId(0),
        source: Some(radix_hir::DefId(1)),
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
                ty: pair_tuple,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(4),
                name: None,
                ty: one_tuple,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(5),
                name: None,
                ty: pair_tuple,
                mutable: false,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                // t = x @ w — the live tensor body.
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
                // u ← construct tuple(t, t) — the unrelated live tuple
                // aggregate.
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(MirLocalId(3)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: pair_tuple,
                            fields: MirAggregateFields::Ordered(vec![
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(2),
                                ))),
                                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(2),
                                ))),
                            ]),
                        },
                    },
                    span,
                },
                // t2 ← u — reads u: the unrelated aggregate is LIVE.
                MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(MirLocalId(5)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(
                                MirLocalId(3),
                            ))),
                            ty: pair_tuple,
                            span,
                        },
                    },
                    span,
                },
                // result ← construct tuple(t) — the multi-output result
                // aggregation.
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(MirLocalId(4)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Tuple,
                            ty: one_tuple,
                            fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(
                                MirOperand::Place(MirPlace::local(MirLocalId(2))),
                            )]),
                        },
                    },
                    span,
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(4),
                )))),
                span,
            },
            span,
        }],
        return_ty: one_tuple,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn unrelated_live_tuple_aggregate_in_tuple_returning_source_fails_target_entry_gate() {
    // S5A-U4: the gate admits EXACTLY the decomposition's
    // result-aggregation tuples. A live unrelated tuple aggregate in a
    // tuple-returning source — which the pre-fix program-level
    // multi-output source `DefId` set (ce36c1285) admitted — must fail
    // closed with the typed aggregate diagnostic.
    let mut types = TypeTable::new();
    let function = tuple_returning_source_with_live_unrelated_tuple(&mut types);
    let program = crate::MirProgram {
        functions: vec![function],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the tuple-returning source must remain well-formed MIR");
    let device = MirDeviceContext::from_hir_roles(FxHashMap::from_iter([(
        radix_hir::DefId(1),
        MirDeviceRole::Kernel,
    )]));

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    let gaps = verdict.gaps();
    assert!(
        !gaps.is_empty(),
        "a live unrelated tuple aggregate in a tuple-returning source must fail the target-entry gate"
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.shape().contains("device aggregate value type Tuple")),
        "the unrelated tuple aggregate must fail with the aggregate diagnostic, got: {gaps:?}"
    );
}
