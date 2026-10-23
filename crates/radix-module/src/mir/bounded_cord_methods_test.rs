//! Need: text methods on bounded cord receivers (`ascii<N>`,
//! `string<N>`) widen the receiver to `textus` at the lowering seam, the same
//! copy a `↦ textus` conversion performs. The checker already resolves these
//! intrinsics for cord receivers; MIR validation requires the unbounded textus.

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
    let errors = match run_source(&session, "bounded-cord.fab", &source, &mut host) {
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
fn bounded_cord_ascii_slice_widens_to_textus() {
    assert_eq!(ok("sit ascii<8> a ← 'abc'\nnota a.sectio(0, 2)"), ["ab"]);
}

#[test]
fn bounded_cord_ascii_uppercase_widens_to_textus() {
    assert_eq!(ok("sit ascii<8> a ← 'abc'\nnota a.maiuscula()"), ["ABC"]);
}

#[test]
fn bounded_cord_ascii_starts_with_widens_to_textus() {
    assert_eq!(ok("sit ascii<8> a ← 'abc'\nnota a.initium(\"ab\")"), ["verum"]);
}

#[test]
fn bounded_cord_ascii_ends_with_widens_to_textus() {
    assert_eq!(ok("sit ascii<8> a ← 'abc'\nnota a.finis(\"bc\")"), ["verum"]);
}

#[test]
fn bounded_cord_ascii_contains_widens_to_textus() {
    assert_eq!(ok("sit ascii<8> a ← 'abc'\nnota a.continet(\"bc\")"), ["verum"]);
}

#[test]
fn bounded_cord_textus_trim_widens_to_textus() {
    assert_eq!(ok("sit textus<8> s ← \"  ab \" ↦ textus<8>\nnota s.recide()"), ["ab"]);
}

#[test]
fn bounded_cord_textus_split_widens_to_textus() {
    assert_eq!(
        ok("sit textus<8> s ← \"a,b\" ↦ textus<8>\nnota s.divide(\",\").longitudo()"),
        ["2"]
    );
}

#[test]
fn bounded_cord_textus_replace_widens_to_textus() {
    assert_eq!(ok("sit textus<8> s ← \"abc\" ↦ textus<8>\nnota s.muta(\"b\", \"z\")"), ["azc"]);
}

#[test]
fn bounded_cord_textus_escape_widens_to_textus() {
    assert_eq!(ok("sit textus<8> s ← \"ab\" ↦ textus<8>\nnota s.munita()"), ["ab"]);
}

#[test]
fn bounded_cord_textus_range_window_reads_a_textus_window() {
    assert_eq!(ok("sit textus<8> s ← \"abcdef\" ↦ textus<8>\nnota s[1‥3]"), ["bc"]);
    assert_eq!(ok("sit textus<8> s ← \"abcdef\" ↦ textus<8>\nnota s[1 usque 3]"), ["bcd"]);
}

#[test]
fn bounded_cord_ascii_range_window_reads_a_textus_window() {
    assert_eq!(ok("sit ascii<8> a ← 'abc'\nnota a[0‥2]"), ["ab"]);
}
