//! Typecheck pins for unbounded `octeti[i]` and `octeti[lo‥hi]`.

use crate::test_support;

#[test]
fn octeti_scalar_index_accepts_as_numerus_u8() {
    test_support::assert_accepts("functio at(octeti b, numerus i) → u8 { redde b[i] }");
}

#[test]
fn octeti_range_index_accepts_as_octeti() {
    test_support::assert_accepts("functio win(octeti b) → octeti { redde b[0‥4] }");
}

#[test]
fn octeti_index_rejects_textus_key() {
    test_support::assert_rejects_with_issue(
        r#"functio at(octeti b) → u8 { redde b["x"] }"#,
        "octeti_index_must_be_numerus_or_range",
    );
}

#[test]
fn lista_range_index_still_rejects() {
    test_support::assert_rejects_with_issue(
        "functio win(lista<numerus> xs) → lista<numerus> { redde xs[0‥4] }",
        "array_index_must_be_numerus",
    );
}

#[test]
fn octeti_literal_static_range_oob_rejects() {
    test_support::assert_rejects_with_issue(
        "functio win() → octeti { redde |de ad be ef|[0‥5] }",
        "octeti_index_out_of_bounds",
    );
}

#[test]
fn octeti_literal_in_bounds_range_accepts() {
    test_support::assert_accepts("functio win() → octeti { redde |de ad be ef|[0‥4] }");
}
