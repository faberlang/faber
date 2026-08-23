//! AFM-3 (air-first-match) base-lane MIR behavior pins: `primus_quem` over
//! array sources through the `collection_higher_order` seam.
//!
//! These prove the delivery contract rows the semantic pins cannot: total
//! evaluation (the predicate runs for every lane, including post-match
//! lanes), `nihil` for no-match and empty sources, and predicate-error
//! propagation through the runner's error channel (a trap in a post-match
//! lane is observable even though a match was already found).

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{run_source, BufferHost, RunSourceError, RunnerError};

/// Run one `incipit` source and return the runner's stdout lines.
fn run_stdout(name: &str, source: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_stdlib(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, name, source, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("runner should run: {errors:?}"),
    }
    host.stdout_lines
}

/// Run one `incipit` source that must fail at interpretation time and return
/// the runner errors (the observable predicate-error channel).
fn run_trapped(name: &str, source: &str) -> Vec<RunnerError> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_stdlib(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, name, source, &mut host) {
        Ok(()) => panic!("source must trap at interpretation time"),
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => errors,
    }
}

#[test]
fn first_match_returns_the_first_live_element() {
    let lines = run_stdout(
        "afm3_match.fab",
        r#"incipit {
    fixum lista<fractus> xs ← [5.0, 7.0, 9.0]
    fixum fractus ∪ nihil r ← primus_quem(xs, ubi x { x > 6.0 })
    fixum fractus out ← r vel -1.0
    nota out
}"#,
    );
    assert_eq!(lines, vec!["7.0".to_owned()], "first live element is 7.0");
}

#[test]
fn first_match_no_match_yields_nihil() {
    let lines = run_stdout(
        "afm3_no_match.fab",
        r#"incipit {
    fixum lista<fractus> xs ← [1.0, 2.0]
    fixum fractus ∪ nihil r ← primus_quem(xs, ubi x { x > 5.0 })
    fixum fractus out ← r vel -1.0
    nota out
}"#,
    );
    assert_eq!(lines, vec!["-1.0".to_owned()], "no match must be nihil (sentinel -1.0)");
}

#[test]
fn first_match_empty_source_yields_nihil() {
    let lines = run_stdout(
        "afm3_empty.fab",
        r#"incipit {
    fixum lista<fractus> xs ← vacua
    fixum fractus ∪ nihil r ← primus_quem(xs, ubi x { x > 5.0 })
    fixum fractus out ← r vel -1.0
    nota out
}"#,
    );
    assert_eq!(lines, vec!["-1.0".to_owned()], "empty source must be nihil (sentinel -1.0)");
}

#[test]
fn first_match_evaluates_the_predicate_after_the_first_match() {
    // Total evaluation: lane 0 matches, then lane 1 ("300") fails the
    // range-checked `↦ u8` conversion (SA-002). The failure must be
    // observable — no early exit after the first match.
    let errors = run_trapped(
        "afm3_total.fab",
        r#"incipit {
    fixum lista<textus> xs ← ["5", "300"]
    fixum textus ∪ nihil r ← primus_quem(xs, ubi x { (x ↦ u8) > 2 })
    fixum textus out ← r vel "none"
    nota out
}"#,
    );
    assert!(
        !errors.is_empty(),
        "post-match predicate trap must propagate through the error channel"
    );
}

#[test]
fn first_match_predicate_error_propagates_before_any_match() {
    // Error propagation: a predicate error rides the error channel; it is
    // never converted into a `nihil` result.
    let errors = run_trapped(
        "afm3_error.fab",
        r#"incipit {
    fixum lista<textus> xs ← ["300"]
    fixum textus ∪ nihil r ← primus_quem(xs, ubi x { (x ↦ u8) > 2 })
    fixum textus out ← r vel "none"
    nota out
}"#,
    );
    assert!(!errors.is_empty(), "predicate error must propagate, never become nihil");
}
