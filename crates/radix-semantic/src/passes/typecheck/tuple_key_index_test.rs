//! D3.2/D3.3: a composite-key `tabula<iuncta<K1, K2>, V>` is rank N, so its
//! bracket list is the key: `m[[k1, k2]]` reads and writes the entry whose
//! key is the `iuncta` of those parts.
//!
//! Assertions are structured (`issue`), never rendered prose.

use crate::test_support;

#[test]
fn bracket_list_reads_and_writes_tuple_key_entry() {
    test_support::assert_accepts_la(
        r#"
incipit {
    varia tabula<iuncta<textus, numerus>, numerus> m ← vacua
    fixum textus regio ← "east"
    m[[regio, 2024]] ← 5
    fixum numerus v ← m[[regio, 2024]]
    nota v
}"#,
    );
}

#[test]
fn bracket_list_with_wrong_part_count_rejects() {
    let result = test_support::analyze_source_la(
        r#"
incipit {
    varia tabula<iuncta<textus, numerus>, numerus> m ← vacua
    m[["east"]] ← 5
}"#,
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && err.issue() == Some("map_index_type_mismatch")),
        "expected map_index_type_mismatch, got {:?}",
        result.diagnostics
    );
}

#[test]
fn bracket_list_with_wrong_part_type_rejects() {
    let result = test_support::analyze_source_la(
        r#"
incipit {
    varia tabula<iuncta<textus, numerus>, numerus> m ← vacua
    nota m[[2024, "east"]]
}"#,
    );
    assert!(
        !result.is_ok(),
        "expected a type error, got {:?}",
        result.diagnostics
    );
}
