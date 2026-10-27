//! D9.4 runner-route pins: built-in `ordo` conversion rows lower to MIR
//! equality chains and run on the MIR runner.

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

fn stdout(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_fixture_source(source, &mut host).expect("fixture runs");
    host.stdout_lines
}

const ORDINES: &str = "
ordo Color { rubrum, viridis, caeruleum }
ordo Status { pendens = 10, actum, finitum = -3, deletum }
";

#[test]
fn ordo_to_numerus_and_textus_run() {
    let lines = stdout(&format!(
        "{ORDINES}
incipit {{
    nota Color.viridis ↦ numerus
    nota Status.actum ↦ numerus
    nota Status.deletum ↦ numerus
    nota Color.caeruleum ↦ textus
}}
"
    ));
    assert_eq!(lines, vec!["1", "11", "-2", "caeruleum"]);
}

#[test]
fn numerus_to_ordo_matches_or_takes_the_default() {
    let lines = stdout(&format!(
        "{ORDINES}
incipit {{
    sit Status hit ← -3 ↦ Status ⊥ Status.pendens
    nota hit ↦ textus
    sit Color miss ← 9 ↦ Color ⊥ Color.rubrum
    nota miss ↦ textus
}}
"
    ));
    assert_eq!(lines, vec!["finitum", "rubrum"]);
}

#[test]
fn numerus_to_ordo_failure_propagates_through_the_channel() {
    let lines = stdout(&format!(
        "{ORDINES}
functio lege(numerus n) → Color ⇥ textus {{
    redde n ↦ Color
}}

functio tuta(numerus n) → textus {{
    fac {{
        redde lege(n) ↦ textus
    }}
    cape err {{
        redde err
    }}
}}

incipit {{
    nota tuta(2)
    nota tuta(5)
}}
"
    ));
    assert_eq!(lines, vec!["caeruleum", "no ordo member has this value"]);
}

#[test]
#[should_panic(expected = "runner abort: no ordo member has this value")]
fn numerus_to_ordo_without_a_channel_aborts() {
    let mut host = BufferHost::default();
    let _ = run_fixture_source(
        &format!("{ORDINES}\nincipit {{\n    sit Color c ← 5 ↦ Color\n    nota c ↦ textus\n}}\n"),
        &mut host,
    );
}
