//! The decomposed-companion surface (S3-A1, MLP/bert-tiny evidence): kept
//! result-tuple locals and temps survive the subchain decomposition and
//! stay admitted by the target-entry gate.

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
    MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId, MirOperand,
    MirParam, MirParamMode, MirPlace, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp,
    MirTempId, MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
};
use handroll::FxHashMap;
use radix_hir::DefId;
use radix_types::{Primitive, TypeTable};

/// A compact primal → companion lossless row for the carrier/identity tests.
fn lossless_companion(primal: u32, companion: u32) -> LosslessMirCompanionEntry {
    LosslessMirCompanionEntry {
        primal: DefId(primal),
        companion: DefId(companion),
        derivative: MirCompanionDerivativeKind::ReverseModeVjp,
        device_resident: true,
        selected_inputs: vec![MirCompanionSelectedInput {
            param: DefId(primal),
            position: 0,
            ty: radix_types::TypeId(1),
            gradient_slot: 0,
        }],
        selected_outputs: vec![MirCompanionSelectedOutput {
            position: 0,
            ty: radix_types::TypeId(1),
            upstream_gradient_ty: radix_types::TypeId(1),
        }],
    }
}

/// The MLP companion's result-tuple local shape: a tuple-typed local in
/// the source's local table that the DECOMPOSED subchain synthetics
/// keep (`build_subchain_function` clones the source's full local
/// table), even though an intermediate single-output subchain's own
/// return type is Vacuum (`Return(None)` — the CUDA artifact override).
/// The tuple local is the multi-output result aggregation (S3-A1), not
/// a device aggregate value; the gate must admit it through the
/// program-level result-tuple set even when the per-function
/// `multi_output_result` flag is false.
fn mlp_companion_six_gradient(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let tuple = MirType::semantic(types.intern(Type::Tuple(vec![r2.semantic_id(); 6])));
    let span = test_span();
    let mut statements = Vec::new();
    // Six gradient-producing matmuls into locals 6..=11.
    for dest in 6..12u32 {
        let a = MirLocalId((dest - 6) % 6);
        let b = MirLocalId((dest - 5) % 6);
        statements.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace::local(MirLocalId(dest))),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                    args: vec![
                        MirOperand::Place(MirPlace::local(a)),
                        MirOperand::Place(MirPlace::local(b)),
                    ],
                    return_ty: r2,
                },
            },
            span,
        });
    }
    // The multi-output result aggregation into local 12.
    statements.push(MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace::local(MirLocalId(12)),
            aggregate: MirAggregate {
                kind: MirAggregateKind::Tuple,
                ty: tuple,
                fields: MirAggregateFields::Ordered(
                    (6..12u32)
                        .map(|local| {
                            MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                MirLocalId(local),
                            )))
                        })
                        .collect(),
                ),
            },
        },
        span,
    });
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: (0..6u32)
            .map(|local| MirParam {
                local: MirLocalId(local),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals: (0..13u32)
            .map(|local| {
                let ty = if local == 12 { tuple } else { r2 };
                MirLocal {
                    id: MirLocalId(local),
                    name: None,
                    ty,
                    mutable: false,
                    span,
                }
            })
            .collect(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(12),
                )))),
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

#[test]
fn decomposed_mlp_companion_six_tuple_passes_target_entry_gate() {
    // The MLP CUDA reject (U9 evidence): the companion's 6-element
    // return tuple. The artifact path decomposes the companion into
    // subchain synthetics that KEEP the source's full local table (the
    // tuple-typed result local) while intermediate single-output
    // subchains return Vacuum (`Return(None)` — the artifact override).
    // Pre-fix the gate fired `device aggregate value type
    // Tuple([...])` on the kept tuple local because `multi_output_result`
    // keys off the subchain's OWN return type. The program-level
    // result-tuple set (the tuple type IS another device function's
    // return type in the emitted program) must admit it.
    let mut types = TypeTable::new();
    let function = mlp_companion_six_gradient(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        crate::kernel_decomposition::decompose_kernel_function(&function, &validation)
            .expect("the six-gradient companion body decomposes");
    let mut subchain_types =
        TypeTable::from_snapshot(types.snapshot()).expect("a live table always restores");
    decomposition.intern_output_tuple_types(&function, &mut subchain_types);
    let subchain_validation = validation.with_types(&subchain_types);
    let mut synthetics = Vec::new();
    for position in 0..decomposition.subchains.len() {
        let mut synthetic = decomposition.subchain_function(
            &function,
            position,
            subchain_validation.types.as_table(),
        );
        // The CUDA artifact's Vacuum override for `Return(None)` synthetics.
        if matches!(
            synthetic.blocks[0].terminator.kind,
            MirTerminatorKind::Return(None)
        ) {
            synthetic.return_ty =
                MirType::semantic(subchain_validation.types.primitive(Primitive::Vacuum));
        }
        synthetic.id = MirFunctionId(u32::try_from(synthetics.len()).unwrap_or(u32::MAX));
        synthetic.source = Some(radix_hir::DefId(1));
        synthetics.push(synthetic);
    }
    assert!(
        synthetics.len() > 1,
        "the six-matmul body must split into multiple subchains"
    );
    let program = crate::MirProgram {
        functions: synthetics,
    };
    let validated = ValidatedMir::new(program, subchain_validation)
        .expect("the decomposed companion synthetics validate");
    let device = MirDeviceContext::from_hir_roles(FxHashMap::from_iter([(
        radix_hir::DefId(1),
        MirDeviceRole::Kernel,
    )]));

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    assert!(
        verdict.is_capable(),
        "the decomposed companion's kept 6-tuple result local must pass the gate, got: {verdict:?}"
    );
}

/// A multi-output companion whose result aggregation lowers into a TEMP
/// (the reverse-AD companion's gradient-tuple shape — exactly the
/// bert-tiny backward companion: the source builds its result tuple into
/// a temp and returns that temp, `Return(Some(Temp(..)))`). Three
/// gradient-producing matmuls into locals 6..=8, then
/// `temp(0) = construct tuple(_6, _7, _8); return temp(0)`.
fn companion_result_tuple_in_temp(types: &mut TypeTable) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [4, 4]);
    let tuple = MirType::semantic(types.intern(Type::Tuple(vec![r2.semantic_id(); 3])));
    let span = test_span();
    let mut statements = Vec::new();
    // Three gradient-producing matmuls into locals 6..=8.
    for dest in 6..9u32 {
        let a = MirLocalId((dest - 6) % 3);
        let b = MirLocalId((dest - 5) % 3);
        statements.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace::local(MirLocalId(dest))),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                    args: vec![
                        MirOperand::Place(MirPlace::local(a)),
                        MirOperand::Place(MirPlace::local(b)),
                    ],
                    return_ty: r2,
                },
            },
            span,
        });
    }
    // The multi-output result aggregation into TEMP(0).
    statements.push(MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace::temp(MirTempId(0)),
            aggregate: MirAggregate {
                kind: MirAggregateKind::Tuple,
                ty: tuple,
                fields: MirAggregateFields::Ordered(
                    (6..9u32)
                        .map(|local| {
                            MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                MirLocalId(local),
                            )))
                        })
                        .collect(),
                ),
            },
        },
        span,
    });
    MirFunction {
        id: MirFunctionId(0),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: (0..3u32)
            .map(|local| MirParam {
                local: MirLocalId(local),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals: (0..9u32)
            .map(|local| MirLocal {
                id: MirLocalId(local),
                name: None,
                ty: r2,
                mutable: false,
                span,
            })
            .collect(),
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: tuple,
            span,
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
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

#[test]
fn decomposed_companion_result_tuple_in_temp_passes_target_entry_gate() {
    // The bert-tiny CUDA reject (U10 evidence): the backward companion's
    // result aggregation lowers into a TEMP (the reverse-AD gradient
    // tuple), not a local. The artifact path decomposes the companion
    // into subchain synthetics that KEEP the source's full temp table
    // (the kept result-tuple temp) while the synthetic's own return is a
    // FRESH tuple temp (`needs_synthetic_tuple_return`). Pre-fix the gate
    // fired `device aggregate value type Tuple([...])` on the kept source
    // tuple temp because the S5A-U4 result-aggregation fact (rule 2)
    // admitted only kept LOCALS, never kept TEMPS.
    let mut types = TypeTable::new();
    let function = companion_result_tuple_in_temp(&mut types);
    let validation = MirValidationContext::new(&types);
    let decomposition =
        crate::kernel_decomposition::decompose_kernel_function(&function, &validation)
            .expect("the temp-tuple companion body decomposes");
    let mut subchain_types =
        TypeTable::from_snapshot(types.snapshot()).expect("a live table always restores");
    decomposition.intern_output_tuple_types(&function, &mut subchain_types);
    let subchain_validation = validation.with_types(&subchain_types);
    let mut synthetics = Vec::new();
    for position in 0..decomposition.subchains.len() {
        let mut synthetic = decomposition.subchain_function(
            &function,
            position,
            subchain_validation.types.as_table(),
        );
        // The CUDA artifact's Vacuum override for `Return(None)` synthetics.
        if matches!(
            synthetic.blocks[0].terminator.kind,
            MirTerminatorKind::Return(None)
        ) {
            synthetic.return_ty =
                MirType::semantic(subchain_validation.types.primitive(Primitive::Vacuum));
        }
        synthetic.id = MirFunctionId(u32::try_from(synthetics.len()).unwrap_or(u32::MAX));
        synthetics.push(synthetic);
    }
    assert!(
        synthetics.len() > 1,
        "the three-matmul body must split into multiple subchains"
    );
    let program = crate::MirProgram {
        functions: synthetics,
    };
    let validated = ValidatedMir::new(program, subchain_validation)
        .expect("the decomposed companion synthetics validate");
    let device = MirDeviceContext::from_hir_roles(FxHashMap::from_iter([(
        radix_hir::DefId(1),
        MirDeviceRole::Kernel,
    )]));

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    assert!(
        verdict.is_capable(),
        "the decomposed companion's kept result-tuple temp must pass the gate, got: {verdict:?}"
    );
}

// ── DDCP1-U4: companion identity is primal + typed relation (fail-closed
//    carrier) ────────────────────────────────────────────────────────────

#[test]
fn companion_identity_is_primal_plus_typed_relation() {
    // Contract rule 2: the companion identity is derived from the primal
    // kernel identity PLUS the typed relation (the differentiation point) —
    // the (primal, derivative) key, never a name or position.
    let forward = lossless_companion(1, 2);
    let same = lossless_companion(1, 2);
    let other_primal = lossless_companion(9, 2);
    assert_eq!(forward.identity_key(), same.identity_key());
    assert_ne!(forward.identity_key(), other_primal.identity_key());
    assert_eq!(
        forward.identity_key(),
        (
            radix_hir::DefId(1),
            MirCompanionDerivativeKind::ReverseModeVjp
        )
    );
}

#[test]
fn companion_carrier_rejects_duplicate_identity_facts() {
    // Contract rule 3: duplicate companion facts for one (primal,
    // differentiation point) fail closed at the fail-closed carrier — never
    // an overwrite.
    let mut carriers = MirCompanionMap::default();
    carriers.insert(lossless_companion(1, 2));
    assert_eq!(
        carriers.try_insert(lossless_companion(1, 2)),
        Err(MirCompanionInsertError::DuplicatePrimal { primal: DefId(1) })
    );
}
