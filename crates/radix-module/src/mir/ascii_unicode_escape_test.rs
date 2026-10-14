//! Need, point A: an `ascii` literal decodes its escapes on the MIR
//! runner exactly as it does for the typechecker, through the one lexer decoder.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

fn run(body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session, "ascii-unicode-escape.fab", &source, &mut host)
        .unwrap_or_else(|error| panic!("{body}: {error:?}"));
    host.stdout_lines
}

#[test]
fn unicode_scalar_escape_prints_the_scalar() {
    assert_eq!(run(r#"nota '\u{41}'"#), ["A"]);
}

#[test]
fn tab_escape_prints_a_tab() {
    assert_eq!(run(r#"nota 'a\tb'"#), ["a\tb"]);
}

#[test]
fn unicode_control_escape_prints_the_scalar() {
    assert_eq!(run(r#"nota '\u{1B}'"#), ["\u{1b}"]);
}
