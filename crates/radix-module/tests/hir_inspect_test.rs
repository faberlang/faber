//! Integration tests for `radix hir` inspection presentation output and the
//! trivia-preservation fixture end-to-end path (parse → lower → FHIR → load
//! → inspect).
//!
//! U-6 (inspect slice): the inspection command exposes the HIR presentation
//! sidecar when a lowering result (U-3) or a loaded FHIR artifact (U-4)
//! supplies one. The presentation key is always present in the inspect
//! payload; its value is the sidecar object once the interlock lands and
//! `null` before it.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The trivia fixture: item, member, entry, statement, block-end, and EOF
/// comments at legal attachment positions.
const TRIVIA_FIXTURE: &str = include_str!("fixtures/trivia_preservation.fab");

/// A comment-free program: exercises the empty/absent presentation path.
#[cfg(feature = "hir-fhir")]
const PLAIN_SOURCE: &str = r"
incipit {
    nota 1
}
";

/// Cargo-managed temp root for scratch files: `<target-dir>/tmp/`.
///
/// Prefers `CARGO_TARGET_TMPDIR` when cargo sets it, otherwise derives
/// `<target-dir>/tmp` from the running test binary's path. Refuses to fall
/// back to the system temp dir.
fn managed_temp_root() -> PathBuf {
    let root = if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        PathBuf::from(dir)
    } else if let Ok(exe) = std::env::current_exe() {
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

/// Run `radix hir --load <path>` and return (stdout, stderr, exit_code).
#[cfg(feature = "hir-fhir")]
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

/// Run `radix hir <path>` (source path) and return (stdout, stderr, exit_code).
fn run_hir_source(path: &str) -> (String, String, Option<i32>) {
    let bin = radix_binary();
    let output = Command::new(&bin)
        .args(["hir", path])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin}: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code();
    (stdout, stderr, code)
}

#[test]
fn hir_inspect_source_path_includes_presentation_field() {
    let path = managed_temp_root().join("hir-inspect-trivia-source.fab");
    std::fs::write(&path, TRIVIA_FIXTURE).expect("write fixture");

    let (stdout, stderr, code) = run_hir_source(path.to_str().expect("valid utf-8 path"));
    assert_eq!(code, Some(0), "radix hir on source failed: {stderr}");

    // The presentation key is always part of the inspect payload; its value is
    // the sidecar object once U-3 lowering supplies presentation data, and
    // `null` while the data is absent.
    assert!(
        stdout.contains("\"presentation\""),
        "expected a presentation field in inspect output:\n{stdout}"
    );

    let _ = std::fs::remove_file(&path);
}

#[cfg(feature = "hir-fhir")]
#[test]
fn hir_inspect_load_plain_source_includes_presentation_field() {
    // A comment-free program round-trips through FHIR with the presentation
    // key present in the inspect payload. Before U-3/U-4 populate the sidecar
    // the value is `null`; after the interlock it is an (empty) sidecar
    // object. The key's shape stays stable either way.
    let root = managed_temp_root();
    let source_path = root.join("hir-inspect-plain.fab");
    let fhir_path = root.join("hir-inspect-plain.fhir");
    std::fs::write(&source_path, PLAIN_SOURCE).expect("write plain source");

    let emit = Command::new(radix_binary())
        .args([
            "emit",
            "-t",
            "fhir",
            source_path.to_str().expect("utf-8"),
            "-o",
            fhir_path.to_str().expect("utf-8"),
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute radix emit -t fhir: {e}"));
    assert!(
        emit.status.success(),
        "emit -t fhir failed: {}",
        String::from_utf8_lossy(&emit.stderr)
    );

    let (stdout, stderr, code) = run_hir_load(fhir_path.to_str().expect("utf-8"));
    assert_eq!(code, Some(0), "radix hir --load failed: {stderr}");
    assert!(
        stdout.contains("\"presentation\""),
        "expected a presentation field in loaded inspect output:\n{stdout}"
    );

    let _ = std::fs::remove_file(&source_path);
    let _ = std::fs::remove_file(&fhir_path);
}

/// End-to-end fixture gate: parse → lower → FHIR → load → inspect preserves
/// item, member, entry, statement, block-end, and program-end comments.
///
/// This test is the U-6/U-3/U-4 interlock gate: it passes once lowering
/// populates the presentation sidecar (U-3) and the FHIR loader reconstructs
/// it (U-4), and it fails with a clear message before that.
#[cfg(feature = "hir-fhir")]
#[test]
fn trivia_fixture_preserves_comments_through_fhir_load() {
    let root = managed_temp_root();
    let source_path = root.join("trivia_preservation.fab");
    let fhir_path = root.join("trivia_preservation.fhir");
    std::fs::write(&source_path, TRIVIA_FIXTURE).expect("write fixture");

    // parse → lower → FHIR
    let emit = Command::new(radix_binary())
        .args([
            "emit",
            "-t",
            "fhir",
            source_path.to_str().expect("utf-8"),
            "-o",
            fhir_path.to_str().expect("utf-8"),
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to execute radix emit -t fhir: {e}"));
    assert!(
        emit.status.success(),
        "emit -t fhir failed: {}",
        String::from_utf8_lossy(&emit.stderr)
    );
    assert!(fhir_path.exists(), "expected a .fhir artifact at {}", fhir_path.display());

    // load → inspect
    let (stdout, stderr, code) = run_hir_load(fhir_path.to_str().expect("utf-8"));
    assert_eq!(code, Some(0), "radix hir --load failed: {stderr}");

    // The presentation sidecar must be populated (not null) — this is the
    // U-3/U-4 interlock: lowering populates the sidecar and the FHIR loader
    // reconstructs it. Before the interlock the inspect payload reports null.
    assert!(
        stdout.contains("\"presentation\": {"),
        "presentation sidecar not populated in inspect output; \
         expects U-3 (lowering) + U-4 (FHIR load) interlock:\n{stdout}"
    );

    // All five owner kinds from done-when 1 surface in the inspect payload.
    for kind in [
        "\"kind\": \"Item\"",
        "\"kind\": \"Member\"",
        "\"kind\": \"Entry\"",
        "\"kind\": \"Statement\"",
        "\"kind\": \"BlockEnd\"",
        "\"kind\": \"ProgramEnd\"",
    ] {
        assert!(stdout.contains(kind), "missing owner kind {kind} in inspect output:\n{stdout}");
    }

    // Raw comment payloads survive the round trip verbatim.
    for text in [
        "item: comment before a genus declaration",
        "member: comment immediately before a field",
        "entry: comment before the entry block",
        "statement: comment before a statement",
        "block-end: comment immediately before the closing brace",
        "EOF: comment before the end of the file",
    ] {
        assert!(
            stdout.contains(text),
            "missing preserved comment {text:?} in inspect output:\n{stdout}"
        );
    }

    // Structural ends and the entry anchor are exposed.
    assert!(stdout.contains("\"block_ends\""), "missing block_ends in output:\n{stdout}");
    assert!(stdout.contains("\"program_end\""), "missing program_end in output:\n{stdout}");
    assert!(
        stdout.contains("\"entry_start_anchor\""),
        "missing entry_start_anchor in output:\n{stdout}"
    );
    assert!(
        !stdout.contains("\"entry_start_anchor\": null"),
        "entry_start_anchor should be set for the incipit comment:\n{stdout}"
    );

    let _ = std::fs::remove_file(&source_path);
    let _ = std::fs::remove_file(&fhir_path);
}
