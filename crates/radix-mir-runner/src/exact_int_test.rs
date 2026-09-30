//! d11-6 U5: exact integer values, floor division, unmasked shifts, `¬`,
//! infinite two's-complement bitwise ops, and exact cross-family comparison.

use crate::exact_int::{
    cmp_int_float, exact_bitnot, exact_int_binop, exact_neg, floor_div, floor_rem,
};
use crate::value::{Value, compare_values, values_equal};
use radix_lexer::Span;
use radix_mir::MirBinOp;
use radix_types::NumericWidth;

const W: NumericWidth = NumericWidth::I64;

fn op(op: MirBinOp, a: Value, b: Value) -> Result<Value, String> {
    exact_int_binop(&a, &b, op, W, Span::default())
        .expect("integer carriers")
        .map_err(|e| e.message)
}

fn int(v: i64) -> Value {
    Value::Int(v)
}

#[test]
fn intermediates_are_exact_over_the_64_bit_range() {
    // ruling 17a: u64::MAX and i64::MIN are both valid values.
    assert_eq!(
        op(MirBinOp::Add, Value::UInt(u64::MAX - 1), int(1)),
        Ok(Value::UInt(u64::MAX))
    );
    assert!(op(MirBinOp::Add, Value::UInt(u64::MAX), int(1)).is_err());
    assert_eq!(
        op(MirBinOp::Sub, Value::UInt(0), Value::UInt(1 << 63)),
        Ok(int(i64::MIN))
    );
    assert!(op(MirBinOp::Sub, Value::UInt(0), Value::UInt((1 << 63) + 1)).is_err());
    // mixed u64 / i64 carriers
    assert_eq!(
        op(MirBinOp::Sub, Value::UInt(1 << 63), Value::UInt(1 << 63)),
        Ok(int(0))
    );
    assert_eq!(
        op(MirBinOp::Add, Value::UInt(1 << 63), int(-1)),
        Ok(int(i64::MAX))
    );
    assert!(op(MirBinOp::Mul, Value::UInt(u64::MAX), Value::UInt(u64::MAX)).is_err());
}

#[test]
fn division_floors_and_remainder_takes_the_divisor_sign() {
    assert_eq!(op(MirBinOp::Div, int(7), int(2)), Ok(int(3)));
    assert_eq!(op(MirBinOp::Div, int(-7), int(2)), Ok(int(-4)));
    assert_eq!(op(MirBinOp::Mod, int(-7), int(2)), Ok(int(1)));
    assert_eq!(op(MirBinOp::Div, int(7), int(-2)), Ok(int(-4)));
    assert_eq!(op(MirBinOp::Mod, int(7), int(-2)), Ok(int(-1)));
    assert_eq!(floor_div(-1, 5), -1);
    assert_eq!(floor_rem(-1, 5), 4);
    assert!(op(MirBinOp::Div, int(1), int(0)).is_err());
    assert!(op(MirBinOp::Mod, int(1), int(0)).is_err());
}

#[test]
fn min_over_minus_one_is_the_exact_quotient() {
    // No special case: 2^63 is a valid intermediate.
    assert_eq!(
        op(MirBinOp::Div, int(i64::MIN), int(-1)),
        Ok(Value::UInt(1 << 63))
    );
}

#[test]
fn bitnot_and_negation_are_exact() {
    let span = Span::default();
    let not = |v: Value| exact_bitnot(&v, W, span).unwrap().map_err(|e| e.message);
    assert_eq!(not(int(250)), Ok(int(-251)));
    assert_eq!(not(int(0)), Ok(int(-1)));
    assert!(
        not(Value::UInt(u64::MAX)).is_err(),
        "-2^64 is outside the range"
    );
    assert_eq!(not(Value::UInt((1 << 63) - 1)), Ok(int(i64::MIN)));
    assert!(
        not(Value::UInt(1 << 63)).is_err(),
        "-2^63 - 1 is outside the range"
    );
    let neg = |v: Value| exact_neg(&v, W, span).unwrap().map_err(|e| e.message);
    assert_eq!(neg(int(-128)), Ok(int(128)));
    assert_eq!(neg(int(i64::MIN)), Ok(Value::UInt(1 << 63)));
}

#[test]
fn shifts_are_unmasked_pure_math() {
    assert_eq!(op(MirBinOp::Shl, int(1), int(63)), Ok(Value::UInt(1 << 63)));
    assert!(
        op(MirBinOp::Shl, int(1), int(64)).is_err(),
        "2^64 is past the range"
    );
    assert!(op(MirBinOp::Shl, int(255), int(60)).is_err());
    assert_eq!(op(MirBinOp::Shl, int(0), int(500)), Ok(int(0)));
    assert_eq!(op(MirBinOp::Shr, int(5), int(64)), Ok(int(0)));
    assert_eq!(op(MirBinOp::Shr, int(5), int(1_000)), Ok(int(0)));
    assert_eq!(op(MirBinOp::Shr, int(-5), int(200)), Ok(int(-1)));
    assert_eq!(op(MirBinOp::Shr, int(-5), int(1)), Ok(int(-3)));
    assert!(op(MirBinOp::Shl, int(1), int(-1)).is_err());
    assert!(op(MirBinOp::Shr, int(1), int(-1)).is_err());
}

#[test]
fn bitwise_ops_use_infinite_twos_complement() {
    assert_eq!(op(MirBinOp::BitAnd, int(-1), int(0xff)), Ok(int(0xff)));
    assert_eq!(op(MirBinOp::BitOr, int(-256), int(0xff)), Ok(int(-1)));
    assert_eq!(op(MirBinOp::BitXor, int(-1), int(5)), Ok(int(-6)));
    assert_eq!(op(MirBinOp::BitAnd, Value::UInt(5), int(-1)), Ok(int(5)));
    assert_eq!(
        op(MirBinOp::BitXor, Value::UInt(u64::MAX), int(i64::MIN)),
        Err("numerus overflow".to_owned())
    );
}

#[test]
fn integer_float_comparison_is_exact_above_2_pow_53() {
    let span = Span::default();
    let big = int((1 << 53) + 1);
    let float = Value::Float((1u64 << 53) as f64);
    assert_eq!(
        compare_values(big.clone(), float.clone(), MirBinOp::Gt, span).unwrap(),
        Value::Bool(true)
    );
    assert!(!values_equal(&big, &float));
    assert!(values_equal(&int(1 << 53), &float));
    assert_eq!(cmp_int_float(3, 3.5), Some(std::cmp::Ordering::Less));
    assert_eq!(cmp_int_float(-3, -3.5), Some(std::cmp::Ordering::Greater));
    assert_eq!(cmp_int_float(1, f64::NAN), None);
    assert_eq!(
        cmp_int_float(i128::from(u64::MAX), f64::INFINITY),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_values(int(1), Value::Float(f64::NAN), MirBinOp::LtEq, span).unwrap(),
        Value::Bool(false)
    );
    assert!(!values_equal(&int(1), &Value::Float(f64::NAN)));
    // u64 above 2^63 against a float
    assert_eq!(
        compare_values(
            Value::UInt(u64::MAX),
            Value::Float(1.9e19),
            MirBinOp::Gt,
            span
        )
        .unwrap(),
        Value::Bool(false)
    );
}
