//! decimal-widths D2 (wildcard audit): every named wrong-inheritance site
//! rejects decimals with its own teaching issue, and the family lattice
//! (widening, cross-family) holds at assignment time.

use crate::test_support;

// --- family lattice (assignment-level) ---

#[test]
fn decimal_to_integer_cross_family_rejects_at_assignment() {
    test_support::assert_rejects_with_issue(
        "functio c(numerus<d64> a) → numerus<i64> { redde a }",
        "return_type_mismatch",
    );
}

#[test]
fn integer_to_decimal_cross_family_rejects_at_assignment() {
    test_support::assert_rejects_with_issue(
        "functio c(numerus<i64> a) → numerus<d64> { redde a }",
        "return_type_mismatch",
    );
}

#[test]
fn decimal_to_fractus_cross_family_rejects_at_assignment() {
    // D3 owns the explicit-↦ conversio row; bare flow must reject today.
    test_support::assert_rejects_with_issue(
        "functio c(numerus<d64> a) → fractus<f64> { redde a }",
        "return_type_mismatch",
    );
}

// --- wrong site class: index scalars ---

#[test]
fn decimal_scalar_bracket_index_rejects_with_teaching_issue() {
    test_support::assert_rejects_with_issue(
        r"functio pick(tf32[16] input, numerus<d64> id) → f32 {
    redde input[id] vel 0.0
}",
        "tensor_bracket_index_decimal_rejected",
    );
}

#[test]
fn integer_scalar_bracket_index_still_accepts() {
    test_support::assert_accepts(
        r"functio pick(tf32[16] input, numerus<i32> id) → f32 {
    redde input[id] vel 0.0
}",
    );
}

// --- wrong site class: tensor media ---

#[test]
fn tensor_media_over_decimal_element_rejects_with_teaching_issue() {
    test_support::assert_rejects_with_issue(
        r"incipit {
  fixum tensor<numerus<d64>, [3]> grid ← vacua
  fixum f64 mean ← grid.media()
  nota mean
}",
        "tensor_media_decimal_rejected",
    );
}

#[test]
fn tensor_media_over_integer_numerus_still_returns_f64() {
    test_support::assert_accepts(
        r"incipit {
  fixum tensor<numerus, [3]> grid ← vacua
  fixum f64 mean ← grid.media()
  nota mean
}",
    );
}

// --- wrong site class: radix hints (Hex/Bin/Oct) ---

#[test]
fn radix_text_parse_into_decimal_rejects() {
    test_support::assert_rejects_with_issue(
        "functio p(textus s) → numerus<d64> { redde s ↦ numerus<d64, Hex> }",
        "decimal_radix_hint_rejected",
    );
}

#[test]
fn radix_text_parse_into_integer_still_accepts() {
    test_support::assert_accepts(
        "functio p(textus s) → numerus<u32> { redde s ↦ numerus<u32, Hex> }",
    );
}

#[test]
fn ascii_format_pack_from_decimal_source_rejects() {
    test_support::assert_rejects_with_issue(
        "functio p(numerus<d64> n) → ascii<8> { redde n ↦ ascii<8, Hex> }",
        "decimal_radix_hint_rejected",
    );
}

#[test]
fn ascii_format_pack_from_integer_source_still_accepts() {
    test_support::assert_accepts("incipit { fixum ascii<2> hex ← 255 ↦ ascii<2, Hex> }");
}

// --- wrong site class: endian hints (Be/Le) ---

#[test]
fn endian_unpack_into_decimal_rejects() {
    test_support::assert_rejects_with_issue(
        "functio win(octeti corpus) → numerus<d64> { redde corpus[0‥4] ↦ numerus<d64, Be> }",
        "decimal_endianness_hint_rejected",
    );
}

#[test]
fn endian_unpack_into_integer_still_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → numerus<u32> { redde corpus[0‥4] ↦ numerus<u32, Be> }",
    );
}

// --- wrong site class: Bits bitcast ---

#[test]
fn bits_bitcast_from_decimal_source_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<d64> n) → fractus<f32> { redde n ↦ fractus<f32, Bits> }",
        "decimal_bits_hint_rejected",
    );
}

#[test]
fn bits_bitcast_into_decimal_target_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(fractus<f32> x) → numerus<d64> { redde x ↦ numerus<d64, Bits> }",
        "decimal_bits_hint_rejected",
    );
}

#[test]
fn bits_bitcast_integer_pair_still_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits> }",
    );
}
