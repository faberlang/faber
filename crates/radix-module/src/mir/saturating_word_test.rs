//! D11.1 runner contract for `saturatus<W>`: add, sub, mul, and div clamp at
//! both ends of the width, signed and unsigned, including the 64-bit edges;
//! division by zero still fails, and `↦` into a saturating width range-checks
//! instead of clamping.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run_fixture_source(source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
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
fn saturating_unsigned_add_sub_mul_clamp_at_both_ends() {
    let lines = run_lines(
        r"
incipit {
    fixum saturatus<u8> high ← 250
    fixum saturatus<u8> low ← 5
    nota high + 10
    nota low - 10
    nota high * 2
    nota high + 5
    nota low - 5
}
",
    );
    assert_eq!(lines, vec!["255", "0", "255", "255", "0"]);
}

#[test]
fn saturating_signed_add_sub_mul_clamp_at_both_ends() {
    let lines = run_lines(
        r"
incipit {
    fixum saturatus<i16> big ← 30000
    fixum saturatus<i16> small ← -30000
    nota big + 10000
    nota small - 10000
    nota big * 2
    nota big * -2
    nota small + 10000
}
",
    );
    assert_eq!(lines, vec!["32767", "-32768", "32767", "-32768", "-20000"]);
}

#[test]
fn saturating_signed_min_negation_and_division_clamp_to_max() {
    let lines = run_lines(
        r"
incipit {
    fixum saturatus<i8> low ← -100
    fixum saturatus<i8> floor ← low - 100
    nota floor
    nota -floor
    nota floor / -1
    nota floor % -1
}
",
    );
    assert_eq!(lines, vec!["-128", "127", "127", "0"]);
}

#[test]
fn saturating_chain_clamps_once_at_the_store() {
    // d11-6 ruling 3: computed exactly, clamped when the value lands. Per-step
    // clamping (the pre-U9 behaviour) gave 155, 100, 63.
    let lines = run_lines(
        r"
incipit {
    fixum saturatus<u8> x ← 250
    fixum saturatus<u8> small ← 10
    fixum saturatus<u8> wide ← 200
    fixum saturatus<u8> up ← x + 200 - 100
    fixum saturatus<u8> down ← small - 300 + 100
    fixum saturatus<u8> quotient ← wide * 2 / 4
    fixum saturatus<u8> fits ← x + x - x
    fixum saturatus<u8> long ← small - wide - wide + wide + wide
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
    fixum saturatus<i8> low ← -100
    fixum saturatus<i8> same ← low - 100 + 100
    fixum saturatus<i8> top ← -low * 3 / 2
    nota same
    nota top
}
",
    );
    assert_eq!(lines, vec!["250", "-100", "127"]);
}

#[test]
fn saturating_64_bit_widths_clamp_exactly() {
    let lines = run_lines(
        r"
incipit {
    fixum saturatus<u64> top ← 18446744073709551615
    fixum saturatus<i64> high ← 9223372036854775807
    fixum saturatus<i64> low ← -9223372036854775807
    nota top + 1
    nota top * 2
    nota high + 1
    nota low - 2
}
",
    );
    assert_eq!(
        lines,
        vec![
            "18446744073709551615",
            "18446744073709551615",
            "9223372036854775807",
            "-9223372036854775808",
        ]
    );
}

#[test]
fn saturating_division_by_zero_still_fails() {
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    fixum saturatus<u8> level ← 9
    fixum saturatus<u8> zero ← 0
    nota level / zero
}
",
        &mut host,
    )
    .expect_err("saturating division by zero fails");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "saturating_division_by_zero"),
        "{errors:?}"
    );
}

#[test]
fn saturating_conversio_range_checks_instead_of_clamping() {
    assert_eq!(
        run_lines(
            r"
incipit {
    fixum numerus n ← 200
    fixum saturatus<u8> level ← n ↦ saturatus<u8>
    nota level + 100
    nota level ↦ numerus
}
",
        ),
        vec!["255", "200"]
    );
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum numerus n ← 300
    fixum saturatus<u8> level ← n ↦ saturatus<u8>
    nota level
}
",
        &mut host,
    )
    .expect_err("an out-of-range conversion into saturatus<u8> fails, never clamps");
}
