//! Bounded cords retain text laws for concatenation and whole-string ordering.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "bounded-text-ops.fab", &source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{body}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

fn ok(body: &str) -> Vec<String> {
    let (out, errors) = run(body);
    assert!(errors.is_empty(), "{body}: {errors:?}");
    out
}

#[test]
fn bounded_textus_add_concatenates() {
    assert_eq!(
        ok(
            "sit textus<magnitudo 8> a ← \"ab\" ↦ textus<magnitudo 8>\nsit textus<magnitudo 8> b ← \"cd\" ↦ textus<magnitudo 8>\nnota a + b"
        ),
        ["abcd"]
    );
}

#[test]
fn bounded_ascii_add_concatenates() {
    assert_eq!(
        ok("sit ascii<magnitudo 8> a ← 'ab'\nsit ascii<magnitudo 8> b ← 'cd'\nnota a + b"),
        ["abcd"]
    );
}

#[test]
fn bounded_textus_ordering_uses_text_order() {
    assert_eq!(
        ok(
            "sit textus<magnitudo 8> a ← \"ab\" ↦ textus<magnitudo 8>\nsit textus<magnitudo 8> b ← \"cd\" ↦ textus<magnitudo 8>\nnota a ≺ b"
        ),
        ["verum"]
    );
}

#[test]
fn bounded_ascii_ordering_uses_text_order() {
    assert_eq!(
        ok("sit ascii<magnitudo 8> a ← 'ab'\nsit ascii<magnitudo 8> b ← 'cd'\nnota a ≺ b"),
        ["verum"]
    );
}
