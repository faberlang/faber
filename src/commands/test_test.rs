//! Companion tests for `faber test` (stepper proba runner).
//!
//! WHY: the hygiene ratchet bans inline `#[cfg(test)] mod tests` in production
//! sources; tests live in dedicated `*_test.rs` files.

use radix::driver::{Config, Session};
use radix::proba::{run_proba_source, TestSelection};
use std::fs;
use std::path::PathBuf;

fn scratch_dir() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-stepper-faber");
    let _ = fs::create_dir_all(&base);
    base
}

#[test]
fn stepper_runner_passes_simple_adfirma_fixture() {
    let dir = scratch_dir();
    let path = dir.join("pass.fab");
    fs::write(
        &path,
        r#"
proba "pass case" {
    adfirma 1 ≡ 1
}
"#,
    )
    .expect("write");
    let session = Session::new(Config::default());
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &session,
        &path.to_string_lossy(),
        &source,
        &TestSelection::default(),
        None,
        false,
        false,
    )
    .expect("run");
    assert!(report.success(), "{report:?}");
    assert_eq!(report.passed(), 1);
}

#[test]
fn stepper_runner_fails_named_case() {
    let dir = scratch_dir();
    let path = dir.join("fail.fab");
    fs::write(
        &path,
        r#"
proba "the broken one" {
    adfirma 1 ≡ 2
}
"#,
    )
    .expect("write");
    let session = Session::new(Config::default());
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &session,
        &path.to_string_lossy(),
        &source,
        &TestSelection::default(),
        None,
        false,
        false,
    )
    .expect("run");
    assert!(!report.success());
    assert_eq!(report.results[0].name, "the broken one");
}

#[test]
fn cmd_test_source_has_no_cargo_or_rust_executor() {
    // Guard: reintroducing Cargo / Target::Rust as the test executor must fail CI.
    // Inspect only production code (exclude any test module, which names banned APIs).
    let source = include_str!("test.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(
        !production.contains("invoke_cargo_test"),
        "cmd_test must not call invoke_cargo_test"
    );
    assert!(
        !production.contains("Target::Rust"),
        "cmd_test must not select Target::Rust for execution"
    );
    assert!(
        !production.contains("emit_generated_crate"),
        "cmd_test must not emit a generated test crate"
    );
    assert!(
        production.contains("run_proba_source") || production.contains("run_proba_on_analyzed"),
        "cmd_test must drive the stepper proba runner"
    );
}
