//! Typecheck pins for unbounded `octeti[i]` and `octeti[lo‥hi]`. A byte list is
//! `lista<b8>`: `buf[i]` is one `b8` (bits are not numbers, ruling B1 of need
//! 6a13d711), `buf[lo‥hi]` is a new byte list.

use crate::test_support;

#[test]
fn octeti_scalar_index_accepts_as_numerus_u8() {
    // The element is a `b8`; a `u8` is reached only by `↦ u8`.
    test_support::assert_accepts("functio at(lista<b8> b, numerus i) → b8 { redde b[i] }");
    test_support::assert_accepts("functio at(lista<b8> b, numerus i) → u8 { redde b[i] ↦ u8 }");
    test_support::assert_rejects_with_issue(
        "functio at(lista<b8> b, numerus i) → u8 { redde b[i] }",
        "return_type_mismatch",
    );
}

#[test]
fn octeti_range_index_accepts_as_octeti() {
    test_support::assert_accepts("functio win(lista<b8> b) → lista<b8> { redde b[0‥4] }");
}

#[test]
fn octeti_index_rejects_textus_key() {
    test_support::assert_rejects_with_issue(
        r#"functio at(lista<b8> b) → b8 { redde b["x"] }"#,
        "array_index_must_be_numerus",
    );
}

#[test]
fn lista_window_accepts_as_new_lista() {
    test_support::assert_accepts(
        "functio win(lista<numerus> xs) → lista<numerus> { redde xs[0‥4] }",
    );
}

#[test]
fn lista_textus_window_accepts_as_new_lista() {
    test_support::assert_accepts("functio win(lista<textus> xs) → lista<textus> { redde xs[1‥3] }");
}

#[test]
fn lista_bits_window_accepts_as_new_lista() {
    test_support::assert_accepts("functio win(lista<b8> xs) → lista<b8> { redde xs[0‥2] }");
}

#[test]
fn lista_window_is_not_an_element() {
    test_support::assert_rejects_with_issue(
        "functio win(lista<numerus> xs) → numerus { redde xs[0‥4] }",
        "return_type_mismatch",
    );
}

#[test]
fn lista_window_write_rejects() {
    test_support::assert_rejects_with_issue(
        "functio win(lista<numerus> xs) { xs[0‥2] ← [1, 2] }",
        "array_index_must_be_numerus",
    );
}

#[test]
fn window_over_a_number_rejects() {
    test_support::assert_rejects_with_issue(
        "functio win(numerus n) → numerus { redde n[0‥2] }",
        "window_requires_list",
    );
}

#[test]
fn octeti_literal_static_range_oob_rejects() {
    test_support::assert_rejects_with_issue(
        "functio win() → lista<b8> { redde |de ad be ef|[0‥5] }",
        "octeti_index_out_of_bounds",
    );
}

#[test]
fn octeti_literal_in_bounds_range_accepts() {
    test_support::assert_accepts("functio win() → lista<b8> { redde |de ad be ef|[0‥4] }");
}
