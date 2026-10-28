//! `empty` at a dense tensor, vector, or matrix slot is the typed zero
//! (operator ruling 2026-10-08): the runner builds the shaped zero value and
//! MIR lowering sees the same program for both spellings.

use super::*;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::{BufferHost, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

fn program(declaration: &str, probes: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\nmain {{\n    {declaration}\n{probes}\n}}\n")
}

const TENSOR_PROBES: &str = "    print x.shape()[0]\n    print x.shape()[1]\n    print x[[1, 2]]\n    print x[[0, 0]]";

fn stdout(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(&session(), "empty-dense.fab", source, &mut host).expect("program runs");
    host.stdout_lines
}

/// MIR text of the entry function with source positions and type-table
/// numbering removed (the typed zero interns extra types of its own), so two
/// spellings of one program compare by structure.
fn entry_mir(source: &str) -> String {
    let unit = analyze_source(&session(), "empty-dense.fab", source, AnalyzeRequest::default()).expect("analyzes");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("lowers");
    let entry = lowered
        .validated
        .program()
        .functions
        .last()
        .expect("entry function");
    let text = format!("{:?}", (&entry.params, &entry.locals, &entry.temps, &entry.blocks));
    strip_type_ids(&strip_spans(&text))
}

fn strip_type_ids(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("TypeId(") {
        let digits = at + "TypeId(".len();
        out.push_str(&rest[..digits]);
        let tail = &rest[digits..];
        let close = tail.find(')').expect("type id closes");
        rest = &tail[close..];
    }
    out.push_str(rest);
    out
}

fn strip_spans(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("Span {") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let close = tail.find('}').expect("span closes");
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    out
}

#[test]
fn empty_tensor_runs_as_the_typed_zero() {
    let empty = stdout(&program("mut tf32[2, 3] x ← empty", TENSOR_PROBES));
    let typed = stdout(&program("mut tf32[2, 3] x ← tf32[2, 3] {}", TENSOR_PROBES));
    assert_eq!(typed, ["2", "3", "0.0", "0.0"]);
    assert_eq!(empty, typed);
}

#[test]
fn empty_long_form_tensor_runs_as_the_typed_zero() {
    let empty = stdout(&program("mut tensor<f32, [2, 3]> x ← empty", TENSOR_PROBES));
    assert_eq!(empty, ["2", "3", "0.0", "0.0"]);
}

#[test]
fn empty_vector_and_matrix_run_as_their_typed_zeros() {
    let probes = "    print x";
    for (annotation, zero) in [("vf32[4]", "vf32[4] {}"), ("mf32[2, 2]", "mf32[2, 2] {}")] {
        let empty = stdout(&program(&format!("mut {annotation} x ← empty"), probes));
        let typed = stdout(&program(&format!("mut {annotation} x ← {zero}"), probes));
        assert_eq!(empty, typed, "{annotation}");
        assert!(!empty.is_empty(), "{annotation}");
    }
}

#[test]
fn empty_tensor_of_vectors_runs_as_the_typed_zero() {
    let probes = "    print x.shape()[0]";
    let empty = stdout(&program("mut tensor<vf32[2], [3]> x ← empty", probes));
    let typed = stdout(&program("mut tensor<vf32[2], [3]> x ← tensor<vf32[2], [3]> {}", probes));
    assert_eq!(empty, ["3"]);
    assert_eq!(empty, typed);
}

#[test]
fn empty_tensor_with_size_generic_extent_runs() {
    let source = "+++\nlocale = \"en\"\n+++\nfn zeros<size N>() → tensor<f32, [N]> {\n    return empty\n}\nmain {\n    print zeros<size 4>().shape()[0]\n}\n";
    assert_eq!(stdout(source), ["4"]);
}

#[test]
fn empty_in_assignment_return_and_declared_size_positions_runs() {
    let source = "+++\nlocale = \"en\"\n+++\nfn pick() → tf32[2] {\n    return empty\n}\nmain {\n    const size K ← 5\n    let tf32[K, 2] k ← empty\n    print k.shape()[0]\n    mut tf32[2] w ← tf32[2] {}\n    w ← empty\n    print w[[1]]\n    print pick().shape()[0]\n}\n";
    assert_eq!(stdout(source), ["5", "0.0", "2"]);
}

#[test]
fn empty_and_typed_zero_lower_to_the_same_mir() {
    for (annotation, zero) in [
        ("tf32[2, 3]", "tf32[2, 3] {}"),
        ("tensor<f32, [2, 3]>", "tensor<f32, [2, 3]> {}"),
        ("vf32[4]", "vf32[4] {}"),
        ("mf32[2, 2]", "mf32[2, 2] {}"),
    ] {
        let empty = entry_mir(&program(&format!("let {annotation} x ← empty"), "    print x"));
        let typed = entry_mir(&program(&format!("let {annotation} x ← {zero}"), "    print x"));
        assert_eq!(empty, typed, "{annotation}");
    }
}

#[test]
fn empty_rank_zero_tensor_keeps_its_single_default_slot() {
    let out = stdout(&program(
        "let tensor<f32, []> x ← empty",
        "    print x.shape().length()\n    print x",
    ));
    assert_eq!(out, ["0", "[]"]);
}

#[test]
fn empty_sparse_tensor_keeps_its_all_zero_shape() {
    let out = stdout(&program("let sparse<f32, [2, 3]> x ← empty", "    print x.shape()[1]"));
    assert_eq!(out, ["3"]);
}

#[test]
fn empty_collections_stay_empty_collections() {
    let out = stdout(&program(
        "let list<int> a ← empty\n    let map<string, int> b ← empty",
        "    print a.length()\n    print b.length()",
    ));
    assert_eq!(out, ["0", "0"]);
}
