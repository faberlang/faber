//! D11.6 U1 / D1.11 runner pins: explicit `↦` integer narrowing is
//! magnitude-checked against the target width, never a bare relabel.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction, MirFunctionId,
    MirOperand, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

fn run_conversion(
    source: Type,
    target: Type,
    value: Value,
    recovery: Option<MirOperand>,
) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source_ty = types.intern(source);
    let target_ty = types.intern(target);
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
        flavor: MirConversionFlavor::Runtime,
        source_ty: MirType::semantic(source_ty),
        target_ty: MirType::semantic(target_ty),
        params: vec![],
        recovery,
        struct_defaults: vec![],
    };
    runner.eval_runtime_convert(&conversion, value, Span::default())
}

fn integer(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

fn int() -> Type {
    Type::Primitive(Primitive::Numerus)
}

const OUT_OF_RANGE: &str = "runner_numerus_to_numerus_conversion_out_of_range";

#[test]
fn int_to_u8_above_range_fails_closed() {
    let error = run_conversion(int(), integer(NumericWidth::U8), Value::Int(300), None)
        .expect_err("300 does not fit u8");
    assert_eq!(error.issue, OUT_OF_RANGE);
}

#[test]
fn int_to_i8_above_range_fails_closed() {
    let error = run_conversion(int(), integer(NumericWidth::I8), Value::Int(300), None)
        .expect_err("300 does not fit i8");
    assert_eq!(error.issue, OUT_OF_RANGE);
    let error = run_conversion(int(), integer(NumericWidth::I8), Value::Int(-129), None)
        .expect_err("-129 does not fit i8");
    assert_eq!(error.issue, OUT_OF_RANGE);
}

#[test]
fn unsigned_carrier_to_narrower_unsigned_fails_closed() {
    let error = run_conversion(
        integer(NumericWidth::U16),
        integer(NumericWidth::U8),
        Value::UInt(300),
        None,
    )
    .expect_err("u16 300 does not fit u8");
    assert_eq!(error.issue, OUT_OF_RANGE);
    let error = run_conversion(
        integer(NumericWidth::U16),
        integer(NumericWidth::U8),
        Value::Int(300),
        None,
    )
    .expect_err("an Int-carried u16 300 does not fit u8");
    assert_eq!(error.issue, OUT_OF_RANGE);
}

#[test]
fn unsigned_carrier_to_narrow_signed_fails_closed() {
    let error = run_conversion(
        integer(NumericWidth::U16),
        integer(NumericWidth::I8),
        Value::UInt(300),
        None,
    )
    .expect_err("u16 300 does not fit i8");
    assert_eq!(error.issue, OUT_OF_RANGE);
}

#[test]
fn in_range_narrowing_keeps_the_value() {
    assert_eq!(
        run_conversion(int(), integer(NumericWidth::U8), Value::Int(255), None)
            .expect("255 fits u8"),
        Value::Int(255),
    );
    assert_eq!(
        run_conversion(int(), integer(NumericWidth::I8), Value::Int(-128), None)
            .expect("-128 fits i8"),
        Value::Int(-128),
    );
}

#[test]
fn out_of_range_narrowing_yields_the_recovery_value() {
    assert_eq!(
        run_conversion(
            int(),
            integer(NumericWidth::U8),
            Value::Int(300),
            Some(MirOperand::Constant(MirConstant::UInt(7))),
        )
        .expect("recovery supplies the value"),
        Value::UInt(7),
    );
}
