//! Typecheck pins for `octeti` window ↦ `numerus<W, Be|Le>`.

use crate::test_support;

#[test]
fn octeti_window_to_u32_be_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → numerus<u32> { redde corpus[0‥4] ↦ numerus<u32, Be> }",
    );
}

#[test]
fn octeti_window_to_i32_be_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → numerus<i32> { redde corpus[0‥4] ↦ numerus<i32, Be> }",
    );
}

#[test]
fn octeti_window_to_u32_le_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → numerus<u32> { redde corpus[0‥4] ↦ numerus<u32, Le> }",
    );
}

#[test]
fn octeti_literal_window_to_u32_be_accepts() {
    test_support::assert_accepts("incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ numerus<u32, Be> }");
}

#[test]
fn octeti_literal_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be ef| ↦ numerus<u32> }",
        "octeti_to_numerus_unspecified",
    );
}

#[test]
fn octeti_window_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ numerus<u32> }",
        "octeti_to_numerus_unspecified",
    );
}

#[test]
fn three_byte_source_mismatches_u32() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be| ↦ numerus<u32, Be> }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn three_byte_window_mismatches_u32() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be|[0‥3] ↦ numerus<u32, Be> }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn four_byte_window_mismatches_u16() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ numerus<u16, Be> }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn octeti_window_to_f32_be_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → fractus<f32> { redde corpus[0‥4] ↦ fractus<f32, Be> }",
    );
}

#[test]
fn octeti_window_to_f32_le_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → fractus<f32> { redde corpus[0‥4] ↦ fractus<f32, Le> }",
    );
}

#[test]
fn octeti_window_to_f64_be_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → fractus<f64> { redde corpus[0‥8] ↦ fractus<f64, Be> }",
    );
}

#[test]
fn octeti_window_to_f64_le_accepts() {
    test_support::assert_accepts(
        "functio win(octeti corpus) → fractus<f64> { redde corpus[0‥8] ↦ fractus<f64, Le> }",
    );
}

#[test]
fn fractus_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ fractus<f32> }",
        "octeti_to_fractus_unspecified",
    );
}

#[test]
fn three_byte_window_mismatches_f32() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be|[0‥3] ↦ fractus<f32, Be> }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn four_byte_window_mismatches_f64() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ fractus<f64, Le> }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn hex_hint_on_fractus_still_rejected() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ n ← |de ad be ef|[0‥4] ↦ fractus<f32, Hex> }",
        "octeti_to_fractus_unspecified",
    );
}

#[test]
fn textus_hex_parse_still_accepts() {
    test_support::assert_accepts(r#"incipit { fixum _ n ← "ff" ↦ numerus<i32, Hex> }"#);
}
