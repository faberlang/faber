//! inf track U9: `itera ab` ranges with `inf` bounds on the MIR runner.
//!
//! An `inf` endpoint makes the binder (and the step) `inf` (rulings 35/43):
//! the walk runs exact past the 64-bit range, mixed bounds (an untyped
//! literal or a bounded integer against an `inf`) join to `inf`, and bounded
//! bounds keep their existing shapes.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

const U64_MAX_MINUS_1: &str = "18446744073709551614";

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirRust))
}

fn run_lines(name: &str, body: &str) -> Vec<String> {
    let source = format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session(), name, &source, &mut host)
        .unwrap_or_else(|error| panic!("{name} should run on the MIR runner: {error:?}"));
    host.stdout_lines
}

fn issues(name: &str, body: &str) -> Vec<String> {
    let source = format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session(), name, &source, &mut host) {
        Ok(()) => Vec::new(),
        Err(super::RunSourceError::Frontend(diagnostics)) => diagnostics
            .iter()
            .filter(|diag| diag.is_error())
            .filter_map(|diag| diag.issue().map(str::to_owned))
            .collect(),
        Err(other) => panic!("{name}: unexpected failure {other:?}"),
    }
}

#[test]
fn literal_start_against_inf_end_walks_exclusive() {
    let lines = run_lines("itera-inf-exclusive.fab", "let inf n ← 3\nfor range 0‥n let i {\nprint i\n}");
    assert_eq!(lines, ["0", "1", "2"]);
}

#[test]
fn inf_inclusive_and_explicit_step_walk_like_bounded() {
    let lines = run_lines(
        "itera-inf-inclusive.fab",
        "let inf n ← 3\n\
         for range 1…n let k {\nprint k\n}\n\
         for range 0‥n step 2 let j {\nprint j\n}",
    );
    assert_eq!(lines, ["1", "2", "3", "0", "2"]);
}

#[test]
fn descending_inf_range_uses_the_negative_step() {
    let lines = run_lines(
        "itera-inf-descending.fab",
        "let inf z ← 0 - 3\nfor range 0‥z let q {\nprint q\n}",
    );
    assert_eq!(lines, ["0", "-1", "-2"]);
}

#[test]
fn a_bounded_start_against_an_inf_end_joins_to_inf() {
    let lines = run_lines(
        "itera-inf-mixed-bounded.fab",
        "let i32 a ← 5\nlet inf hi ← 7\nfor range a‥hi let m {\nprint m\n}",
    );
    assert_eq!(lines, ["5", "6"]);
}

#[test]
fn the_binder_is_inf_so_the_walk_crosses_the_u64_boundary_exactly() {
    let body = format!(
        "let inf lo ← {U64_MAX_MINUS_1}\nlet inf hi ← 18446744073709551618\n\
         for range lo‥hi let m {{\nprint m + m\n}}"
    );
    let lines = run_lines("itera-inf-beyond-u64.fab", &body);
    assert_eq!(
        lines,
        [
            "36893488147419103228",
            "36893488147419103230",
            "36893488147419103232",
            "36893488147419103234",
        ]
    );
}

#[test]
fn a_descending_walk_and_a_stepped_inclusive_walk_cross_the_boundary() {
    let body = format!(
        "let inf lo ← {U64_MAX_MINUS_1}\nlet inf hi ← 18446744073709551618\n\
         for range hi‥lo let d {{\nprint d\n}}\n\
         for range lo…hi step 3 let s {{\nprint s\n}}"
    );
    let lines = run_lines("itera-inf-boundary-directions.fab", &body);
    assert_eq!(
        lines,
        [
            "18446744073709551618",
            "18446744073709551617",
            "18446744073709551616",
            "18446744073709551615",
            "18446744073709551614",
            "18446744073709551617",
        ]
    );
}

#[test]
fn an_inf_range_product_walks_first_range_outermost() {
    let lines = run_lines(
        "itera-inf-product.fab",
        "let inf n ← 2\nfor range 0‥n, 0‥n let i, j {\n\
         print i * 18446744073709551616 + j\n}",
    );
    assert_eq!(lines, ["0", "1", "18446744073709551616", "18446744073709551617"]);
}

#[test]
fn a_bounded_range_binder_stays_bounded() {
    // Bounded bounds are unchanged: the binder is a plain bounded integer, so
    // it stores into an `int` slot without a narrowing...
    assert!(
        issues(
            "itera-bounded-binder.fab",
            "let int n ← 3\nfor range 0‥n let i {\nlet int x ← i\nprint x\n}",
        )
        .is_empty()
    );
}

#[test]
fn an_inf_binder_storing_into_a_bounded_slot_is_checked_at_run_time() {
    // The binder is `inf`, so a store into `u64` is a checked narrowing: the
    // last in-range value passes, the first beyond it traps.
    let source = "+++\nlocale = \"en\"\n+++\nmain {\n\
        let inf lo ← 18446744073709551615\nlet inf hi ← 18446744073709551618\n\
        for range lo‥hi let m {\nlet u64 x ← m\nprint x\n}\n}\n";
    let mut host = BufferHost::default();
    let outcome = run_source(&session(), "itera-inf-binder-store.fab", source, &mut host);
    assert!(
        matches!(outcome, Err(super::RunSourceError::Stepper(_))),
        "expected a run-time trap, got {outcome:?}"
    );
    assert_eq!(host.stdout_lines, ["18446744073709551615"]);
}

#[test]
fn an_inf_binder_narrows_with_the_checked_conversion() {
    let lines = run_lines(
        "itera-inf-binder-narrow.fab",
        "let inf n ← 3\nfor range 0‥n let i {\nlet int x ← i ↦ int\nprint x\n}",
    );
    assert_eq!(lines, ["0", "1", "2"]);
}

#[test]
fn materializing_an_inf_range_is_rejected_by_name() {
    // The interval helpers admit only the plain `numerus` bound today (the
    // same gate that rejects sized bounds); `intervallum<inf>` is not a
    // shipped type, so the materialize never reaches `IntervalMaterialize`.
    let found = issues(
        "itera-inf-materialize.fab",
        "let inf n ← 3\nlet list<inf> xs ← 0‥n ↦ list<inf>\nprint xs",
    );
    assert!(
        found
            .iter()
            .any(|issue| issue == "intervallum_helper_bound_unsupported"),
        "got {found:?}"
    );
}
