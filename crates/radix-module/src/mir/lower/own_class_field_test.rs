//! Value-side `own` bind of a class value: the binding
//! stores the payload type, so a field read on it runs on the MIR runner
//! instead of failing validation with "field projection base is not a struct
//! or record".

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

#[test]
fn own_bind_of_class_value_reads_field_on_runner() {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let source = r"genus Holder {
    varia f64 x = 0.0
}

incipit {
    fixum Holder d ← Holder { x = 2.0 }
    fixum Holder f ← own d
    nota f.x
}
";
    let mut host = BufferHost::default();
    match run_source(&session, "own_class_field.fab", source, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("runner should run: {errors:?}"),
    }
    assert_eq!(host.stdout_lines, vec!["2.0".to_owned()]);
}
