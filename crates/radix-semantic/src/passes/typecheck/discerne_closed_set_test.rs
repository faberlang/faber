//! `discerne` matches a closed set (union members, enum variants) only.
//!
//! Operator ruling 2026-10-02: numeric-width and length-shape type patterns are
//! not set discrimination. A type pattern whose target is not a member of the
//! scrutinee's closed set is `discerne_pattern_not_in_closed_set`.

use crate::test_support;

const ISSUE: &str = "discerne_pattern_not_in_closed_set";

#[test]
fn bounded_list_pattern_over_unbounded_list_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(lista<numerus> values) → vacuum {
    discerne values {
        casu lista<numerus, 4> { tacet }
        casu lista<numerus> { tacet }
    }
}",
        ISSUE,
    );
}

#[test]
fn numeric_width_pattern_over_numerus_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(numerus values) → vacuum {
    discerne values {
        casu i32 { tacet }
        casu numerus { tacet }
    }
}",
        ISSUE,
    );
}

#[test]
fn rejection_names_pattern_and_scrutinee_types() {
    let diagnostics = test_support::analyze_and_get_diagnostics(
        r"functio probe(lista<numerus> values) → vacuum {
    discerne values {
        casu lista<numerus, 4> { tacet }
        casu lista<numerus> { tacet }
    }
}",
    );
    let error = diagnostics
        .iter()
        .find(|err| test_support::semantic_error_has_issue(err, ISSUE))
        .expect("closed-set rejection");
    let names: Vec<&str> = error.args.iter().map(|arg| arg.name).collect();
    assert!(names.contains(&"pattern"), "args: {:?}", error.args);
    assert!(names.contains(&"scrutinee"), "args: {:?}", error.args);
}

#[test]
fn family_pattern_over_a_union_that_lacks_it_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"functio probe(lista<numerus, 4> ∪ lista<numerus, 8> values) → vacuum {
    discerne values {
        casu lista<numerus> { tacet }
    }
}",
        ISSUE,
    );
}

#[test]
fn union_member_type_pattern_is_accepted() {
    test_support::assert_accepts(
        r"functio probe(lista<numerus, 4> ∪ lista<numerus, 8> values) → vacuum {
    discerne values {
        casu lista<numerus, 4> { tacet }
        casu lista<numerus, 8> { tacet }
    }
}",
    );
}

#[test]
fn numeric_member_of_a_union_is_accepted() {
    test_support::assert_accepts(
        r"functio probe(i32 ∪ textus value) → vacuum {
    discerne value {
        casu i32 { tacet }
        casu textus { tacet }
    }
}",
    );
}
