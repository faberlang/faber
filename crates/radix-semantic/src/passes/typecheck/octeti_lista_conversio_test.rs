//! Typecheck pins for `octeti ↦ lista<numerus<u8>>` (OEC-2 boundary bridge).

use crate::test_support;

#[test]
fn octeti_to_lista_u8_accepts() {
    test_support::assert_accepts_la(
        "functio bridge(lista<b8> o) → lista<u8> { redde o ↦ lista<u8> }",
    );
}

#[test]
fn octeti_literal_to_lista_u8_accepts() {
    test_support::assert_accepts_la("incipit { sit _ b ← |de ad be ef| ↦ lista<u8> }");
}

#[test]
fn lista_u8_to_octeti_still_accepts() {
    test_support::assert_accepts_la(
        "functio from_list(lista<u8> b) → lista<b8> { redde b ↦ lista<b8> }",
    );
}

#[test]
fn octeti_to_lista_textus_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ b ← |de ad| ↦ lista<textus> }",
        "unsupported_conversio",
    );
}

#[test]
fn octeti_to_lista_unsized_numerus_rejects() {
    // An unsized numerus element has no bit width to match: the bits row
    // names it (`lista<b8>` converts to a sized same-width `lista<uN>` only).
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ b ← |de ad| ↦ lista<numerus> }",
        "bits_conversion_width_mismatch",
    );
}

#[test]
fn numeric_to_octeti_unspecified_unchanged() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ b ← 42 ↦ lista<b8> }",
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
                "functio w({ty} n) → lista<b8, magnitudo {width}> {{ redde n ↦ lista<b8, magnitudo {width}> via {hint} }}"
            );
            test_support::assert_accepts_la(&source);
        }
    }
}

#[test]
fn scalar_to_octeti_without_endian_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → lista<b8, magnitudo 2> { redde n ↦ lista<b8, magnitudo 2> }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn scalar_to_octeti_capacity_mismatch_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → lista<b8, magnitudo 4> { redde n ↦ lista<b8, magnitudo 4> via Le }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn scalar_to_octeti_unsized_numerus_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(numerus n) → lista<b8, magnitudo 2> { redde n ↦ lista<b8, magnitudo 2> via Le }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn scalar_to_octeti_u8_source_rejects() {
    // Ruling B6: an 8-bit value has no byte order, so the row is not built.
    test_support::assert_rejects_with_issue_la(
        "functio w(u8 n) → lista<b8, magnitudo 1> { redde n ↦ lista<b8, magnitudo 1> via Le }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn scalar_to_octeti_unbounded_target_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → lista<b8> { redde n ↦ lista<b8> via Le }",
        "numeric_to_octeti_unspecified",
    );
}

// --- `lista<b8, magnitudo N>` is the bounded buffer spelling ----------

#[test]
fn lista_b8_n_write_row_all_widths_accept() {
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
                "functio w({ty} n) → lista<b8, magnitudo {width}> {{ redde n ↦ lista<b8, magnitudo {width}> via {hint} }}"
            );
            test_support::assert_accepts_la(&source);
        }
    }
}

#[test]
fn lista_b8_n_write_row_without_hint_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → lista<b8, magnitudo 2> { redde n ↦ lista<b8, magnitudo 2> }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn lista_b8_n_write_row_capacity_mismatch_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u16 n) → lista<b8, magnitudo 4> { redde n ↦ lista<b8, magnitudo 4> via Le }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn lista_b8_n_u8_source_has_no_row() {
    test_support::assert_rejects_with_issue_la(
        "functio w(u8 n) → lista<b8, magnitudo 1> { redde n ↦ lista<b8, magnitudo 1> via Le }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn lista_b8_one_byte_is_b8_conversion() {
    test_support::assert_accepts_la("functio w(u8 n) → b8 { redde n ↦ b8 }");
    test_support::assert_accepts_la("functio w(i8 n) → b8 { redde n ↦ b8 }");
}

#[test]
fn lista_b8_n_is_octeti_n() {
    test_support::assert_accepts_la(
        "functio w(lista<b8, magnitudo 2> o) → lista<b8, magnitudo 2> { redde o }",
    );
    test_support::assert_accepts_la(
        "functio w(lista<b8, magnitudo 2> o) → lista<b8, magnitudo 2> { redde o }",
    );
}
