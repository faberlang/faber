//! Need, unit 7: the rank-1 tensor bracket range (`t[1 ‥ 4]`)
//! lowers to the `TensorSliceNd` collection op and runs on the MIR runner.

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
    match run_source(&session(), "tensor-bracket-range.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{body}: {other:?}"),
    }
}

const T: &str = "mut tf32[6] t ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] ↦ tf32[6]";

#[test]
fn tensor_bracket_range_rank1_slices_with_step() {
    let body = format!(
        "{T}\n\
         let tf32[3] s ← t[1 ‥ 4]\n\
         let tf32[3] q ← t[0 ‥ 6 step 2]\n\
         print s\n\
         print q"
    );
    assert_eq!(ok(&body), ["[2.0, 3.0, 4.0]", "[1.0, 3.0, 5.0]"]);
}

#[test]
fn tensor_bracket_range_rank2_slots_slice_and_drop() {
    let body = "\
        let tf32[2, 3] g ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tf32[2, 3]\n\
        let tf32[3] row ← g[[1, 0 ‥ 3]]\n\
        let tf32[1, 3] keep ← g[[1 … 1, 0 ‥ 3]]\n\
        print row\n\
        let f32 x ← keep[[0, 2]]\n\
        print x";
    assert_eq!(ok(body), ["[4.0, 5.0, 6.0]", "6.0"]);
}

const V: &str = "let vector<f32, size 4> v ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, size 4>";

#[test]
fn vector_bracket_range_slice_runs() {
    // WHY: a register vector slice keeps the vector family, so lowering builds
    // a fresh vector from the kept lanes instead of reading one lane.
    let body = format!(
        "{V}\n\
         let vector<f32, size 2> w ← v[1 ‥ 3]\n\
         print w[0]\n\
         print w[1]"
    );
    assert_eq!(ok(&body), ["2.0", "3.0"]);
}

#[test]
fn vector_bracket_range_step_slice_runs() {
    let body = format!(
        "{V}\n\
         let vector<f32, size 2> w ← v[0 ‥ 4 step 2]\n\
         print w[0]\n\
         print w[1]"
    );
    assert_eq!(ok(&body), ["1.0", "3.0"]);
}

#[test]
fn matrix_bracket_slot_range_runs() {
    // WHY: a matrix is a register aggregate, not a tensor; a slot list with a
    // range keeps a row, a column or a block as cell reads, not `TensorSliceNd`.
    let body = "\
        let matrix<f32, [2, 3]> m ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ matrix<f32, [2, 3]>\n\
        let vector<f32, size 3> row ← m[[1, 0 ‥ 3]]\n\
        let vector<f32, size 2> col ← m[[0 ‥ 2, 2]]\n\
        let matrix<f32, [1, 2]> block ← m[[0 … 0, 1 ‥ 3]]\n\
        print row[0]\n\
        print col[1]\n\
        print block[[0, 1]]";
    assert_eq!(ok(body), ["4.0", "6.0", "3.0"]);
}
