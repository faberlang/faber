use super::build_device_program;
use crate::device::MirCompanionMap;
use crate::device_program::DeviceProgramLifetime;
use crate::{
    MirBlock, MirBlockId, MirFunction, MirFunctionId, MirKernelShaderStage, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirStatement, MirTerminator,
    MirTerminatorKind, MirType, MirValidationContext, ValidatedMir,
};
use radix_lexer::{Interner, Span};
use radix_types::{NumericWidth, Primitive, TypeTable};

fn f32(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

#[test]
fn constructor_returns_program_and_semantics_from_validated_mir() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let scalar = f32(&mut types);
    let local = MirLocal {
        id: MirLocalId(10),
        name: None,
        ty: scalar,
        mutable: true,
        span: Span::default(),
    };
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(1),
            source: None,
            name: Some(interner.intern("scale_kernel")),
            params: vec![MirParam {
                local: local.id,
                name: None,
                ty: scalar,
                mode: MirParamMode::Owned,
                span: Span::default(),
            }],
            locals: vec![local],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::<MirStatement>::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                        MirLocalId(10),
                    )))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: scalar,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: Some(MirKernelShaderStage::Compute),
            span: Span::default(),
        }],
    };
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("the constructor fixture validates");

    let (device_program, semantics) = build_device_program(
        &validated,
        &interner,
        &MirCompanionMap::default(),
        None,
        DeviceProgramLifetime::SingleRun,
    )
    .expect("the constructor derives the device program");

    assert_eq!(device_program.kernels.len(), 1);
    assert_eq!(device_program.kernels[0].entry, "scale_kernel");
    assert_eq!(device_program.launches.len(), 1);
    assert_eq!(device_program.results.len(), 1);
    assert_eq!(semantics.values.len(), 2);
    assert_eq!(semantics.bindings.len(), 2);
    assert!(semantics.dependencies.is_empty());
    device_program
        .validate_with_semantics(&semantics)
        .expect("constructor output passes the shared semantic gate");
}
