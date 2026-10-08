//! `TensorExtent` inside a kernel unit: a scalar extent read of a tensor
//! parameter passes the target-entry gate where a list read is rejected
//! (operator ruling 2026-10-08, option C: lists stay out of kernels).

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirValidationContext,
    MirValue, MirValueId, MirValueKind,
};
use radix_types::{Lowerability, TypeTable};

/// A `LayerNorm` kernel over a rank-2 tensor parameter (local 0) whose body
/// first reads either the runtime extent of axis 0 into an `i32` local
/// (`list_read == false`) or copies a list parameter (local 3) into a list
/// local (`list_read == true`).
fn kernel_with_read(types: &mut TypeTable, list_read: bool) -> MirFunction {
    let r2 = rank2_tensor_ty(types, [2, 4]);
    let i32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I32));
    let list_ty = shape_lista_ty(types);
    let span = test_span();
    let local = |id: u32, ty: MirType| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: false,
        span,
    };
    let mut params = vec![MirParam {
        local: MirLocalId(0),
        name: None,
        ty: r2,
        mode: MirParamMode::Owned,
        span,
    }];
    let mut locals = vec![local(0, r2), local(1, r2)];
    let read = if list_read {
        params.push(MirParam {
            local: MirLocalId(3),
            name: None,
            ty: list_ty,
            mode: MirParamMode::Owned,
            span,
        });
        locals.push(local(3, list_ty));
        locals.push(local(4, list_ty));
        MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::local(MirLocalId(4)),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(MirLocalId(3)))),
                    ty: list_ty,
                    span,
                },
            },
            span,
        }
    } else {
        locals.push(local(2, i32_ty));
        MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::local(MirLocalId(2)),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::TensorExtent {
                        tensor: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        axis: 0,
                    },
                    ty: i32_ty,
                    span,
                },
            },
            span,
        }
    };
    let layer_norm = MirStatement {
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
    };
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params,
        locals,
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![read, layer_norm],
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

fn verdict_for(list_read: bool) -> Lowerability {
    let mut types = TypeTable::new();
    let function = kernel_with_read(&mut types, list_read);
    let program = crate::MirProgram {
        functions: vec![function],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the kernel body must be well-formed MIR");
    classify_device_safe_for_target_entry(&validated, &kernel_device_context())
}

#[test]
fn tensor_extent_read_is_accepted_in_a_kernel_unit() {
    let verdict = verdict_for(false);
    assert!(
        verdict.is_capable(),
        "a TensorExtent scalar read must pass the kernel gate, got: {verdict:?}"
    );
}

#[test]
fn tensor_extent_control_list_read_is_rejected_in_a_kernel_unit() {
    let verdict = verdict_for(true);
    assert!(
        !verdict.gaps().is_empty(),
        "a list read must stay rejected in a kernel unit"
    );
}
