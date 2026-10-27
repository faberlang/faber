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
    test_support::assert_accepts_la("incipit { sit _ n ← |de ad be ef|[0‥4] ↦ u32 via Be }");
}

#[test]
fn octeti_literal_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be ef| ↦ u32 }",
        "octeti_to_numerus_unspecified",
    );
}

#[test]
fn octeti_window_without_endian_still_unspecified() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be ef|[0‥4] ↦ u32 }",
        "octeti_to_numerus_unspecified",
    );
}

#[test]
fn three_byte_source_mismatches_u32() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be| ↦ u32 via Be }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn three_byte_window_mismatches_u32() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be|[0‥3] ↦ u32 via Be }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn four_byte_window_mismatches_u16() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be ef|[0‥4] ↦ u16 via Be }",
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
        "incipit { sit _ n ← |de ad be ef|[0‥4] ↦ f32 }",
        "octeti_to_fractus_unspecified",
    );
}

#[test]
fn three_byte_window_mismatches_f32() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be|[0‥3] ↦ f32 via Be }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn four_byte_window_mismatches_f64() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be ef|[0‥4] ↦ f64 via Le }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn hex_hint_on_fractus_still_rejected() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit _ n ← |de ad be ef|[0‥4] ↦ f32 via Hex }",
        "octeti_to_fractus_unspecified",
    );
}

#[test]
fn textus_hex_parse_still_accepts() {
    test_support::assert_accepts_la(r#"incipit { sit _ n ← "ff" ↦ i32 via Hex }"#);
}

// --- bounded `octeti<N> ↦ W via Be|Le` read -----------------

#[test]
fn bounded_octeti_endian_read_accepts_all_widths() {
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
            let source =
                format!("functio r(lista<b8, {width}> b) → {ty} {{ redde b ↦ {ty} via {hint} }}");
            test_support::assert_accepts_la(&source);
        }
    }
}

#[test]
fn bounded_octeti_endian_read_without_hint_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio r(lista<b8, 2> b) → u16 { redde b ↦ u16 }",
        "octeti_to_numerus_unspecified",
    );
    test_support::assert_rejects_with_issue_la(
        "functio r(lista<b8, 4> b) → f32 { redde b ↦ f32 }",
        "octeti_to_fractus_unspecified",
    );
}

#[test]
fn bounded_octeti_endian_read_too_small_capacity_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio r(lista<b8, 2> b) → u32 { redde b ↦ u32 via Le }",
        "octeti_endian_width_mismatch",
    );
    test_support::assert_rejects_with_issue_la(
        "functio r(lista<b8, 4> b) → f64 { redde b ↦ f64 via Le }",
        "octeti_endian_width_mismatch",
    );
}

#[test]
fn bounded_octeti_endian_read_larger_capacity_accepts() {
    test_support::assert_accepts_la("functio r(lista<b8, 8> b) → u16 { redde b ↦ u16 via Le }");
}

#[test]
fn bounded_octeti_endian_read_u8_target_rejects() {
    test_support::assert_rejects_with_issue_la(
        "functio r(lista<b8, 1> b) → u8 { redde b ↦ u8 via Le }",
        "octeti_to_numerus_unspecified",
    );
}

#[test]
fn bounded_octeti_to_bivalens_stays_rejected() {
    test_support::assert_rejects_with_issue_la(
        "functio r(lista<b8, 2> b) → bivalens { redde b ↦ bivalens }",
        "unsupported_conversio",
    );
}
