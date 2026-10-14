//! F9 ruling 30 (dec2) on the runner: an untyped constant beside a `d64`
//! operand, in a comparison, or in a `d64` slot joins by exact value, and a
//! decimal constant passed as a call argument lowers to a typed `d64` value.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(source: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, "dec.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Stepper(errors)) => panic!("{source}: {errors:?}"),
        Err(other) => panic!("{source}: {other:?}"),
    }
}

fn main_body(body: &str) -> Vec<String> {
    run(&format!("incipit {{\nvaria d64 d ← 4.2\n{body}\n}}\n"))
}

#[test]
fn decimal_literal_adds_exactly() {
    assert_eq!(main_body("nota d + 0.1"), ["4.3"]);
}

#[test]
fn decimal_literal_multiplies_exactly() {
    assert_eq!(main_body("nota d * 2.0"), ["8.4"]);
}

#[test]
fn integer_constant_multiplies_exactly() {
    assert_eq!(main_body("nota d * 2"), ["8.4"]);
}

#[test]
fn integer_constant_adds_exactly() {
    assert_eq!(main_body("nota d + 1\nnota 1 + d\nnota d - 1"), ["5.2", "5.2", "3.2"]);
}

#[test]
fn integer_constant_divides_exactly() {
    assert_eq!(main_body("nota d / 2"), ["2.1"]);
}

#[test]
fn negated_integer_constant_joins() {
    assert_eq!(main_body("nota d + -1"), ["3.2"]);
}

#[test]
fn approx_equality_takes_the_decimal_constant() {
    assert_eq!(
        main_body("nota d ≅ 4.2\nnota 4.2 ≅ d\nnota d ≇ 4.2"),
        ["verum", "verum", "falsum"]
    );
}

#[test]
fn ordering_takes_the_decimal_constant() {
    assert_eq!(
        main_body("nota d ≺ 0.1\nnota d ≻ 4.19\nnota d ≤ 4.2\nnota d ≥ 5"),
        ["falsum", "verum", "verum", "falsum"]
    );
}

#[test]
fn integer_constant_fills_a_d64_slot() {
    let lines = run("incipit {\nfixum d64 e ← 5\nfixum d64 n ← -5\nfixum d64 s ← 1 + 2\nnota e\nnota n\nnota s\n}\n");
    assert_eq!(lines, ["5", "-5", "3"]);
}

#[test]
fn decimal_constant_arithmetic_fills_a_d64_slot() {
    assert_eq!(run("incipit {\nfixum d64 e ← 0.1 + 0.2\nnota e\n}\n"), ["0.3"]);
}

#[test]
fn integer_constant_fills_a_d64_field_and_return() {
    let lines = run("genus P { varia d64 v }\n\
         functio five() → d64 { redde 5 }\n\
         incipit {\nfixum P p ← P { v = 7 }\nnota p.v\nnota five()\n}\n");
    assert_eq!(lines, ["7", "5"]);
}

#[test]
fn decimal_literal_call_argument_is_a_typed_d64() {
    let lines = run("functio half(d64 x) → d64 { redde x }\n\
         incipit {\nnota half(3.0)\nnota half(2.5)\n}\n");
    assert_eq!(lines, ["3", "2.5"]);
}

#[test]
fn integer_constant_call_argument_is_a_typed_d64() {
    let lines = run("functio half(d64 x) → d64 { redde x }\n\
         incipit {\nnota half(3)\n}\n");
    assert_eq!(lines, ["3"]);
}
