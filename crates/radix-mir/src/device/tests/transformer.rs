//! The S6-P2 transformer surface: the recipe ops and admitted tensor unaries
//! pass the target-entry gate; a genuinely unplannable collection op stays
//! fail-closed with the `kernel runtime call` diagnostic.

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirUnOp, MirValidationContext,
    MirValue, MirValueId, MirValueKind,
};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

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
