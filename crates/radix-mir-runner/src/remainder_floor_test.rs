//! F9 ruling 29: `%` is the floor remainder `a - b*floor(a/b)` (sign of the
//! divisor) on every numeric family. Pins the runner, the oracle for every
//! backend, on `d64` carriers and on `f64`/`f32`.
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::float_cmp)]

use super::*;
use crate::value::decimal_numeric_binop;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBinOp, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram, MirType,
    MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, TypeTable};

const SCALE: i64 = 100_000_000;

fn dec(carrier: i64) -> Value {
    Value::Int(carrier)
}

fn dec_rem(a: i64, b: i64) -> Result<Value, RunnerError> {
    decimal_numeric_binop(
        dec(a),
        dec(b),
        MirBinOp::Mod,
        NumericWidth::D64,
        Span::default(),
    )
}

fn float_rem(a: f64, b: f64, width: NumericWidth) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let ty = types.sized_numeric(Primitive::Fractus, width);
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
        return_ty: MirType::semantic(ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.eval_binop(
        MirBinOp::Mod,
        &MirOperand::Constant(MirConstant::Float(a)),
        &MirOperand::Constant(MirConstant::Float(b)),
        MirType::semantic(ty),
        Span::default(),
    )
}

fn float(value: Result<Value, RunnerError>) -> f64 {
    match value.expect("float remainder") {
        Value::Float(x) => x,
        other => panic!("expected a float, got {other:?}"),
    }
}

#[test]
fn d64_remainder_takes_the_divisor_sign() {
    // -7.5 % 2.0 = 0.5 and 7.5 % -2.0 = -0.5 (truncation gave -1.5 / 1.5).
    assert_eq!(dec_rem(-75 * SCALE / 10, 2 * SCALE), Ok(dec(SCALE / 2)));
    assert_eq!(dec_rem(75 * SCALE / 10, -2 * SCALE), Ok(dec(-SCALE / 2)));
    // Same-sign operands are unchanged.
    assert_eq!(dec_rem(75 * SCALE / 10, 2 * SCALE), Ok(dec(3 * SCALE / 2)));
    assert_eq!(
        dec_rem(-75 * SCALE / 10, -2 * SCALE),
        Ok(dec(-3 * SCALE / 2))
    );
    // An exact multiple is zero, never the divisor.
    assert_eq!(dec_rem(-4 * SCALE, 2 * SCALE), Ok(dec(0)));
    assert_eq!(dec_rem(4 * SCALE, -2 * SCALE), Ok(dec(0)));
    // Crossing zero: (a + b) % b == a % b.
    assert_eq!(dec_rem(-SCALE / 10, SCALE), Ok(dec(9 * SCALE / 10)));
}

#[test]
fn d64_remainder_is_exact_on_mixed_scales() {
    // -0.000000015 (scale 9) % 0.00000001 (scale 8): floor mod at scale 9.
    let result = decimal_numeric_binop(
        Value::Dec(-15, 9),
        dec(1),
        MirBinOp::Mod,
        NumericWidth::D64,
        Span::default(),
    );
    assert_eq!(result, Ok(Value::Dec(5, 9)));
}

#[test]
fn d64_remainder_by_zero_fails() {
    assert!(dec_rem(SCALE, 0).is_err());
}

#[test]
fn f64_remainder_takes_the_divisor_sign() {
    let w = NumericWidth::F64;
    assert_eq!(float(float_rem(-7.5, 2.0, w)), 0.5);
    assert_eq!(float(float_rem(7.5, -2.0, w)), -0.5);
    assert_eq!(float(float_rem(7.5, 2.0, w)), 1.5);
    assert_eq!(float(float_rem(-7.5, -2.0, w)), -1.5);
    // An exact multiple is a zero carrying the divisor's sign (Python).
    let z = float(float_rem(-4.0, 2.0, w));
    assert!(z == 0.0 && z.is_sign_positive());
    let z = float(float_rem(4.0, -2.0, w));
    assert!(z == 0.0 && z.is_sign_negative());
}

#[test]
fn f32_remainder_takes_the_divisor_sign() {
    let w = NumericWidth::F32;
    assert_eq!(float(float_rem(-7.5, 2.0, w)), 0.5);
    assert_eq!(float(float_rem(7.5, -2.0, w)), -0.5);
    assert_eq!(float(float_rem(-7.5, -2.0, w)), -1.5);
}

#[test]
fn float_remainder_nan_and_infinity_classes() {
    let w = NumericWidth::F64;
    let inf = f64::INFINITY;
    // Zero divisor and infinite dividend are NaN.
    assert!(float(float_rem(1.0, 0.0, w)).is_nan());
    assert!(float(float_rem(-1.0, 0.0, w)).is_nan());
    assert!(float(float_rem(inf, 2.0, w)).is_nan());
    assert!(float(float_rem(-inf, 2.0, w)).is_nan());
    assert!(float(float_rem(f64::NAN, 2.0, w)).is_nan());
    assert!(float(float_rem(2.0, f64::NAN, w)).is_nan());
    // x % inf is x when the signs agree, the floor identity otherwise.
    assert_eq!(float(float_rem(3.0, inf, w)), 3.0);
    assert_eq!(float(float_rem(-3.0, -inf, w)), -3.0);
    assert_eq!(float(float_rem(-3.0, inf, w)), inf);
    assert_eq!(float(float_rem(3.0, -inf, w)), -inf);
}
