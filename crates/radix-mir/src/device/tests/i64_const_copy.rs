//! The dead non-kernel-scalar const-copy regressions 3–5: a param destination, an output-projection
//! store, a projected store into a dead slot, a non-constant dead copy, and
//! a terminator-read const all stay fail-closed. Regressions 1–2 live in
//! [`i64_axis_const`].

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProjection, MirStatement,
    MirStatementKind, MirSwitchCase, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirValue, MirValueId, MirValueKind,
};
use radix_types::{IndexExpr, Primitive, Type, TypeTable};

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
                            kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(1))),
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
                            kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(1))),
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
                            kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(1))),
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
                            kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(1))),
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
fn dead_i64_const_copy_read_by_switch_terminator_function(types: &mut TypeTable) -> MirFunction {
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
