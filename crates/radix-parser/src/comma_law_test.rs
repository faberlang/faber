//! Focused proofs for the two-family comma separator law.

use crate::test_support::{parse_ok_la, parse_program_la};

fn assert_issue(source: &str, issue: &str, position: &str) {
    let result = parse_program_la(source);
    assert!(
        result.errors.iter().any(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "position" && arg.value == position)
        }),
        "expected {issue} at {position}, got {:?}",
        result.errors
    );
}

#[test]
fn missing_discretio_variant_separator_is_targeted() {
    assert_issue(
        "discretio Color { Rubrum Viridis }",
        "item_list_missing_separator",
        "discretio_variants",
    );
}

#[test]
fn discretio_payload_comma_is_targeted() {
    assert_issue(
        "discretio Color { Rubrum { numerus value, numerus other } }",
        "declaration_block_comma",
        "discretio_payload",
    );
}

#[test]
fn comma_less_discretio_payload_remains_lawful() {
    let _ = parse_ok_la("discretio Color { Rubrum { numerus red numerus green } }");
}

#[test]
fn item_list_trailing_comma_is_targeted() {
    assert_issue(
        "functio f(numerus value,) → vacuum { }",
        "item_list_trailing_comma",
        "function_parameters",
    );
}

#[test]
fn iuncta_type_arguments_trailing_comma_is_targeted() {
    assert_issue(
        "incipit { fixum _ pair ← iuncta<textus, numerus,> [\"salve\", 42] }",
        "item_list_trailing_comma",
        "type_arguments",
    );
}
