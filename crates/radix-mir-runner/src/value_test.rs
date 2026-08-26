//! Unit tests for runner value helpers (companion to `value.rs`).

use super::value::*;
use radix_lexer::Span;
use radix_mir::MirBinOp;
use radix_types::NumericWidth;

#[test]
fn bitwise_rejects_negative_signed_in_unsigned_context() {
    // Defense in depth: typecheck forbids signed/unsigned mixing upstream,
    // but the runner boundary must never reinterpret sign bits when
    // carriers mix.
    let span = Span::default();
    let error = int_bitwise_binop(Value::UInt(5), Value::Int(-1), MirBinOp::BitAnd, span)
        .expect_err("negative signed operand in unsigned bitwise context fails closed");
    assert!(
        format!("{error:?}").contains("bitwise operand requires numerus"),
        "unexpected error: {error:?}"
    );
}

#[test]
fn mixed_carrier_equality_and_keys_are_value_canonical() {
    assert!(values_equal(&Value::Int(1), &Value::UInt(1)));
    assert!(values_equal(&Value::UInt(1), &Value::Int(1)));
    assert!(!values_equal(&Value::Int(-1), &Value::UInt(u64::MAX)));
    let span = Span::default();
    assert_eq!(Value::Int(5).to_key(span).unwrap(), MapKey::UInt(5));
    assert_eq!(Value::UInt(5).to_key(span).unwrap(), MapKey::UInt(5));
    assert_eq!(Value::Int(-5).to_key(span).unwrap(), MapKey::Int(-5));
}

#[test]
fn mixed_integer_float_equality_uses_numeric_promotion() {
    assert!(values_equal(&Value::Int(1), &Value::Float(1.0)));
    assert!(values_equal(&Value::Float(1.0), &Value::UInt(1)));
    assert!(!values_equal(&Value::Int(42), &Value::Float(42.145)));
}

#[test]
fn scalar_get_ascii1_equals_textus_literal() {
    // `s.get(i)` yields `ascii<1>`; textus literals are heap `Str`. Equality
    // joins the carriers by character value per the Textus `accipe` contract.
    let span = Span::default();
    let ch = scalar_text_index("abc", 0, span).expect("in-range ASCII index");
    assert!(values_equal(&ch, &Value::Str("a".to_owned())));
    assert!(values_equal(&Value::Str("a".to_owned()), &ch));
    assert!(!values_equal(&ch, &Value::Str("b".to_owned())));
    // Multi-byte / non-ASCII textus is never equal to a one-byte carrier.
    assert!(!values_equal(&ch, &Value::Str("ab".to_owned())));
    assert!(!values_equal(&ch, &Value::Str("é".to_owned())));
}

#[test]
fn scalar_f32_addition_rounds_operands_and_result() {
    let span = Span::default();
    let result = add_values(
        Value::Float(0.1),
        Value::Float(0.2),
        NumericWidth::F32,
        span,
    )
    .expect("f32 addition");
    assert_eq!(result, Value::Float(f64::from(0.1_f32 + 0.2_f32)));

    // This distinguishes f32 arithmetic from f64 arithmetic followed by a
    // final cast: both operands must be rounded before the operation.
    let result = add_values(
        Value::Float(16_777_217.0),
        Value::Float(1.0),
        NumericWidth::F32,
        span,
    )
    .expect("f32 addition rounds operands");
    assert_eq!(result, Value::Float(f64::from(16_777_216.0_f32)));
}

#[test]
fn scalar_f64_addition_remains_double_width() {
    let result = add_values(
        Value::Float(0.1),
        Value::Float(0.2),
        NumericWidth::F64,
        Span::default(),
    )
    .expect("f64 addition");
    assert_eq!(result, Value::Float(0.1_f64 + 0.2_f64));
}
