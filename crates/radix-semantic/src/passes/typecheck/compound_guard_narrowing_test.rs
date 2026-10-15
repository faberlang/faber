//! Narrowing through compound conditions: `A et B` holds both
//! parts' facts in the taken branch, a diverging `si A aut B { <leave> }`
//! leaves both negations for the rest of the block, and `non (...)` swaps the
//! two edges. A field path narrows inside a branch body only — not across
//! joins or the implicit fallthrough (need case 2).
//!
//! Fixtures use the latin spellings (`et`, `aut`, `non`); the unit-test
//! harness compiles bare source without the en-locale lex pack. The en
//! spellings (`and`, `or`, `not`) lower to the same HIR operators.

use crate::test_support;

const NOT_NARROWED: &str = "optional_receiver_requires_chain";

#[test]
fn and_narrows_non_leftmost_conjunct() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    si a et x non est nihil {\n        redde x.longitudo()\n    }\n    redde 0\n}",
    );
}

#[test]
fn and_narrows_both_conjuncts() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, textus ∪ nihil y) → numerus {\n    si x non est nihil et y non est nihil {\n        redde x.longitudo() + y.longitudo()\n    }\n    redde 0\n}",
    );
}

#[test]
fn nested_and_narrows_at_any_depth() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, bivalens a, bivalens b) → numerus {\n    si a et (b et x non est nihil) {\n        redde x.longitudo()\n    }\n    redde 0\n}",
    );
}

#[test]
fn and_does_not_narrow_the_else_edge() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    si a et x non est nihil {\n        redde 0\n    } secus {\n        redde x.longitudo()\n    }\n}",
        NOT_NARROWED,
    );
}

#[test]
fn diverging_or_guard_narrows_the_rest() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    si a aut x est nihil {\n        redde 0\n    }\n    redde x.longitudo()\n}",
    );
}

#[test]
fn diverging_or_guard_narrows_both_operands() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, textus ∪ nihil y) → numerus {\n    si x est nihil aut y est nihil {\n        redde 0\n    }\n    redde x.longitudo() + y.longitudo()\n}",
    );
}

#[test]
fn or_guard_narrows_the_else_branch() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    si a aut x est nihil {\n        redde 0\n    } secus {\n        redde x.longitudo()\n    }\n}",
    );
}

#[test]
fn or_guard_in_a_loop_narrows_after_continue() {
    test_support::assert_accepts(
        "functio f(lista<textus ∪ nihil> xs, bivalens a) → numerus {\n    varia numerus total ← 0\n    itera ex xs fixum x {\n        si a aut x est nihil { perge }\n        total ← total + x.longitudo()\n    }\n    redde total\n}",
    );
}

#[test]
fn or_does_not_narrow_the_taken_branch() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    si a aut x non est nihil {\n        redde x.longitudo()\n    }\n    redde 0\n}",
        NOT_NARROWED,
    );
}

#[test]
fn non_diverging_or_guard_does_not_narrow() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    varia numerus n ← 0\n    si a aut x est nihil { n ← 1 }\n    redde x.longitudo()\n}",
        NOT_NARROWED,
    );
}

#[test]
fn not_swaps_the_taken_branch() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus {\n    si non (x est nihil) {\n        redde x.longitudo()\n    }\n    redde 0\n}",
    );
}

#[test]
fn not_of_not_null_probe_is_a_diverging_guard() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus {\n    si non (x non est nihil) {\n        redde 0\n    }\n    redde x.longitudo()\n}",
    );
}

#[test]
fn not_of_and_narrows_the_fallthrough_of_a_guard() {
    // `non (A et B)` is false exactly when both hold: the else edge carries
    // both facts.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, textus ∪ nihil y) → numerus {\n    si non (x non est nihil et y non est nihil) {\n        redde 0\n    }\n    redde x.longitudo() + y.longitudo()\n}",
    );
}

#[test]
fn not_of_and_does_not_narrow_the_taken_branch() {
    // `non (A et B)` is true when either part is false: no fact holds.
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    si non (a et x non est nihil) {\n        redde x.longitudo()\n    }\n    redde 0\n}",
        NOT_NARROWED,
    );
}

#[test]
fn not_of_or_narrows_the_taken_branch_by_both_negations() {
    // `non (A aut B)` is true exactly when both A and B are false.
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, bivalens a) → numerus {\n    si non (a aut x est nihil) {\n        redde x.longitudo()\n    }\n    redde 0\n}",
    );
}

#[test]
fn field_guard_in_or_is_not_narrowed() {
    test_support::assert_rejects_with_issue(
        "genus G {\n    varia textus ∪ nihil nomen\n}\n\nfunctio f(G g, bivalens a) → numerus {\n    si a aut g.nomen est nihil { redde 0 }\n    redde g.nomen.longitudo()\n}",
        NOT_NARROWED,
    );
}

#[test]
fn field_guard_in_and_narrows() {
    test_support::assert_accepts(
        "genus G {\n    varia textus ∪ nihil nomen\n}\n\nfunctio f(G g, bivalens a) → numerus {\n    si a et g.nomen non est nihil {\n        redde g.nomen.longitudo()\n    }\n    redde 0\n}",
    );
}

#[test]
fn field_path_null_check_narrows_the_branch_body() {
    test_support::assert_accepts(
        "genus G {\n    varia textus ∪ nihil nomen\n}\n\nfunctio f(G g) → numerus {\n    si g.nomen non est nihil {\n        redde g.nomen.longitudo()\n    }\n    redde 0\n}",
    );
}

#[test]
fn field_path_test_on_wrong_branch_stays_optional() {
    // `g.nomen est nihil` narrows the else edge only; the taken branch still
    // sees the optional receiver.
    test_support::assert_rejects_with_issue(
        "genus G {\n    varia textus ∪ nihil nomen\n}\n\nfunctio f(G g) → numerus {\n    si g.nomen est nihil {\n        redde g.nomen.longitudo()\n    }\n    redde 0\n}",
        NOT_NARROWED,
    );
}

#[test]
fn assignment_invalidates_a_compound_fact() {
    // The conjunction narrows `x`; assigning `x` inside the branch drops the
    // fact exactly as it does for a single null probe.
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, textus ∪ nihil y, bivalens a) → numerus {\n    varia textus ∪ nihil v ← x\n    si a et v non est nihil {\n        v ← y\n        redde v.longitudo()\n    }\n    redde 0\n}",
        NOT_NARROWED,
    );
}

#[test]
fn and_narrows_the_right_conjunct_inside_the_condition() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → bivalens {\n    redde x non est nihil et x.longitudo() ≻ 0\n}",
    );
}

#[test]
fn and_threading_narrows_at_any_depth() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x, bivalens a) → bivalens {\n    redde a et x non est nihil et x.longitudo() ≻ 0\n}",
    );
}

#[test]
fn or_does_not_narrow_the_right_disjunct_inside_the_condition() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens a) → bivalens {\n    redde a aut x.longitudo() ≻ 0\n}",
        NOT_NARROWED,
    );
}
