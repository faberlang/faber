//! Typecheck pins for `octeti` window ↦ `W via Be|Le`.

use crate::test_support;

#[test]
fn octeti_window_to_u32_be_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → u32 { redde corpus[0‥4] ↦ u32 via Be }",
    );
}

#[test]
fn octeti_window_to_i32_be_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → i32 { redde corpus[0‥4] ↦ i32 via Be }",
    );
}

#[test]
fn octeti_window_to_u32_le_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → u32 { redde corpus[0‥4] ↦ u32 via Le }",
    );
}

#[test]
fn octeti_literal_window_to_u32_be_accepts() {
    test_support::assert_accepts_la("incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ u32 via Be }");
}

#[test]
fn octeti_literal_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be ef| ↦ u32 }",
        "octeti_to_numerus_unspecified",
    );
}

#[test]
fn octeti_window_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ u32 }",
        "octeti_to_numerus_unspecified",
    );
}

#[test]
fn three_byte_source_mismatches_u32() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be| ↦ u32 via Be }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn three_byte_window_mismatches_u32() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be|[0‥3] ↦ u32 via Be }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn four_byte_window_mismatches_u16() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ u16 via Be }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn octeti_window_to_f32_be_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → f32 { redde corpus[0‥4] ↦ f32 via Be }",
    );
}

#[test]
fn octeti_window_to_f32_le_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → f32 { redde corpus[0‥4] ↦ f32 via Le }",
    );
}

#[test]
fn octeti_window_to_f64_be_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → f64 { redde corpus[0‥8] ↦ f64 via Be }",
    );
}

#[test]
fn octeti_window_to_f64_le_accepts() {
    test_support::assert_accepts_la(
        "functio win(lista<b8> corpus) → f64 { redde corpus[0‥8] ↦ f64 via Le }",
    );
}

#[test]
fn fractus_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ f32 }",
        "octeti_to_fractus_unspecified",
    );
}

#[test]
fn three_byte_window_mismatches_f32() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be|[0‥3] ↦ f32 via Be }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn four_byte_window_mismatches_f64() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ f64 via Le }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn hex_hint_on_fractus_still_rejected() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ f32 via Hex }",
        "octeti_to_fractus_unspecified",
    );
}

#[test]
fn textus_hex_parse_still_accepts() {
    test_support::assert_accepts_la(r#"incipit { fixum _ n ← "ff" ↦ i32 via Hex }"#);
}
