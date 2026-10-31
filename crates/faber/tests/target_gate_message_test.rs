//! The `PKG001:target_not_in_this_build` refusal carries its message and help.
//!
//! The target gate prints through a renderer that keeps an issue'd `PKG001`
//! diagnostic compact, so the refusal used to reach the terminal as
//! `error[PKG001:target_not_in_this_build]: ` with nothing after it: a public
//! build user could not tell what to do. The gate exits must name the target,
//! say the refusal is about this build, and point at `faber targets`.
//!
//! Runs on the public tree: `pascal` is a default-off Cargo feature in every
//! public configuration, so the gate always refuses it here.

#![cfg(not(feature = "hir-pascal"))]

use std::fs;
use std::process::{Command, Output};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

const SOURCE: &str = "+++\nlocale = \"en\"\n+++\nmain {\n    print \"Salve, Munde!\"\n}\n";

fn run_faber(args: &[&str]) -> Output {
    let dir = TempDir::new("faber-target-gate", "refusal");
    let source = dir.join("hello.fab");
    fs::write(&source, SOURCE).expect("write source");
    Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .arg(&source)
        .output()
        .expect("spawn faber")
}

/// The blank-output regression: the identity line alone is not enough.
fn assert_refusal_carries_text(output: &Output, stderr: &str) {
    assert_ne!(
        output.status.code(),
        Some(0),
        "gated build must fail:\n{stderr}"
    );
    assert!(
        !stderr
            .trim_end()
            .ends_with("error[PKG001:target_not_in_this_build]:"),
        "gate refusal printed with no message:\n{stderr}"
    );
    assert!(
        stderr.contains("error[PKG001:target_not_in_this_build]"),
        "refusal must keep the structured identity, got:\n{stderr}"
    );
    assert!(
        stderr.contains("target `pascal` is not compiled into this faber build"),
        "refusal must name the target and this build, got:\n{stderr}"
    );
    assert!(
        stderr.contains("faber targets"),
        "refusal must point at `faber targets` for the targets this build has, got:\n{stderr}"
    );
}

#[test]
fn gated_build_refusal_carries_message_and_help() {
    let output = run_faber(&["build", "--target", "pascal"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_refusal_carries_text(&output, &stderr);
}

#[test]
fn gated_emit_refusal_carries_message_and_help() {
    let output = run_faber(&["emit", "-t", "pascal"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_refusal_carries_text(&output, &stderr);
}
