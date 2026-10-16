use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "matrix-store.fab", &source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{body}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

#[test]
fn matrix_i8_matmul_traps_when_a_finished_cell_leaves_i8() {
    let body = "sit matrix<i8, [1, 2]> a ← [[100, 100]] ↦ matrix<i8, [1, 2]>\nsit matrix<i8, [2, 1]> c ← [[1], [1]] ↦ matrix<i8, [2, 1]>\nsit matrix<i8, [1, 1]> d ← a · c\nnota d";
    let (out, errors) = run(body);
    assert!(out.is_empty(), "printed {out:?}");
    let (issue, _message) = errors.into_iter().next().expect("expected a store trap");
    assert_eq!(issue, "implicit_store_out_of_range");
}

#[test]
fn matrix_i8_matmul_in_range_cell_runs_and_partial_stay_exact() {
    let body = "sit matrix<i8, [1, 2]> a ← [[100, 100]] ↦ matrix<i8, [1, 2]>\nsit matrix<i8, [2, 1]> b ← [[1], [-1]] ↦ matrix<i8, [2, 1]>\nsit matrix<i8, [1, 1]> d ← a · b\nnota d";
    let (out, errors) = run(body);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["[[0]]"]);
}
