//! MC3 — run rows for the zero-vector constructor `crea()` through the MIR
//! runner (OQ1: all lanes element-zero at the receiver's admitted width).
//! Sweeps the admitted register widths 1–4 (goal §5 deliberate ceiling) and
//! pins the integer element path (`vi32` lanes are integer zero, not float
//! zero — the `numeric_zero_constant` split).

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    )
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
fn crea_builds_zero_vector_at_each_admitted_width() {
    run_and_expect(
        "crea-vf32-1.fab",
        r"incipit {
    fixum vf32[1] z ← [0.5] ↦ vf32[1]
    fixum vf32[1] w ← z.crea()
    nota w
}",
        &["[0.0]"],
    );
    run_and_expect(
        "crea-vf32-2.fab",
        r"incipit {
    fixum vf32[2] z ← [0.5, 0.5] ↦ vf32[2]
    fixum vf32[2] w ← z.crea()
    nota w
}",
        &["[0.0, 0.0]"],
    );
    run_and_expect(
        "crea-vf32-3.fab",
        r"incipit {
    fixum vf32[3] z ← [0.5, 0.5, 0.5] ↦ vf32[3]
    fixum vf32[3] w ← z.crea()
    nota w
}",
        &["[0.0, 0.0, 0.0]"],
    );
    run_and_expect(
        "crea-vf32-4.fab",
        r"incipit {
    fixum vf32[4] z ← [0.5, 0.5, 0.5, 0.5] ↦ vf32[4]
    fixum vf32[4] w ← z.crea()
    nota w
}",
        &["[0.0, 0.0, 0.0, 0.0]"],
    );
}

#[test]
fn crea_integer_element_lanes_are_integer_zero() {
    run_and_expect(
        "crea-vi32-3.fab",
        r"incipit {
    fixum vi32[3] z ← [7, 7, 7] ↦ vi32[3]
    fixum vi32[3] w ← z.crea()
    nota w
}",
        &["[0, 0, 0]"],
    );
}
