//! Typecheck pins for `n ↦ ascii<N, Hex|Bin|Oct>`.

use crate::test_support;

#[test]
fn numerus_to_ascii_hex_accepts() {
    test_support::assert_accepts("incipit { fixum ascii<2> hex ← 255 ↦ ascii<2, Hex> }");
}

#[test]
fn numerus_to_ascii_hex_pads() {
    test_support::assert_accepts("incipit { fixum ascii<8> hex ← 37 ↦ ascii<8, Hex> }");
}

#[test]
fn numerus_to_ascii_bin_accepts() {
    test_support::assert_accepts("incipit { fixum ascii<8> bits ← 255 ↦ ascii<8, Bin> }");
}

#[test]
fn numerus_to_ascii_oct_accepts() {
    test_support::assert_accepts("incipit { fixum ascii<3> oct ← 255 ↦ ascii<3, Oct> }");
}

#[test]
fn sized_unsigned_to_ascii_hex_accepts() {
    test_support::assert_accepts(
        "incipit { fixum numerus<u8> n ← 255
fixum ascii<2> hex ← n ↦ ascii<2, Hex> }",
    );
}

#[test]
fn overflow_literal_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ hex ← 256 ↦ ascii<2, Hex> }",
        "ascii_pack_width_overflow",
    );
}

#[test]
fn negative_literal_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ hex ← -1 ↦ ascii<2, Hex> }",
        "ascii_pack_negative_source",
    );
}

#[test]
fn float_source_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ hex ← 1.5 ↦ ascii<2, Hex> }",
        "unsupported_conversio",
    );
}

#[test]
fn hint_in_slot_one_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ hex ← 255 ↦ ascii<Hex> }",
        "ascii_format_hint_not_capacity",
    );
}

#[test]
fn runtime_source_overflow_is_not_a_type_error() {
    test_support::assert_accepts("functio pack(numerus n) → ascii<2> { redde n ↦ ascii<2, Hex> }");
}
