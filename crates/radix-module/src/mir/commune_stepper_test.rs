//! TEU3: MIR stepper exercises `e.message` on a `@ commune` union.

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
    run_source(&session, "commune.fab", source, host).map_err(|error| match error {
        RunSourceError::Stepper(errors) => errors,
        RunSourceError::Frontend(diagnostics) => {
            panic!("fixture source should analyze cleanly: {diagnostics:?}")
        }
        RunSourceError::Mir(error) => {
            panic!("fixture source should lower cleanly: {error:?}")
        }
    })
}

#[test]
fn commune_member_access_reads_shared_field_on_every_variant() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
discretio ShapeError {
    @ commune
    textus message

    Incompatible,
    ElementMismatch { numerus expected numerus actual }
}

incipit {
    fixum ShapeError a ← finge Incompatible { message = "incompatible" } ∷ ShapeError
    fixum ShapeError b ← finge ElementMismatch { message = "mismatch", expected = 1, actual = 2 } ∷ ShapeError
    nota a.message
    nota b.message
}
"#,
        &mut host,
    )
    .expect("commune member access runs on the MIR stepper");
    assert_eq!(host.stdout_lines, vec!["incompatible".to_owned(), "mismatch".to_owned()]);
}
