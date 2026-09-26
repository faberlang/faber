//! Flow narrowing into `✓ ✗` branches (DECISIONS D17.5).
//!
//! The value conditional's branches narrow exactly like `si` branches: after
//! `r est numerus`, `r` is `numerus` in the `✓` branch. The rejections prove
//! the acceptances are real narrowing, not a permissive union.

use crate::test_support;

#[test]
fn type_test_narrows_the_check_branch() {
    test_support::assert_accepts(
        "functio f(numerus ∪ textus r) → numerus { redde r est numerus ✓ r ✗ 0 }",
    );
}

#[test]
fn wrong_type_test_does_not_narrow_to_the_return_type() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus ∪ textus r) → numerus { redde r est textus ✓ r ✗ 0 }",
        "return_type_mismatch",
    );
}

#[test]
fn non_null_test_narrows_the_check_branch() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus { redde x non est nihil ✓ x.longitudo() ✗ 0 }",
    );
}

#[test]
fn null_test_narrows_the_ballot_branch() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus { redde x est nihil ✓ 0 ✗ x.longitudo() }",
    );
}

#[test]
fn narrowing_does_not_reach_the_wrong_branch() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus { redde x non est nihil ✓ 0 ✗ x.longitudo() }",
        "optional_receiver_requires_chain",
    );
}
