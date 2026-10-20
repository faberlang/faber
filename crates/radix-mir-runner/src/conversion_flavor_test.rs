//! `MirConversionFlavor::Proven`: a narrowing proven in range at compile
//! time reaches the exact carrier through the flavor dispatch, unchecked.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction, MirFunctionId,
    MirOperand, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

fn run_proven_i16(flavor: MirConversionFlavor, value: i64) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source_ty = types.intern(Type::Primitive(Primitive::Numerus));
    let target_ty = types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::I16));
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(target_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let conversion = MirConversion {
        store: None,
        flavor,
        source_ty: MirType::semantic(source_ty),
        target_ty: MirType::semantic(target_ty),
        params: vec![],
        recovery: None,
        struct_defaults: vec![],
    };
    runner.eval_convert(
        &conversion,
        &[MirOperand::Constant(MirConstant::Int(value))],
        Span::default(),
    )
}

#[test]
fn proven_narrowing_keeps_the_proven_value() {
    assert_eq!(
        run_proven_i16(MirConversionFlavor::Proven, 16384).unwrap(),
        Value::Int(16384)
    );
}

#[test]
fn proven_narrowing_preserves_sign() {
    assert_eq!(
        run_proven_i16(MirConversionFlavor::Proven, -2).unwrap(),
        Value::Int(-2)
    );
}

#[test]
fn cast_flavor_still_cannot_narrow_integers() {
    assert!(run_proven_i16(MirConversionFlavor::Cast, 16384).is_err());
}
