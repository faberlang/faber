//! Need: bounded `string<N>` cells run on the MIR runner with the
//! store-time width check and the Rust carrier's failure text.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> Result<Vec<String>, RunSourceError> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session, "bounded.fab", &source, &mut host).map(|()| host.stdout_lines)
}

#[test]
fn store_within_the_bound_keeps_the_text() {
    let lines = run(
        "let string raw ← \"héllo\"\nlet string<size 5> a ← raw ↦ string<size 5>\nprint a.length()\nprint a ↦ string",
    )
    .expect("runs");
    assert_eq!(lines, ["5", "héllo"]);
}

#[test]
fn store_over_the_bound_traps_with_the_rust_text() {
    let error = run("let string raw ← \"abcdef\"\nlet string<size 4> a ← raw ↦ string<size 4>\nprint a ↦ string")
        .expect_err("overflow must fail");
    assert!(
        format!("{error:?}").contains("textus to bounded textus conversion failed"),
        "got: {error:?}"
    );
}

#[test]
fn recovery_supplies_the_fallback_on_overflow() {
    let lines = run(
        "let string raw ← \"abcdef\"\nlet string<size 4> a ← raw ↦ string<size 4> ⊥ (\"fall\" ↦ string<size 4>)\nprint a ↦ string",
    )
    .expect("runs");
    assert_eq!(lines, ["fall"]);
}

#[test]
fn bounded_cells_assign_and_pass_to_a_string_parameter() {
    let lines = run(
        "let string<size 8> a ← \"abc\" ↦ string<size 8>\nmut string<size 8> b ← \"x\" ↦ string<size 8>\nb ← a\nprint b ↦ string\nprint b.slice(0, 2)",
    )
    .expect("runs");
    assert_eq!(lines, ["abc", "ab"]);
}
