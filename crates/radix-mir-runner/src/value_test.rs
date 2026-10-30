//! Unit tests for runner value helpers (companion to `value.rs`).
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::useless_conversion,
    clippy::float_cmp
)]

use super::value::*;
use radix_lexer::Span;
use radix_mir::MirBinOp;
use radix_types::NumericWidth;
use std::sync::Arc;
use std::{cell::RefCell, rc::Rc};

impl TensorValue {
    /// Test constructor: fixture shapes always have representable strides.
    pub(crate) fn contiguous(data: Vec<Value>, shape: Vec<i64>) -> Self {
        Self::try_contiguous(data, shape)
            .expect("fixture tensor shape must have representable strides")
    }
}

#[test]
fn affine_prefix_view_keeps_shared_backing_without_weakening_bounds_checks() {
    let base = TensorValue::contiguous(
        vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
        vec![2, 2],
    );
    let view = base
        .view(vec![1, 2], vec![2, 1], 0)
        .expect("the first row is a valid shared view");

    assert!(!view.has_contiguous_backing_mismatch());
    assert_eq!(
        view.logical_values().expect("view reads its backing"),
        vec![Value::Int(1), Value::Int(2)]
    );
    let materialized = TensorValue::contiguous(
        view.logical_values().expect("materialize logical view"),
        view.shape.clone(),
    );
    base.set_value_at(0, Value::Int(9)).expect("alias write");
    assert_eq!(
        view.logical_values().expect("view observes alias write"),
        vec![Value::Int(9), Value::Int(2)]
    );
    assert_eq!(
        materialized
            .logical_values()
            .expect("owned copy stays valid"),
        vec![Value::Int(1), Value::Int(2)]
    );

    // The shared backing is gone short: a view whose layout no longer fits it.
    let short = TensorValue::contiguous(vec![Value::Int(9)], vec![1]);
    let view = short
        .view_allowing_out_of_bounds(vec![1, 2], vec![2, 1], 0)
        .expect("an out-of-bounds view over the short backing");
    assert!(view.has_contiguous_backing_mismatch());
    assert!(view.logical_values().is_none());

    let malformed_owner = TensorValue::contiguous(vec![Value::Int(1)], vec![2]);
    assert!(malformed_owner.has_contiguous_backing_mismatch());
}

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
fn mixed_carrier_list_equality_is_element_canonical() {
    let int_cells = Value::Array(Rc::new(RefCell::new(vec![
        Value::Int(222),
        Value::Int(173),
    ])));
    let uint_cells = Value::Array(Rc::new(RefCell::new(vec![
        Value::UInt(222),
        Value::UInt(173),
    ])));
    assert!(values_equal(&int_cells, &uint_cells));
    assert!(values_equal(&uint_cells, &int_cells));
    assert!(!values_equal(
        &Value::Array(Rc::new(RefCell::new(vec![Value::Int(1)]))),
        &Value::Array(Rc::new(RefCell::new(vec![Value::UInt(2)])))
    ));
    assert!(!values_equal(
        &Value::Array(Rc::new(RefCell::new(vec![Value::Int(1)]))),
        &Value::Array(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])))
    ));
    let nested_int = Value::Array(Rc::new(RefCell::new(vec![Value::Array(Rc::new(
        RefCell::new(vec![Value::Int(1)]),
    ))])));
    let nested_uint = Value::Array(Rc::new(RefCell::new(vec![Value::Array(Rc::new(
        RefCell::new(vec![Value::UInt(1)]),
    ))])));
    assert!(values_equal(&nested_int, &nested_uint));
    assert!(values_equal(
        &Value::Tuple(vec![Value::Int(1), Value::Str("a".to_owned())]),
        &Value::Tuple(vec![Value::UInt(1), Value::Str("a".to_owned())])
    ));
    let vector_int = Value::Vector(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])));
    let vector_uint = Value::Vector(Rc::new(RefCell::new(vec![Value::UInt(1), Value::UInt(2)])));
    assert!(values_equal(&vector_int, &vector_uint));
    assert!(values_equal(&vector_uint, &vector_int));
    let nan_array = Value::Array(Rc::new(RefCell::new(vec![Value::Float(f64::NAN)])));
    assert!(!values_equal(&nan_array, &nan_array));
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
/// predicate against NaN is false and never traps; `≡` is false and `≢` true.
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
) -> std::rc::Rc<std::cell::RefCell<handroll::FxHashMap<radix_lexer::Symbol, Value>>> {
    std::rc::Rc::new(std::cell::RefCell::new(pairs.iter().cloned().collect()))
}

#[test]
fn class_value_map_keys_compare_by_fields_not_allocation() {
    let mut interner = radix_lexer::Interner::new();
    let name = interner.intern("name");
    let def = radix_hir::DefId(6301);
    let first = Value::Struct(def, field_map(&[(name, Value::Str("a".to_owned()))]));
    let second = Value::Struct(def, field_map(&[(name, Value::Str("a".to_owned()))]));
    let first_key = first.to_key(Span::default()).expect("hashable class key");
    let second_key = second.to_key(Span::default()).expect("equal class key");
    assert_eq!(first_key, second_key);

    let mut map = handroll::FxHashMap::default();
    map.insert(first_key, Value::Int(1));
    assert_eq!(map.get(&second_key), Some(&Value::Int(1)));
    assert!(matches!(
        second_key.to_value(),
        Value::Struct(found, fields)
            if found == def && fields.borrow().get(&name) == Some(&Value::Str("a".to_owned()))
    ));
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
    let empty = handroll::FxHashMap::default();
    let text = record
        .display(&interner, FieldOrder::new(&empty, &empty), Span::default())
        .unwrap();
    assert_eq!(text, r#"{ zed = 3, alpha = 1, mid = 2 }"#);
}

#[test]
fn record_with_text_field_quotes_it() {
    let mut interner = radix_lexer::Interner::new();
    let (name, n) = (interner.intern("name"), interner.intern("n"));
    let record = Value::Record(
        field_map(&[(name, Value::Str("x".to_owned())), (n, Value::Int(3))]),
        std::rc::Rc::from([name, n]),
    );
    let empty = handroll::FxHashMap::default();
    let text = record
        .display(&interner, FieldOrder::new(&empty, &empty), Span::default())
        .unwrap();
    assert_eq!(text, r#"{ name = "x", n = 3 }"#);
}

#[test]
fn empty_record_prints_empty_braces() {
    let interner = radix_lexer::Interner::new();
    let record = Value::Record(
        field_map(&[]),
        std::rc::Rc::from(Vec::<radix_lexer::Symbol>::new()),
    );
    let empty = handroll::FxHashMap::default();
    let text = record
        .display(&interner, FieldOrder::new(&empty, &empty), Span::default())
        .unwrap();
    assert_eq!(text, "{}");
}

#[test]
fn record_through_dynamic_carrier_keeps_json_shape() {
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
    let empty = handroll::FxHashMap::default();
    let text = record
        .display(
            &interner,
            FieldOrder::new(&empty, &empty).with_dynamic(true),
            Span::default(),
        )
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
    let structs = handroll::FxHashMap::default();
    let mut variants = handroll::FxHashMap::default();
    variants.insert(variant_def, vec![z, x, y]);
    let mut names = DisplayNames::default();
    names.insert(variant_def, "Nuntius.Pulsus".to_owned());
    let text = variant
        .display(
            &interner,
            FieldOrder::new(&structs, &variants).with_names(&names),
            Span::default(),
        )
        .unwrap();
    assert_eq!(text, "Nuntius.Pulsus { z = 3, x = 1, y = 2 }");
}

/// Metadata for a genus `Involucrum { p: Punctum, gradus: lista, titulus }`.
struct GenusFixture {
    interner: radix_lexer::Interner,
    structs: StructFieldOrder,
    names: DisplayNames,
    punctum: radix_hir::DefId,
    involucrum: radix_hir::DefId,
    vacuus: radix_hir::DefId,
}

fn genus_fixture() -> GenusFixture {
    let mut interner = radix_lexer::Interner::new();
    let (x, nomen, p, gradus, titulus) = (
        interner.intern("x"),
        interner.intern("nomen"),
        interner.intern("p"),
        interner.intern("gradus"),
        interner.intern("titulus"),
    );
    let (punctum, involucrum, vacuus) = (
        radix_hir::DefId(5001),
        radix_hir::DefId(5002),
        radix_hir::DefId(5003),
    );
    let mut structs = StructFieldOrder::default();
    structs.insert(punctum, vec![x, nomen]);
    structs.insert(involucrum, vec![p, gradus, titulus]);
    structs.insert(vacuus, Vec::new());
    let mut names = DisplayNames::default();
    names.insert(punctum, "Punctum".to_owned());
    names.insert(involucrum, "Involucrum".to_owned());
    names.insert(vacuus, "Vacuus".to_owned());
    GenusFixture {
        interner,
        structs,
        names,
        punctum,
        involucrum,
        vacuus,
    }
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned().into())
}

#[test]
fn genus_displays_its_construction_literal() {
    let fx = genus_fixture();
    let variants = StructFieldOrder::default();
    let order = FieldOrder::new(&fx.structs, &variants).with_names(&fx.names);
    let sym = |name: &str| fx.interner.lookup(name).expect("interned");
    // Inserted in scrambled order: display is declaration order, not hash order.
    let punctum = Value::Struct(
        fx.punctum,
        field_map(&[(sym("nomen"), text("hello")), (sym("x"), Value::Int(1))]),
    );
    assert_eq!(
        punctum
            .display(&fx.interner, order, Span::default())
            .unwrap(),
        r#"Punctum { x = 1, nomen = "hello" }"#
    );
    let list = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        Value::Int(0),
        Value::Int(1),
        Value::Int(2),
    ])));
    let involucrum = Value::Struct(
        fx.involucrum,
        field_map(&[
            (sym("titulus"), text("t")),
            (sym("gradus"), list),
            (sym("p"), punctum),
        ]),
    );
    assert_eq!(
        involucrum
            .display(&fx.interner, order, Span::default())
            .unwrap(),
        r#"Involucrum { p = Punctum { x = 1, nomen = "hello" }, gradus = [0, 1, 2], titulus = "t" }"#
    );
}

#[test]
fn genus_without_fields_displays_empty_braces() {
    let fx = genus_fixture();
    let variants = StructFieldOrder::default();
    let order = FieldOrder::new(&fx.structs, &variants).with_names(&fx.names);
    let vacuus = Value::Struct(fx.vacuus, field_map(&[]));
    assert_eq!(
        vacuus
            .display(&fx.interner, order, Span::default())
            .unwrap(),
        "Vacuus {}"
    );
}

#[test]
fn bare_text_stays_unquoted_but_genus_text_fields_quote_escapes() {
    let fx = genus_fixture();
    let variants = StructFieldOrder::default();
    let order = FieldOrder::new(&fx.structs, &variants).with_names(&fx.names);
    assert_eq!(
        text("hello")
            .display(&fx.interner, order, Span::default())
            .unwrap(),
        "hello"
    );
    let nomen = fx.interner.lookup("nomen").expect("interned");
    let x = fx.interner.lookup("x").expect("interned");
    let punctum = Value::Struct(
        fx.punctum,
        field_map(&[(x, Value::Int(-3)), (nomen, text("a \"b\"\n"))]),
    );
    assert_eq!(
        punctum
            .display(&fx.interner, order, Span::default())
            .unwrap(),
        r#"Punctum { x = -3, nomen = "a \"b\"\n" }"#
    );
}

#[test]
fn discretio_variant_displays_qualified_with_declared_fields() {
    let mut interner = radix_lexer::Interner::new();
    let (x, y) = (interner.intern("x"), interner.intern("y"));
    let (pulsus, finis) = (radix_hir::DefId(6001), radix_hir::DefId(6002));
    let structs = StructFieldOrder::default();
    let mut variants = StructFieldOrder::default();
    variants.insert(pulsus, vec![x, y]);
    variants.insert(finis, Vec::new());
    let mut names = DisplayNames::default();
    names.insert(pulsus, "Nuntius.Pulsus".to_owned());
    names.insert(finis, "Nuntius.Finis".to_owned());
    let order = FieldOrder::new(&structs, &variants).with_names(&names);
    let payload = Value::EnumVariant(pulsus, field_map(&[(y, text("hello")), (x, Value::Int(1))]));
    assert_eq!(
        payload.display(&interner, order, Span::default()).unwrap(),
        r#"Nuntius.Pulsus { x = 1, y = "hello" }"#
    );
    // D2.8 (pr3): a payload-free variant has no field list, so no braces.
    let unit = Value::EnumVariant(finis, field_map(&[]));
    assert_eq!(
        unit.display(&interner, order, Span::default()).unwrap(),
        "Nuntius.Finis"
    );
}

#[test]
fn unit_only_enum_members_display_as_type_dot_member() {
    // An all-unit `discretio` and an `ordo` print alike (pr3): the lowering
    // registers a name for every enum variant, so neither needs a HIR flag.
    let interner = radix_lexer::Interner::new();
    let structs = StructFieldOrder::default();
    let variants = StructFieldOrder::default();
    let mut names = DisplayNames::default();
    names.insert(radix_hir::DefId(6101), "Signum.Alpha".to_owned());
    names.insert(radix_hir::DefId(6102), "Color.Red".to_owned());
    let order = FieldOrder::new(&structs, &variants).with_names(&names);
    for (def, shown) in [(6101, "Signum.Alpha"), (6102, "Color.Red")] {
        let member = Value::EnumVariant(radix_hir::DefId(def), field_map(&[]));
        assert_eq!(
            member.display(&interner, order, Span::default()).unwrap(),
            shown
        );
    }
}

#[test]
fn composites_quote_text_chars_and_ascii_but_top_level_scalars_do_not() {
    let fx = genus_fixture();
    let variants = StructFieldOrder::default();
    let order = FieldOrder::new(&fx.structs, &variants).with_names(&fx.names);
    let show = |value: &Value| value.display(&fx.interner, order, Span::default()).unwrap();
    let list = |items: Vec<Value>| Value::Array(std::rc::Rc::new(std::cell::RefCell::new(items)));
    // Top level: a scalar prints its value.
    assert_eq!(show(&text("hello")), "hello");
    assert_eq!(show(&Value::Char('q')), "q");
    // Inside a composite: text, char and ascii are quoted as in source.
    assert_eq!(show(&list(vec![text("a"), text("b")])), r#"["a", "b"]"#);
    assert_eq!(
        show(&list(vec![Value::Char('x'), Value::Char('y')])),
        "['x', 'y']"
    );
    // An ascii-carried value is text unless its static type is a `littera`
    // (pr4): the byte count never decides.
    assert_eq!(show(&list(vec![Value::Ascii(b'k')])), r#"["k"]"#);
    let chars = order.with_char_text(true);
    assert_eq!(
        list(vec![Value::Ascii(b'k'), Value::Char('z')])
            .display(&fx.interner, chars, Span::default())
            .unwrap(),
        "['k', 'z']"
    );
    assert_eq!(
        show(&list(vec![list(vec![text("n")]), list(Vec::new())])),
        r#"[["n"], []]"#
    );
    assert_eq!(
        show(&list(vec![Value::Bool(true), Value::Nil])),
        "[verum, nihil]"
    );
}

#[test]
fn genus_fields_quote_chars_and_lists_and_show_options_bare() {
    let mut interner = radix_lexer::Interner::new();
    let (c, names_field, opt, ok) = (
        interner.intern("c"),
        interner.intern("names"),
        interner.intern("opt"),
        interner.intern("ok"),
    );
    let def = radix_hir::DefId(5101);
    let mut structs = StructFieldOrder::default();
    structs.insert(def, vec![ok, opt, c, names_field]);
    let variants = StructFieldOrder::default();
    let mut names = DisplayNames::default();
    names.insert(def, "Rec".to_owned());
    let order = FieldOrder::new(&structs, &variants).with_names(&names);
    let names_list = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        text("a"),
        text("b"),
    ])));
    let present = Value::Struct(
        def,
        field_map(&[
            (ok, Value::Bool(true)),
            (opt, Value::Int(3)),
            (c, Value::Char('q')),
            (names_field, names_list),
        ]),
    );
    assert_eq!(
        present.display(&interner, order, Span::default()).unwrap(),
        r#"Rec { ok = verum, opt = 3, c = 'q', names = ["a", "b"] }"#
    );
    let absent = Value::Struct(
        def,
        field_map(&[
            (ok, Value::Bool(false)),
            (opt, Value::Nil),
            (c, Value::Char('q')),
            (names_field, Value::Array(Default::default())),
        ]),
    );
    assert_eq!(
        absent.display(&interner, order, Span::default()).unwrap(),
        "Rec { ok = falsum, opt = nihil, c = 'q', names = [] }"
    );
}

#[test]
fn dynamic_carrier_list_keeps_unquoted_elements() {
    // A value shown through `valor`/`json` keeps its JSON-shaped display.
    let fx = genus_fixture();
    let variants = StructFieldOrder::default();
    let order = FieldOrder::new(&fx.structs, &variants)
        .with_names(&fx.names)
        .with_dynamic(true);
    let list = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        text("a"),
        text("b"),
    ])));
    assert_eq!(
        list.display(&fx.interner, order, Span::default()).unwrap(),
        "[a, b]"
    );
}

#[test]
fn unnamed_variant_and_dynamic_struct_keep_json_shaped_display() {
    // A variant with no registered construction-literal name (defensive
    // fallback), and a genus shown through a `valor` carrier (a dynamic
    // object), are not covered by D2.8.
    let fx = genus_fixture();
    let variants = StructFieldOrder::default();
    let order = FieldOrder::new(&fx.structs, &variants).with_names(&fx.names);
    let unnamed = Value::EnumVariant(radix_hir::DefId(7001), field_map(&[]));
    assert_eq!(
        unnamed
            .display(&fx.interner, order, Span::default())
            .unwrap(),
        "{}"
    );
    let sym = |name: &str| fx.interner.lookup(name).expect("interned");
    let punctum = Value::Struct(
        fx.punctum,
        field_map(&[(sym("nomen"), text("hello")), (sym("x"), Value::Int(1))]),
    );
    assert_eq!(
        punctum
            .display(&fx.interner, order.with_dynamic(true), Span::default())
            .unwrap(),
        r#"{"x": 1, "nomen": hello}"#
    );
}

#[test]
fn f32_interpolation_matches_print_at_declared_width() {
    let interner = radix_lexer::Interner::new();
    let empty = handroll::FxHashMap::default();
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
        &[false],
        &interner,
        order,
        Span::default(),
    )
    .unwrap();
    assert_eq!(interpolated, printed);
    // f64 (undeclared width) keeps full digits; integral f32 keeps `N.0`.
    assert_eq!(display_float_at_width(5.0, Some(NumericWidth::F32)), "5.0");
    assert_eq!(
        format_template(
            "§",
            &[value],
            &[None],
            &[false],
            &interner,
            order,
            Span::default()
        )
        .unwrap(),
        "66.66666412353516"
    );
}

#[test]
fn f32_struct_field_and_list_display_at_declared_width() {
    let mut interner = radix_lexer::Interner::new();
    let field = interner.intern("a");
    let def = radix_hir::DefId(9);
    let structs = handroll::FxHashMap::default();
    let variants = handroll::FxHashMap::default();
    let mut widths = FieldFloatWidths::default();
    widths.insert((def, field), NumericWidth::F32);
    let mut names = DisplayNames::default();
    names.insert(def, "Mensura".to_owned());
    let order = FieldOrder {
        float_widths: &widths,
        names: &names,
        ..FieldOrder::new(&structs, &variants)
    };
    let value = Value::Float(f64::from(200.0_f32 / 3.0_f32));
    let strukt = Value::Struct(def, field_map(&[(field, value.clone())]));
    assert_eq!(
        strukt.display(&interner, order, Span::default()).unwrap(),
        "Mensura { a = 66.666664 }"
    );
    let list = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![value])));
    assert_eq!(
        list.display_typed(&interner, order, Span::default(), Some(NumericWidth::F32))
            .unwrap(),
        "[66.666664]"
    );
}

#[test]
fn d64_carrier_displays_as_shortest_decimal_by_declared_type() {
    // D2.6: a stored d64 is `Int(carrier)`; the static type picks the form.
    let d64 = Some(NumericWidth::D64);
    for (carrier, shown) in [
        (420_000_000_i64, "4.2"),
        (1, "0.00000001"),
        (-150_000_000, "-1.5"),
        (10_000_000_000, "100"),
        (i64::MAX, "92233720368.54775807"),
        (0, "0"),
    ] {
        assert_eq!(display_int_at_width(carrier, d64), shown);
    }
    // An undeclared integer keeps its digits.
    assert_eq!(display_int_at_width(420_000_000, None), "420000000");
}

#[test]
fn d64_display_threads_through_lists_fields_and_templates() {
    let mut interner = radix_lexer::Interner::new();
    let field = interner.intern("price");
    let def = radix_hir::DefId(11);
    let structs = handroll::FxHashMap::default();
    let variants = handroll::FxHashMap::default();
    let mut widths = FieldFloatWidths::default();
    widths.insert((def, field), NumericWidth::D64);
    let mut names = DisplayNames::default();
    names.insert(def, "Pretium".to_owned());
    let order = FieldOrder {
        float_widths: &widths,
        names: &names,
        ..FieldOrder::new(&structs, &variants)
    };
    let carrier = Value::Int(420_000_000);
    let strukt = Value::Struct(def, field_map(&[(field, carrier.clone())]));
    assert_eq!(
        strukt.display(&interner, order, Span::default()).unwrap(),
        "Pretium { price = 4.2 }"
    );
    let list = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        carrier.clone(),
        Value::Int(-50_000_000),
    ])));
    assert_eq!(
        list.display_typed(&interner, order, Span::default(), Some(NumericWidth::D64))
            .unwrap(),
        "[4.2, -0.5]"
    );
    assert_eq!(
        format_template(
            "§ / §",
            &[carrier, Value::Dec(25, 9)],
            &[Some(NumericWidth::D64), Some(NumericWidth::D64)],
            &[false, false],
            &interner,
            order,
            Span::default()
        )
        .unwrap(),
        "4.2 / 0.000000025"
    );
}

fn wrap(op: MirBinOp, left: i64, right: i64, width: NumericWidth) -> Value {
    modular_word_numeric_binop(
        Value::Int(left),
        Value::Int(right),
        op,
        width,
        Span::default(),
    )
    .expect("wrapping arithmetic")
}

#[test]
fn modular_origin_zero_divisor_reports_the_modular_issue_before_exact_dispatch() {
    // Range-based typing can give `a / b` on two `modulus<W>` operands a plain
    // integer result, so the modular result carrier is absent. The divisor's
    // zero is still modular, identified by `issue` rather than prose.
    let span = Span::default();
    for op in [MirBinOp::Div, MirBinOp::Mod] {
        for zero in [Value::Int(0), Value::UInt(0)] {
            let error = modular_zero_divisor(op, &zero, true, span)
                .expect("a modular-origin zero divisor traps");
            assert_eq!(error.issue, "modular_division_by_zero");
            assert_eq!(error.span, span);
        }
    }
}

#[test]
fn modular_origin_nonzero_divisor_stays_on_the_exact_integer_path() {
    let span = Span::default();
    // A nonzero divisor is not this guard's business: the exact value is kept
    // and only the store boundary reduces it (300 / 2 stays 150, not 22).
    for op in [MirBinOp::Div, MirBinOp::Mod] {
        assert!(modular_zero_divisor(op, &Value::Int(2), true, span).is_none());
        assert!(modular_zero_divisor(op, &Value::UInt(2), true, span).is_none());
    }
    // Non-modular origins keep the checked `numerus` diagnostic, and operators
    // other than `/` and `%` never consult this guard.
    assert!(modular_zero_divisor(MirBinOp::Div, &Value::Int(0), false, span).is_none());
    assert!(modular_zero_divisor(MirBinOp::Add, &Value::Int(0), true, span).is_none());
}

#[test]
fn signed_wrapping_words_are_the_ring_with_signed_representatives() {
    // D11.8: wrapping<i8> is Z/2^8 with representatives in [-128, 127].
    assert!(values_equal(
        &wrap(MirBinOp::Add, 127, 1, NumericWidth::I8),
        &Value::Int(-128)
    ));
    assert!(values_equal(
        &wrap(MirBinOp::Sub, -128, 1, NumericWidth::I8),
        &Value::Int(127)
    ));
    assert!(values_equal(
        &wrap(MirBinOp::Mul, 100, 3, NumericWidth::I8),
        &Value::Int(44)
    ));
    // MIN / -1 wraps to MIN; `/` and `%` floor on the representatives.
    assert!(values_equal(
        &wrap(MirBinOp::Div, -128, -1, NumericWidth::I8),
        &Value::Int(-128)
    ));
    assert!(values_equal(
        &wrap(MirBinOp::Div, -7, 2, NumericWidth::I8),
        &Value::Int(-4)
    ));
    assert!(values_equal(
        &wrap(MirBinOp::Mod, -7, 2, NumericWidth::I8),
        &Value::Int(1)
    ));
    assert!(values_equal(
        &wrap(MirBinOp::Div, -7, -2, NumericWidth::I8),
        &Value::Int(3)
    ));
    assert!(values_equal(
        &wrap(MirBinOp::Mod, -7, -2, NumericWidth::I8),
        &Value::Int(-1)
    ));
    assert!(values_equal(
        &wrap(MirBinOp::Div, i64::MIN, -1, NumericWidth::I64),
        &Value::Int(i64::MIN)
    ));
    let zero = modular_word_numeric_binop(
        Value::Int(1),
        Value::Int(0),
        MirBinOp::Div,
        NumericWidth::I32,
        Span::default(),
    );
    assert!(zero.is_err(), "division by zero traps");
}

/// Run `operand op divisor` through the runner's dispatch, with the operand's
/// static type and the binary value's declared type each either a `modulus<W>`
/// or the exact integer carrier of the same width.
fn runner_binop(
    width: NumericWidth,
    modular_operand: bool,
    modular_result: bool,
    operand: i64,
    divisor: i64,
    op: MirBinOp,
) -> Result<Value, crate::RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram, MirTemp,
        MirTempId, MirType, MirValidationContext,
    };
    use radix_types::{Primitive, Type, TypeTable};

    let mut types = TypeTable::new();
    let modular = types.intern(Type::ModularWord(width));
    let exact = types.sized_numeric(Primitive::Numerus, width);
    let operand_ty = if modular_operand { modular } else { exact };
    let result_ty = if modular_result { modular } else { exact };
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: MirType::semantic(operand_ty),
            span: Span::default(),
        }],
        blocks: vec![],
        return_ty: MirType::semantic(result_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.frames.last_mut().expect("frame").temps[0] = Some(Value::Int(operand));
    runner.eval_binop(
        op,
        &MirOperand::Temp(MirTempId(0)),
        &MirOperand::Constant(MirConstant::Int(divisor)),
        MirType::semantic(result_ty),
        Span::default(),
    )
}

#[test]
fn modular_operands_report_the_ring_issue_for_a_zero_divisor() {
    // The lowered binary value carries the exact integer carrier; the operand
    // types still identify the ring, so `/` and `%` by zero share the
    // structured modular issue.
    for op in [MirBinOp::Div, MirBinOp::Mod] {
        let error = runner_binop(NumericWidth::U8, true, false, 1, 0, op)
            .expect_err("a zero divisor traps");
        assert_eq!(error.issue, "modular_division_by_zero", "{op:?}");
    }
    // A modular result type reports the same issue from the wrap path.
    let error = runner_binop(NumericWidth::U8, true, true, 1, 0, MirBinOp::Div)
        .expect_err("a zero divisor traps");
    assert_eq!(error.issue, "modular_division_by_zero");
}

#[test]
fn exact_division_without_a_modular_operand_keeps_the_numerus_issue() {
    let error = runner_binop(NumericWidth::U8, false, false, 1, 0, MirBinOp::Div)
        .expect_err("a zero divisor traps");
    assert_eq!(error.issue, "runner_numerus_division_failed");
}

#[test]
fn modular_operands_keep_nonzero_exact_intermediates_until_the_store() {
    // -128 / -1 is 128 exactly; only the store into a `modulus<i8>` reduces it
    // to -128. Reducing at the operation would hide the exact intermediate.
    assert!(values_equal(
        &runner_binop(NumericWidth::I8, true, false, -128, -1, MirBinOp::Div)
            .expect("exact quotient"),
        &Value::Int(128)
    ));
    // `%` stays the exact floor remainder with the divisor's sign.
    assert!(values_equal(
        &runner_binop(NumericWidth::I8, true, false, -7, 2, MirBinOp::Mod)
            .expect("floor remainder"),
        &Value::Int(1)
    ));
    assert!(values_equal(
        &runner_binop(NumericWidth::I8, true, false, -7, -2, MirBinOp::Mod)
            .expect("floor remainder"),
        &Value::Int(-1)
    ));
    // A modular result type still reduces at this boundary.
    assert!(values_equal(
        &runner_binop(NumericWidth::I8, true, true, -128, -1, MirBinOp::Div)
            .expect("wrapping quotient"),
        &Value::Int(-128)
    ));
}

#[test]
fn signed_wrapping_shifts_complement_and_negation_stay_in_the_ring() {
    let span = Span::default();
    let shift = |value: i64, count: i64, op: MirBinOp| {
        modular_word_bitwise_binop(
            Value::Int(value),
            Value::Int(count),
            op,
            NumericWidth::I8,
            span,
        )
        .expect("shift")
    };
    assert!(values_equal(&shift(-8, 1, MirBinOp::Shr), &Value::Int(-4)));
    assert!(values_equal(&shift(-8, 40, MirBinOp::Shr), &Value::Int(-1)));
    assert!(values_equal(
        &shift(65, 1, MirBinOp::Shl),
        &Value::Int(-126)
    ));
    let not = modular_word_bitnot(Value::Int(-8), NumericWidth::I8, span).expect("complement");
    assert!(values_equal(&not, &Value::Int(7)));
    let neg = modular_word_neg(Value::Int(-128), NumericWidth::I8, span).expect("negation");
    assert!(values_equal(&neg, &Value::Int(-128)));
    let less = modular_word_compare_values(Value::Int(-1), Value::Int(1), MirBinOp::Lt, span)
        .expect("comparison on representatives");
    assert!(values_equal(&less, &Value::Bool(true)));
}

#[test]
fn signed_wrapping_reduction_maps_out_of_range_values_into_the_ring() {
    assert!(values_equal(
        &modular_word_result(300, NumericWidth::I8),
        &Value::Int(44)
    ));
    assert!(values_equal(
        &modular_word_result(200, NumericWidth::I8),
        &Value::Int(-56)
    ));
    assert!(values_equal(
        &modular_word_result(-129, NumericWidth::I8),
        &Value::Int(127)
    ));
    assert!(values_equal(
        &modular_word_result(i128::from(i64::MAX) + 1, NumericWidth::I64),
        &Value::Int(i64::MIN)
    ));
    assert!(values_equal(
        &modular_word_result(-1, NumericWidth::U8),
        &Value::Int(255)
    ));
}

#[test]
fn ascii_carried_fields_quote_by_declared_type_not_by_width() {
    // pr4: `'q'` reaches the runner as `Ascii` whether its slot is a `littera`,
    // an `ascii<1>` or an unbounded `ascii`. Only the declared `littera` field
    // (and a list of them) prints as a char; every other ascii prints as text.
    let mut interner = radix_lexer::Interner::new();
    let (c, u, cs, us) = (
        interner.intern("c"),
        interner.intern("u"),
        interner.intern("cs"),
        interner.intern("us"),
    );
    let def = radix_hir::DefId(5201);
    let mut structs = StructFieldOrder::default();
    structs.insert(def, vec![c, u, cs, us]);
    let variants = StructFieldOrder::default();
    let mut names = DisplayNames::default();
    names.insert(def, "Rec".to_owned());
    let mut char_fields = FieldCharSet::default();
    char_fields.insert((def, c));
    char_fields.insert((def, cs));
    let order = FieldOrder::new(&structs, &variants)
        .with_names(&names)
        .with_char_fields(&char_fields);
    let list = |items: Vec<Value>| Value::Array(std::rc::Rc::new(std::cell::RefCell::new(items)));
    let value = Value::Struct(
        def,
        field_map(&[
            (c, Value::Ascii(b'q')),
            (u, Value::Ascii(b'q')),
            (cs, list(vec![Value::Ascii(b'x')])),
            (us, list(vec![Value::Ascii(b'x'), text("abc")])),
        ]),
    );
    assert_eq!(
        value.display(&interner, order, Span::default()).unwrap(),
        r#"Rec { c = 'q', u = "q", cs = ['x'], us = ["x", "abc"] }"#
    );
    // A nested genus resets the kind: its own fields decide.
    let outer = interner.intern("outer");
    let outer_def = radix_hir::DefId(5202);
    structs.insert(outer_def, vec![outer]);
    names.insert(outer_def, "Outer".to_owned());
    let order = FieldOrder::new(&structs, &variants)
        .with_names(&names)
        .with_char_fields(&char_fields);
    let nested = Value::Struct(outer_def, field_map(&[(outer, value)]));
    assert_eq!(
        nested.display(&interner, order, Span::default()).unwrap(),
        r#"Outer { outer = Rec { c = 'q', u = "q", cs = ['x'], us = ["x", "abc"] } }"#
    );
}

#[test]
fn empty_tensor_is_rank_zero_with_no_elements() {
    let empty = TensorValue::empty();
    assert!(empty.shape.is_empty());
    assert!(empty.strides.is_empty());
    assert_eq!(empty.logical_values(), Some(Vec::new()));
}

#[test]
fn deep_copy_of_a_tensor_does_not_share_backing_storage() {
    let source = Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        vec![Value::Int(1), Value::Int(2)],
        vec![2],
    ))));
    let copy = source.deep_copy();
    let (Value::Tensor(source_cell), Value::Tensor(copy_cell)) = (&source, &copy) else {
        panic!("tensor deep copy stays a tensor");
    };
    assert!(!Rc::ptr_eq(source_cell, copy_cell));
    assert_eq!(
        copy_cell.borrow().logical_values(),
        source_cell.borrow().logical_values()
    );
    assert!(!Arc::ptr_eq(
        &source_cell.borrow().data,
        &copy_cell.borrow().data
    ));
}

#[test]
fn ledger_value_insert_order_and_update_keeps_position() {
    let mut ledger = LedgerValue::new();
    ledger.insert(MapKey::Str("b".to_owned()), Value::Int(1));
    ledger.insert(MapKey::Str("a".to_owned()), Value::Int(2));
    ledger.insert(MapKey::Str("c".to_owned()), Value::Int(3));
    ledger.insert(MapKey::Str("a".to_owned()), Value::Int(20));

    let keys: Vec<String> = ledger.keys().map(MapKey::document_key).collect();
    assert_eq!(keys, vec!["b", "a", "c"]);
    assert_eq!(
        ledger.get(&MapKey::Str("a".to_owned())),
        Some(&Value::Int(20))
    );
    assert_eq!(ledger.len(), 3);
}

#[test]
fn ledger_value_remove_then_insert_appends() {
    let mut ledger = LedgerValue::new();
    ledger.insert(MapKey::Str("a".to_owned()), Value::Int(1));
    ledger.insert(MapKey::Str("b".to_owned()), Value::Int(2));
    ledger.insert(MapKey::Str("c".to_owned()), Value::Int(3));
    assert_eq!(
        ledger.remove(&MapKey::Str("b".to_owned())),
        Some(Value::Int(2))
    );

    ledger.insert(MapKey::Str("b".to_owned()), Value::Int(9));
    let keys: Vec<String> = ledger.keys().map(MapKey::document_key).collect();
    assert_eq!(keys, vec!["a", "c", "b"]);
    assert_eq!(ledger.len(), 3);
    assert_eq!(
        ledger.get(&MapKey::Str("b".to_owned())),
        Some(&Value::Int(9))
    );
}

#[test]
fn ledger_value_compacts_holes() {
    let mut ledger = LedgerValue::new();
    for index in 0..8 {
        ledger.insert(MapKey::UInt(index), Value::UInt(index));
    }
    for index in [1_u64, 3, 5, 6, 7] {
        assert!(ledger.remove(&MapKey::UInt(index)).is_some());
    }

    let keys: Vec<String> = ledger.keys().map(MapKey::document_key).collect();
    assert_eq!(keys, vec!["0", "2", "4"]);
    assert_eq!(ledger.len(), 3);
    for index in [0_u64, 2, 4] {
        assert_eq!(ledger.get(&MapKey::UInt(index)), Some(&Value::UInt(index)));
    }
}

#[test]
fn ledger_value_equality_is_order_sensitive() {
    let mut first = LedgerValue::new();
    first.insert(MapKey::Str("a".to_owned()), Value::Int(1));
    first.insert(MapKey::Str("b".to_owned()), Value::Int(2));

    let mut second = LedgerValue::new();
    second.insert(MapKey::Str("b".to_owned()), Value::Int(2));
    second.insert(MapKey::Str("a".to_owned()), Value::Int(1));

    let same = first.clone();
    assert_eq!(first, same);
    assert_ne!(first, second);
}

/// Build a `Value::Ledger` from `(key, value)` pairs in the given insertion
/// order; `values_equal` fixtures for the L2e.1 order-sensitive equality.
fn ledger_value_eq_fixture(pairs: &[(&str, i64)]) -> Value {
    let mut ledger = LedgerValue::new();
    for (key, value) in pairs {
        ledger.insert(MapKey::Str((*key).to_owned()), Value::Int(*value));
    }
    Value::Ledger(Rc::new(RefCell::new(ledger)))
}

#[test]
fn ledger_value_eq_same_order_is_equal() {
    let left = ledger_value_eq_fixture(&[("b", 1), ("a", 2)]);
    let right = ledger_value_eq_fixture(&[("b", 1), ("a", 2)]);
    assert!(values_equal(&left, &right));
}

#[test]
fn ledger_value_eq_different_order_is_unequal() {
    let left = ledger_value_eq_fixture(&[("b", 1), ("a", 2)]);
    let right = ledger_value_eq_fixture(&[("a", 2), ("b", 1)]);
    assert!(!values_equal(&left, &right));
}

#[test]
fn ledger_value_eq_different_value_is_unequal() {
    let left = ledger_value_eq_fixture(&[("b", 1), ("a", 2)]);
    let right = ledger_value_eq_fixture(&[("b", 1), ("a", 3)]);
    assert!(!values_equal(&left, &right));
}

#[test]
fn ledger_value_eq_map_ignores_order() {
    let mut first = handroll::FxHashMap::default();
    first.insert(MapKey::Str("b".to_owned()), Value::Int(1));
    first.insert(MapKey::Str("a".to_owned()), Value::Int(2));
    let mut second = handroll::FxHashMap::default();
    second.insert(MapKey::Str("a".to_owned()), Value::Int(2));
    second.insert(MapKey::Str("b".to_owned()), Value::Int(1));
    let left = Value::Map(Rc::new(RefCell::new(first)));
    let right = Value::Map(Rc::new(RefCell::new(second)));
    assert!(values_equal(&left, &right));
}

#[test]
fn ledger_value_display_follows_insertion_order() {
    let interner = radix_lexer::Interner::new();
    let empty = handroll::FxHashMap::default();
    let order = FieldOrder::new(&empty, &empty);
    let value = ledger_value_eq_fixture(&[("b", 2), ("a", 1), ("c", 3)]);
    assert_eq!(
        value.display(&interner, order, Span::default()).unwrap(),
        r#"{"b": 2, "a": 1, "c": 3}"#
    );
}

#[test]
fn ledger_value_display_after_remove_and_readd() {
    let interner = radix_lexer::Interner::new();
    let empty = handroll::FxHashMap::default();
    let order = FieldOrder::new(&empty, &empty);
    let mut ledger = LedgerValue::new();
    ledger.insert(MapKey::Str("a".to_owned()), Value::Int(1));
    ledger.insert(MapKey::Str("b".to_owned()), Value::Int(2));
    ledger.insert(MapKey::Str("c".to_owned()), Value::Int(3));
    assert_eq!(
        ledger.remove(&MapKey::Str("b".to_owned())),
        Some(Value::Int(2))
    );
    ledger.insert(MapKey::Str("b".to_owned()), Value::Int(7));
    let value = Value::Ledger(Rc::new(RefCell::new(ledger)));
    assert_eq!(
        value.display(&interner, order, Span::default()).unwrap(),
        r#"{"a": 1, "c": 3, "b": 7}"#
    );
}

#[test]
fn enum_variant_map_key_payload_free_round_trips() {
    let key = MapKey::Enum(radix_hir::DefId(6001), Vec::new());
    let value = key.to_value();
    assert!(matches!(
        &value,
        Value::EnumVariant(def, fields)
            if *def == radix_hir::DefId(6001) && fields.borrow().is_empty()
    ));
    assert!(!key.document_key().is_empty());
}

#[test]
fn enum_variant_map_key_distinct_defs_distinct() {
    let first = MapKey::Enum(radix_hir::DefId(6101), Vec::new());
    let second = MapKey::Enum(radix_hir::DefId(6102), Vec::new());
    assert_ne!(first, second);
    let mut map = handroll::FxHashMap::default();
    map.insert(first.clone(), Value::Int(1));
    map.insert(second.clone(), Value::Int(2));
    assert_eq!(map.get(&first), Some(&Value::Int(1)));
    assert_eq!(map.get(&second), Some(&Value::Int(2)));
}

/// Q-L3 (audit GAPS #13): an `intervallum` carries its bounds as a tuple-shaped
/// `Value::Tuple`, but its static type makes it print as the construction
/// literal — `2‥9` exclusive, `2…9` inclusive, a trailing step as ` per 2`.
#[test]
fn interval_displays_its_construction_literal() {
    let interner = radix_lexer::Interner::new();
    let empty = handroll::FxHashMap::default();
    let display = |value: &Value, order: FieldOrder<'_>| {
        value
            .display(&interner, order, Span::default())
            .expect("interval display")
    };
    let exclusive = Value::Tuple(vec![Value::Int(2), Value::Int(9), Value::Bool(false)]);
    assert_eq!(
        display(
            &exclusive,
            FieldOrder::new(&empty, &empty).with_interval(true)
        ),
        "2‥9"
    );
    let inclusive = Value::Tuple(vec![Value::Int(2), Value::Int(9), Value::Bool(true)]);
    assert_eq!(
        display(
            &inclusive,
            FieldOrder::new(&empty, &empty).with_interval(true)
        ),
        "2…9"
    );
    let empty_interval = Value::Tuple(vec![Value::Int(5), Value::Int(5), Value::Bool(false)]);
    assert_eq!(
        display(
            &empty_interval,
            FieldOrder::new(&empty, &empty).with_interval(true)
        ),
        "5‥5"
    );
    let stepped = Value::Tuple(vec![
        Value::Int(2),
        Value::Int(9),
        Value::Bool(false),
        Value::Int(2),
    ]);
    assert_eq!(
        display(
            &stepped,
            FieldOrder::new(&empty, &empty).with_interval(true)
        ),
        "2‥9 per 2"
    );
    // Without the declared-interval flag the carrier keeps its debug text.
    assert_eq!(
        display(&exclusive, FieldOrder::new(&empty, &empty)),
        format!("{exclusive:?}")
    );
}

#[test]
fn enum_variant_map_key_payload_fields_sorted_and_read_back() {
    let mut interner = radix_lexer::Interner::new();
    let n = interner.intern("n");
    let m = interner.intern("m");
    let def = radix_hir::DefId(6201);
    let span = Span::default();

    let single = Value::EnumVariant(def, field_map(&[(n, Value::Int(1))]))
        .to_key(span)
        .expect("hashable enum payload is a key");
    assert_eq!(single, MapKey::Enum(def, vec![(n, MapKey::UInt(1))]));

    let first_order = Value::EnumVariant(def, field_map(&[(m, Value::Int(2)), (n, Value::Int(1))]))
        .to_key(span)
        .expect("hashable enum payload is a key");
    let second_order =
        Value::EnumVariant(def, field_map(&[(n, Value::Int(1)), (m, Value::Int(2))]))
            .to_key(span)
            .expect("hashable enum payload is a key");
    assert_eq!(first_order, second_order);

    let MapKey::Enum(key_def, parts) = &first_order else {
        panic!("enum variant produces an enum key");
    };
    assert_eq!(*key_def, def);
    assert!(n.0 < m.0);
    assert_eq!(
        parts.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        vec![n, m]
    );

    let read_back = first_order.to_value();
    assert!(matches!(
        &read_back,
        Value::EnumVariant(found_def, fields)
            if *found_def == def && fields.borrow().get(&n) == Some(&Value::UInt(1))
    ));
    assert!(!first_order.document_key().is_empty());
}
