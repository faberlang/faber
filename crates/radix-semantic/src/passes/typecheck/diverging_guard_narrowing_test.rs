//! A diverging guard narrows the optional for the rest of the enclosing block
//!: `si x est nihil { <leave> }` leaves `x` non-null afterwards
//! whether the guard leaves by `redde`, `iace`, `perge` or `rumpe`. A guard
//! that can fall through must not narrow.
//!
//! Fixtures use the latin spellings; the unit-test harness compiles bare
//! source without the en-locale lex pack.

use crate::test_support;

const NOT_NARROWED: &str = "optional_receiver_requires_chain";

fn loop_with_guard(guard: &str) -> String {
    format!(
        "functio f(lista<textus ∪ nihil> xs) → numerus {{\n    varia numerus total ← 0\n    itera ex xs sit x {{\n        {guard}\n        total ← total + x.longitudo()\n    }}\n    redde total\n}}"
    )
}

#[test]
fn continue_guard_narrows_loop_local() {
    test_support::assert_accepts(&loop_with_guard("si x est nihil { perge }"));
}

#[test]
fn break_guard_narrows_loop_local() {
    test_support::assert_accepts(&loop_with_guard("si x est nihil { rumpe }"));
}

#[test]
fn diverging_else_of_negated_test_narrows_the_rest() {
    // `x non est nihil { .. } secus { perge }`: the else edge leaves, so the
    // fallthrough carries the narrowed edge only.
    test_support::assert_accepts(&loop_with_guard(
        "si x non est nihil { total ← total + 1 } secus { perge }",
    ));
}

#[test]
fn continue_guard_narrows_null_equality_form() {
    test_support::assert_accepts(&loop_with_guard("si x ≡ nulla { perge }"));
}

#[test]
fn continue_guard_narrows_negated_null_equality_form() {
    test_support::assert_accepts(&loop_with_guard(
        "si x ≢ nulla { total ← total + 1 } secus { perge }",
    ));
}

#[test]
fn null_equality_guard_narrows_with_literal_on_the_left() {
    test_support::assert_accepts(&loop_with_guard("si nulla ≡ x { perge }"));
}

#[test]
fn non_diverging_loop_guard_does_not_narrow() {
    test_support::assert_rejects_with_issue(
        &loop_with_guard("si x est nihil { total ← total + 1 }"),
        NOT_NARROWED,
    );
}

#[test]
fn conditionally_diverging_loop_guard_does_not_narrow() {
    test_support::assert_rejects_with_issue(
        &loop_with_guard("si x est nihil { si total ≻ 3 { perge } }"),
        NOT_NARROWED,
    );
}

#[test]
fn continue_guard_does_not_narrow_past_the_loop() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil y, lista<numerus> xs) → numerus {\n    itera ex xs sit x {\n        si y est nihil { perge }\n    }\n    redde y.longitudo()\n}",
        NOT_NARROWED,
    );
}

#[test]
fn return_guard_narrows_parameter() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus {\n    si x est nihil { redde 0 }\n    redde x.longitudo()\n}",
    );
}

#[test]
fn throw_guard_narrows_parameter() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus ⇥ textus {\n    si x est nihil { iace \"nihil\" }\n    redde x.longitudo()\n}",
    );
}

#[test]
fn throw_guard_narrows_local() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil p) → numerus ⇥ textus {\n    sit textus ∪ nihil x ← p\n    si x ≡ nulla { iace \"nihil\" }\n    redde x.longitudo()\n}",
    );
}

#[test]
fn non_diverging_guard_does_not_narrow_parameter() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus {\n    varia numerus n ← 0\n    si x est nihil { n ← 1 }\n    redde x.longitudo()\n}",
        NOT_NARROWED,
    );
}

#[test]
fn field_guard_is_not_narrowed() {
    // A field can change between statements, so the guard records nothing for
    // it: the use after the guard is still an optional receiver.
    test_support::assert_rejects_with_issue(
        "genus G {\n    varia textus ∪ nihil nomen\n}\n\nfunctio f(G g) → numerus {\n    si g.nomen est nihil { redde 0 }\n    redde g.nomen.longitudo()\n}",
        NOT_NARROWED,
    );
}
