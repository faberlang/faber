//! Need, unit 15: the tensor↔vector conversions
//! (`tf32[3] ↦ vector<f32, size 3>` and `vector<f32, size 3> ↦ tf32[3]`) run on the MIR
//! runner. Unit 16 adds the tensor↔matrix crossing: a tensor flattens and
//! builds the register matrix row-major, and a matrix converts back to the
//! target tensor's extents.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

/// Corpus-style English source: the TOML head selects the code locale.
fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

fn ok(body: &str) -> Vec<String> {
    let source = english(&format!("main {{\n{body}\n}}\n"));
    let mut host = BufferHost::default();
    match run_source(&session(), "tensor-family-conversio.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{body}: {other:?}"),
    }
}

/// The flat `list<f32>` the tensor side flattens into is the target element
/// lista; spelling the list type in the program keeps that arena entry
/// interned, as it is in any program that names a `f32` list.
const F32_LIST: &str = "let list<f32> seed ← [1.0, 2.0, 3.0] ↦ list<f32>";

#[test]
fn tensor_family_conversio_tensor_to_vector_runs() {
    let body = format!(
        "{F32_LIST}\n\
         let tf32[3] t ← seed ↦ tf32[3]\n\
         let vector<f32, size 3> w ← t ↦ vector<f32, size 3>\n\
         print w[1]"
    );
    assert_eq!(ok(&body), ["2.0"]);
}

#[test]
fn tensor_family_conversio_vector_to_tensor_runs() {
    let body = "let vector<f32, size 3> w ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>\n\
                let tf32[3] t ← w ↦ tf32[3]\n\
                print t";
    assert_eq!(ok(body), ["[1.0, 2.0, 3.0]"]);
}

#[test]
fn tensor_family_conversio_tensor_to_matrix_runs() {
    let body = "let tf32[2, 2] t ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2]\n\
                let matrix<f32, [2, 2]> m ← t ↦ matrix<f32, [2, 2]>\n\
                print m[[1, 0]]";
    assert_eq!(ok(body), ["3.0"]);
}

#[test]
fn tensor_family_conversio_matrix_to_tensor_runs() {
    let body = "let matrix<f32, [2, 2]> m ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>\n\
                let tf32[2, 2] t ← m ↦ tf32[2, 2]\n\
                print t[[1, 1]]";
    assert_eq!(ok(body), ["4.0"]);
}
