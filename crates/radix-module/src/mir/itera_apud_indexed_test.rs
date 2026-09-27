//! D3.1–D3.3: `itera ex … apud` over indexed iterables on the MIR runner.
//!
//! The `apud` names are exactly what goes in the index brackets to get the
//! element back: a lista binds its position, a tabula its key, and a
//! composite-key tabula (`tabula<iuncta<K1, K2>, V>`) each key part. Map
//! iteration order is unspecified (D19.2), so map rows are compared sorted.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    match run_source(&session, "itera-apud-indexed.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

fn sorted(mut lines: Vec<String>) -> Vec<String> {
    lines.sort();
    lines
}

#[test]
fn lista_apud_binds_position_and_element() {
    let lines = run(r#"
incipit {
    fixum lista<textus> xs ← ["a", "b", "c"]
    itera ex xs apud [i] fixum x {
        nota i, x
    }
}"#);
    assert_eq!(lines, ["0 a", "1 b", "2 c"]);
}

#[test]
fn tabula_apud_binds_key_and_value() {
    let lines = run(r#"
incipit {
    varia tabula<textus, numerus> m ← vacua
    m["alpha"] ← 1
    m["beta"] ← 2
    itera ex m apud [k] fixum v {
        nota k, v
    }
}"#);
    assert_eq!(sorted(lines), ["alpha 1", "beta 2"]);
}

#[test]
fn composite_key_tabula_looks_up_and_iterates_rank_two() {
    let lines = run(r#"
incipit {
    varia tabula<iuncta<textus, numerus>, numerus> sales ← vacua
    sales[["east", 2024]] ← 5
    sales[["west", 2024]] ← 7
    sales[["east", 2024]] ← 6
    nota sales[["east", 2024]]
    nota sales.longitudo()
    itera ex sales apud [regio, annus] fixum v {
        nota regio, annus, v
    }
}"#);
    assert_eq!(lines[..2], ["6", "2"]);
    assert_eq!(sorted(lines[2..].to_vec()), ["east 2024 6", "west 2024 7"]);
}
