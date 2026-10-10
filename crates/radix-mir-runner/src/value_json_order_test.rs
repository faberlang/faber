//! A `valor`/`json` object displays its keys sorted (byte order, the order a
//! `BTreeMap<String, _>` gives), whatever the map's insertion order or the
//! iteration shuffle (`RADIX_RUNNER_SHUFFLE_SEED`); an ordinary
//! `tabula` display stays unspecified-order.
//!
//! The shuffle seed is read once per process and is random by default, so a
//! specific seed is covered by running this test binary under
//! `RADIX_RUNNER_SHUFFLE_SEED=<n>` (`0` turns the shuffle off).

use crate::value::{FieldOrder, MapKey, StructFieldOrder, Value};
use radix_lexer::{Interner, Span};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

fn object(entries: &[(&str, Value)]) -> Value {
    let mut map = FxHashMap::default();
    for (key, value) in entries {
        map.insert(MapKey::Str((*key).to_owned()), value.clone());
    }
    Value::Map(Rc::new(RefCell::new(map)))
}

fn show_valor(value: &Value) -> String {
    let structs = StructFieldOrder::default();
    let variants = StructFieldOrder::default();
    let order = FieldOrder::new(&structs, &variants).with_dynamic(true);
    value
        .display_typed(&Interner::new(), order, Span::default(), None)
        .expect("display")
}

#[test]
fn a_valor_object_prints_keys_in_sorted_order() {
    let value = object(&[
        ("z", Value::Int(1)),
        ("a", Value::Int(2)),
        ("m", Value::Int(3)),
        ("ab", Value::Int(4)),
        ("a ", Value::Int(5)),
        ("b", Value::Int(6)),
    ]);
    assert_eq!(
        show_valor(&value),
        r#"{"a": 2, "a ": 5, "ab": 4, "b": 6, "m": 3, "z": 1}"#
    );
}

#[test]
fn nested_valor_objects_print_sorted_at_every_depth() {
    let inner = object(&[("y", Value::Int(20)), ("x", Value::Int(10))]);
    let list = Value::Array(Rc::new(RefCell::new(vec![object(&[
        ("q", Value::Int(1)),
        ("p", Value::Int(2)),
    ])])));
    let value = object(&[("outer", inner), ("items", list)]);
    assert_eq!(
        show_valor(&value),
        r#"{"items": [{"p": 2, "q": 1}], "outer": {"x": 10, "y": 20}}"#
    );
}
