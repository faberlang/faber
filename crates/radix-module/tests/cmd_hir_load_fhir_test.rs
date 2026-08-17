//! Integration tests for `radix hir --load <file.fhir>`.
//!
//! Validates that the `--load` flag is accepted by clap, routes through the
//! shared `load_fhir` loader (F6), and produces structured `HirArtifactError`
//! messages on failure paths (F7).

// Lane-gated: the `--load` FHIR loader exists only in hir-fhir builds; a
// core-only build fails closed with a target-unavailable message instead.
#![cfg(feature = "hir-fhir")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Cargo-managed temp root for scratch files: `<target-dir>/tmp/`.
///
/// Prefers `CARGO_TARGET_TMPDIR` when cargo sets it, otherwise derives
/// `<target-dir>/tmp` from the running test binary's path. Refuses to fall
/// back to the system temp dir (leaked `/tmp` test files have filled operator
/// disk before).
fn managed_temp_root() -> PathBuf {
    let root = if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        PathBuf::from(dir)
    } else if let Ok(exe) = std::env::current_exe() {
        // <target-dir>/<profile>/deps/<crate>-<hash> → <target-dir>/tmp
        let under_deps_dir = exe
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "deps" | "examples" | "benches"));
        if under_deps_dir {
            if let Some(target) = exe.parent().and_then(Path::parent).and_then(Path::parent) {
                target.join("tmp")
            } else {
                panic!("managed_temp_root: cannot walk up to the target dir from {}", exe.display());
            }
        } else {
            panic!(
                "managed_temp_root: test binary {} is not running from \
                 target/<profile>/deps|examples|benches",
                exe.display()
            );
        }
    } else {
        panic!("managed_temp_root: cannot resolve the test binary path");
    };
    std::fs::create_dir_all(&root).expect("create cargo-managed temp root");
    root
}

/// Path to the compiled `radix` binary, set by cargo's integration test
/// harness via `CARGO_BIN_EXE_radix`.
fn radix_binary() -> String {
    std::env::var("CARGO_BIN_EXE_radix")
        .unwrap_or_else(|_| panic!("CARGO_BIN_EXE_radix not set; run with `cargo test`"))
}

/// Helper: run `radix hir --load <path>` and return (stdout, stderr, exit_code).
fn run_hir_load(path: &str) -> (String, String, Option<i32>) {
    let bin = radix_binary();
    let output = Command::new(&bin)
        .args(["hir", "--load", path])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin}: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code();
    (stdout, stderr, code)
}

#[test]
fn cmd_hir_load_fhir_malformed_input() {
    // Write garbage bytes to a temp file (under the cargo-managed temp root)
    // and load as fhir.
    let path = managed_temp_root().join("hir-load-malformed");
    {
        let mut f = std::fs::File::create(&path).expect("create temp file");
        f.write_all(b"not a valid fhir artifact").expect("write");
    }

    let (_stdout, stderr, code) = run_hir_load(path.to_str().expect("valid utf-8 path"));

    // postcard may produce Malformed or Truncated depending on whether it
    // starts recognizing the payload — both are valid structured errors.
    assert!(
        stderr.starts_with("Error:"),
        "expected stderr to start with 'Error:', got: {stderr}"
    );
    assert!(
        stderr.contains("HIR artifact"),
        "expected 'HIR artifact' in stderr error message, got: {stderr}"
    );
    assert_eq!(code, Some(1), "expected exit code 1");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn cmd_hir_load_fhir_nonexistent_file() {
    let path = managed_temp_root().join("does-not-exist-42.fhir");
    let (_stdout, stderr, code) = run_hir_load(path.to_str().expect("valid utf-8 path"));

    assert!(
        stderr.contains("Error: cannot read"),
        "expected 'Error: cannot read' in stderr, got: {stderr}"
    );
    assert_eq!(code, Some(1), "expected exit code 1");
}

/// Schema-v1 artifact prefix → CLI reports the unsupported-version error
/// (schema-v2 clean break; no v1 compatibility decoder).
///
/// The loader reads the version prefix (first wire field) before interpreting
/// the payload, so a v1 version prefix alone is enough to exercise the CLI
/// error path.
#[test]
fn cmd_hir_load_fhir_schema_v1_rejected() {
    let path = managed_temp_root().join("schema-v1.fhir");
    {
        let mut f = std::fs::File::create(&path).expect("create temp file");
        // 0x01 = postcard varint u32 1 (schema_version 1), then payload junk.
        f.write_all(&[0x01, 0xAA, 0xBB, 0xCC])
            .expect("write v1 prefix");
    }

    let (_stdout, stderr, code) = run_hir_load(path.to_str().expect("valid utf-8 path"));

    assert!(
        stderr.contains("unsupported HIR artifact schema version 1"),
        "expected unsupported-version error for schema-v1 artifact, got: {stderr}"
    );
    assert_eq!(code, Some(1), "expected exit code 1");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn cmd_hir_load_fhir_binary_input() {
    // Write arbitrary binary data (not valid postcard) to a temp file under
    // the cargo-managed temp root.
    let path = managed_temp_root().join("hir-load-binary");
    {
        let mut f = std::fs::File::create(&path).expect("create temp file");
        f.write_all(&[0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD, 0xFC])
            .expect("write");
    }

    let (_stdout, stderr, code) = run_hir_load(path.to_str().expect("valid utf-8 path"));

    // postcard may produce Malformed or Truncated depending on whether it
    // starts recognizing the payload — both are valid structured errors.
    assert!(
        stderr.starts_with("Error:"),
        "expected stderr to start with 'Error:', got: {stderr}"
    );
    assert!(
        stderr.contains("HIR artifact"),
        "expected 'HIR artifact' in stderr error message, got: {stderr}"
    );
    assert_eq!(code, Some(1), "expected exit code 1");

    let _ = std::fs::remove_file(&path);
}
