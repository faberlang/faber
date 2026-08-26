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

#[test]
fn mixed_integer_float_ordering_joins_at_float_carrier() {
    // Ordering promotes mixed integer/float operands to the float carrier,
    // mirroring the `numeric_binop` arithmetic promotion (fractus wins) and
    // the Rust emitter's join cast `(a as f64) < b`.
    let span = Span::default();
    assert_eq!(
        compare_values(Value::Int(3), Value::Float(3.5), MirBinOp::Lt, span)
            .expect("int < fractus joins at fractus"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Float(3.5), Value::Int(3), MirBinOp::Gt, span)
            .expect("fractus > int joins at fractus"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::UInt(3), Value::Float(3.5), MirBinOp::Lt, span)
            .expect("uint < fractus joins at fractus"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Float(3.5), Value::UInt(4), MirBinOp::Lt, span)
            .expect("fractus < uint joins at fractus"),
        Value::Bool(true)
    );
    // Negative signed operands widen to f64 before comparing.
    assert_eq!(
        compare_values(Value::Int(-1), Value::Float(0.5), MirBinOp::Lt, span)
            .expect("negative int < fractus joins at fractus"),
        Value::Bool(true)
    );
    // The join equality boundary: 3 <= 3.0 holds, 3 < 3.0 does not.
    assert_eq!(
        compare_values(Value::Int(3), Value::Float(3.0), MirBinOp::LtEq, span)
            .expect("int <= fractus joins at fractus"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Int(3), Value::Float(3.0), MirBinOp::Lt, span)
            .expect("int < fractus joins at fractus"),
        Value::Bool(false)
    );
    // Same-type ordering paths are untouched by the new arms.
    assert_eq!(
        compare_values(Value::Int(3), Value::Int(4), MirBinOp::Lt, span)
            .expect("int < int stays exact"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Float(3.0), Value::Float(4.0), MirBinOp::Lt, span)
            .expect("fractus < fractus stays exact"),
        Value::Bool(true)
    );
}

#[test]
fn ordering_and_arithmetic_accept_the_same_mixed_operand_mixes() {
    // Consistency law: `a < b` and `a + b` accept the same mixed int/float
    // and uint/float operand pairs on identical shapes.
    let span = Span::default();
    let pairs = [
        (Value::Int(3), Value::Float(3.5)),
        (Value::UInt(3), Value::Float(3.5)),
        (Value::Float(3.5), Value::Int(3)),
        (Value::Float(3.5), Value::UInt(3)),
        (Value::Int(-1), Value::Float(0.5)),
    ];
    for (left, right) in pairs {
        let arithmetic = add_values(left.clone(), right.clone(), NumericWidth::F64, span);
        let ordering = compare_values(left, right, MirBinOp::Lt, span);
        assert!(
            arithmetic.is_ok(),
            "arithmetic rejected a mix ordering accepts: {arithmetic:?}"
        );
        assert!(
            ordering.is_ok(),
            "ordering rejected a mix arithmetic accepts: {ordering:?}"
        );
    }
}
