//! `valor` ↔ `tabula<K, V>` conversions require a textus key (need,
//! G-01a). The conversion never parses keys, so a non-text key is rejected in
//! both directions before any target can fail closed on its own.

use crate::passes::typecheck::test_support;

#[test]
fn valor_to_map_with_int_key_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit {
  fixum valor v ← { "a": 1 } ↦ valor
  fixum tabula<numerus, textus> m ← v ↦ tabula<numerus, textus>
}"#,
        "conversio_map_key_not_textus",
    );
}

#[test]
fn map_with_int_key_to_value_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
  fixum tabula<numerus, textus> m ← vacua
  fixum valor v ← m ↦ valor
}",
        "conversio_map_key_not_textus",
    );
}

#[test]
fn valor_to_map_with_text_key_checks() {
    test_support::assert_accepts_la(
        r#"incipit {
  fixum valor v ← { "a": 1 } ↦ valor
  fixum tabula<textus, numerus> m ← v ↦ tabula<textus, numerus>
}"#,
    );
}

#[test]
fn valor_to_nested_map_with_int_key_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit {
  fixum valor v ← { "a": 1 } ↦ valor
  fixum lista<tabula<numerus, textus>> m ← v ↦ lista<tabula<numerus, textus>>
}"#,
        "conversio_map_key_not_textus",
    );
}
