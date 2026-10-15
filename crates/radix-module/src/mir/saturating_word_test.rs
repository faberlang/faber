//! D11.1 runner contract for `saturatus<W>` under d11-6 ruling 3: `+ - * / %`
//! over saturating operands is exact like every integer expression, and only a
//! store into a saturating slot clamps, at both ends of the width, signed and
//! unsigned, including the 64-bit edges; division by zero is an ordinary
//! integer division failure, and `↦` into a saturating width clamps to the
//! range and never fails (ruling 24).

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run_fixture_source(source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    run_source(&session, "test.fab", source, host).map_err(|error| match error {
        RunSourceError::Stepper(errors) => errors,
        RunSourceError::Frontend(diagnostics) => {
            panic!("fixture source should analyze cleanly: {diagnostics:?}")
        }
        RunSourceError::Mir(error) => {
            panic!("fixture source should lower cleanly: {error:?}")
        }
    })
}

fn run_lines(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_fixture_source(source, &mut host).expect("runner runs saturating fixture");
    host.stdout_lines
}

#[test]
fn saturating_unsigned_arithmetic_is_exact_and_the_store_clamps_at_both_ends() {
    let lines = run_lines(
        r"
incipit {
    sit saturatus<u8> high ← 250
    sit saturatus<u8> low ← 5
    nota high + 10
    nota low - 10
    nota high * 2
    nota high + 5
    nota low - 5
    sit saturatus<u8> up ← high + 10
    sit saturatus<u8> down ← low - 10
    sit saturatus<u8> twice ← high * 2
    nota up
    nota down
    nota twice
}
",
    );
    assert_eq!(lines, vec!["260", "-5", "500", "255", "0", "255", "0", "255"]);
}

#[test]
fn saturating_signed_arithmetic_is_exact_and_the_store_clamps_at_both_ends() {
    let lines = run_lines(
        r"
incipit {
    sit saturatus<i16> big ← 30000
    sit saturatus<i16> small ← -30000
    nota big + 10000
    nota small - 10000
    nota big * 2
    nota big * -2
    nota small + 10000
    sit saturatus<i16> up ← big + 10000
    sit saturatus<i16> down ← small - 10000
    sit saturatus<i16> twice ← big * 2
    sit saturatus<i16> negTwice ← big * -2
    nota up
    nota down
    nota twice
    nota negTwice
}
",
    );
    assert_eq!(
        lines,
        vec![
            "40000", "-40000", "60000", "-60000", "-20000", "32767", "-32768", "32767", "-32768"
        ]
    );
}

#[test]
fn saturating_signed_min_negation_and_division_are_exact_until_stored() {
    let lines = run_lines(
        r"
incipit {
    sit saturatus<i8> low ← -100
    sit saturatus<i8> floor ← low - 100
    nota floor
    nota -floor
    nota floor / -1
    nota floor % -1
    sit saturatus<i8> negated ← -floor
    sit saturatus<i8> quotient ← floor / -1
    nota negated
    nota quotient
}
",
    );
    assert_eq!(lines, vec!["-128", "128", "128", "0", "127", "127"]);
}

#[test]
fn saturating_chain_clamps_once_at_the_store() {
    // d11-6 ruling 3: computed exactly, clamped when the value lands. Per-step
    // clamping (the pre-U9 behaviour) gave 155, 100, 63.
    let lines = run_lines(
        r"
incipit {
    sit saturatus<u8> x ← 250
    sit saturatus<u8> small ← 10
    sit saturatus<u8> wide ← 200
    sit saturatus<u8> up ← x + 200 - 100
    sit saturatus<u8> down ← small - 300 + 100
    sit saturatus<u8> quotient ← wide * 2 / 4
    sit saturatus<u8> fits ← x + x - x
    sit saturatus<u8> long ← small - wide - wide + wide + wide
    nota up
    nota down
    nota quotient
    nota fits
    nota long
}
",
    );
    assert_eq!(lines, vec!["255", "0", "100", "250", "10"]);
}

#[test]
fn saturating_chain_in_a_reassignment_and_signed_width_clamps_once() {
    let lines = run_lines(
        r"
incipit {
    varia saturatus<u8> level ← 200
    level ← level + 100 - 50
    nota level
    sit saturatus<i8> low ← -100
    sit saturatus<i8> same ← low - 100 + 100
    sit saturatus<i8> top ← -low * 3 / 2
    nota same
    nota top
}
",
    );
    assert_eq!(lines, vec!["250", "-100", "127"]);
}

#[test]
fn saturating_64_bit_widths_clamp_exactly() {
    // The exact value must lie in [-2^63, 2^64 - 1] (ruling 17); inside it the
    // store clamps to the word at the 64-bit edges.
    let lines = run_lines(
        r"
incipit {
    sit saturatus<u64> zero ← 0
    sit saturatus<i64> high ← 9223372036854775807
    sit saturatus<i64> low ← -9223372036854775807
    nota high + 1
    nota low - 1
    sit saturatus<i64> up ← high + 1
    sit saturatus<i64> down ← low - 1
    sit saturatus<u64> floor ← zero - 5
    sit saturatus<u64> lifted ← zero + 18446744073709551615
    nota up
    nota down
    nota floor
    nota lifted
}
",
    );
    assert_eq!(
        lines,
        vec![
            "9223372036854775808",
            "-9223372036854775808",
            "9223372036854775807",
            "-9223372036854775808",
            "0",
            "18446744073709551615",
        ]
    );
}

#[test]
fn saturating_division_by_zero_is_an_integer_division_failure() {
    for body in ["nota level / zero", "nota level % zero", "nota level + 1 / zero"] {
        let mut host = BufferHost::default();
        let source = format!(
            r"
incipit {{
    sit saturatus<u8> level ← 9
    sit saturatus<u8> zero ← 0
    {body}
}}
"
        );
        let errors = run_fixture_source(&source, &mut host).expect_err("saturating division by zero fails");
        assert!(
            errors
                .iter()
                .any(|error| error.issue == "runner_numerus_division_failed"),
            "{body}: {errors:?}"
        );
    }
}

#[test]
fn saturating_conversio_clamps_and_never_fails() {
    // Ruling 24: `↦` into `saturatus<W>` clamps the exact source to the range.
    assert_eq!(
        run_lines(
            r"
incipit {
    sit numerus n ← 200
    sit numerus over ← 300
    sit numerus under ← -5
    sit numerus neg ← -200
    sit saturatus<u8> level ← n ↦ saturatus<u8>
    nota level + 100
    nota level ↦ numerus
    nota over ↦ saturatus<u8>
    nota under ↦ saturatus<u8>
    nota neg ↦ saturatus<i8>
}
",
        ),
        vec!["300", "200", "255", "0", "-128"]
    );
}
