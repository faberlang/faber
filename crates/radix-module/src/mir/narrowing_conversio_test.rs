//! D11.6 U1 / D1.11 runner-route pins: explicit integer `↦` narrowing is
//! magnitude-checked, and its `⊥` default literal shapes to the target width
//! (p17b previously failed MIR validation with a recovery type mismatch).

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run_fixture_source(source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
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

fn narrowing_issue(source: &str) -> String {
    let mut host = BufferHost::default();
    let errors = run_fixture_source(source, &mut host).expect_err("narrowing must fail closed");
    assert!(host.stdout_lines.is_empty(), "no value may print: {:?}", host.stdout_lines);
    errors.first().expect("one runner error").issue.clone()
}

const OUT_OF_RANGE: &str = "runner_numerus_to_numerus_conversion_out_of_range";

#[test]
fn int_above_u8_range_fails_closed() {
    let issue = narrowing_issue(
        r"
incipit {
    sit numerus n ← 300
    sit u8 b ← n ↦ u8
    nota b
}
",
    );
    assert_eq!(issue, OUT_OF_RANGE);
}

#[test]
fn int_above_i8_range_fails_closed() {
    let issue = narrowing_issue(
        r"
incipit {
    sit numerus n ← 300
    sit i8 b ← n ↦ i8
    nota b
}
",
    );
    assert_eq!(issue, OUT_OF_RANGE);
}

#[test]
fn negative_into_unsigned_still_fails_closed() {
    let issue = narrowing_issue(
        r"
incipit {
    sit numerus n ← -1
    sit u8 b ← n ↦ u8
    nota b
}
",
    );
    assert_eq!(issue, OUT_OF_RANGE);
}

#[test]
fn literal_recovery_shapes_to_the_target_width() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit numerus n ← 300
    sit u8 b ← n ↦ u8 ⊥ 7
    nota b
    sit i8 c ← n ↦ i8 ⊥ -7
    nota c
    sit u8 d ← 200 ↦ u8 ⊥ 7
    nota d
}
",
        &mut host,
    )
    .expect("recovery supplies the value");
    assert_eq!(host.stdout_lines, vec!["7", "-7", "200"]);
}
