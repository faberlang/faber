//! D11.6 U1 / D1.11 runner pins: explicit `↦` integer narrowing is
//! magnitude-checked against the target width, never a bare relabel.
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]

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
    // d64 carriers are scaled by 10^8: 300.7 -> 301, -1.5 -> -2 (half-even).
    let d64 = integer(NumericWidth::D64);
    let wrapped = run_conversion(
        d64.clone(),
        wrapping(NumericWidth::U8),
        Value::Int(30_070_000_000),
        None,
    )
    .expect("cannot fail");
    assert_eq!(wrapped, Value::Int(45));
    let wrapped_neg = run_conversion(
        d64.clone(),
        wrapping(NumericWidth::U8),
        Value::Int(-150_000_000),
        None,
    )
    .expect("cannot fail");
    assert_eq!(wrapped_neg, Value::Int(254));
    let clamped = run_conversion(
        d64,
        saturating(NumericWidth::U8),
        Value::Int(-150_000_000),
        None,
    )
    .expect("cannot fail");
    assert_eq!(clamped, Value::Int(0));
}

fn fractus() -> Type {
    Type::Primitive(Primitive::Fractus)
}

/// Exact expected residues of a float's truncated integer value in `u8`/`i8`.
///
/// Small inputs reduce by plain integer math (300 mod 256 = 44; -5 mod 256 =
/// 251, signed -5; 255 signed is -1). A double with magnitude >= 2^60 has an
/// ulp of at least 2^8, so it is an exact multiple of 2^8 and both residues are
/// 0: that covers 1e25, 1e300 and -1e300. 1e19 is 2^19 * 5^19 with 5^19 < 2^53,
/// so it is exactly representable and a multiple of 2^8 too.
fn wrapping_float_expectations() -> Vec<(f64, i64, i64)> {
    vec![
        // (input, wrapping<u8>, wrapping<i8>); fractional inputs truncate first.
        (3.7, 3, 3),
        (300.7, 44, 44),
        (-5.5, 251, -5),
        (255.0, 255, -1),
        (1e19, 0, 0),
        (1e25, 0, 0),
        (1e300, 0, 0),
        (-1e300, 0, 0),
    ]
}

#[test]
fn float_into_wrapping_reduces_exactly_for_any_finite_magnitude() {
    for (input, unsigned, signed) in wrapping_float_expectations() {
        let u8_result = run_conversion(
            fractus(),
            wrapping(NumericWidth::U8),
            Value::Float(input),
            None,
        )
        .expect("a finite float cannot fail");
        assert_eq!(u8_result, Value::Int(unsigned), "wrapping<u8> of {input}");
        let i8_result = run_conversion(
            fractus(),
            wrapping(NumericWidth::I8),
            Value::Float(input),
            None,
        )
        .expect("a finite float cannot fail");
        assert_eq!(i8_result, Value::Int(signed), "wrapping<i8> of {input}");
    }
}

#[test]
fn float_into_saturating_clamps_for_any_finite_magnitude() {
    // (input, saturating<u8>, saturating<i8>)
    let cases: [(f64, i64, i64); 8] = [
        (3.7, 3, 3),
        (300.7, 255, 127),
        (-5.5, 0, -5),
        (255.0, 255, 127),
        (1e19, 255, 127),
        (1e25, 255, 127),
        (1e300, 255, 127),
        (-1e300, 0, -128),
    ];
    for (input, unsigned, signed) in cases {
        let u8_result = run_conversion(
            fractus(),
            saturating(NumericWidth::U8),
            Value::Float(input),
            None,
        )
        .expect("a finite float cannot fail");
        assert_eq!(u8_result, Value::Int(unsigned), "saturating<u8> of {input}");
        let i8_result = run_conversion(
            fractus(),
            saturating(NumericWidth::I8),
            Value::Float(input),
            None,
        )
        .expect("a finite float cannot fail");
        assert_eq!(i8_result, Value::Int(signed), "saturating<i8> of {input}");
    }
}

#[test]
fn float_into_wrapping_keeps_nan_and_infinity_on_the_error_channel() {
    for input in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let error = run_conversion(
            fractus(),
            wrapping(NumericWidth::U8),
            Value::Float(input),
            None,
        )
        .expect_err("a non-finite float has no integer");
        assert!(
            error.issue.contains("fractus_to_numerus"),
            "{}",
            error.issue
        );
    }
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

// u64-parse: `textus ↦ u64` parses the exact magnitude. The parse used to run
// through `i64`, so every text from 2^63 up failed as if it were not a number.

const TEXT_OUT_OF_RANGE: &str = "runner_textus_to_numerus_conversion_out_of_range";
const TEXT_FAILED: &str = "runner_textus_to_numerus_conversion_failed";

fn parse_text(width: NumericWidth, text: &str) -> Result<Value, RunnerError> {
    run_conversion(
        Type::Primitive(Primitive::Textus),
        integer(width),
        Value::Str(text.to_owned()),
        None,
    )
}

#[test]
fn text_parses_into_u64_through_its_full_range() {
    assert_eq!(
        parse_text(NumericWidth::U64, "18446744073709551615").expect("u64::MAX parses"),
        Value::UInt(u64::MAX)
    );
    assert_eq!(
        parse_text(NumericWidth::U64, "9223372036854775808").expect("2^63 parses"),
        Value::UInt(1 << 63)
    );
    assert_eq!(
        parse_text(NumericWidth::U64, "9223372036854775807").expect("i64::MAX parses"),
        Value::Int(i64::MAX)
    );
    assert_eq!(
        parse_text(NumericWidth::U64, "0").expect("zero parses"),
        Value::Int(0)
    );
}

#[test]
fn text_beyond_u64_or_below_zero_fails_closed_for_u64() {
    let past = parse_text(NumericWidth::U64, "18446744073709551616").expect_err("2^64 > u64::MAX");
    assert_eq!(past.issue, TEXT_OUT_OF_RANGE);
    let negative = parse_text(NumericWidth::U64, "-1").expect_err("negative into u64");
    assert_eq!(negative.issue, TEXT_OUT_OF_RANGE);
    let word = parse_text(NumericWidth::U64, "twelve").expect_err("not a number");
    assert_eq!(word.issue, TEXT_FAILED);
}

#[test]
fn text_into_i64_keeps_its_bounds() {
    assert_eq!(
        parse_text(NumericWidth::I64, "-9223372036854775808").expect("i64::MIN parses"),
        Value::Int(i64::MIN)
    );
    let past = parse_text(NumericWidth::I64, "9223372036854775808").expect_err("2^63 > i64::MAX");
    assert_eq!(past.issue, TEXT_OUT_OF_RANGE);
}

/// `int ↦ f32` rounds once, nearest-even, from the integer itself. 2^60 +
/// 2^36 + 1 sits just above the midpoint of two f32 values, so a detour
/// through `f64` (which drops the `+ 1`) lands on the tie and picks 2^60.
#[test]
fn int_to_f32_rounds_once() {
    let f32_ty = Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32);
    let big = (1_i64 << 60) + (1_i64 << 36) + 1;
    let once = (1_u64 << 60) + (1_u64 << 37);
    assert_eq!(
        run_conversion(int(), f32_ty.clone(), Value::Int(big), None).expect("int to f32"),
        Value::Float(once as f64)
    );
    assert_eq!(
        run_conversion(
            integer(NumericWidth::U64),
            f32_ty,
            Value::UInt(big as u64),
            None
        )
        .expect("u64 to f32"),
        Value::Float(once as f64)
    );
}

/// `f64 ↦ f32` rounds to binary32 at the store: 0.1 is not representable, so
/// the narrowed cell holds the nearest f32, not the f64 literal.
#[test]
fn float_to_f32_rounds_to_binary32() {
    let f32_ty = Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32);
    let narrowed = run_conversion(fractus(), f32_ty, Value::Float(0.1), None).expect("f64 to f32");
    assert_eq!(narrowed, Value::Float(f64::from(0.1_f64 as f32)));
    assert_ne!(narrowed, Value::Float(0.1));
}

#[test]
fn text_to_f32_rounds_to_binary32() {
    let f32_ty = Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32);
    let narrowed = run_conversion(
        Type::Primitive(Primitive::Textus),
        f32_ty,
        Value::Str("0.1".into()),
        None,
    )
    .expect("text to f32");
    assert_eq!(narrowed, Value::Float(f64::from(0.1_f64 as f32)));
    assert_ne!(narrowed, Value::Float(0.1));
}

/// Beyond `f32::MAX` the nearest binary32 is infinity (IEEE overflow); the
/// store does not clamp.
#[test]
fn float_above_f32_max_narrows_to_infinity() {
    let f32_ty = Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32);
    assert_eq!(
        run_conversion(fractus(), f32_ty, Value::Float(1e39), None).expect("f64 to f32"),
        Value::Float(f64::INFINITY)
    );
}

#[test]
fn float_to_f64_keeps_the_value() {
    let f64_ty = Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64);
    assert_eq!(
        run_conversion(fractus(), f64_ty, Value::Float(0.1), None).expect("f64 to f64"),
        Value::Float(0.1)
    );
}
