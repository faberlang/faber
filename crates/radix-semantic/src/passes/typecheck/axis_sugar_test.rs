//! Axis-reduction sugar facts: typecheck rewrites
//! `t.summa(axis n)` / `t.media(axis n)` into the grouped reduction, so the
//! presentation sidecar records the authored axis and method against the
//! expanded node id. Fixtures use latin spellings because the radix unit-test
//! harness compiles bare source without the en-locale lex pack.

use crate::test_support::analyze_ok;
use radix_hir::HirAxisMethod;

#[test]
fn axis_reduction_expansion_is_recorded_as_sugar() {
    let source = "functio f(tf32[2, 3] g) → tf32[3] { redde g.summa(axis 0) }\n\
functio m(tf32[2, 3] g) → tf32[2] { redde g.media(axis 1) }\n\
incipit { }";
    let analyzed = analyze_ok(source);
    let sugar = &analyzed.presentation.axis_sugar;
    assert_eq!(sugar.len(), 2, "sugar: {sugar:?}");
    assert!(
        sugar
            .iter()
            .any(|entry| entry.axis == 0 && entry.method == HirAxisMethod::Sum),
        "missing Sum axis 0: {sugar:?}"
    );
    assert!(
        sugar
            .iter()
            .any(|entry| entry.axis == 1 && entry.method == HirAxisMethod::Mean),
        "missing Mean axis 1: {sugar:?}"
    );
}

#[test]
fn non_axis_reduction_records_no_sugar() {
    let source = "functio f(tf32[4] xs) → f32 { redde xs.summa() }\n\
incipit { }";
    let analyzed = analyze_ok(source);
    assert!(
        analyzed.presentation.axis_sugar.is_empty(),
        "sugar: {:?}",
        analyzed.presentation.axis_sugar
    );
}
