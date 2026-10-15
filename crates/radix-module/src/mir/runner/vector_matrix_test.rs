//! `v · m` (vector times matrix, row-vector convention) runs on the MIR runner:
//! a width-K vector times a [K, N] matrix is a width-N vector with
//! out[j] = sum over k of v[k] * m[k, j] (need, ruling 2026-10-04).

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run_lines(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        "vector-matrix.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("vector · matrix must run: {error:?}"));
    host.stdout_lines
}

#[test]
fn vector_matrix_three_by_three_two_matches_hand_computed_result() {
    // v = [1, 2, 3], m = [[1, 4], [2, 5], [3, 6]]
    // out[0] = 1*1 + 2*2 + 3*3 = 14, out[1] = 1*4 + 2*5 + 3*6 = 32
    let source = r#"+++
locale = "en"
+++
main {
    let vf32[3] v ← [1.0, 2.0, 3.0] ↦ vf32[3]
    let matrix<f32, [3, 2]> m ← [[1.0, 4.0], [2.0, 5.0], [3.0, 6.0]] ↦ matrix<f32, [3, 2]>
    let vf32[2] r ← v · m
    print r
}
"#;
    assert_eq!(run_lines(source), vec!["[14.0, 32.0]"]);
}

#[test]
fn vector_matrix_is_not_the_matrix_vector_product() {
    // Row-vector convention: v · m uses columns of m, m · v uses rows.
    // v = [1, 2], m = [[1, 2], [3, 4]]: v · m = [7, 10]; m · v = [5, 11].
    let source = r#"+++
locale = "en"
+++
main {
    let vf32[2] v ← [1.0, 2.0] ↦ vf32[2]
    let matrix<f32, [2, 2]> m ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>
    let vf32[2] left ← v · m
    let vf32[2] right ← m · v
    print left
    print right
}
"#;
    assert_eq!(run_lines(source), vec!["[7.0, 10.0]", "[5.0, 11.0]"]);
}
