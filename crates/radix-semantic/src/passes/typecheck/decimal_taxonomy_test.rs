//! decimal-widths D2 (wildcard audit): every named wrong-inheritance site
//! rejects decimals with its own teaching issue, and the family lattice
//! (widening, cross-family) holds at assignment time.

use crate::test_support;

// --- family lattice (assignment-level) ---

#[test]
fn decimal_to_integer_cross_family_rejects_at_assignment() {
    test_support::assert_rejects_with_issue_la(
        "functio c(d64 a) → i64 { redde a }",
        "return_type_mismatch",
    );
}

#[test]
fn integer_to_decimal_cross_family_rejects_at_assignment() {
    test_support::assert_rejects_with_issue_la(
        "functio c(i64 a) → d64 { redde a }",
        "return_type_mismatch",
    );
}

#[test]
fn decimal_to_fractus_cross_family_rejects_at_assignment() {
    // D3 owns the explicit-↦ conversio row; bare flow must reject today.
    test_support::assert_rejects_with_issue_la(
        "functio c(d64 a) → f64 { redde a }",
        "return_type_mismatch",
    );
}

// --- wrong site class: index scalars ---

#[test]
fn decimal_scalar_bracket_index_rejects_with_teaching_issue() {
    test_support::assert_rejects_with_issue_la(
        r"functio pick(tf32[16] input, d64 id) → f32 {
    redde input[id]
}",
        "tensor_bracket_index_decimal_rejected",
    );
}

#[test]
fn integer_scalar_bracket_index_still_accepts() {
    test_support::assert_accepts_la(
        r"functio pick(tf32[16] input, i32 id) → f32 {
    redde input[id]
}",
    );
}

// --- wrong site class: tensor media ---

#[test]
fn tensor_media_over_decimal_element_rejects_with_teaching_issue() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
  sit tensor<d64, [3]> grid ← vacua
  sit f64 mean ← grid.media()
  nota mean
}",
        "tensor_media_decimal_rejected",
    );
}

#[test]
fn tensor_media_over_integer_numerus_still_returns_f64() {
    test_support::assert_accepts_la(
        r"incipit {
  sit tensor<numerus, [3]> grid ← vacua
  sit f64 mean ← grid.media()
  nota mean
}",
    );
}

// --- wrong site class: radix hints (Hex/Bin/Oct) ---

#[test]
fn radix_text_parse_into_decimal_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio p(textus s) → d64 { redde s ↦ d64 via Hex }",
        "decimal_radix_hint_rejected",
    );
}

#[test]
fn radix_text_parse_into_integer_still_accepts() {
    test_support::assert_accepts_la("functio p(textus s) → u32 ⇥ textus { redde s ↦ u32 via Hex }");
}

#[test]
fn ascii_format_pack_from_decimal_source_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio p(d64 n) → ascii<magnitudo 8> { redde n ↦ ascii<magnitudo 8> via Hex }",
        "decimal_radix_hint_rejected",
    );
}

#[test]
fn ascii_format_pack_from_integer_source_still_accepts() {
    test_support::assert_accepts_la(
        "incipit { sit ascii<magnitudo 2> hex ← 255 ↦ ascii<magnitudo 2> via Hex }",
    );
}

// --- wrong site class: endian hints (Be/Le) ---

#[test]
fn endian_unpack_into_decimal_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio win(lista<b8> corpus) → d64 { redde corpus[0‥4] ↦ d64 via Be }",
        "decimal_endianness_hint_rejected",
    );
}

#[test]
fn endian_unpack_into_integer_still_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → u32 { redde corpus[0‥4] ↦ u32 via Be }",
    );
}

// --- wrong site class: retired Bits hint ---

#[test]
fn via_bits_on_a_decimal_is_the_retirement_diagnostic() {
    test_support::assert_rejects_with_issue_la(
        "functio bits(d64 n) → f32 { redde n ↦ f32 via Bits }",
        "conversio_via_bits_retired",
    );
    test_support::assert_rejects_with_issue_la(
        "functio bits(f32 x) → d64 { redde x ↦ d64 via Bits }",
        "conversio_via_bits_retired",
    );
}
