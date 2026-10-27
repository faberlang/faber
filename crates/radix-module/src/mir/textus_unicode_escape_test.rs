//! Need, unit 2: the `\u{H…}` text escape decodes on the MIR runner
//! exactly as it does for the typechecker, through the one lexer decoder.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

fn run(body: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session, "textus-unicode-escape.fab", &source, &mut host)
        .unwrap_or_else(|error| panic!("{body}: {error:?}"));
    host.stdout_lines
}

#[test]
fn unicode_scalar_escape_prints_the_scalar() {
    assert_eq!(run(r#"nota "\u{41}""#), ["A"]);
    assert_eq!(run(r#"nota "a\u{e9}b""#), ["a\u{e9}b"]);
}

// Need, ruling 2026-10-13: anything else after a backslash is a
// compile error, so as-written text with a backslash is spelled with the
// supported `\\` escape ("\\q" is the two characters backslash, q).
#[test]
fn malformed_unicode_escape_keeps_the_text_as_written() {
    assert_eq!(run(r#"nota "\\u{41""#), [r"\u{41"]);
    assert_eq!(run(r#"nota "\\q""#), [r"\q"]);
}

#[test]
fn unknown_escape_is_rejected_by_the_runner() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = String::from("incipit {\nnota \"\\q\"\n}\n");
    let mut host = BufferHost::default();
    assert!(run_source(&session, "textus-unicode-escape.fab", &source, &mut host).is_err());
}
