//! Unit tests for runner value helpers (companion to `value.rs`).

use super::value::*;
use radix_lexer::Span;
use radix_mir::MirBinOp;
use radix_types::NumericWidth;

#[test]
fn bitwise_mixes_signed_and_unsigned_carriers_on_infinite_twos_complement() {
    // d11-6 ruling 19b supersedes the earlier fail-closed pin: `&` is defined
    // on infinite two's-complement integers, so `5 & -1` is `5` whichever
    // carrier holds each operand.
    let span = Span::default();
    let result = int_bitwise_binop(
        Value::UInt(5),
        Value::Int(-1),
        MirBinOp::BitAnd,
        NumericWidth::I64,
        span,
    )
    .expect("mixed carriers are exact integers");
    assert!(values_equal(&result, &Value::Int(5)));
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
fn scaled_decimal_join_carriers_compare_by_value() {
    // The lowerer scales integer peers to the d64 carrier before this
    // helper sees them. These are the resulting d64 carriers, not raw integer
    // units, so comparison stays numeric rather than carrier-blind.
    let span = Span::default();
    assert_eq!(
        compare_values(
            Value::Int(4_200_000_000),
            Value::Int(1_000_000_000),
            MirBinOp::Gt,
            span,
        )
        .expect("joined decimal carriers compare"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(
            Value::Int(42_000_000),
            Value::Int(10_000_000_000),
            MirBinOp::Gt,
            span,
        )
        .expect("joined decimal/integer carriers compare"),
        Value::Bool(false)
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

/// IEEE 754 ordered comparison (operator ruling 2026-09-24): every ordered
/// predicate against NaN is false and never traps; `≡` is false and `≠` true.
#[test]
fn float_compare_with_nan_is_ieee_ordered_false() {
    let span = Span::default();
    let nan = f64::NAN;
    let pairs = [
        (Value::Float(nan), Value::Float(1.0)),
        (Value::Float(1.0), Value::Float(nan)),
        (Value::Float(nan), Value::Float(nan)),
        (Value::Float(nan), Value::Int(1)),
        (Value::Int(1), Value::Float(nan)),
        (Value::Float(nan), Value::UInt(1)),
        (Value::UInt(1), Value::Float(nan)),
    ];
    for (left, right) in pairs {
        for op in [MirBinOp::Lt, MirBinOp::Gt, MirBinOp::LtEq, MirBinOp::GtEq] {
            let result = compare_values(left.clone(), right.clone(), op, span)
                .unwrap_or_else(|error| panic!("{left:?} {op:?} {right:?} trapped: {error:?}"));
            assert_eq!(result, Value::Bool(false), "{left:?} {op:?} {right:?}");
        }
        assert!(!values_equal(&left, &right), "{left:?} ≡ {right:?}");
    }
}

/// IEEE 754-2019 `minimum`/`maximum` (operator ruling 2026-09-24): NaN
/// propagates from either operand, and -0.0 orders below +0.0.
#[test]
fn float_minimum_maximum_propagate_nan_and_order_signed_zero() {
    let nan = f64::NAN;
    for (a, b) in [(nan, 1.0), (1.0, nan), (nan, nan)] {
        assert!(ieee_minimum(a, b).is_nan(), "minimum({a}, {b})");
        assert!(ieee_maximum(a, b).is_nan(), "maximum({a}, {b})");
    }
    for (a, b) in [(-0.0, 0.0), (0.0, -0.0)] {
        let min = ieee_minimum(a, b);
        let max = ieee_maximum(a, b);
        assert!(
            min == 0.0 && min.is_sign_negative(),
            "minimum({a}, {b}) = {min}"
        );
        assert!(
            max == 0.0 && max.is_sign_positive(),
            "maximum({a}, {b}) = {max}"
        );
    }
    assert_eq!(ieee_minimum(2.0, 3.0), 2.0);
    assert_eq!(ieee_maximum(2.0, 3.0), 3.0);
    assert_eq!(ieee_minimum(f64::NEG_INFINITY, -1.0), f64::NEG_INFINITY);
    assert_eq!(ieee_maximum(f64::INFINITY, 1.0), f64::INFINITY);
}

/// IEEE 754 totalOrder (operator ruling 2026-09-24): sorting floats orders
/// -NaN < -∞ < … < -0.0 < +0.0 < … < +∞ < +NaN.
#[test]
fn float_sort_order_is_ieee_total_order() {
    let neg_nan = -f64::NAN;
    let mut values = vec![
        Value::Float(3.5),
        Value::Float(f64::NAN),
        Value::Float(1.0),
        Value::Float(0.0),
        Value::Float(f64::NEG_INFINITY),
        Value::Float(neg_nan),
        Value::Float(-0.0),
        Value::Float(f64::INFINITY),
        Value::Float(-2.0),
    ];
    values.sort_by(compare_sort_values);
    let bits: Vec<u64> = values
        .iter()
        .map(|value| match value {
            Value::Float(v) => v.to_bits(),
            other => panic!("non-float {other:?}"),
        })
        .collect();
    let expected: Vec<u64> = [
        neg_nan,
        f64::NEG_INFINITY,
        -2.0,
        -0.0,
        0.0,
        1.0,
        3.5,
        f64::INFINITY,
        f64::NAN,
    ]
    .iter()
    .map(|v| v.to_bits())
    .collect();
    assert_eq!(bits, expected);
}

#[test]
fn tuple_value_keys_by_its_parts_and_reads_back() {
    // D3.2: a composite `iuncta` key hashes by value, so an equal tuple
    // built elsewhere finds the same entry.
    let span = Span::default();
    let key = Value::Tuple(vec![Value::Str("east".to_owned()), Value::Int(2024)])
        .to_key(span)
        .expect("tuple of hashable parts is a key");
    assert_eq!(
        key,
        MapKey::Tuple(vec![MapKey::Str("east".to_owned()), MapKey::UInt(2024)])
    );
    assert!(matches!(key.to_value(), Value::Tuple(parts) if parts.len() == 2));
    assert!(
        Value::Tuple(vec![Value::Float(1.5)]).to_key(span).is_err(),
        "a fractus part is not a key"
    );
}

fn field_map(
    pairs: &[(radix_lexer::Symbol, Value)],
) -> std::rc::Rc<std::cell::RefCell<rustc_hash::FxHashMap<radix_lexer::Symbol, Value>>> {
    std::rc::Rc::new(std::cell::RefCell::new(pairs.iter().cloned().collect()))
}

#[test]
fn record_displays_fields_in_written_order() {
    let mut interner = radix_lexer::Interner::new();
    let (zed, alpha, mid) = (
        interner.intern("zed"),
        interner.intern("alpha"),
        interner.intern("mid"),
    );
    let record = Value::Record(
        field_map(&[
            (alpha, Value::Int(1)),
            (mid, Value::Int(2)),
            (zed, Value::Int(3)),
        ]),
        std::rc::Rc::from([zed, alpha, mid]),
    );
    let empty = rustc_hash::FxHashMap::default();
    let text = record
        .display(&interner, FieldOrder::new(&empty, &empty), Span::default())
        .unwrap();
    assert_eq!(text, r#"{"zed": 3, "alpha": 1, "mid": 2}"#);
}

#[test]
fn enum_variant_displays_payload_in_declaration_order() {
    let mut interner = radix_lexer::Interner::new();
    let (x, y, z) = (
        interner.intern("x"),
        interner.intern("y"),
        interner.intern("z"),
    );
    let variant_def = radix_hir::DefId(4097);
    let variant = Value::EnumVariant(
        variant_def,
        field_map(&[(x, Value::Int(1)), (y, Value::Int(2)), (z, Value::Int(3))]),
    );
    let structs = rustc_hash::FxHashMap::default();
    let mut variants = rustc_hash::FxHashMap::default();
    variants.insert(variant_def, vec![z, x, y]);
    let text = variant
        .display(
            &interner,
            FieldOrder::new(&structs, &variants),
            Span::default(),
        )
        .unwrap();
    assert_eq!(text, r#"{"z": 3, "x": 1, "y": 2}"#);
}

#[test]
fn f32_interpolation_matches_print_at_declared_width() {
    let interner = radix_lexer::Interner::new();
    let empty = rustc_hash::FxHashMap::default();
    let order = FieldOrder::new(&empty, &empty);
    let value = Value::Float(f64::from(200.0_f32 / 3.0_f32));
    let printed = value
        .display_typed(&interner, order, Span::default(), Some(NumericWidth::F32))
        .unwrap();
    assert_eq!(printed, "66.666664");
    let interpolated = format_template(
        "§",
        &[value.clone()],
        &[Some(NumericWidth::F32)],
        &interner,
        order,
        Span::default(),
    )
    .unwrap();
    assert_eq!(interpolated, printed);
    // f64 (undeclared width) keeps full digits; integral f32 keeps `N.0`.
    assert_eq!(display_float_at_width(5.0, Some(NumericWidth::F32)), "5.0");
    assert_eq!(
        format_template("§", &[value], &[None], &interner, order, Span::default()).unwrap(),
        "66.66666412353516"
    );
}

#[test]
fn f32_struct_field_and_list_display_at_declared_width() {
    let mut interner = radix_lexer::Interner::new();
    let field = interner.intern("a");
    let def = radix_hir::DefId(9);
    let structs = rustc_hash::FxHashMap::default();
    let variants = rustc_hash::FxHashMap::default();
    let mut widths = FieldFloatWidths::default();
    widths.insert((def, field), NumericWidth::F32);
    let order = FieldOrder {
        structs: &structs,
        variants: &variants,
        float_widths: &widths,
    };
    let value = Value::Float(f64::from(200.0_f32 / 3.0_f32));
    let strukt = Value::Struct(def, field_map(&[(field, value.clone())]));
    assert_eq!(
        strukt.display(&interner, order, Span::default()).unwrap(),
        r#"{"a": 66.666664}"#
    );
    let list = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![value])));
    assert_eq!(
        list.display_typed(&interner, order, Span::default(), Some(NumericWidth::F32))
            .unwrap(),
        "[66.666664]"
    );
}
