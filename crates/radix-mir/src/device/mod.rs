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
}
