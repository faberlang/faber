//! The S6-P2 transformer surface: the recipe ops and admitted tensor unaries
//! pass the target-entry gate; a genuinely unplannable collection op stays
//! fail-closed with the `kernel runtime call` diagnostic.

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirUnOp,
    MirValidationContext, MirValue, MirValueId, MirValueKind,
};
use handroll::FxHashMap;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

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
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorLayerNorm),
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

fn extremum_device_function(types: &mut TypeTable, op: MirCollectionOp) -> MirFunction {
    let tensor = rank2_tensor_ty(types, [4, 4]);
    let scalar = f32_ty(types);
    let span = test_span();
    MirFunction {
        id: MirFunctionId(0),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: vec![MirLocal {
            id: MirLocalId(1),
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
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: scalar,
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
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn max_min_reduction_ops_pass_target_entry_gate() {
    for op in [MirCollectionOp::TensorMax, MirCollectionOp::TensorMin] {
        let mut types = TypeTable::new();
        let program = crate::MirProgram {
            functions: vec![extremum_device_function(&mut types, op)],
        };
        let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
            .expect("the extremum body must remain well-formed MIR");
        let device = MirDeviceContext::from_hir_roles(FxHashMap::from_iter([(
            radix_hir::DefId(1),
            MirDeviceRole::Kernel,
        )]));

        let verdict = classify_device_safe_for_target_entry(&validated, &device);
        assert!(
            verdict.is_capable(),
            "{op:?} must pass the target-entry gate, got: {verdict:?}"
        );
    }
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
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCruxEntropia),
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

/// A rank-1 `tf32[D]` tensor type (the `RMSNorm` gamma row and the rope
/// cos/sin table shapes).
fn tensor_ty_1d(types: &mut TypeTable, dim: u64) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let dim_id = types.intern_index(IndexExpr::Literal(dim));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim_id]));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

fn tensor_ty_3d(types: &mut TypeTable, dims: [u64; 3]) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

/// The `tensor<u32, [count]>` ids-vector type (the GI3-1 gather signature's
/// first extra input at the u32 kernel-index layout, K7c: a rank-1 tensor
/// whose extent is the id count).
fn ids_tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let ids_element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let extent = types.intern_index(IndexExpr::Literal(count));
    let shape = types.intern_index(IndexExpr::Tuple(vec![extent]));
    MirType::semantic(types.intern(Type::Tensor(ids_element, shape)))
}

/// A `lista<u32>` param: never a device buffer (campaign rule 12.3).
fn ids_array_ty(types: &mut TypeTable) -> MirType {
    let ids_element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    MirType::semantic(types.array(ids_element))
}

/// The GI3-1 row-gather kernel shape (mirrors the faber `prefill_gather`
/// and the `gather_kernel_program` CUDA probe): `out[T, D] = table[ids, :]`
/// — a rank-2 `[V, D]` table, the ids vector as a `tensor<u32, [T]>`
/// param/local, and the `TensorGather` recipe statement.
fn gi3_gather_kernel_function(types: &mut TypeTable, id: u32) -> MirFunction {
    let table = rank2_tensor_ty(types, [960, 49_152]);
    let output = rank2_tensor_ty(types, [9, 49_152]);
    let ids = ids_tensor_ty(types, 9);
    let span = test_span();
    MirFunction {
        id: MirFunctionId(id),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: table,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: ids,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: table,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: ids,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: output,
                mutable: true,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(2))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGather),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: output,
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
        return_ty: output,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// The GI3-1 `RMSNorm` kernel shape (mirrors `rms_norm_kernel_program`):
/// `out = rms_norm(x, axis, eps, gamma)` — gamma `[D]`, eps from the const
/// operand.
fn gi3_rms_norm_kernel_function(types: &mut TypeTable, id: u32) -> MirFunction {
    let tensor = rank2_tensor_ty(types, [9, 960]);
    let gamma = tensor_ty_1d(types, 960);
    let span = test_span();
    MirFunction {
        id: MirFunctionId(id),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: gamma,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: gamma,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: tensor,
                mutable: true,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(2))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRmsNorm),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Constant(MirConstant::Int(1)),
                            MirOperand::Constant(MirConstant::Float(1e-5)),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: tensor,
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
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// The GI3-1 `RoPE` kernel shape (mirrors `rope_kernel_program`): `x` plus
/// the explicit interleaved `[9, 32, 3]` table as an extra input param.
fn gi3_rope_kernel_function(types: &mut TypeTable, id: u32) -> MirFunction {
    let tensor = rank2_tensor_ty(types, [9, 64]);
    let table = tensor_ty_3d(types, [9, 32, 3]);
    let span = test_span();
    MirFunction {
        id: MirFunctionId(id),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: table,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: table,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: tensor,
                mutable: true,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(3))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRope),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Constant(MirConstant::Int(0)),
                            MirOperand::Constant(MirConstant::Int(64)),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: tensor,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(3),
                )))),
                span,
            },
            span,
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// The GI3-1 causal masked-row softmax kernel shape (mirrors
/// `causal_masked_softmax_kernel_program`): `out = causal_softmax(x)` over
/// the pinned `[9, 9]` causal score matrix.
fn gi3_causal_masked_softmax_kernel_function(types: &mut TypeTable, id: u32) -> MirFunction {
    let tensor = rank2_tensor_ty(types, [9, 9]);
    let span = test_span();
    MirFunction {
        id: MirFunctionId(id),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor,
                mutable: true,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(
                            MirCollectionOp::TensorCausalMaskedSoftmax,
                        ),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: tensor,
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
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn gi3_inference_recipes_and_gather_ids_pass_target_entry_gate() {
    // GI3-7 CUDA gap 3: the device-safe gate must admit the four GI3-1
    // inference recipe statements (`TensorRmsNorm` / `TensorRope` /
    // `TensorGather` / `TensorCausalMaskedSoftmax`) the same way it admits
    // the S6-P1 transformer recipes — the plan surface resolves each via
    // the `from_transformer_op`-sibling admission path
    // (`is_transformer_recipe_op`, device_program_plans.rs) and the
    // emitters lower the recipe bodies (the GI3-4 NVVM / GI3-3 Metal arms)
    // — plus the row-gather's ids vector (a `tensor<u32, [T]>`, the
    // gather signature's first extra input at the u32 kernel-index layout,
    // gi3-contract.md §1.2). Pre-fix this program failed at the first
    // recipe runtime call.
    let mut types = TypeTable::new();
    let program = crate::MirProgram {
        functions: vec![
            gi3_gather_kernel_function(&mut types, 0),
            gi3_rms_norm_kernel_function(&mut types, 1),
            gi3_rope_kernel_function(&mut types, 2),
            gi3_causal_masked_softmax_kernel_function(&mut types, 3),
        ],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the GI3 recipe bodies must remain well-formed MIR");
    let device = MirDeviceContext {
        by_function_id: FxHashMap::from_iter([
            (MirFunctionId(0), MirDeviceRole::Kernel),
            (MirFunctionId(1), MirDeviceRole::Kernel),
            (MirFunctionId(2), MirDeviceRole::Kernel),
            (MirFunctionId(3), MirDeviceRole::Kernel),
        ]),
        ..MirDeviceContext::default()
    };

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    assert!(
        verdict.is_capable(),
        "the GI3 inference recipe kernels must pass the gate, got: {verdict:?}"
    );
}

#[test]
fn unrelated_array_param_still_fails_target_entry_gate() {
    // There is no array allowance (rule 12.3): a device function carrying
    // an `Array(numerus<u32>)` param — gather ids included, which are a
    // tensor now — fails the gate with the aggregate diagnostic.
    let mut types = TypeTable::new();
    let tensor = rank2_tensor_ty(&mut types, [4, 4]);
    let ids = ids_array_ty(&mut types);
    let span = test_span();
    let function = MirFunction {
        id: MirFunctionId(0),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: ids,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(2),
                name: None,
                ty: tensor,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: ids,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: tensor,
                mutable: true,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(2))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(2))),
                        ],
                        return_ty: tensor,
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
        return_ty: tensor,
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
        .expect("the elementwise body must remain well-formed MIR");
    let device = kernel_device_context();

    let verdict = classify_device_safe_for_target_entry(&validated, &device);
    let gaps = verdict.gaps();
    assert!(
        !gaps.is_empty(),
        "an unrelated array param must stay fail-closed with the aggregate diagnostics"
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.shape().contains("device aggregate value type")),
        "the unrelated array must fail with the aggregate diagnostic, got: {gaps:?}"
    );
}
