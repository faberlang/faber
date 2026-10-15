//! Loops over tensors read their bounds from the carrier at run time.
//!
//! A literal extent is the folded case of the same carrier read, so a tensor
//! whose extent is a run-time `int`, a `size` generic or a `mut` parameter
//! walks through the same loop lowering as a literal-shape tensor.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::{BufferHost, MirLoweringOptions, dump_program, lower_analyzed_unit, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

/// Corpus-style English source: the TOML head selects the code locale.
fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn run(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(&session(), "runtime-extent-itera.fab", &english(source), &mut host)
        .unwrap_or_else(|error| panic!("runtime-extent itera must run: {error:?}"));
    host.stdout_lines
}

fn dump(source: &str) -> String {
    let source = english(source);
    let unit = analyze_source(&session(), "runtime-extent-itera.fab", &source, AnalyzeRequest::default())
        .expect("runtime-extent itera source analyzes");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .unwrap_or_else(|error| panic!("runtime-extent itera must lower: {error:?}"));
    dump_program(lowered.validated.program())
}

#[test]
fn runtime_extent_rank2_loop_reads_bounds_from_the_carrier() {
    let source = r"
fn rt(int v) → int { return v }
main {
    let int n ← rt(3)
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    let tf32[2, size n] a ← flat ↦ tf32[2, size n]
    mut f32 acc ← 0.0
    mut int coords ← 0
    for from a at [i, j] const c {
        acc ← acc + c
        coords ← coords + i * 10 + j
    }
    print acc
    print coords
}
";
    assert_eq!(run(source), ["21.0", "36"]);
}

#[test]
fn runtime_extent_rank1_loops_run_with_and_without_a_coordinate() {
    let source = r"
fn rt(int v) → int { return v }
main {
    let int n ← rt(3)
    let list<f32> flat ← [1.0, 2.0, 3.0]
    let tf32[size n] a ← flat ↦ tf32[size n]
    mut f32 plain ← 0.0
    for from a const c { plain ← plain + c }
    mut f32 coord ← 0.0
    for from a at [i] const c { coord ← coord + c }
    print plain
    print coord
}
";
    assert_eq!(run(source), ["6.0", "6.0"]);
}

#[test]
fn zero_runtime_extent_runs_zero_iterations() {
    let source = r"
fn rt(int v) → int { return v }
main {
    let int n ← rt(0)
    let int m ← rt(2)
    let list<f32> none ← []
    let tf32[2, size n] a ← none ↦ tf32[2, size n]
    let tf32[size n] b ← none ↦ tf32[size n]
    let tf32[size m, 0] e ← none ↦ tf32[size m, 0]
    mut int rank2 ← 0
    for from a at [i, j] const c { rank2 ← rank2 + 1 }
    mut int rank1 ← 0
    for from b const c { rank1 ← rank1 + 1 }
    for from b at [i] const c { rank1 ← rank1 + 1 }
    mut int empty_inner ← 0
    for from e at [i, j] const c { empty_inner ← empty_inner + 1 }
    print rank2
    print rank1
    print empty_inner
}
";
    assert_eq!(run(source), ["0", "0", "0"]);
}

#[test]
fn size_generic_tensor_loop_runs_over_each_instance_shape() {
    let source = r"
fn rt(int v) → int { return v }
fn total<size H, size W>(tf32[H, W] src) → f32 {
    mut f32 acc ← 0.0
    for from src at [i, j] const c { acc ← acc + c }
    return acc
}
main {
    let tf32[2, 3] a ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tf32[2, 3]
    print total(a)
    let int n ← rt(2)
    let list<f32> flat_b ← [1.0, 2.0, 3.0, 4.0]
    let tf32[size n, 2] b ← flat_b ↦ tf32[size n, 2]
    print total(b)
}
";
    assert_eq!(run(source), ["21.0", "10.0"]);
}

#[test]
fn mut_tensor_parameter_loop_walks_and_writes_in_place() {
    let source = r"
fn bump(mut tf32[2, 3] dst) → void {
    for from dst at [i, j] thread lane const c {
        dst[[i, j]] ← c + 1.0
    }
}
main {
    mut tf32[2, 3] b ← tf32[2, 3] {}
    bump(mut b)
    print b.flatten()
}
";
    assert_eq!(run(source), ["[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]"]);
}

#[test]
fn literal_extent_loop_keeps_literal_bounds_and_runtime_extent_reads_the_carrier() {
    let literal = dump(
        r"
main {
    let tf32[2, 3] a ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tf32[2, 3]
    mut int acc ← 0
    for from a at [i, j] const c { acc ← acc + i * 10 + j }
    print acc
}
",
    );
    assert!(
        !literal.contains("tensor_shape"),
        "a literal extent folds to a constant bound with no carrier read: {literal}"
    );

    let runtime = dump(
        r"
fn rt(int v) → int { return v }
main {
    let int n ← rt(3)
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    let tf32[2, size n] a ← flat ↦ tf32[2, size n]
    mut int acc ← 0
    for from a at [i, j] const c { acc ← acc + i * 10 + j }
    print acc
}
",
    );
    assert!(
        runtime.contains("tensor_shape"),
        "a runtime extent is read from the carrier shape: {runtime}"
    );
}
