//! Need: an optional value (`T ∪ nihil`) stored into a declared cell
//! that cannot hold `nihil` is `SEM010 optional_into_plain_cell`, not a silent
//! unwrap. Narrowing, `vel` and an optional destination stay accepted.
//!
//! Fixtures use the latin spellings (the unit-test harness has no en pack).

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

const ISSUE: &str = "optional_into_plain_cell";

#[test]
fn optional_parameter_into_plain_declaration_is_rejected() {
    assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → textus {\n    fixum textus y ← x\n    redde y\n}",
        ISSUE,
    );
}

#[test]
fn optional_method_result_into_plain_declaration_is_rejected() {
    assert_rejects_with_issue(
        r"incipit {
    fixum lista<fractus> xs ← vacua
    fixum numerus first ← xs.argmaxima()
    nota first
}",
        ISSUE,
    );
}

#[test]
fn optional_into_plain_assignment_is_rejected() {
    assert_rejects_with_issue(
        "functio f(numerus ∪ nihil x) {\n    varia numerus y ← 0\n    y ← x\n    nota y\n}",
        ISSUE,
    );
}

#[test]
fn narrowed_optional_into_plain_declaration_is_accepted() {
    assert_accepts(
        "functio f(textus ∪ nihil x) → textus {\n    si x non est nihil {\n        fixum textus y ← x\n        redde y\n    }\n    redde \"\"\n}",
    );
}

#[test]
fn coalesced_optional_into_plain_declaration_is_accepted() {
    assert_accepts(
        r"incipit {
    fixum lista<fractus> xs ← vacua
    fixum numerus first ← xs.argmaxima() vel 0
    nota first
}",
    );
}

#[test]
fn optional_into_optional_declaration_is_accepted() {
    assert_accepts(
        "functio f(textus ∪ nihil x) → textus ∪ nihil {\n    fixum textus ∪ nihil y ← x\n    redde y\n}",
    );
}

#[test]
fn optional_into_inferred_declaration_is_accepted() {
    assert_accepts(
        "functio f(textus ∪ nihil x) → textus ∪ nihil {\n    fixum _ y ← x\n    redde y\n}",
    );
}

#[test]
fn plain_into_optional_and_plain_into_plain_are_accepted() {
    assert_accepts(
        "functio f(textus x) → textus {\n    fixum textus ∪ nihil y ← x\n    fixum textus z ← x\n    redde z\n}",
    );
}

// Need: the same named error at argument, return, field-init, throw,
// yield, element and element-assignment stores.

#[test]
fn optional_argument_into_plain_parameter_is_rejected() {
    assert_rejects_with_issue(
        "functio take(numerus v) → vacuum {\n    nota v\n}\nfunctio f(numerus ∪ nihil x) {\n    take(x)\n}",
        ISSUE,
    );
}

#[test]
fn optional_return_into_plain_return_type_is_rejected() {
    assert_rejects_with_issue(
        "functio f(numerus ∪ nihil x) → numerus {\n    redde x\n}",
        ISSUE,
    );
}

#[test]
fn optional_field_initializer_into_plain_field_is_rejected() {
    assert_rejects_with_issue(
        "genus Arca {\n    fixum numerus a\n}\nfunctio f(numerus ∪ nihil x) → Arca {\n    redde Arca { a = x }\n}",
        ISSUE,
    );
}

#[test]
fn optional_throw_into_plain_error_type_is_rejected() {
    assert_rejects_with_issue(
        "functio f(numerus ∪ nihil x) → numerus ⇥ numerus {\n    iace x\n}",
        ISSUE,
    );
}

#[test]
fn optional_yield_into_plain_element_type_is_rejected() {
    assert_rejects_with_issue(
        "functio f(numerus ∪ nihil x) fiunt → numerus {\n    cede x\n}",
        ISSUE,
    );
}

#[test]
fn optional_element_into_plain_list_literal_is_rejected() {
    assert_rejects_with_issue(
        "functio f(numerus ∪ nihil x) {\n    fixum lista<numerus> xs ← [x, 1]\n    nota xs\n}",
        ISSUE,
    );
}

#[test]
fn optional_element_assignment_into_plain_list_is_rejected() {
    assert_rejects_with_issue(
        "functio f(numerus ∪ nihil x) {\n    varia lista<numerus> xs ← [1]\n    xs[0] ← x\n    nota xs\n}",
        ISSUE,
    );
}

#[test]
fn optional_argument_into_optional_parameter_is_accepted() {
    assert_accepts(
        "functio take(numerus ∪ nihil v) → vacuum {\n    nota v\n}\nfunctio f(numerus ∪ nihil x) {\n    take(x)\n}",
    );
}

#[test]
fn coalesced_and_narrowed_optional_at_other_stores_are_accepted() {
    assert_accepts(
        "functio take(numerus v) → vacuum {\n    nota v\n}\nfunctio f(numerus ∪ nihil x) → numerus {\n    take(x vel 0)\n    si x non est nihil {\n        take(x)\n        redde x\n    }\n    redde x vel 0\n}",
    );
}

// A `sponte` parameter or field is an optional target: an optional value
// stored into it is an optional-into-optional store, not an unwrap.

#[test]
fn optional_argument_into_sponte_parameter_is_accepted() {
    assert_accepts(
        "functio greet(textus nomen sponte) {\n    nota nomen\n}\n\nfunctio f(textus ∪ nihil x) {\n    greet(x)\n}",
    );
}

#[test]
fn optional_into_sponte_field_initializer_is_accepted() {
    assert_accepts(
        "genus Node {\n    varia textus name\n    varia textus parent sponte\n}\n\nfunctio f(textus ∪ nihil p) → Node {\n    redde Node {\n        name = \"n\",\n        parent = p\n    }\n}",
    );
}
