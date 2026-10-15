//! Range step law (operator ruling 2026-10-02): the step is a positive
//! stride, the bounds pick the direction, a zero or negative step is an
//! error (compile error for a literal, runtime trap otherwise), and a `varia`
//! range binder is a fresh per-iteration copy.

use super::{BufferHost, RunSourceError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirRust))
}

fn program(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n")
}

fn run_lines(name: &str, body: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(&session(), name, &program(body), &mut host)
        .unwrap_or_else(|error| panic!("{name} should run on the MIR runner: {error:?}"));
    host.stdout_lines
}

fn issues(name: &str, body: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    match run_source(&session(), name, &program(body), &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Frontend(diagnostics)) => diagnostics
            .iter()
            .filter(|diag| diag.is_error())
            .filter_map(|diag| diag.issue().map(str::to_owned))
            .collect(),
        Err(other) => panic!("{name}: unexpected failure {other:?}"),
    }
}

#[test]
fn descending_range_with_positive_step_counts_down() {
    let lines = run_lines(
        "range-step-descending.fab",
        "for range 10‥0 step 2 const i {\nprint i\n}\n\
         print \"-\"\n\
         for range 10…0 step 5 const j {\nprint j\n}",
    );
    assert_eq!(lines, ["10", "8", "6", "4", "2", "-", "10", "5", "0"]);
}

#[test]
fn ascending_range_with_positive_step_counts_up() {
    let lines = run_lines(
        "range-step-ascending.fab",
        "for range 0‥10 step 2 const i {\nprint i\n}\n\
         print \"-\"\n\
         for range 0…10 step 5 const j {\nprint j\n}",
    );
    assert_eq!(lines, ["0", "2", "4", "6", "8", "-", "0", "5", "10"]);
}

#[test]
fn equal_bounds_walk_the_ascending_way() {
    let lines = run_lines(
        "range-step-equal-bounds.fab",
        "for range 5‥5 step 1 const i {\nprint i\n}\n\
         for range 5…5 step 1 const j {\nprint j\n}",
    );
    assert_eq!(lines, ["5"]);
}

#[test]
fn product_axis_takes_direction_from_its_own_bounds() {
    let lines = run_lines(
        "range-step-product.fab",
        "for range 4‥0 step 2, 1…2 const i, j {\nprint i\nprint j\n}",
    );
    assert_eq!(lines, ["4", "1", "4", "2", "2", "1", "2", "2"]);
}

#[test]
fn literal_zero_or_negative_step_is_a_compile_error() {
    for step in ["0", "-1", "-2"] {
        let found = issues(
            "range-step-literal.fab",
            &format!("for range 0‥4 step {step} const i {{\nprint i\n}}"),
        );
        assert_eq!(found, ["range_step_not_positive"], "step {step}");
    }
}

#[test]
fn literal_negative_step_is_rejected_on_a_range_product_too() {
    let found = issues(
        "range-step-product-literal.fab",
        "for range 10‥0 step -2, 1‥3 const i, j {\nprint i\n}",
    );
    assert_eq!(found, ["range_step_not_positive"]);
}

#[test]
fn computed_zero_step_traps_at_run_time() {
    let mut host = BufferHost::default();
    let source = program(
        "mut int s ← 1\nfor range 0‥2 step s const i {\nprint i\n}\ns ← 0\nfor range 0‥2 step s const j {\nprint j\n}",
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        run_source(&session(), "range-step-trap.fab", &source, &mut host)
    }));
    assert!(outcome.is_err(), "a zero step must trap, not return");
    assert_eq!(host.stdout_lines, ["0", "1"]);
    assert!(host.last_abort.is_some(), "the trap must abort through the host");
}

#[test]
fn computed_negative_step_traps_at_run_time() {
    let mut host = BufferHost::default();
    let source = program("mut int s ← 2\ns ← s - 5\nfor range 0‥2 step s const i {\nprint i\n}\nprint \"unreached\"");
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        run_source(&session(), "range-step-negative-trap.fab", &source, &mut host)
    }));
    assert!(outcome.is_err(), "a negative step must trap, not return");
    assert!(host.stdout_lines.is_empty());
    assert!(host.last_abort.is_some());
}

#[test]
fn varia_range_binder_is_a_per_iteration_copy() {
    let lines = run_lines("range-varia-binder.fab", "for range 0‥6 mut i {\nprint i\ni ← i + 1\n}");
    assert_eq!(lines, ["0", "1", "2", "3", "4", "5"]);
}

#[test]
fn varia_range_binder_copy_survives_a_step_and_perge() {
    let lines = run_lines(
        "range-varia-binder-step.fab",
        "for range 10‥0 step 3 mut i {\ni ← i + 100\nprint i\n}",
    );
    assert_eq!(lines, ["110", "107", "104", "101"]);
}

#[test]
fn varia_range_product_binders_are_per_iteration_copies() {
    let lines = run_lines(
        "range-varia-product.fab",
        "for range 0‥2, 0‥2 mut i, j {\nprint i\nprint j\ni ← i + 5\nj ← j + 5\n}",
    );
    // Each binder is copied once per iteration of its own axis: the outer
    // write (+5) stays visible to the rest of that outer iteration's inner
    // walk, and the next outer iteration starts from the counter again.
    assert_eq!(lines, ["0", "0", "5", "1", "1", "0", "6", "1"]);
}
