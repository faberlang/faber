//! D4 runner pins for scaled decimal carriers and the D3 conversion matrix.

use super::*;
use crate::value::{decimal_numeric_binop, round_half_even_quotient};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBinOp, MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction,
    MirFunctionId, MirOperand, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

fn run_conversion(source: Type, target: Type, value: Value) -> Result<Value, RunnerError> {
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
        flavor: MirConversionFlavor::Runtime,
        source_ty: MirType::semantic(source_ty),
        target_ty: MirType::semantic(target_ty),
        params: vec![],
        recovery: None,
        struct_defaults: vec![],
    };
    runner.eval_runtime_convert(&conversion, value, Span::default())
}

fn decimal(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

fn integer(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

fn fractus(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Fractus, width)
}

#[test]
fn decimal_arithmetic_uses_scaled_carriers_and_checked_rounding() {
    let span = Span::default();
    let scale = 10_000_000_i64;
    assert_eq!(
        decimal_numeric_binop(
            Value::Int(42 * scale / 10),
            Value::Int(scale / 10),
            MirBinOp::Add,
            NumericWidth::D32,
            span,
        )
        .expect("d32 add"),
        Value::Int(43 * scale / 10),
    );
    assert_eq!(
        decimal_numeric_binop(
            Value::Int(42 * scale),
            Value::Int(2 * scale),
            MirBinOp::Mul,
            NumericWidth::D32,
            span,
        )
        .expect("d32 multiply"),
        Value::Int(84 * scale),
    );
    // 1.0000001 × 0.5 lands exactly at an odd/even midpoint in carrier units;
    // the even result is selected rather than truncating toward zero.
    assert_eq!(
        decimal_numeric_binop(
            Value::Int(10_000_001),
            Value::Int(5_000_000),
            MirBinOp::Mul,
            NumericWidth::D32,
            span,
        )
        .expect("d32 midpoint multiply"),
        Value::Int(5_000_000),
    );
    assert_eq!(round_half_even_quotient(5, 2), Some(2));
    assert_eq!(round_half_even_quotient(7, 2), Some(4));
    assert_eq!(round_half_even_quotient(-5, 2), Some(-2));
    assert_eq!(round_half_even_quotient(-7, 2), Some(-4));
}

#[test]
fn decimal_runtime_binop_normalizes_float_constants() {
    let mut types = TypeTable::new();
    let d32 = types.sized_numeric(Primitive::Numerus, NumericWidth::D32);
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
        return_ty: MirType::semantic(d32),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let result = runner
        .eval_binop(
            MirBinOp::Add,
            &MirOperand::Constant(MirConstant::Float(4.2)),
            &MirOperand::Constant(MirConstant::Float(0.1)),
            MirType::semantic(d32),
            Span::default(),
        )
        .expect("4.2 + 0.1 d32");
    assert_eq!(result, Value::Int(43_000_000));
}

#[test]
fn decimal_conversio_matrix_rounds_and_widens() {
    let d32 = decimal(NumericWidth::D32);
    let d64 = decimal(NumericWidth::D64);
    let i64 = integer(NumericWidth::I64);
    let u32 = integer(NumericWidth::U32);
    let f64 = fractus(NumericWidth::F64);

    assert_eq!(
        run_conversion(
            Type::Primitive(Primitive::Textus),
            d32.clone(),
            Value::Str("2.50000005".to_owned()),
        )
        .expect("textus to d32 midpoint"),
        Value::Int(25_000_000),
    );
    assert_eq!(
        run_conversion(
            Type::Primitive(Primitive::Textus),
            d32.clone(),
            Value::Str("2.50000015".to_owned()),
        )
        .expect("textus to d32 odd midpoint"),
        Value::Int(25_000_002),
    );
    assert_eq!(
        run_conversion(i64.clone(), d32.clone(), Value::Int(42),).expect("integer to d32"),
        Value::Int(420_000_000),
    );
    assert_eq!(
        run_conversion(fractus(NumericWidth::F64), d32.clone(), Value::Float(4.2),)
            .expect("fractus to d32"),
        Value::Int(42_000_000),
    );
    assert_eq!(
        run_conversion(d32.clone(), i64.clone(), Value::Int(45_000_000),)
            .expect("4.5 to integer uses even tie"),
        Value::Int(4),
    );
    assert_eq!(
        run_conversion(d32.clone(), u32, Value::Int(35_000_000),)
            .expect("3.5 to unsigned integer uses even tie"),
        Value::UInt(4),
    );
    assert_eq!(
        run_conversion(d32.clone(), d64.clone(), Value::Int(42_000_000))
            .expect("d32 to d64 widening"),
        Value::Int(4_200_000_000),
    );
    assert_eq!(
        run_conversion(d32.clone(), f64, Value::Int(42_000_000)).expect("d32 to fractus"),
        Value::Float(4.2),
    );
    assert_eq!(
        run_conversion(
            d32.clone(),
            Type::Primitive(Primitive::Textus),
            Value::Int(42_000_000),
        )
        .expect("d32 to textus display"),
        Value::Str("4.2".to_owned()),
    );
    let narrowing = run_conversion(d64, d32, Value::Int(4_200_000_000));
    assert_eq!(
        narrowing
            .expect_err("d64 to d32 must remain rejected")
            .message,
        "decimal width narrowing rejected"
    );
}

#[test]
fn decimal_overflow_is_checked_at_the_declared_carrier_width() {
    let error = decimal_numeric_binop(
        Value::Int(i64::MAX),
        Value::Int(2),
        MirBinOp::Add,
        NumericWidth::D32,
        Span::default(),
    )
    .expect_err("d32 carrier overflow");
    assert_eq!(error.message, "numerus overflow");
}
