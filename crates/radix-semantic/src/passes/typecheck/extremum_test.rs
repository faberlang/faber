//! M2 (reduction-verbs-minmax) typecheck drivers.
//!
//! These tests pin shaped-source admission, shared `apud` arity, nullable
//! dynamic-shape results, static non-empty elision, and caller identities.

use crate::test_support;

fn assert_accepts(source: &str) {
    test_support::assert_accepts(source);
}

fn assert_rejects_with_issue(source: &str, issue: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected {issue:?}, got {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.is_error() && diagnostic.issue() == Some(issue)),
        "missing {issue:?} in {:?}",
        result.diagnostics
    );
}

#[test]
fn static_non_empty_extrema_elide_nihil() {
    assert_accepts(
        r"
functio maximum(tensor<f32, [4]> xs) → f32 {
    redde maxima ex xs apud [i]
}

functio minimum(tensor<f32, [4, 8]> xs) → f32 {
    redde minima ex xs apud [i, j]
}
",
    );
}

#[test]
fn dynamic_extrema_produce_nullable_results() {
    assert_accepts(
        r"
functio maximum<magnitudo N>(tensor<f32, [N]> xs) → f32 ∪ nihil {
    redde maxima ex xs apud [i]
}
",
    );
}

#[test]
fn binderless_extrema_keep_nullable_and_coalesce_typing() {
    assert_accepts(
        r"
functio maximum<magnitudo N>(tensor<f32, [N]> xs) → f32 ∪ nihil {
    redde maxima ex xs
}

functio minimum<magnitudo N>(tensor<f32, [N]> xs, f32 identity) → f32 {
    redde minima ex xs vel identity
}
",
    );
    assert_rejects_with_issue(
        r"
functio maximum<magnitudo N>(tensor<f32, [N]> xs) → f32 {
    redde maxima ex xs
}
",
        "extremum_nullable_result_requires_coalesce",
    );
}

#[test]
fn caller_coalesce_identity_eliminates_dynamic_nihil() {
    assert_accepts(
        r"
functio maximum<magnitudo N>(tensor<f32, [N]> xs, f32 identity) → f32 {
    redde maxima ex xs apud [i] vel identity
}
",
    );
}

#[test]
fn dynamic_nihil_cannot_flow_silently_into_plain_t() {
    assert_rejects_with_issue(
        r"
functio maximum<magnitudo N>(tensor<f32, [N]> xs) → f32 {
    redde maxima ex xs apud [i]
}
",
        "extremum_nullable_result_requires_coalesce",
    );
}

#[test]
fn identity_must_match_the_element_type() {
    assert_rejects_with_issue(
        r#"
functio maximum(tensor<f32, [4]> xs) → f32 {
    redde maxima ex xs apud [i] vel ""
}
"#,
        "extremum_identity_type_mismatch",
    );
}

#[test]
fn extrema_reuse_shaped_apud_and_numeric_element_laws() {
    assert_rejects_with_issue(
        r"
functio maximum(matrix<f32, [4, 8]> xs) → f32 {
    redde maxima ex xs apud [i]
}
",
        "itera_apud_arity_mismatch",
    );
    assert_rejects_with_issue(
        r"
functio maximum(tensor<textus, [4]> xs) → textus {
    redde maxima ex xs apud [i]
}
",
        "extremum_term_type_non_scalar",
    );
}
