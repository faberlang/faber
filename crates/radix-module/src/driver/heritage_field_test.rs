//! ON-R8: inherited-field aggregate construction consistent
//! semantic ↔ MIR.
//!
//! The focused p4b child-constructor probe, re-authored per the
//! admitted-old-needs-lowering goal's Open question 1 default (the original
//! `p4b-heritage.fab` was absent at HEAD): a child genus literal supplies a
//! parent field, and the run observes the inherited values.
//!
//! The semantic merged heritage view admits the parent field; the MIR side
//! (validation metadata and the construction fill) consumes the same
//! resolver-published view, so the literal passes semantic analysis, MIR
//! validation, and execution.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

const P4B_HERITAGE: &str = r#"+++
locale = "en"
+++
class Animal {
    string nomen
    int legs = 4
}

class Dog extends Animal {
    int barks
}

main {
    const Dog rex ← Dog { nomen = "Rex", barks = 3 }
    print rex.nomen
    print rex.legs
}
"#;

#[test]
fn p4b_child_genus_literal_supplies_parent_field() {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, "p4b-heritage.fab", P4B_HERITAGE, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("semantic analysis must admit the merged field view: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => {
            panic!("MIR lowering/validation must accept inherited fields: {error:?}")
        }
        Err(RunSourceError::Stepper(errors)) => {
            panic!("the child construction must execute: {errors:?}")
        }
    }
    // The supplied parent field keeps its value, and the omitted defaulted
    // parent field materializes through the merged view.
    assert_eq!(
        host.stdout_lines,
        vec!["Rex".to_owned(), "4".to_owned()],
        "the run must observe both inherited field values"
    );
}
