//! The a0184031 shared-seam dead non-kernel-scalar const-copy skip,
//! regressions 1–2.
//!
//! The Metal emitter skips a DEAD const-copy Assign whose value is a scalar
//! the kernel surface cannot express (26424928f Gap 1 — the LayerNorm
//! lowering's i64 axis const); the NVVM emitter emits it as a harmless
//! native-i64 slot store. The production CUDA artifact path runs the shared
//! gate BEFORE the emitter, so the gate must agree: a DEAD const-copy
//! Assign (`Assign(Local <- Constant)`) whose value is a non-kernel scalar
//! passes, while the CTO-council fail-closed cases stay rejected. The
//! param / projected-store / place-copy / terminator-read regressions
//! (3–5) live in [`i64_const_copy`].

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirValidationContext,
    MirValue, MirValueId, MirValueKind,
};
use radix_types::TypeTable;

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
