//! Ratchet at budget 0: no `FABER_*` / `RADIX_*` environment variable is read
//! outside the internal test harness.
//!
//! Operator ruling 2026-10-14: Faber has no environment variables internally.
//! Anything that changes the behavior of the compiler, the product CLI or a
//! program is a command-line flag or a `faber.toml` setting; the only
//! exception is the internal test harness. Variables that only locate things
//! (`HOME`, `PATH`, `XDG_CACHE_HOME`, `CUDA_HOME`, …) are not `FABER_*` /
//! `RADIX_*` names and are not counted.
//!
//! Scanned set — every non-test `src` tree of every workspace member, found
//! through `cargo metadata`, not a hand-maintained crate list. Harness paths
//! (`tests`, `test_support`, `*_test.rs`, `golden.rs`, `crates/corpus-matrix`)
//! are exempt through [`radix_hygiene::is_env_harness_path`]. The budget is 0:
//! the list of reads only ever shrinks, and a new read is a failure, not a
//! number to raise.

use radix_hygiene::{ScanConfig, collect_production_files, scan_tool_env_reads};
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("radix-hygiene lives under crates/")
        .to_path_buf()
}

/// The `src` directory of every workspace member.
fn member_source_roots(workspace: &Path) -> Vec<PathBuf> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace)
        .output()
        .expect("run cargo metadata to discover workspace members");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse cargo metadata JSON");
    let members: std::collections::HashSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("cargo metadata workspace_members array")
        .iter()
        .map(|id| id.as_str().expect("workspace member package ID"))
        .collect();
    metadata["packages"]
        .as_array()
        .expect("cargo metadata packages array")
        .iter()
        .filter(|package| members.contains(package["id"].as_str().expect("package ID")))
        .map(|package| {
            Path::new(package["manifest_path"].as_str().expect("manifest path"))
                .parent()
                .expect("package manifest directory")
                .join("src")
        })
        .collect()
}

#[test]
fn no_faber_or_radix_environment_reads_outside_the_test_harness() {
    let workspace = workspace();
    let source_roots = member_source_roots(&workspace);
    assert!(
        !source_roots.is_empty(),
        "no workspace members found; the scan would pass vacuously"
    );
    let config = ScanConfig {
        source_roots,
        exclude_path_suffixes: Vec::new(),
        subtract_self_expect: true,
        check_companion_convention: false,
    };
    let files = collect_production_files(&config);
    assert!(
        !files.is_empty(),
        "no production sources collected; the scan would pass vacuously"
    );
    let reads = scan_tool_env_reads(&files);

    assert!(
        reads.is_empty(),
        "FABER_*/RADIX_* environment reads are banned outside the test harness \
         (use a command-line flag or a faber.toml key); found {}:\n{}",
        reads.len(),
        reads
            .iter()
            .map(|read| format!(
                "  {}:{} {}",
                read.path
                    .strip_prefix(&workspace)
                    .unwrap_or(&read.path)
                    .display(),
                read.line,
                read.name
            ))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
