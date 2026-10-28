//! Runner behavior pins for `reducta via prima` (en `reduce via first`) over
//! tensor and matrix sources, through the reduction fold seam.
//!
//! These prove the contract rows the semantic pins cannot: total evaluation
//! (the predicate runs for every element, including post-match elements),
//! `nihil` for no-match and empty sources, first-live selection in row-major
//! order, and predicate-error propagation through the runner's error channel
//! (a trap in a post-match element is observable even though a match was
//! already found).

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, RunnerError, run_source};

/// Run one `incipit` source and return the runner's stdout lines.
fn run_stdout(name: &str, source: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
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
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
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
fn first_returns_the_first_live_element() {
    let lines = run_stdout(
        "first_reduction.fab",
        r"functio elige(tf32[3] x) → fractus ∪ nihil {
    redde reducta via prima ex x sit v { redde v ≻ 6.0 }
}
incipit {
    sit lista<f32> flat ← [5.0, 7.0, 9.0]
    sit tf32[3] a ← flat ↦ tf32[3]
    sit fractus out ← elige(a) vel -1.0
    nota out
}",
    );
    assert_eq!(lines, vec!["7.0".to_owned()], "first live element is 7.0, never the later 9.0");
}

#[test]
fn first_no_match_yields_nihil() {
    let lines = run_stdout(
        "first_no_match.fab",
        r"functio elige(tf32[2] x) → fractus ∪ nihil {
    redde reducta via prima ex x sit v { redde v ≻ 5.0 }
}
incipit {
    sit lista<f32> flat ← [1.0, 2.0]
    sit tf32[2] a ← flat ↦ tf32[2]
    sit fractus out ← elige(a) vel -1.0
    nota out
}",
    );
    assert_eq!(lines, vec!["-1.0".to_owned()], "no match must be nihil (sentinel -1.0)");
}

#[test]
fn first_is_optional_even_over_a_statically_non_empty_source() {
    // No element need satisfy the predicate, so the result is `T ∪ nihil`
    // regardless of the source extent: binding it to a plain `fractus` is a
    // type error.
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let mut host = BufferHost::default();
    let result = run_source(
        &session,
        "first_plain.fab",
        r"functio elige(tf32[2] x) → fractus {
    redde reducta via prima ex x sit v { redde v ≻ 5.0 }
}
incipit { }",
        &mut host,
    );
    assert!(
        matches!(result, Err(RunSourceError::Frontend(_))),
        "a first reduction into a non-optional result must not check: {result:?}"
    );
}

#[test]
fn first_zero_extent_tensor_yields_nihil() {
    let lines = run_stdout(
        "first_empty.fab",
        r"functio elige_vacuum(tf32[0] x) → fractus ∪ nihil {
    redde reducta via prima ex x sit v { redde v ≻ 0.0 }
}
incipit {
    sit tf32[0] a ← vacua
    sit fractus out ← elige_vacuum(a) vel -9.0
    nota out
}",
    );
    assert_eq!(lines, vec!["-9.0".to_owned()], "zero-extent tensor yields nihil");
}

#[test]
fn first_over_a_matrix_selects_in_row_major_order() {
    let lines = run_stdout(
        "first_matrix.fab",
        r"functio elige(tf32[2, 2] x) → fractus ∪ nihil {
    redde reducta via prima ex x apud [i, j] sit v { redde v ≻ 1.0 }
}
incipit {
    sit lista<f32> flat ← [0.5, 2.0, 3.0, 4.0]
    sit tf32[2, 2] a ← flat ↦ tf32[2, 2]
    sit fractus out ← elige(a) vel -9.0
    nota out
}",
    );
    assert_eq!(lines, vec!["2.0".to_owned()], "row-major first live element is 2.0");
}

#[test]
fn first_binds_coordinates_for_the_predicate() {
    let lines = run_stdout(
        "first_coordinate.fab",
        r"functio elige(tf32[4] x) → fractus ∪ nihil {
    redde reducta via prima ex x apud [i] sit v { redde i ≻ 1 }
}
incipit {
    sit lista<f32> flat ← [10.0, 20.0, 30.0, 40.0]
    sit tf32[4] a ← flat ↦ tf32[4]
    sit fractus out ← elige(a) vel -9.0
    nota out
}",
    );
    assert_eq!(lines, vec!["30.0".to_owned()], "first element past coordinate 1 is 30.0");
}

#[test]
fn first_evaluates_the_predicate_after_the_first_match() {
    // Total evaluation: element 0 matches, then element 1 (0) divides by zero.
    // The failure must be observable: no early exit after the first match.
    let errors = run_trapped(
        "first_total.fab",
        r"functio elige(tensor<numerus, [2]> x) → numerus ∪ nihil {
    redde reducta via prima ex x sit v { redde 10 / v ≻ 1 }
}
incipit {
    sit lista<numerus> flat ← [5, 0]
    sit tensor<numerus, [2]> a ← flat ↦ tensor<numerus, [2]>
    sit numerus out ← elige(a) vel -1
    nota out
}",
    );
    assert!(
        !errors.is_empty(),
        "post-match predicate trap must propagate through the error channel"
    );
}

#[test]
fn first_predicate_error_propagates_before_any_match() {
    // A predicate error rides the error channel; it is never converted into a
    // `nihil` result.
    let errors = run_trapped(
        "first_error.fab",
        r"functio elige(tensor<numerus, [1]> x) → numerus ∪ nihil {
    redde reducta via prima ex x sit v { redde 10 / v ≻ 1 }
}
incipit {
    sit lista<numerus> flat ← [0]
    sit tensor<numerus, [1]> a ← flat ↦ tensor<numerus, [1]>
    sit numerus out ← elige(a) vel -1
    nota out
}",
    );
    assert!(!errors.is_empty(), "predicate error must propagate, never become nihil");
}
