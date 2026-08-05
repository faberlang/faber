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
        MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall, MirStatement,
        MirStatementKind, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType,
        MirValidationContext, MirValue, MirValueId, MirValueKind,
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
}
