//! Typecheck pins for `n ↦ ascii<N> via Hex|Bin|Oct`.

use crate::test_support;

#[test]
fn numerus_to_ascii_hex_accepts() {
    test_support::assert_accepts_la(
        "incipit { sit ascii<magnitudo 2> hex ← 255 ↦ ascii<magnitudo 2> via Hex }",
    );
}

#[test]
fn numerus_to_ascii_hex_pads() {
    test_support::assert_accepts_la(
        "incipit { sit ascii<magnitudo 8> hex ← 37 ↦ ascii<magnitudo 8> via Hex }",
    );
}

#[test]
fn numerus_to_ascii_bin_accepts() {
    test_support::assert_accepts_la(
        "incipit { sit ascii<magnitudo 8> bits ← 255 ↦ ascii<magnitudo 8> via Bin }",
    );
}

#[test]
fn numerus_to_ascii_oct_accepts() {
    test_support::assert_accepts_la(
        "incipit { sit ascii<magnitudo 3> oct ← 255 ↦ ascii<magnitudo 3> via Oct }",
    );
}

#[test]
fn sized_unsigned_to_ascii_hex_accepts() {
    test_support::assert_accepts_la(
        "incipit { sit u8 n ← 255
sit ascii<magnitudo 2> hex ← n ↦ ascii<magnitudo 2> via Hex }",
    );
}

#[test]
fn overflow_literal_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ hex ← 256 ↦ ascii<magnitudo 2> via Hex }",
        "ascii_pack_width_overflow",
    );
}

#[test]
fn negative_literal_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ hex ← -1 ↦ ascii<magnitudo 2> via Hex }",
        "ascii_pack_negative_source",
    );
}

#[test]
fn float_source_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ hex ← 1.5 ↦ ascii<magnitudo 2> via Hex }",
        "unsupported_conversio",
    );
}

/// The hint rides a `via` clause; written as the capacity argument it is a
/// capacity error (the second-argument form is a parse error, D11.9).
#[test]
fn hint_as_the_capacity_argument_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ hex ← 255 ↦ ascii<Hex> }",
        "ascii_format_hint_not_capacity",
    );
}

#[test]
fn const_source_hole_infers_hex_digit_capacity() {
    test_support::assert_accepts_la("incipit { sit ascii<_> hex ← 255 ↦ ascii<_> via Hex }");
}

#[test]
fn const_source_hole_infers_bin_digit_capacity() {
    test_support::assert_accepts_la("incipit { sit ascii<_> bits ← 255 ↦ ascii<_> via Bin }");
}

#[test]
fn const_source_hole_infers_oct_digit_capacity() {
    test_support::assert_accepts_la("incipit { sit ascii<_> oct ← 255 ↦ ascii<_> via Oct }");
}

#[test]
fn runtime_source_hole_reports_unsolved_capacity() {
    test_support::assert_analysis_rejects_with_issue_la(
        "functio pack(numerus n) → ascii<_> { redde n ↦ ascii<_> via Hex }",
        "bounded_capacity_unsolved",
    );
}

#[test]
fn runtime_source_overflow_is_not_a_type_error() {
    test_support::assert_accepts_la(
        "functio pack(numerus n) → ascii<magnitudo 2> { redde n ↦ ascii<magnitudo 2> via Hex }",
    );
}
