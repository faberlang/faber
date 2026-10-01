//! A static (`generis`) genus field reads through the type name on the runner:
//! the read is the field's folded initializer, with no instance and no struct
//! field projection.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

const TAB: &str =
    "genus Tab {\n    generis numerus Z = 3\n    generis textus NOMEN = \"tabula\"\n    varia numerus w = 1\n}\n";

fn run(body: &str) -> Result<Vec<String>, String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("{TAB}incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "static_field_read.fab", &source, &mut host) {
        Ok(()) => Ok(host.stdout_lines),
        Err(RunSourceError::Stepper(errors)) => Err(format!("{errors:?}")),
        Err(other) => Err(format!("{other:?}")),
    }
}

#[test]
fn static_field_read_through_type_name_prints_the_initializer() {
    let lines = run("nota Tab.Z\nnota Tab.NOMEN").expect("static reads run");
    assert_eq!(lines, vec!["3", "tabula"]);
}

#[test]
fn static_field_read_in_an_expression_and_beside_an_instance() {
    let lines = run("fixum Tab t ← Tab {}\nfixum numerus n ← Tab.Z + t.w\nnota n").expect("run");
    assert_eq!(lines, vec!["4"]);
}

#[test]
fn static_field_read_through_an_instance_is_a_compile_error_not_invalid_mir() {
    let err = run("fixum Tab t ← Tab {}\nnota t.Z").expect_err("instance read of a static");
    assert!(!err.contains("invalid MIR"), "{err}");
    assert!(err.contains("static_field_read_through_instance"), "{err}");
}

#[test]
fn static_field_read_of_an_instance_field_through_the_type_is_a_compile_error() {
    let err = run("nota Tab.w").expect_err("type read of an instance field");
    assert!(!err.contains("invalid MIR"), "{err}");
    assert!(err.contains("instance_field_read_through_type"), "{err}");
}
