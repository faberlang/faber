//! Typecheck pins for `octeti ↦ lista<numerus<u8>>` (OEC-2 boundary bridge).

use crate::test_support;

#[test]
fn octeti_to_lista_u8_accepts() {
    test_support::assert_accepts("functio bridge(octeti o) → lista<u8> { redde o ↦ lista<u8> }");
}

#[test]
fn octeti_literal_to_lista_u8_accepts() {
    test_support::assert_accepts("incipit { fixum _ b ← |de ad be ef| ↦ lista<u8> }");
}

#[test]
fn lista_u8_to_octeti_still_accepts() {
    test_support::assert_accepts("functio from_list(lista<u8> b) → octeti { redde b ↦ octeti }");
}

#[test]
fn octeti_to_lista_textus_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ b ← |de ad| ↦ lista<textus> }",
        "unsupported_conversio",
    );
}

#[test]
fn octeti_to_lista_unsized_numerus_rejects() {
    // An unsized numerus element has no bit width to match: the bits row
    // names it (`lista<b8>` converts to a sized same-width `lista<uN>` only).
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ b ← |de ad| ↦ lista<numerus> }",
        "bits_conversion_width_mismatch",
    );
}

#[test]
fn numeric_to_octeti_unspecified_unchanged() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum _ b ← 42 ↦ octeti }",
        "numeric_to_octeti_unspecified",
    );
}
