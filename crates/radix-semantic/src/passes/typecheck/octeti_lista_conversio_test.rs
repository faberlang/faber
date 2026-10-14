//! Typecheck pins for `octeti ↦ lista<numerus<u8>>` (OEC-2 boundary bridge).

use crate::test_support;

#[test]
fn octeti_to_lista_u8_accepts() {
    test_support::assert_accepts_la("functio bridge(octeti o) → lista<u8> { redde o ↦ lista<u8> }");
}

#[test]
fn octeti_literal_to_lista_u8_accepts() {
    test_support::assert_accepts_la("incipit { fixum _ b ← |de ad be ef| ↦ lista<u8> }");
}

#[test]
fn lista_u8_to_octeti_still_accepts() {
    test_support::assert_accepts_la("functio from_list(lista<u8> b) → octeti { redde b ↦ octeti }");
}

#[test]
fn octeti_to_lista_textus_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ b ← |de ad| ↦ lista<textus> }",
        "unsupported_conversio",
    );
}

#[test]
fn octeti_to_lista_unsized_numerus_rejects() {
    // An unsized numerus element has no bit width to match: the bits row
    // names it (`lista<b8>` converts to a sized same-width `lista<uN>` only).
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ b ← |de ad| ↦ lista<numerus> }",
        "bits_conversion_width_mismatch",
    );
}

#[test]
fn numeric_to_octeti_unspecified_unchanged() {
    test_support::assert_rejects_with_issue_la(
        "incipit { fixum _ b ← 42 ↦ octeti }",
        "numeric_to_octeti_unspecified",
    );
}

// --- write inverse: `W ↦ octeti<N> via Be|Le` -------------------------------

#[test]
fn scalar_to_octeti_all_widths_and_endians_accept() {
    for (ty, width) in [
        ("u16", 2),
        ("i16", 2),
        ("u32", 4),
        ("i32", 4),
        ("f32", 4),
        ("u64", 8),
        ("i64", 8),
        ("f64", 8),
    ] {
        for hint in ["Le", "Be"] {
            let source = format!(
                "functio w({ty} n) → octeti<{width}> {{ redde n ↦ octeti<{width}> via {hint} }}"
            );
            test_support::assert_accepts_la(&source);
        }
    }
}

#[test]
fn scalar_to_octeti_without_endian_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → octeti<2> { redde n ↦ octeti<2> }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn scalar_to_octeti_capacity_mismatch_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → octeti<4> { redde n ↦ octeti<4> via Le }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn scalar_to_octeti_unsized_numerus_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(numerus n) → octeti<2> { redde n ↦ octeti<2> via Le }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn scalar_to_octeti_u8_source_rejects() {
    // Ruling B6: an 8-bit value has no byte order, so the row is not built.
    test_support::assert_rejects_with_issue_la(
        "functio w(u8 n) → octeti<1> { redde n ↦ octeti<1> via Le }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn scalar_to_octeti_unbounded_target_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → octeti { redde n ↦ octeti via Le }",
        "numeric_to_octeti_unspecified",
    );
}
