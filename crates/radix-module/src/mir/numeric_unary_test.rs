//! MC2 — run rows for the numeric-unary family (`nega`/`negativa`/
//! `complementa`/`complementata`) through the MIR runner.
//!
//! `complement*` are numerus-only: the fractus SEM011 admission gate is
//! pinned here as a NEGATIVE row (fail-closed stays law, never widened).

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

fn run_and_expect(name: &str, source: &str, expected: &[&str]) {
    let mut host = BufferHost::default();
    match run_source(&session(), name, source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, expected),
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

#[test]
fn nega_negates_numerus_in_place() {
    run_and_expect(
        "nega-numerus.fab",
        r"incipit {
    varia numerus x ← 7
    x.nega()
    nota x
}",
        &["-7"],
    );
}

#[test]
fn negativa_returns_negation_on_numerus_and_fractus() {
    run_and_expect(
        "negativa-numerus.fab",
        r"incipit {
    sit numerus x ← 3
    sit numerus y ← x.negativa()
    nota y
    nota x
}",
        &["-3", "3"],
    );
    run_and_expect(
        "negativa-fractus.fab",
        r"incipit {
    sit fractus x ← 1.5
    sit fractus y ← x.negativa()
    nota y
}",
        &["-1.5"],
    );
}

#[test]
fn complementa_complements_numerus_in_place() {
    run_and_expect(
        "complementa-numerus.fab",
        r"incipit {
    varia numerus x ← 12
    x.complementa()
    nota x
}",
        &["-13"],
    );
}

#[test]
fn complementata_returns_complement_on_numerus() {
    run_and_expect(
        "complementata-numerus.fab",
        r"incipit {
    sit numerus x ← 5
    sit numerus y ← x.complementata()
    nota y
    nota x
}",
        &["-6", "5"],
    );
}

#[test]
fn complement_methods_reject_fractus_at_sem011() {
    let session = Session::new(Config::default());
    for call in ["x.complementa()", "x.complementata()"] {
        let source = format!(
            "incipit {{
    varia fractus x ← 1.5
    {call}
}}"
        );
        let result = analyze_source(&session, "complement-fractus.fab", &source, AnalyzeRequest::default());
        let Err(diagnostics) = result else {
            panic!("expected fractus complement to stay SEM011-fail-closed: {call}");
        };
        assert!(
            diagnostics.iter().any(|d| d.code == Some("SEM011")),
            "expected SEM011 for {call}, got: {diagnostics:?}"
        );
    }
}
