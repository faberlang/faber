//! Public-cut target gate: run with `cargo test -p faber --no-default-features --features hir-faber,hir-fhir,mir-fmir,runner --test public_cut_target_gate_test`.
#![cfg(all(
    not(feature = "hir-rust"),
    not(feature = "hir-ts"),
    not(feature = "hir-go")
))]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

const SOURCE: &str = "+++\nlocale = \"en\"\n+++\nmain {\n    print \"Salve, Munde!\"\n}\n";

fn write_source(dir: &TempDir) -> PathBuf {
    let path = dir.join("hello.fab");
    fs::write(&path, SOURCE).expect("write source");
    path
}

fn run_faber(args: &[&str], source: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .arg(source)
        .output()
        .expect("spawn faber")
}

fn assert_target_gated(args: &[&str]) {
    let dir = TempDir::new("faber-public-cut", "gate");
    let source = write_source(&dir);
    let output = run_faber(args, &source);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0), "{args:?}: {stderr}");
    assert!(!stderr.is_empty(), "{args:?}: empty stderr");
    assert!(
        stderr.contains("target_not_in_this_build"),
        "{args:?}: {stderr}"
    );
    assert!(!stderr.contains("panicked"), "{args:?}: {stderr}");
}

#[test]
fn build_rust_is_gated() {
    assert_target_gated(&["build", "--target", "rust"]);
}

#[test]
fn build_ts_is_gated() {
    assert_target_gated(&["build", "--target", "ts"]);
}

#[test]
fn emit_go_is_gated() {
    assert_target_gated(&["emit", "-t", "go"]);
}

#[test]
fn emit_ts_is_gated() {
    assert_target_gated(&["emit", "-t", "ts"]);
}

#[test]
fn emit_rust_is_gated() {
    assert_target_gated(&["emit", "-t", "rust"]);
}

#[test]
fn targets_table_marks_private_targets_unavailable() {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("targets")
        .output()
        .expect("spawn faber targets");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in ["rust", "ts", "go"] {
        let prefix = format!("{name} ");
        let row = stdout
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("no `{name}` row in targets table:\n{stdout}"));
        assert!(row.contains("available=no"), "{row}");
    }
}

#[test]
fn emit_faber_still_succeeds() {
    let dir = TempDir::new("faber-public-cut", "faber");
    let source = write_source(&dir);
    let output = run_faber(&["emit", "-t", "faber"], &source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
