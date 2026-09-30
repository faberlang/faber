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

// D11.6 ruling 24: `↦` into `wrapping<W>` reduces mod 2^W, into
// `saturating<W>` clamps; neither can fail.

fn wrapping(width: NumericWidth) -> Type {
    Type::ModularWord(width)
}

fn saturating(width: NumericWidth) -> Type {
    Type::SaturatingWord(width)
}

#[test]
fn conversion_into_wrapping_reduces_the_exact_value() {
    let cases = [
        (int(), Value::Int(300), NumericWidth::U8, Value::Int(44)),
        (int(), Value::Int(-1), NumericWidth::U8, Value::Int(255)),
        (
            integer(NumericWidth::U64),
            Value::UInt(u64::MAX),
            NumericWidth::U32,
            Value::Int(4_294_967_295),
        ),
        (
            int(),
            Value::Int(-1),
            NumericWidth::U64,
            Value::UInt(u64::MAX),
        ),
        (
            saturating(NumericWidth::U8),
            Value::Int(255),
            NumericWidth::U8,
            Value::Int(255),
        ),
    ];
    for (source, value, width, expected) in cases {
        let result = run_conversion(source, wrapping(width), value, None).expect("cannot fail");
        assert_eq!(result, expected);
    }
}

#[test]
fn conversion_into_saturating_clamps_the_exact_value() {
    let cases = [
        (int(), Value::Int(300), NumericWidth::U8, Value::Int(255)),
        (int(), Value::Int(-5), NumericWidth::U8, Value::Int(0)),
        (int(), Value::Int(-200), NumericWidth::I8, Value::Int(-128)),
        (
            integer(NumericWidth::U64),
            Value::UInt(u64::MAX),
            NumericWidth::I8,
            Value::Int(127),
        ),
        (
            wrapping(NumericWidth::U16),
            Value::Int(60_000),
            NumericWidth::I16,
            Value::Int(32_767),
        ),
    ];
    for (source, value, width, expected) in cases {
        let result = run_conversion(source, saturating(width), value, None).expect("cannot fail");
        assert_eq!(result, expected);
    }
}

#[test]
fn decimal_source_rounds_like_numerus_narrowing_then_applies_the_policy() {
    // d64 carriers are scaled by 10^9: 300.7 -> 301, -1.5 -> -2 (half-even).
    let d64 = integer(NumericWidth::D64);
    let wrapped = run_conversion(
        d64.clone(),
        wrapping(NumericWidth::U8),
        Value::Int(300_700_000_000),
        None,
    )
    .expect("cannot fail");
    assert_eq!(wrapped, Value::Int(45));
    let wrapped_neg = run_conversion(
        d64.clone(),
        wrapping(NumericWidth::U8),
        Value::Int(-1_500_000_000),
        None,
    )
    .expect("cannot fail");
    assert_eq!(wrapped_neg, Value::Int(254));
    let clamped = run_conversion(
        d64,
        saturating(NumericWidth::U8),
        Value::Int(-1_500_000_000),
        None,
    )
    .expect("cannot fail");
    assert_eq!(clamped, Value::Int(0));
}

/// A value at the top of `i128` (far past every width) clamps at the store
/// instead of overflowing; only the store clamps (d11-6 ruling 3).
#[test]
fn saturating_store_of_huge_value_clamps() {
    assert_eq!(
        crate::value::saturating_word_result(i128::MAX, NumericWidth::U64),
        Value::UInt(u64::MAX)
    );
    assert_eq!(
        crate::value::saturating_word_result(i128::MIN, NumericWidth::I16),
        Value::Int(-32_768)
    );
}
