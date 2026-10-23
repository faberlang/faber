//! The runtime conversion intrinsic is outside the kernel subset and must be
//! identified by name in the target-policy rejection.

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBlock, MirBlockId, MirConversion, MirConversionFlavor, MirFunction, MirFunctionId,
    MirIntrinsic, MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
    MirValidationContext,
};
use radix_types::{IndexExpr, Type, TypeTable};

#[test]
fn runtime_conversio_is_named_in_kernel_policy_rejection() {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);
    let width = types.intern_index(IndexExpr::Literal(3));
    let vector = MirType::semantic(types.intern(Type::Vector(f32.semantic_id(), width)));
    let shape = types.intern_index(IndexExpr::Tuple(vec![width]));
    let tensor = MirType::semantic(types.intern(Type::Tensor(f32.semantic_id(), shape)));
    let span = test_span();
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: vector,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: vector,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor,
                mutable: false,
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
                        intrinsic: MirIntrinsic::Convert(MirConversion {
                            store: None,
                            flavor: MirConversionFlavor::Runtime,
                            source_ty: vector,
                            target_ty: tensor,
                            params: Vec::new(),
                            recovery: None,
                            struct_defaults: Vec::new(),
                        }),
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
    };
    let program = crate::MirProgram {
        functions: vec![function],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the runtime conversion body must remain well-formed MIR");

    let verdict = classify_device_safe_for_target_entry(&validated, &kernel_device_context());
    assert!(
        !verdict.is_capable(),
        "runtime conversion must stay outside the kernel subset"
    );
    let first_gap = verdict
        .first_gap()
        .expect("runtime conversion must have a target-policy rejection");
    assert_eq!(first_gap.slug(), "target-policy-rejection");
    assert!(first_gap.shape().contains("kernel runtime call conversio"));
}
