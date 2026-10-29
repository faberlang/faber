//! F9 ruling 30 (dec2): an untyped constant beside a `d64` operand, in a
//! comparison, or in a `d64` slot joins the decimal family by exact value.
//! It needs at most 8 fractional digits and must lie in the `d64` range; a
//! typed integer or float operand beside `d64` still needs `↦`.

use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

const D: &str = "d64";

fn body(text: &str) -> String {
    format!("functio f({D} d) → {D} {{ {text} }}")
}

fn compare(text: &str) -> String {
    format!("functio f({D} d) → bivalens {{ {text} }}")
}

#[test]
fn decimal_literal_beside_d64_joins() {
    assert_accepts_la(&body("redde d + 0.1"));
    assert_accepts_la(&body("redde d - 0.1"));
    assert_accepts_la(&body("redde 0.1 + d"));
    assert_accepts_la(&body("redde d * 2.0"));
    assert_accepts_la(&body("redde d / 0.5"));
}

#[test]
fn decimal_literal_in_mut_d64_parameter_matches_local_expectation() {
    assert_accepts_la(
        r#"
functio local() {
    varia d64 p ← 2.5
    p ← p + 1.0
}
functio increment(varia d64 p) {
    p ← p + 1.0
}
"#,
    );
}

#[test]
fn integer_constant_beside_d64_joins() {
    assert_accepts_la(&body("redde d + 1"));
    assert_accepts_la(&body("redde d * 2"));
    assert_accepts_la(&body("redde d / 2"));
    assert_accepts_la(&body("redde 2 * d"));
    assert_accepts_la(&body("redde d + -1"));
}

#[test]
fn integer_constant_fills_a_d64_slot() {
    assert_accepts_la(&format!("incipit {{ sit {D} e ← 5 }}"));
    assert_accepts_la(&format!("incipit {{ sit {D} e ← -5 }}"));
    assert_accepts_la(&format!("incipit {{ sit {D} e ← 1 + 2 }}"));
}

#[test]
fn decimal_constant_arithmetic_fills_a_d64_slot() {
    assert_accepts_la(&format!("incipit {{ sit {D} e ← 0.1 + 0.2 }}"));
}

#[test]
fn constant_joins_in_comparisons() {
    for op in ["≺", "≻", "≤", "≥", "≅", "≇", "≈", "≉"] {
        assert_accepts_la(&compare(&format!("redde d {op} 0.1")));
    }
    assert_accepts_la(&compare("redde d ≅ 4"));
    assert_accepts_la(&compare("redde 4.2 ≅ d"));
}

#[test]
fn constant_beside_d64_is_checked_for_fraction_digits() {
    assert_rejects_with_issue_la(
        &body("redde d + 0.123456789"),
        "decimal_literal_out_of_range",
    );
    assert_rejects_with_issue_la(
        &body("redde d * 0.123456789"),
        "decimal_literal_out_of_range",
    );
    assert_rejects_with_issue_la(
        &compare("redde d ≅ 0.123456789"),
        "decimal_literal_out_of_range",
    );
}

#[test]
fn constant_beside_d64_is_checked_for_range() {
    assert_rejects_with_issue_la(
        &body("redde d + 92233720369"),
        "decimal_literal_out_of_range",
    );
    assert_rejects_with_issue_la(
        &format!("incipit {{ sit {D} e ← 92233720369 }}"),
        "decimal_literal_out_of_range",
    );
    assert_accepts_la(&format!("incipit {{ sit {D} e ← 92233720368 }}"));
}

#[test]
fn typed_operands_beside_d64_still_need_conversio() {
    assert_rejects_with_issue_la(
        &format!("functio f({D} d, numerus n) → {D} {{ redde d + n }}"),
        "numeric_family_crossing_requires_conversio",
    );
    assert_rejects_with_issue_la(
        &format!("functio f({D} d, fractus x) → {D} {{ redde d + x }}"),
        "numeric_family_crossing_requires_conversio",
    );
}

#[test]
fn explicit_conversion_still_accepts() {
    assert_accepts_la(&format!("incipit {{ sit {D} e ← 5 ↦ {D} }}"));
}

#[test]
fn integer_and_float_slots_are_unchanged() {
    assert_accepts_la("functio f(numerus n) → numerus { redde n + 1 }");
    assert_accepts_la("incipit { sit d64 e ← 4.2 }");
}
