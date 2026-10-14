//! d11-6 U4: implicit store nodes through the MIR runner.
//!
//! A store that does not fit a trapping slot traps and never enters `⇥` or a
//! `fac … cape` handler (ruling 5); a constant stored with `←` into a
//! `wrapping` or `saturating` slot follows the slot's policy (ruling 21).

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

fn run(name: &str, source: &str) -> (Result<(), RunSourceError>, Vec<String>) {
    let mut host = BufferHost::default();
    let result = run_source(&session(), name, source, &mut host);
    (result, host.stdout_lines)
}

fn run_ok(name: &str, source: &str) -> Vec<String> {
    let (result, stdout) = run(name, source);
    assert!(result.is_ok(), "fixture failed: {:?}", result.err());
    stdout
}

#[test]
fn saturating_constant_clamps_at_the_store() {
    let out = run_ok(
        "sat-const.fab",
        "incipit {\n    fixum saturatus<u8> w ← 300\n    fixum saturatus<i8> v ← -300\n    nota w\n    nota v\n}",
    );
    assert_eq!(out, ["255", "-128"]);
}

#[test]
fn wrapping_constant_reduces_at_the_store() {
    let out = run_ok(
        "wrap-const.fab",
        "incipit {\n    fixum modulus<u8> w ← 300\n    fixum modulus<u8> v ← -1\n    nota w\n    nota v\n}",
    );
    assert_eq!(out, ["44", "255"]);
}

#[test]
fn a_computed_value_that_fits_is_stored() {
    // `x * 3 / 2` is 150 at `x = 100`: the multiply does not trap, the store fits.
    let out = run_ok(
        "fits.fab",
        "incipit {\n    fixum u8 x ← 100\n    fixum u8 y ← x * 3 / 2\n    nota y\n}",
    );
    assert_eq!(out, ["150"]);
}

#[test]
fn a_computed_value_that_does_not_fit_traps_at_the_store() {
    // `x * 3 / 2` is 300 at `x = 200`: the trap is at the store, not the multiply.
    let (result, stdout) = run(
        "trap.fab",
        "incipit {\n    fixum u8 x ← 200\n    nota \"before\"\n    fixum u8 y ← x * 3 / 2\n    nota \"after\"\n}",
    );
    assert!(matches!(result, Err(RunSourceError::Stepper(_))));
    assert_eq!(stdout, ["before"]);
}

#[test]
fn a_failed_implicit_store_is_not_caught_by_cape() {
    let (result, stdout) = run(
        "cape.fab",
        "incipit {\n    fixum numerus n ← 300\n    fac {\n        fixum u8 z ← n\n        nota z\n    } cape err {\n        nota \"caught\"\n    }\n}",
    );
    assert!(matches!(result, Err(RunSourceError::Stepper(_))));
    assert!(stdout.is_empty(), "the handler must not run: {stdout:?}");
}

#[test]
fn an_explicit_conversion_stays_recoverable() {
    let out = run_ok(
        "explicit.fab",
        "incipit {\n    fixum numerus n ← 300\n    fixum u8 z ← n ↦ u8 ⊥ 7\n    nota z\n}",
    );
    assert_eq!(out, ["7"]);
}

const FETCH: &str = "functio fetch(numerus n) fiet → numerus {\n    redde n\n}\n";

#[test]
fn a_narrowing_await_bind_that_fits_is_stored() {
    let out = run_ok(
        "await-fits.fab",
        &format!("{FETCH}incipiet {{\n    fixum u8 v ↢ fetch(5)\n    nota v\n}}"),
    );
    assert_eq!(out, ["5"]);
}

#[test]
fn a_narrowing_await_bind_that_does_not_fit_traps() {
    let (result, stdout) = run(
        "await-trap.fab",
        &format!("{FETCH}incipiet {{\n    nota \"before\"\n    fixum u8 v ↢ fetch(300)\n    nota v\n}}"),
    );
    assert!(matches!(result, Err(RunSourceError::Stepper(_))));
    assert_eq!(stdout, ["before"]);
}

#[test]
fn a_failed_await_store_is_not_caught_by_cape() {
    let (result, stdout) = run(
        "await-cape.fab",
        &format!(
            "{FETCH}incipiet {{\n    fac {{\n        fixum u8 v ↢ fetch(300)\n        nota v\n    }} cape err {{\n        nota \"caught\"\n    }}\n}}"
        ),
    );
    assert!(matches!(result, Err(RunSourceError::Stepper(_))));
    assert!(stdout.is_empty(), "the handler must not run: {stdout:?}");
}

#[test]
fn a_figendum_await_bind_traps_like_its_twin() {
    let (result, _) = run(
        "await-figendum.fab",
        &format!("{FETCH}incipiet {{\n    figendum u8 v ← fetch(300)\n    nota v\n}}"),
    );
    assert!(matches!(result, Err(RunSourceError::Stepper(_))));
}
