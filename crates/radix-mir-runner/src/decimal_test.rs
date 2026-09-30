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
        store: None,
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
fn decimal_arithmetic_is_exact_until_the_store() {
    let span = Span::default();
    let scale = 100_000_000_i64;
    let binop = |l: Value, r: Value, op: MirBinOp| {
        decimal_numeric_binop(l, r, op, NumericWidth::D64, span).expect("d64 binop")
    };
    // Stored-shaped results stay stored carriers.
    assert_eq!(
        binop(
            Value::Int(42 * scale / 10),
            Value::Int(scale / 10),
            MirBinOp::Add
        ),
        Value::Int(43 * scale / 10),
    );
    assert_eq!(
        binop(Value::Int(42 * scale), Value::Int(2 * scale), MirBinOp::Mul),
        Value::Int(84 * scale),
    );
    // 1.00000001 x 0.5 = 0.500000005 exactly: the product keeps its scale
    // (9 after dropping zeros) and only the store rounds it, half-even, to
    // 0.50000000.
    let midpoint = binop(
        Value::Int(100_000_001),
        Value::Int(50_000_000),
        MirBinOp::Mul,
    );
    assert_eq!(midpoint, Value::Dec(500_000_005, 9));
    assert_eq!(
        crate::decimal::store_carrier(500_000_005, 9),
        Some(50_000_000)
    );
    // The odd side of the tie rounds up to the even carrier.
    assert_eq!(
        crate::decimal::store_carrier(500_000_015, 9),
        Some(50_000_002)
    );
    // Scales grow through `*` and compare exactly.
    let p = Value::Int(1_999_000_000); // 19.99
    let r = Value::Int(7_250_000); // 0.0725
    let pr = binop(p.clone(), r.clone(), MirBinOp::Mul);
    assert_eq!(pr, Value::Int(144_927_500)); // 1.449275
    let prr = binop(pr, r, MirBinOp::Mul);
    assert_eq!(prr, Value::Dec(1_050_724_375, 10)); // 0.1050724375
    assert_eq!(crate::decimal::format(1_050_724_375, 10), "0.1050724375");
    assert_eq!(
        crate::decimal::store_carrier(1_050_724_375, 10),
        Some(10_507_244)
    );
    // `/` rounds half-even to the larger operand scale.
    let third = binop(Value::Int(scale), Value::Int(3 * scale), MirBinOp::Div);
    assert_eq!(third, Value::Int(33_333_333));
    let wide = binop(Value::Dec(10, 16), Value::Int(3 * scale), MirBinOp::Div);
    assert_eq!(wide, Value::Dec(3, 16));
    assert_eq!(round_half_even_quotient(5, 2), Some(2));
    assert_eq!(round_half_even_quotient(7, 2), Some(4));
    assert_eq!(round_half_even_quotient(-5, 2), Some(-2));
    assert_eq!(round_half_even_quotient(-7, 2), Some(-4));
}

#[test]
fn decimal_runtime_binop_normalizes_float_constants() {
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
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
        return_ty: MirType::semantic(d64),
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
            MirType::semantic(d64),
            Span::default(),
        )
        .expect("4.2 + 0.1 d64");
    assert_eq!(result, Value::Int(430_000_000));
}

#[test]
fn decimal_conversio_matrix_rounds() {
    let d64 = decimal(NumericWidth::D64);
    let i64 = integer(NumericWidth::I64);
    let u32 = integer(NumericWidth::U32);
    let f64 = fractus(NumericWidth::F64);

    assert_eq!(
        run_conversion(
            Type::Primitive(Primitive::Textus),
            d64.clone(),
            Value::Str("2.000000005".to_owned()),
        )
        .expect("textus to d64 midpoint"),
        Value::Int(200_000_000),
    );
    assert_eq!(
        run_conversion(
            Type::Primitive(Primitive::Textus),
            d64.clone(),
            Value::Str("2.000000015".to_owned()),
        )
        .expect("textus to d64 odd midpoint"),
        Value::Int(200_000_002),
    );
    assert_eq!(
        run_conversion(i64.clone(), d64.clone(), Value::Int(42),).expect("integer to d64"),
        Value::Int(4_200_000_000),
    );
    assert_eq!(
        run_conversion(fractus(NumericWidth::F64), d64.clone(), Value::Float(4.2),)
            .expect("fractus to d64"),
        Value::Int(420_000_000),
    );
    assert_eq!(
        run_conversion(d64.clone(), i64.clone(), Value::Int(450_000_000),)
            .expect("4.5 to integer uses even tie"),
        Value::Int(4),
    );
    assert_eq!(
        run_conversion(d64.clone(), u32, Value::Int(350_000_000),)
            .expect("3.5 to unsigned integer uses even tie"),
        Value::UInt(4),
    );
    assert_eq!(
        run_conversion(d64.clone(), f64, Value::Int(420_000_000)).expect("d64 to fractus"),
        Value::Float(4.2),
    );
    assert_eq!(
        run_conversion(
            d64,
            Type::Primitive(Primitive::Textus),
            Value::Int(420_000_000),
        )
        .expect("d64 to textus display"),
        Value::Str("4.2".to_owned()),
    );
}

#[test]
fn decimal_overflow_traps_at_the_128_bit_bound_and_the_store() {
    // Past the d64 range an intermediate is still exact; only the store traps.
    let sum = decimal_numeric_binop(
        Value::Int(i64::MAX),
        Value::Int(2),
        MirBinOp::Add,
        NumericWidth::D64,
        Span::default(),
    )
    .expect("exact sum beyond the d64 range");
    assert_eq!(sum, Value::Dec(i128::from(i64::MAX) + 2, 8));
    assert_eq!(
        crate::decimal::store_carrier(i128::from(i64::MAX) + 2, 8),
        None
    );
    // The intermediate itself is bounded by the 128-bit carrier.
    let error = decimal_numeric_binop(
        Value::Dec(i128::MAX, 0),
        Value::Int(2),
        MirBinOp::Mul,
        NumericWidth::D64,
        Span::default(),
    )
    .expect_err("i128 carrier overflow");
    assert_eq!(error.issue, "runner_numerus_overflow");
}

#[test]
fn negative_signed_to_unsigned_conversion_fails_closed() {
    let i32 = integer(NumericWidth::I32);
    let u32 = integer(NumericWidth::U32);
    let error = run_conversion(i32.clone(), u32.clone(), Value::Int(-2))
        .expect_err("negative signed value has no unsigned carrier");
    assert_eq!(
        error.issue,
        "runner_numerus_to_numerus_conversion_out_of_range",
    );
    assert_eq!(
        run_conversion(i32, u32, Value::Int(5)).expect("non-negative converts"),
        Value::Int(5),
    );
}
