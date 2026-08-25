//! Focused runner coverage for scalar integer `potentia`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{run_source, BufferHost, RunSourceError};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

#[test]
fn integer_potentia_runner_handles_negative_base_and_zero_exponent() {
    let source = r"incipit {
    fixum numerus base ← -2
    fixum numerus odd ← base.potentia(3)
    fixum numerus even ← base.potentia(4)
    fixum numerus identity ← base.potentia(0)
    nota odd
    nota even
    nota identity
}";
    let mut host = BufferHost::default();
    match run_source(&session(), "potentia.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, vec!["-8", "16", "1"]),
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}
