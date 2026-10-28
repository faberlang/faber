//! Need: text methods on bounded cord receivers (`ascii<N>`,
//! `string<N>`) widen the receiver to `textus` at the lowering seam, the same
//! copy a `↦ textus` conversion performs. The checker already resolves these
//! intrinsics for cord receivers; MIR validation requires the unbounded textus.
//!
//! ASCII-typed arguments of the text predicates, `split` and `replace` take the
//! same seam: bare `ascii` and `ascii<1>` widen implicitly to `textus` (the
//! checker's rule) and are retyped at lowering; `ascii<N>` with `N > 1` is
//! refused at check as `argument_type_mismatch`, never reaching MIR validation.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";

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
    assert_eq!(ok("sit ascii<magnitudo 8> a ← 'abc'\nnota a.sectio(0, 2)"), ["ab"]);
}

#[test]
fn bounded_cord_ascii_uppercase_widens_to_textus() {
    assert_eq!(ok("sit ascii<magnitudo 8> a ← 'abc'\nnota a.maiuscula()"), ["ABC"]);
}

#[test]
fn bounded_cord_ascii_starts_with_widens_to_textus() {
    assert_eq!(ok("sit ascii<magnitudo 8> a ← 'abc'\nnota a.initium(\"ab\")"), ["verum"]);
}

#[test]
fn bounded_cord_ascii_ends_with_widens_to_textus() {
    assert_eq!(ok("sit ascii<magnitudo 8> a ← 'abc'\nnota a.finis(\"bc\")"), ["verum"]);
}

#[test]
fn bounded_cord_ascii_contains_widens_to_textus() {
    assert_eq!(ok("sit ascii<magnitudo 8> a ← 'abc'\nnota a.continet(\"bc\")"), ["verum"]);
}

#[test]
fn bounded_cord_textus_trim_widens_to_textus() {
    assert_eq!(
        ok("sit textus<magnitudo 8> s ← \"  ab \" ↦ textus<magnitudo 8>\nnota s.recide()"),
        ["ab"]
    );
}

#[test]
fn bounded_cord_textus_split_widens_to_textus() {
    assert_eq!(
        ok("sit textus<magnitudo 8> s ← \"a,b\" ↦ textus<magnitudo 8>\nnota s.divide(\",\").longitudo()"),
        ["2"]
    );
}

#[test]
fn bounded_cord_textus_replace_widens_to_textus() {
    assert_eq!(
        ok("sit textus<magnitudo 8> s ← \"abc\" ↦ textus<magnitudo 8>\nnota s.muta(\"b\", \"z\")"),
        ["azc"]
    );
}

#[test]
fn bounded_cord_textus_escape_widens_to_textus() {
    assert_eq!(
        ok("sit textus<magnitudo 8> s ← \"ab\" ↦ textus<magnitudo 8>\nnota s.munita()"),
        ["ab"]
    );
}

#[test]
fn bounded_cord_textus_range_window_reads_a_textus_window() {
    assert_eq!(
        ok("sit textus<magnitudo 8> s ← \"abcdef\" ↦ textus<magnitudo 8>\nnota s[1‥3]"),
        ["bc"]
    );
    assert_eq!(
        ok("sit textus<magnitudo 8> s ← \"abcdef\" ↦ textus<magnitudo 8>\nnota s[1 usque 3]"),
        ["bcd"]
    );
}

#[test]
fn bounded_cord_ascii_range_window_reads_a_textus_window() {
    assert_eq!(ok("sit ascii<magnitudo 8> a ← 'abc'\nnota a[0‥2]"), ["ab"]);
}

fn frontend_rejection(body: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    let Err(RunSourceError::Frontend(diagnostics)) = run_source(&session, "bounded-cord.fab", &source, &mut host)
    else {
        panic!("{body}: must be refused at check");
    };
    format!("{diagnostics:?}")
}

#[test]
fn bounded_cord_ascii_literal_arguments_widen_to_textus() {
    let prelude = "sit ascii<magnitudo 16> a ← ' Hello '\n";
    assert_eq!(ok(&format!("{prelude}nota a.initium(' H')")), ["verum"]);
    assert_eq!(ok(&format!("{prelude}nota a.finis('o ')")), ["verum"]);
    assert_eq!(ok(&format!("{prelude}nota a.muta('l', 'L')")), [" HeLLo "]);
    assert_eq!(ok(&format!("{prelude}nota a.divide('l').longitudo()")), ["3"]);
}

#[test]
fn bounded_cord_ascii_variable_arguments_widen_to_textus() {
    let prelude = "sit ascii<magnitudo 16> a ← ' Hello '\nsit ascii p ← ' H'\nsit ascii<magnitudo 1> l ← 'l'\nsit ascii<magnitudo 1> u ← 'L'\n";
    assert_eq!(ok(&format!("{prelude}nota a.initium(p)")), ["verum"]);
    assert_eq!(ok(&format!("{prelude}nota a.finis(p)")), ["falsum"]);
    assert_eq!(ok(&format!("{prelude}nota a.muta(l, u)")), [" HeLLo "]);
    assert_eq!(ok(&format!("{prelude}nota a.divide(l).longitudo()")), ["3"]);
}

#[test]
fn textus_receiver_ascii_arguments_widen_to_textus() {
    let prelude = "sit textus s ← \"hello\"\nsit ascii p ← 'he'\n";
    assert_eq!(ok(&format!("{prelude}nota s.initium('he')")), ["verum"]);
    assert_eq!(ok(&format!("{prelude}nota s.initium(p)")), ["verum"]);
    assert_eq!(ok(&format!("{prelude}nota s.muta(p, 'J')")), ["Jllo"]);
}

#[test]
fn bounded_ascii_argument_over_one_is_refused_at_check() {
    for call in ["initium(p)", "finis(p)", "muta(p, p)", "divide(p)"] {
        let text = frontend_rejection(&format!(
            "sit ascii<magnitudo 16> a ← ' Hello '\nsit ascii<magnitudo 4> p ← 'll'\nnota a.{call}"
        ));
        assert!(text.contains(ISSUE_ARGUMENT_TYPE_MISMATCH), "{call}: {text}");
    }
}

#[test]
fn bounded_cord_ascii_slice_out_of_range_aborts_like_textus() {
    // The runner host aborts by panicking; both receivers report the same reason.
    let abort_reason = |body: &str| {
        let body = body.to_owned();
        let payload = std::panic::catch_unwind(move || run(&body)).expect_err("out-of-range slice must abort");
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "<non-string panic>".to_owned())
    };
    let ascii = abort_reason("sit ascii<magnitudo 8> a ← 'abc'\nnota a.sectio(2, 9)");
    let textus = abort_reason("sit textus<magnitudo 8> s ← \"abc\" ↦ textus<magnitudo 8>\nnota s.sectio(2, 9)");
    assert!(ascii.contains("slice_out_of_range"), "{ascii}");
    assert_eq!(ascii, textus);
}
