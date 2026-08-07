//! Shared MIR device roles and device-safe lowerability gate.
//!
//! TARGET: device-facing MIR consumers (`metal-text`, future `wgsl-text`,
//! NVVM/PTX, and LLVM's device-entry proof comments).
//!
//! WHY: device roles were first extracted inside `llvm_text`, while `metal_text`
//! carried a duplicate role context and local rejection policy. MIR GPU Stage 1
//! hoists the role context and adds one conservative pre-emission gate so
//! device emitters reject host-only MIR shapes before target-specific printing.

pub use crate::device_semantics::{
    LosslessMirCompanionEntry, MirCompanionSelectedInput, MirCompanionSelectedOutput,
};

mod companion;
mod roles;
mod safe;

pub use companion::*;
pub use roles::*;
pub use safe::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::ValidatedMir;
    use crate::{
        MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
        MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLocal,
        MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProjection,
        MirRuntimeCall, MirStatement, MirStatementKind, MirSwitchCase, MirTemp, MirTempId,
        MirTerminator, MirTerminatorKind, MirType, MirUnOp, MirValidationContext, MirValue,
        MirValueId, MirValueKind,
    };
    use radix_lexer::Span;
    use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};
    use rustc_hash::FxHashMap;

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

    /// The `lista<numerus>` shape-list type `magnitudines()` returns.
    fn shape_lista_ty(types: &mut TypeTable) -> MirType {
        MirType::semantic(types.array(types.primitive(Primitive::Numerus)))
    }

    /// A single-element tuple of one tensor type (the multi-output result
    /// contract).
    fn tuple_of_one_ty(types: &mut TypeTable, element: MirType) -> MirType {
        MirType::semantic(types.intern(Type::Tuple(vec![element.semantic_id()])))
    }

    /// A device context marking `MirFunctionId(0)` as a device kernel (the
    /// target-entry gate's routing surface).
    fn kernel_device_context() -> MirDeviceContext {
        MirDeviceContext {
            by_function_id: FxHashMap::from_iter([(MirFunctionId(0), MirDeviceRole::Kernel)]),
            ..MirDeviceContext::default()
        }
    }

    /// The S5-U2c `train_step` artifact shape the emitters see AFTER the
    /// static-shape fold: a DEAD `Array` shape-list construct whose temp
    /// destination forwards into an unread `lista` local (the fold's
    /// untraceable `temp→local` artifact), plus a live tensor body
    /// (`t = x @ w`) returning a result tuple.
    ///
    /// Locals: 0-1 tensor params, 2 `t`, 3 dead shape-list forward target,
    /// 4 result tuple. Temp: 0 dead shape-list artifact destination.
    fn dead_fold_artifact_train_step(types: &mut TypeTable) -> MirFunction {
        let r2 = rank2_tensor_ty(types, [4, 4]);
        let lista = shape_lista_ty(types);
        let tuple = tuple_of_one_ty(types, r2);
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
                    ty: lista,
                    mutable: false,
                    span,
                },
                MirLocal {
                    id: MirLocalId(4),
                    name: None,
                    ty: tuple,
                    mutable: false,
                    span,
                },
            ],
            temps: vec![MirTemp {
                id: MirTempId(0),
                ty: lista,
                span,
            }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    // shape_list ← construct lista[4, 4] — the dead fold
                    // artifact (S5-U2c).
                    MirStatement {
                        kind: MirStatementKind::Construct {
                            destination: MirPlace::temp(MirTempId(0)),
                            aggregate: MirAggregate {
                                kind: MirAggregateKind::Array,
                                ty: lista,
                                fields: MirAggregateFields::Ordered(vec![
                                    MirAggregateItem::Operand(MirOperand::Constant(
                                        MirConstant::Int(4),
                                    )),
                                    MirAggregateItem::Operand(MirOperand::Constant(
                                        MirConstant::Int(4),
                                    )),
                                ]),
                            },
                        },
                        span,
                    },
                    // Local(3) ← Temp(0) — the fold's temp→local forward;
                    // nothing reads Local(3).
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(3)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
                                ty: lista,
                                span,
                            },
                        },
                        span,
                    },
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
                    // result ← construct tuple(t) — the multi-output result.
                    MirStatement {
                        kind: MirStatementKind::Construct {
                            destination: MirPlace::local(MirLocalId(4)),
                            aggregate: MirAggregate {
                                kind: MirAggregateKind::Tuple,
                                ty: tuple,
                                fields: MirAggregateFields::Ordered(vec![
                                    MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                        MirLocalId(2),
                                    ))),
                                ]),
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
            return_ty: tuple,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }
    }

    /// A device function whose `Array` shape-list construct destination is
    /// LIVE — returned by the terminator — the fail-closed counterpart of
    /// the dead artifact.
    fn live_array_construct_function(types: &mut TypeTable) -> MirFunction {
        let lista = shape_lista_ty(types);
        let span = test_span();
        MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: lista,
                mutable: false,
                span,
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(MirLocalId(1)),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Array,
                            ty: lista,
                            fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(
                                MirOperand::Constant(MirConstant::Int(4)),
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
            return_ty: lista,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }
    }

    #[test]
    fn dead_fold_artifact_construct_passes_target_entry_gate() {
        // S5-W-A1 (Part 2): a train_step body carrying the static-shape
        // fold's DEAD `Array` shape-list artifact (a temp destination
        // traced through the `temp→local` forward into an unread `lista`
        // local) must pass the target-entry gate — the emitters' S5-U2c
        // dead-artifact skip requires the gate to agree. Pre-A1 this body
        // failed with `device aggregate value type Array(...)` at the local,
        // temp, and construct checks.
        let mut types = TypeTable::new();
        let function = dead_fold_artifact_train_step(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the dead-artifact body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        assert!(
            verdict.is_capable(),
            "dead fold-artifact Array/Tensor constructs must pass the gate, got: {verdict:?}"
        );
    }

    #[test]
    fn live_array_construct_still_fails_target_entry_gate() {
        // The same gate keeps a LIVE `Array` construct fail-closed with the
        // existing aggregate diagnostic — the emitters fail on a live
        // aggregate construct, so the gate must too.
        let mut types = TypeTable::new();
        let function = live_array_construct_function(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the live-construct body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "a live Array construct must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("device aggregate value type Array")),
            "the live Array construct must fail with the aggregate diagnostic, got: {gaps:?}"
        );
    }

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
            let mut synthetic =
                decomposition.subchain_function(&function, position, subchain_validation.types);
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
            let mut synthetic =
                decomposition.subchain_function(&function, position, subchain_validation.types);
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
                                fields: MirAggregateFields::Ordered(vec![
                                    MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                        MirLocalId(2),
                                    ))),
                                ]),
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
                                fields: MirAggregateFields::Ordered(vec![
                                    MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(
                                        MirLocalId(2),
                                    ))),
                                ]),
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

    /// A transformer-bearing device body (S6-P2): the three S6-P1
    /// transformer recipe ops (`TensorSoftmax` / `TensorSumAxis` /
    /// `TensorLayerNorm`) plus the Exp/Sqrt TENSOR unary assigns (the
    /// softmax exp and the `LayerNorm` sqrt lanes). Each recipe runtime call
    /// reads a prior tensor local and writes a tensor local; the unary
    /// assigns apply the admitted elementwise tensor unaries (`MirUnOp::Exp`
    /// / `MirUnOp::Sqrt` — `is_admitted_tensor_unary`,
    /// `device_program_plans.rs`).
    ///
    /// Locals: 0 input tensor param, 1 `s` softmax result, 2 `e` exp
    /// result, 3 `q` sqrt result, 4 `r` sum-axis result, 5 `l` layernorm
    /// result (returned).
    fn transformer_device_function(types: &mut TypeTable) -> MirFunction {
        let r2 = rank2_tensor_ty(types, [4, 4]);
        let span = test_span();
        MirFunction {
            id: MirFunctionId(0),
            source: Some(radix_hir::DefId(1)),
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: r2,
                mode: MirParamMode::Owned,
                span,
            }],
            locals: (1..6u32)
                .map(|local| MirLocal {
                    id: MirLocalId(local),
                    name: None,
                    ty: r2,
                    mutable: false,
                    span,
                })
                .collect(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    // s = softmax(x) — the RowSoftmax recipe.
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::local(MirLocalId(1))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
                                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                                return_ty: r2,
                            },
                        },
                        span,
                    },
                    // e = exp(s) — the softmax exp tensor unary.
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(2)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Unary {
                                    op: MirUnOp::Exp,
                                    operand: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                                },
                                ty: r2,
                                span,
                            },
                        },
                        span,
                    },
                    // q = sqrt(s) — the LayerNorm sqrt tensor unary.
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(3)),
                            value: MirValue {
                                id: MirValueId(1),
                                kind: MirValueKind::Unary {
                                    op: MirUnOp::Sqrt,
                                    operand: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                                },
                                ty: r2,
                                span,
                            },
                        },
                        span,
                    },
                    // r = sum_axis(s, 1) — the AxisReduction recipe.
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::local(MirLocalId(4))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSumAxis),
                                args: vec![
                                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                                    MirOperand::Constant(MirConstant::Int(1)),
                                ],
                                return_ty: r2,
                            },
                        },
                        span,
                    },
                    // l = layernorm(s, 1, 1e-5) — the LayerNormalization recipe.
                    MirStatement {
                        kind: MirStatementKind::RuntimeCall {
                            destination: Some(MirPlace::local(MirLocalId(5))),
                            call: MirRuntimeCall {
                                intrinsic: MirIntrinsic::Collection(
                                    MirCollectionOp::TensorLayerNorm,
                                ),
                                args: vec![
                                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                                    MirOperand::Constant(MirConstant::Int(1)),
                                    MirOperand::Constant(MirConstant::Float(1e-5)),
                                ],
                                return_ty: r2,
                            },
                        },
                        span,
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(5),
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
    fn transformer_recipe_ops_and_tensor_unaries_pass_target_entry_gate() {
        // S6-P2: a transformer-bearing device function (the S6-P1 recipe
        // ops `TensorSumAxis` / `TensorSoftmax` / `TensorLayerNorm` plus
        // the Exp/Sqrt tensor unaries) must pass the target-entry gate —
        // the P1 plan surface admits them (`is_transformer_recipe_op` /
        // `is_admitted_tensor_unary`, device_program_plans.rs) and the
        // emitters lower the recipe bodies (the S6-P2 Metal arms), so the
        // gate's `kernel runtime call` rejection — which the metal_text
        // probe path surfaces for transformer kernels — must admit them.
        // Pre-fix this body failed at the first recipe runtime call.
        let mut types = TypeTable::new();
        let function = transformer_device_function(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the transformer body must remain well-formed MIR");
        let device = MirDeviceContext::from_hir_roles(FxHashMap::from_iter([(
            radix_hir::DefId(1),
            MirDeviceRole::Kernel,
        )]));

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        assert!(
            verdict.is_capable(),
            "a transformer-bearing device function must pass the gate, got: {verdict:?}"
        );
    }

    #[test]
    fn unplannable_collection_op_still_fails_target_entry_gate() {
        // The transformer admission is a CLOSED set (S6-P2): a genuinely
        // unplannable collection op (`TensorCruxEntropia` — no recipe, no
        // elementwise arm) must still fail the gate with the `kernel
        // runtime call` diagnostic — never a silent admission.
        let mut types = TypeTable::new();
        let r2 = rank2_tensor_ty(&mut types, [4, 4]);
        let scalar = f32_ty(&mut types);
        let span = test_span();
        let function = MirFunction {
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
            locals: vec![MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: scalar,
                mutable: false,
                span,
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(2))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(
                                MirCollectionOp::TensorCruxEntropia,
                            ),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            ],
                            return_ty: scalar,
                        },
                    },
                    span,
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(2),
                    )))),
                    span,
                },
                span,
            }],
            return_ty: scalar,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        };
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the crux-entropia body must remain well-formed MIR");
        let device = MirDeviceContext::from_hir_roles(FxHashMap::from_iter([(
            radix_hir::DefId(1),
            MirDeviceRole::Kernel,
        )]));

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "a crux-entropia body must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("kernel runtime call")),
            "the unplannable op must fail with the kernel runtime call diagnostic, got: {gaps:?}"
        );
    }

    // ── a0184031: the shared-seam dead non-kernel-scalar const-copy skip ──
    //
    // The Metal emitter skips a DEAD const-copy Assign whose value is a
    // scalar the kernel surface cannot express (26424928f Gap 1 — the
    // LayerNorm lowering's i64 axis const); the NVVM emitter emits it as a
    // harmless native-i64 slot store. The production CUDA artifact path runs
    // the shared gate BEFORE the emitter, so the gate must agree: a DEAD
    // const-copy Assign (`Assign(Local <- Constant)`) whose value is a
    // non-kernel scalar passes, while the CTO-council fail-closed cases stay
    // rejected. The five regressions below prove each constraint
    // structurally (head-cto report 5f1b1348).

    fn i64_ty(types: &mut TypeTable) -> MirType {
        MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I64))
    }

    /// The `bert_tiny_loss__0` shape: a `LayerNorm` kernel
    /// body carrying a DEAD non-kernel-scalar const-copy Assign at statement
    /// 0 — `Assign(Local(3) <- Constant(Int(axis)))` typed i64 (the recipe
    /// call carries the axis directly, so the const local is never read).
    /// `read_axis` adds a statement-level consumer so the "same value when
    /// READ" regression shares the exact body.
    ///
    /// Locals: 0 tensor param, 1 tensor result, 3 i64 axis const, 4 (only
    /// when read) the i64 copy target.
    fn layernorm_dead_axis_const_function(types: &mut TypeTable, read_axis: bool) -> MirFunction {
        let r2 = rank2_tensor_ty(types, [2, 4]);
        let i64 = i64_ty(types);
        let span = test_span();
        let mut locals = vec![
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
                id: MirLocalId(3),
                name: None,
                ty: i64,
                mutable: false,
                span,
            },
        ];
        let mut statements = vec![
            // _3 = 1i64 — the DEAD axis const-copy.
            MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(3)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(1))),
                        ty: i64,
                        span,
                    },
                },
                span,
            },
        ];
        if read_axis {
            locals.push(MirLocal {
                id: MirLocalId(4),
                name: None,
                ty: i64,
                mutable: false,
                span,
            });
            statements.push(MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(4)),
                    value: MirValue {
                        id: MirValueId(1),
                        kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(
                            MirLocalId(3),
                        ))),
                        ty: i64,
                        span,
                    },
                },
                span,
            });
        }
        statements.push(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace::local(MirLocalId(1))),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
                    args: vec![
                        MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        MirOperand::Constant(MirConstant::Int(1)),
                        MirOperand::Constant(MirConstant::Float(1e-5)),
                    ],
                    return_ty: r2,
                },
            },
            span,
        });
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
            locals,
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements,
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
    fn dead_i64_axis_const_copy_passes_target_entry_gate() {
        // a0184031 (regression 1): the DEAD i64 axis const-copy — the
        // `bert_tiny_loss__0` shape — must pass the target-entry gate (the
        // classification the production CUDA artifact path runs). Pre-fix
        // the gate rejected the dead axis local AND value with `kernel
        // scalar layout i64`.
        let mut types = TypeTable::new();
        let function = layernorm_dead_axis_const_function(&mut types, false);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the dead-axis-const body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        assert!(
            verdict.is_capable(),
            "a dead i64 axis const-copy must pass the gate, got: {verdict:?}"
        );
    }

    #[test]
    fn dead_i64_axis_const_copy_when_read_still_fails_target_entry_gate() {
        // a0184031 (regression 2): the SAME value when READ must still fail
        // the gate with the layout diagnostic — the skip is dead-only, and
        // a LIVE non-kernel scalar (a consumer reads the axis local) stays
        // fail-closed (constraint: "a LIVE or merely-difficult i64 remains
        // rejected").
        let mut types = TypeTable::new();
        let function = layernorm_dead_axis_const_function(&mut types, true);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the read-axis body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "a READ non-kernel-scalar value must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("kernel scalar layout")),
            "the read axis value must fail with the layout diagnostic, got: {gaps:?}"
        );
    }

    /// A const-copy Assign INTO a function parameter — the CTO "destination
    /// is not a parameter" case (a param destination is a side effect; the
    /// parameter slot is resource-visible).
    fn i64_param_const_copy_function(types: &mut TypeTable) -> MirFunction {
        let i64 = i64_ty(types);
        let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
        let span = test_span();
        MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: i64,
                mode: MirParamMode::Owned,
                span,
            }],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: i64,
                mutable: false,
                span,
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    // _0 = 1i64 — a const-copy into the param (never dead).
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(0)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(
                                    MirConstant::Int(1),
                                )),
                                ty: i64,
                                span,
                            },
                        },
                        span,
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }
    }

    #[test]
    fn dead_i64_const_copy_into_param_fails_target_entry_gate() {
        // a0184031 (regression 3a): a const-copy whose destination is a
        // parameter must fail — the skip's "not a parameter" restriction.
        let mut types = TypeTable::new();
        let function = i64_param_const_copy_function(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the param const-copy body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "a param-destination const-copy must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("kernel scalar layout")),
            "the param i64 must fail with the layout diagnostic, got: {gaps:?}"
        );
    }

    /// An output-style PROJECTED store: `out[idx] = <i64 result>` — the
    /// kernel output-buffer write shape (`output[i] = <result>`), whose
    /// destination is a rank-1 i64-element tensor slot carrying an `Index`
    /// projection. The read of the i64 local through the write's operand
    /// keeps the const-copy local LIVE, so neither declaration nor value is
    /// dead-skipped.
    fn i64_output_projection_const_store_function(types: &mut TypeTable) -> MirFunction {
        let i64 = i64_ty(types);
        // A rank-1 i64-element tensor — the kernel output-buffer slot the
        // emitters write through `output[i] = <result>`.
        let len = types.intern_index(IndexExpr::Literal(4));
        let out_ty = MirType::semantic(types.intern(Type::Tensor(i64.semantic_id(), len)));
        let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
        let span = test_span();
        MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![
                MirLocal {
                    id: MirLocalId(1),
                    name: None,
                    ty: out_ty,
                    mutable: true,
                    span,
                },
                MirLocal {
                    id: MirLocalId(3),
                    name: None,
                    ty: i64,
                    mutable: false,
                    span,
                },
            ],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    // _3 = 1i64 — the const-copy (the axis const local).
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(3)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(
                                    MirConstant::Int(1),
                                )),
                                ty: i64,
                                span,
                            },
                        },
                        span,
                    },
                    // _1[0] = _3 — an output-projection write: the projected
                    // store reads the i64 local, so it is LIVE and the write
                    // is never skipped.
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(1)),
                                projections: vec![MirProjection::Index(MirOperand::Constant(
                                    MirConstant::Int(0),
                                ))],
                            },
                            value: MirValue {
                                id: MirValueId(1),
                                kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(3),
                                ))),
                                ty: i64,
                                span,
                            },
                        },
                        span,
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }
    }

    #[test]
    fn dead_i64_const_copy_output_projection_fails_target_entry_gate() {
        // a0184031 (regression 3b): an output-style projected store must
        // fail — the skip never fires for a projected destination (the CTO
        // "not an output / projected store" restriction): the base tensor
        // slot is not a dead scalar const-copy slot, and the i64 value is
        // still classified.
        let mut types = TypeTable::new();
        let function = i64_output_projection_const_store_function(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the output-projection body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "an output-projection const store must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("kernel scalar layout")),
            "the projected i64 value must fail with the layout diagnostic, got: {gaps:?}"
        );
    }

    /// A projected store into a dead non-kernel-scalar ARRAY slot — the CTO
    /// "projected store" restriction exercised on a destination base that
    /// IS dead and non-scalar: `_1[0] = 1i64` with `_1` a dead
    /// `lista<numerus>` slot. The projection alone must keep the skip from
    /// firing; the i64 value is still classified.
    fn i64_projected_store_into_dead_array_function(types: &mut TypeTable) -> MirFunction {
        let lista = shape_lista_ty(types);
        let i64 = i64_ty(types);
        let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
        let span = test_span();
        MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: lista,
                mutable: true,
                span,
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    // _1[0] = 1i64 — a projected const store into a DEAD
                    // non-scalar slot (nothing ever reads `_1`).
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(1)),
                                projections: vec![MirProjection::Index(MirOperand::Constant(
                                    MirConstant::Int(0),
                                ))],
                            },
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(
                                    MirConstant::Int(1),
                                )),
                                ty: i64,
                                span,
                            },
                        },
                        span,
                    },
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }
    }

    #[test]
    fn dead_i64_const_copy_projected_store_fails_target_entry_gate() {
        // a0184031 (regression 3c): a projected const-copy store must fail
        // even when the destination base is dead — the gate's skip requires
        // an unprojected plain slot, so the "projected store" restriction
        // holds structurally.
        let mut types = TypeTable::new();
        let function = i64_projected_store_into_dead_array_function(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the projected-store body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "a projected const-copy store must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("kernel scalar layout")),
            "the projected i64 const must fail with the layout diagnostic, got: {gaps:?}"
        );
    }

    /// A DEAD i64 copied from another DEAD i64 local — `_4 = _3` where both
    /// are dead i64 slots — with a NON-constant RHS (an `Operand` place, not
    /// a `Constant`). The Metal emitter's 26424928f Gap 1 skip admits only
    /// `MirOperand::Constant` RHS values; this "merely-difficult" dead i64 is
    /// NOT skipped by the emitters, so the gate must keep rejecting it — the
    /// admission set never broadens beyond what the emitters can discard
    /// (Metal/WGSL probe behavior does not broaden).
    fn dead_i64_place_copy_function(types: &mut TypeTable) -> MirFunction {
        let r2 = rank2_tensor_ty(types, [2, 4]);
        let i64 = i64_ty(types);
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
                MirLocal {
                    id: MirLocalId(3),
                    name: None,
                    ty: i64,
                    mutable: false,
                    span,
                },
                MirLocal {
                    id: MirLocalId(4),
                    name: None,
                    ty: i64,
                    mutable: false,
                    span,
                },
            ],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    // _3 = 1i64 — the dead const-copy.
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(3)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(
                                    MirConstant::Int(1),
                                )),
                                ty: i64,
                                span,
                            },
                        },
                        span,
                    },
                    // _4 = _3 — a dead copy with a NON-constant RHS; the
                    // emitters' dead-skip does not cover it.
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(4)),
                            value: MirValue {
                                id: MirValueId(1),
                                kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(
                                    MirLocalId(3),
                                ))),
                                ty: i64,
                                span,
                            },
                        },
                        span,
                    },
                ],
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
    fn dead_i64_place_copy_not_admitted_target_entry_gate() {
        // a0184031 (regression 4): a dead i64 with a non-constant RHS stays
        // fail-closed — only the exact `Constant` const-copy shape is
        // admitted, matching the Metal emitter's skip surface so Metal/WGSL
        // probe behavior cannot broaden.
        let mut types = TypeTable::new();
        let function = dead_i64_place_copy_function(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the dead-place-copy body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "a dead i64 place copy must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("kernel scalar layout")),
            "the copied i64 must fail with the layout diagnostic, got: {gaps:?}"
        );
    }

    /// The liveness-visible counterpart of the dead axis const: the i64
    /// const-copy local is read ONLY by the block's SWITCH terminator — no
    /// statement ever reads it. A liveness scan that skipped terminator
    /// reads would treat the slot as dead; the gate's read set counts
    /// terminator reads (regression 5), so the local stays live and its
    /// declaration keeps the layout diagnostic.
    fn dead_i64_const_copy_read_by_switch_terminator_function(
        types: &mut TypeTable,
    ) -> MirFunction {
        let i64 = i64_ty(types);
        let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
        let span = test_span();
        MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: i64,
                mutable: false,
                span,
            }],
            temps: Vec::new(),
            blocks: vec![
                MirBlock {
                    id: MirBlockId(0),
                    statements: vec![
                        // _3 = 1i64 — the const-copy; the ONLY read of `_3`
                        // is the switch TERMINATOR below.
                        MirStatement {
                            kind: MirStatementKind::Assign {
                                place: MirPlace::local(MirLocalId(3)),
                                value: MirValue {
                                    id: MirValueId(0),
                                    kind: MirValueKind::Operand(MirOperand::Constant(
                                        MirConstant::Int(1),
                                    )),
                                    ty: i64,
                                    span,
                                },
                            },
                            span,
                        },
                    ],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Switch {
                            value: MirOperand::Place(MirPlace::local(MirLocalId(3))),
                            cases: vec![MirSwitchCase {
                                value: MirConstant::Int(1),
                                target: MirBlockId(1),
                            }],
                            default: MirBlockId(1),
                        },
                        span,
                    },
                    span,
                },
                MirBlock {
                    id: MirBlockId(1),
                    statements: Vec::new(),
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(None),
                        span,
                    },
                    span,
                },
            ],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }
    }

    #[test]
    fn dead_i64_const_copy_read_by_terminator_still_fails_target_entry_gate() {
        // a0184031 (regression 5): liveness includes terminator reads — a
        // value read only by the block's switch terminator is still live, so
        // the const-copy slot is NOT dead and the local keeps its layout
        // diagnostic. If the dead-skip's read set ignored terminators, the
        // slot would be skipped and NO layout gap would appear.
        let mut types = TypeTable::new();
        let function = dead_i64_const_copy_read_by_switch_terminator_function(&mut types);
        let program = crate::MirProgram {
            functions: vec![function],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the switch-read body must remain well-formed MIR");
        let device = kernel_device_context();

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        let gaps = verdict.gaps();
        assert!(
            !gaps.is_empty(),
            "a terminator-read i64 const-copy must fail the target-entry gate"
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.shape().contains("kernel scalar layout")),
            "the terminator-read i64 must fail with the layout diagnostic, got: {gaps:?}"
        );
    }
}
