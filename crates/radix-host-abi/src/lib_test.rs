//! Structural tests for the host ABI symbol table.

use super::{GRADIENT_SYMBOL_ROWS, SYMBOL_ROWS};
use std::collections::HashMap;

fn all_rows() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    SYMBOL_ROWS.iter().chain(GRADIENT_SYMBOL_ROWS)
}

#[test]
fn genus_field_policy_family_is_a_closed_two_row_contract() {
    // The genus field-layout contract consumed by `valor_get_genus` callers:
    // two codes, derived from one declaration list. The LLVM emitter and the
    // host descriptor-table callers read these codes by name.
    use super::{
        GENUS_FIELD_POLICY_DEFAULTABLE, GENUS_FIELD_POLICY_MANDATORY, GENUS_FIELD_POLICY_ROWS,
    };
    assert_eq!(
        GENUS_FIELD_POLICY_ROWS,
        &[
            ("GENUS_FIELD_POLICY_MANDATORY", 0u32),
            ("GENUS_FIELD_POLICY_DEFAULTABLE", 1u32)
        ]
    );
    assert_eq!(u32::from(GENUS_FIELD_POLICY_MANDATORY), 0);
    assert_eq!(u32::from(GENUS_FIELD_POLICY_DEFAULTABLE), 1);
}

#[test]
fn symbol_names_and_values_are_unique() {
    let mut by_name: HashMap<&str, &str> = HashMap::new();
    let mut by_value: HashMap<&str, &str> = HashMap::new();
    for (name, value) in all_rows() {
        assert!(
            by_name.insert(name, value).is_none(),
            "duplicate constant name {name}"
        );
        if let Some(previous) = by_value.insert(value, name) {
            panic!("symbol {value} is declared twice: {previous} and {name}");
        }
    }
}

#[test]
fn escape_and_match_accessors_have_one_spelling() {
    let count = |symbol: &str| all_rows().filter(|(_, value)| *value == symbol).count();
    for shared in [
        "__faber_rt_v1_text_escape",
        "__faber_rt_v1_match_text",
        "__faber_rt_v1_match_start",
        "__faber_rt_v1_match_end",
        "__faber_rt_v1_match_group",
        "__faber_rt_v1_match_named",
    ] {
        assert_eq!(count(shared), 1, "{shared} must be one row");
    }
}

#[test]
fn regex_family_rows_are_operations_on_the_regex_value() {
    // `regex_*` names operations whose receiver is the regex value; escape
    // belongs to `text_*` and the match accessors to `match_*`.
    const REGEX_RECEIVER_OPERATIONS: &[&str] = &[
        "from_text",
        "from_ascii",
        "get_text",
        "literal_1_ptr_to_ptr",
        "matches",
        "find",
        "find_all",
        "split",
        "replace",
        "replace_each",
    ];
    for (name, value) in all_rows() {
        if let Some(operation) = value.strip_prefix("__faber_rt_v1_regex_") {
            assert!(
                REGEX_RECEIVER_OPERATIONS.contains(&operation),
                "{name} ({value}) is not an operation on the regex value"
            );
        }
    }
}
