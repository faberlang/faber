//! d11-6 rulings 16 and 20: integers never cross into floats implicitly;
//! constants into float slots are checked once at compile time.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

#[test]
fn typed_integer_into_float_slot_rejects() {
    assert_rejects_with_issue(
        "incipit { fixum numerus<i32> n ← 1 \n fixum fractus<f64> g ← n }",
        "integer_to_float_requires_conversio",
    );
}

#[test]
fn typed_i64_into_float_slot_rejects() {
    assert_rejects_with_issue(
        "incipit { fixum numerus<i64> n ← 1 \n fixum fractus<f64> g ← n }",
        "integer_to_float_requires_conversio",
    );
}

#[test]
fn explicit_conversio_of_typed_integer_to_float_accepts() {
    assert_accepts(
        "incipit { fixum numerus<i64> n ← 1 \n fixum fractus<f64> g ← n ↦ fractus<f64> }",
    );
}

#[test]
fn integer_literal_into_float_slot_accepts() {
    assert_accepts("incipit { fixum fractus f ← 3 }");
    assert_accepts("incipit { fixum fractus<f64> f ← 9007199254740992 }");
    assert_accepts("incipit { fixum fractus<f32> f ← 16777216 }");
}

#[test]
fn inexact_integer_literal_into_float_slot_rejects() {
    assert_rejects_with_issue(
        "incipit { fixum fractus<f64> f ← 9007199254740993 }",
        "float_constant_inexact",
    );
    assert_rejects_with_issue(
        "incipit { fixum fractus<f32> f ← 16777217 }",
        "float_constant_inexact",
    );
}

#[test]
fn typed_integer_assignment_carries_conversio_issue() {
    assert_rejects_with_issue(
        "incipit { fixum numerus<i32> n ← 1 \n varia fractus<f64> g ← 1.0 \n g ← n }",
        "integer_to_float_requires_conversio",
    );
}

#[test]
fn negative_integer_literal_into_float_slot_accepts() {
    assert_accepts("incipit { fixum fractus f ← -3 }");
    assert_accepts("incipit { fixum fractus<f64> f ← -9007199254740992 }");
}

#[test]
fn inexact_negative_integer_literal_into_float_slot_rejects() {
    assert_rejects_with_issue(
        "incipit { fixum fractus<f64> f ← -9007199254740993 }",
        "float_constant_inexact",
    );
}
