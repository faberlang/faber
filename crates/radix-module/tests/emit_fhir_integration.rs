#![cfg(feature = "hir-fhir")]

//! Integration tests for FHIR emit → load CLI round-trip.
//!
//! UNIT-U3.4: Proves that `radix emit -t fhir` produces valid `.fhir`
//! artifacts that `load_fhir` accepts, and that the loaded unit produces
//! codegen output identical to the original. Also covers CLI error paths.
//!
//! done_when §1: round-trip test
//! done_when §2: CLI error path tests

use radix_module::codegen::{generate_from_analyzed, Target};
use radix_module::driver::{analyze_source, Config, Session};
use radix_module::hir::artifact::load_fhir;
use radix_module::locale::{latin_locale_pack, KeywordSurface};
use std::process::Command;

// ── CLI binary path ──────────────────────────────────────────────────────

/// Path to the compiled `radix` binary, set by cargo's integration test
/// harness via `CARGO_BIN_EXE_radix`.
fn radix_binary() -> String {
    std::env::var("CARGO_BIN_EXE_radix")
        .unwrap_or_else(|_| panic!("CARGO_BIN_EXE_radix not set; run with `cargo test`"))
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Extract the code string from a codegen output for byte-for-byte comparison.
fn extract_code(output: &radix_module::Output) -> String {
    match output {
        radix_module::Output::Rust(o) => o.code.clone(),
        radix_module::Output::TypeScript(o) => o.code.clone(),
        radix_module::Output::Go(o) => o.code.clone(),
        radix_module::Output::Swift(o) => o.code.clone(),
        radix_module::Output::Faber(o) => o.code.clone(),
        _ => panic!("unexpected non-HIR target output variant"),
    }
}

// ── done_when §1: emit → load round-trip ─────────────────────────────────

/// Round-trip test: analyze hello-world.fab, serialize to FHIR, load, and
/// verify codegen output matches byte-for-byte across all HIR-backed targets.
///
/// Uses the programmatic API (not CLI subprocess) so the test runs fast
/// and is hermetic. CLI exercise is covered by §2 error-path tests.
#[test]
fn emit_fhir_round_trip() {
    const FIXTURE: &str = include_str!("fixtures/hir_artifact/hello-world.fab");

    let config = Config::default().with_dev_stdlib();
    let session = Session::new(config);
    let pack = latin_locale_pack();
    let surface = KeywordSurface::new(&pack);

    // Step 1: analyze the fixture source → original unit
    let original =
        analyze_source(&session, "hello-world.fab", FIXTURE).expect("analyze_source failed for hello-world.fab");

    // Step 2: generate codegen output for each HIR-backed target → original codes
    let hir_targets = [
        Target::HirRust,
        Target::HirTypeScript,
        Target::HirGo,
        Target::HirSwift,
        Target::HirFaber,
    ];
    let mut original_codes: Vec<(Target, String)> = Vec::new();
    for target in &hir_targets {
        let output = generate_from_analyzed(*target, &original, &surface)
            .unwrap_or_else(|e| panic!("generate_from_analyzed failed for hello-world.fab target {target:?}: {e}"));
        original_codes.push((*target, extract_code(&output)));
    }

    // Step 3: serialize → postcard bytes
    let artifact = radix_module::hir::serialize::from_analyzed(&original, "test_hash", "hello-world.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("postcard serialize should succeed");

    // Step 4: load_fhir → loaded unit
    let loaded = load_fhir(&bytes).expect("load_fhir failed for hello-world.fab round-trip");

    // Step 4b: schema-v2 presentation sidecar round-trips exactly (U-4 gate).
    assert_eq!(
        loaded.presentation, artifact.presentation,
        "presentation sidecar lost or mutated across FHIR emit→load"
    );

    // Step 5: generate codegen from loaded unit, compare byte-for-byte
    for (target, original_code) in &original_codes {
        let output = generate_from_analyzed(*target, &loaded, &surface).unwrap_or_else(|e| {
            panic!("generate_from_analyzed (loaded) failed for hello-world.fab target {target:?}: {e}")
        });
        let loaded_code = extract_code(&output);

        if original_code != &loaded_code {
            let mismatch_pos = original_code
                .chars()
                .zip(loaded_code.chars())
                .position(|(a, b)| a != b);
            if let Some(pos) = mismatch_pos {
                let start = pos.saturating_sub(40);
                let orig_end = (pos + 40).min(original_code.len());
                let load_end = (pos + 40).min(loaded_code.len());
                panic!(
                    "EMIT→LOAD ROUND-TRIP MISMATCH (U3.4): hello-world.fab, target={target:?}\n\
                     first diff at byte offset {pos}\n\
                     original[{start}..{orig_end}]: {:?}\n\
                     loaded[{start}..{load_end}]:    {:?}",
                    &original_code[start..orig_end],
                    &loaded_code[start..load_end],
                );
            }
            panic!(
                "EMIT→LOAD ROUND-TRIP MISMATCH (U3.4): hello-world.fab, target={target:?}\n\
                 original length: {}, loaded length: {}",
                original_code.len(),
                loaded_code.len(),
            );
        }
    }
}

// ── done_when §2: CLI error paths ────────────────────────────────────────

/// `radix emit -t fhir` without a file argument → non-zero exit, error message.
#[test]
fn emit_fhir_cli_no_file() {
    let bin = radix_binary();
    let output = Command::new(&bin)
        .args(["emit", "-t", "fhir"])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin}: {e}"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code();

    // Should exit non-zero and report that FHIR requires a file path.
    assert_ne!(code, Some(0), "expected non-zero exit code, got {code:?}");
    assert!(
        stderr.contains("requires a file path") || stderr.contains("FHIR"),
        "expected error about file path requirement, got: {stderr}"
    );
}

/// `radix emit -t fhir nonexistent.fab` → non-zero exit, file-not-found message.
#[test]
fn emit_fhir_cli_nonexistent_file() {
    let bin = radix_binary();
    let path = "/tmp/does-not-exist-radix-u34.fab";

    // Ensure the file does not exist.
    let _ = std::fs::remove_file(path);

    let output = Command::new(&bin)
        .args(["emit", "-t", "fhir", path])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin}: {e}"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code();

    assert_ne!(code, Some(0), "expected non-zero exit code, got {code:?}");
    assert!(
        stderr.contains("failed to read"),
        "expected 'failed to read' in stderr, got: {stderr}"
    );
}

/// `radix emit -t fhir /dev/null` → non-zero exit (extensionless path rejected).
///
/// The extension guard rejects any input that is not a `.fab` file, including
/// extensionless paths like `/dev/null`. The CLI must exit non-zero and report
/// that a `.fab` input file is required.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore = "/dev/null requires a Unix target")]
fn emit_fhir_cli_dev_null() {
    let bin = radix_binary();

    let output = Command::new(&bin)
        .args(["emit", "-t", "fhir", "/dev/null"])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin}: {e}"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code();

    assert_ne!(code, Some(0), "expected non-zero exit code, got {code:?}");
    assert!(stderr.contains(".fab input"), "expected '.fab input' in stderr, got: {stderr}");
}

/// `radix emit -t fhir` with stdin (`-`) → non-zero exit (stdin not supported).
#[test]
fn emit_fhir_cli_stdin() {
    let bin = radix_binary();

    let output = Command::new(&bin)
        .args(["emit", "-t", "fhir", "-"])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin}: {e}"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code();

    assert_ne!(code, Some(0), "expected non-zero exit code, got {code:?}");
    assert!(
        stderr.contains("stdin"),
        "expected error about stdin not supported, got: {stderr}"
    );
}
