//! Need, unit 4: matrix bracket sugar (`m[[r, c]]`) and the coordinate
//! methods (`accipe` / `apud` / `ponde`) lower through the tensor `TensorGet` /
//! `TensorSet` ops and run on the MIR runner.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

/// A `matrix<f32, [2, 2]>` built from its nested literal grid.
const GRID: &str = "varia matrix<f32, [2, 2]> grid ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>";

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    )
}

fn wrap(body: &str) -> String {
    format!("incipit {{\n{body}\n}}\n")
}

fn ok(body: &str) -> Vec<String> {
    let session = session();
    let source = wrap(body);
    let mut host = BufferHost::default();
    match run_source(&session, "matrix-bracket.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{body}: {other:?}"),
    }
}

#[test]
fn matrix_bracket_access_reads_writes_get_and_at() {
    let body = format!(
        "{GRID}\n\
         sit f32 a ← grid[[1, 0]]\n\
         grid[[0, 1]] ← 9.0\n\
         sit numerus r ← 1\n\
         grid[[r, r]] ← grid[[r, r]] + 0.5\n\
         sit f32 ∪ nihil g ← grid.accipe([0, 1])\n\
         sit f32 t ← grid.apud([1, 1])\n\
         nota a\n\
         nota grid\n\
         nota g\n\
         nota t"
    );
    assert_eq!(ok(&body), ["3.0", "[[1.0, 9.0], [3.0, 4.5]]", "9.0", "4.5"]);
}

#[test]
fn matrix_bracket_read_with_computed_out_of_range_coordinate_traps() {
    let session = session();
    let source = wrap(&format!("{GRID}\nsit numerus far ← 5\nnota grid[[far, 0]]"));
    let mut host = BufferHost::default();
    let Err(RunSourceError::Stepper(errors)) = run_source(&session, "matrix-bracket.fab", &source, &mut host) else {
        panic!("a computed out-of-range coordinate read must fail the run");
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_tensor_accipe_invalid_index"),
        "{errors:?}"
    );
}

#[test]
fn matrix_bracket_read_with_literal_out_of_range_coordinate_fails_analysis() {
    let session = session();
    let source = wrap(&format!("{GRID}\nnota grid[[2, 0]]"));
    let mut host = BufferHost::default();
    let Err(RunSourceError::Frontend(diagnostics)) = run_source(&session, "matrix-bracket.fab", &source, &mut host)
    else {
        panic!("a literal out-of-range coordinate read must fail analysis");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("index_provably_out_of_bounds")),
        "{diagnostics:?}"
    );
}
