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
