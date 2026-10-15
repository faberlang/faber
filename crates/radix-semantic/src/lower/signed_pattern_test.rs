//! Signed and NaN literal patterns (DECISIONS D22.2, D22.3).
//!
//! Contract: a negated number literal is a legal pattern in `elige`, lowered
//! to `HirPattern::NegatedLiteral` over the unsigned magnitude and typed like
//! the literal it negates. `discerne` matches closed sets only (M2), so a
//! negated literal there is refused as `discerne_literal_arm_not_in_closed_set`.
//! A NaN pattern is rejected as SEM008 `nan_pattern` in `elige`; a named NaN
//! constant cannot reach a pattern because `elige` accepts only literals and
//! variants, and `discerne` treats a bare name as a binding.

use crate::test_support::{analyze_ok_la, assert_accepts_la, assert_rejects_with_issue_la};
use radix_hir::{HirExpressionKind, HirLiteral, HirPattern, HirStatementKind};

/// First-arm head patterns of the first `discerne`/`elige` in the entry block.
fn entry_match_patterns(source: &str) -> Vec<HirPattern> {
    let fixture = analyze_ok_la(source);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("entry block");
    for stmt in &entry.statements {
        if let HirStatementKind::Expr(expr) = &stmt.kind
            && let HirExpressionKind::Discerne { arms, .. } = &expr.kind
        {
            return arms.iter().flat_map(|arm| arm.patterns.clone()).collect();
        }
    }
    panic!("no discerne/elige in entry block");
}

#[test]
fn elige_accepts_negated_decimal_and_infinity_patterns() {
    let patterns = entry_match_patterns(
        "incipit {\n    sit fractus x ← -2.5\n    elige x {\n        casu -1.0 { tacet }\n        casu -2.5 { tacet }\n        casu -∞ { tacet }\n        ceterum { tacet }\n    }\n}",
    );
    assert!(matches!(
        patterns[0],
        HirPattern::NegatedLiteral(HirLiteral::FloatText { value, .. }) if value == 1.0
    ));
    assert!(matches!(
        patterns[2],
        HirPattern::NegatedLiteral(HirLiteral::Float(value)) if value.is_infinite()
    ));
}

#[test]
fn discerne_refuses_negated_integer_pattern() {
    assert_rejects_with_issue_la(
        "incipit {\n    sit numerus n ← -1\n    discerne n {\n        casu -1 { tacet }\n        casu _ { tacet }\n    }\n}",
        "discerne_literal_arm_not_in_closed_set",
    );
}

#[test]
fn elige_accepts_negated_number_case_values() {
    let patterns = entry_match_patterns(
        "incipit {\n    sit numerus n ← -1\n    elige n {\n        casu -1 { tacet }\n        casu 0 { tacet }\n        ceterum { tacet }\n    }\n}",
    );
    assert!(matches!(
        patterns[0],
        HirPattern::NegatedLiteral(HirLiteral::Int(1))
    ));
    assert_accepts_la(
        "incipit {\n    sit fractus x ← -∞\n    elige x {\n        casu -∞ { tacet }\n        casu -0.5 { tacet }\n        ceterum { tacet }\n    }\n}",
    );
}

#[test]
fn elige_rejects_negation_over_a_non_number() {
    assert_rejects_with_issue_la(
        "incipit {\n    sit bivalens b ← verum\n    elige b {\n        casu -verum { tacet }\n        ceterum { tacet }\n    }\n}",
        "elige_case_value_shape",
    );
}

#[test]
fn negated_pattern_against_textus_is_a_type_mismatch() {
    assert_rejects_with_issue_la(
        "incipit {\n    sit textus s ← \"a\"\n    elige s {\n        casu -1 { tacet }\n        ceterum { tacet }\n    }\n}",
        "pattern_type_mismatch",
    );
}

#[test]
fn nan_pattern_is_rejected_in_discerne() {
    assert_rejects_with_issue_la(
        "incipit {\n    sit fractus x ← nonnumerus\n    discerne x {\n        casu nonnumerus { tacet }\n        casu _ { tacet }\n    }\n}",
        "discerne_literal_arm_not_in_closed_set",
    );
}

#[test]
fn nan_pattern_is_rejected_in_elige() {
    assert_rejects_with_issue_la(
        "incipit {\n    sit fractus x ← nonnumerus\n    elige x {\n        casu nonnumerus { tacet }\n        ceterum { tacet }\n    }\n}",
        "nan_pattern",
    );
    assert_rejects_with_issue_la(
        "incipit {\n    sit fractus x ← nonnumerus\n    elige x {\n        casu -nonnumerus { tacet }\n        ceterum { tacet }\n    }\n}",
        "nan_pattern",
    );
}

#[test]
fn named_nan_constant_cannot_reach_an_elige_pattern() {
    // No NaN spelling other than the literal reaches a pattern: an `elige`
    // case name must resolve to a variant.
    assert_rejects_with_issue_la(
        "sit fractus NAN ← nonnumerus\nincipit {\n    sit fractus x ← 1.0\n    elige x {\n        casu NAN { tacet }\n        ceterum { tacet }\n    }\n}",
        "elige_case_value_shape",
    );
}
