//! `faber check --device D --partitions N`: "serialized by the backend"
//! advice (`INFO001`, info severity) on an acceptance package. The check is
//! static (the device image is built, nothing runs), so no GPU is needed.
//!
//! Identity assertions target the rendered label (`INFO001:<issue>`) and the
//! back-quoted kernel and entry names, never prose. The rows are read from the
//! compact form (`--no-diagnostics`), one line per advice row.
#![cfg(all(feature = "runner", feature = "device-runtime"))]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn acceptance(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../acceptance")
        .join(name)
}

fn faber_check(args: &[&str], package: &Path) -> (String, Option<i32>) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("check")
        .arg("--no-diagnostics")
        .arg(package)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber check");
    (
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

/// The advice lines of `stderr`: `info[INFO001:<issue>]` rows.
fn advice(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("info[INFO001:"))
        .collect()
}

#[test]
fn library_recipes_are_advised_as_not_splitting_across_metal_partitions() {
    let (stderr, code) = faber_check(
        &["--device", "metal", "--partitions", "2"],
        &acceptance("a1-rungs"),
    );
    assert_eq!(code, Some(0), "advice never fails the check:\n{stderr}");
    let rows = advice(&stderr);
    assert_eq!(rows.len(), 3, "{stderr}");
    for (kernel, entry) in [
        ("residual_add", "k0"),
        ("gemv", "k1"),
        ("softmax_rows", "k2"),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.starts_with("info[INFO001:not_split]")
                    && row.contains(&format!("`{kernel}`"))
                    && row.contains(&format!("`{entry}`"))),
            "{kernel} ({entry}) is advised as not splitting:\n{stderr}"
        );
    }
}

#[test]
fn without_device_the_check_prints_no_advice() {
    let (stderr, code) = faber_check(&[], &acceptance("a1-rungs"));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(advice(&stderr).is_empty(), "{stderr}");
}

#[test]
fn a_cuda_check_over_one_partition_prints_no_sync_audit() {
    let (stderr, code) = faber_check(&["--device", "cuda"], &acceptance("a2-flood-stencil"));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        !stderr.contains("INFO001:avoidable_sync"),
        "one partition syncs nothing:\n{stderr}"
    );
}

/// The planner audit (need, U3): on A2 the stencil kernels gather
/// their own writes whole for want of a read set, and the CFL maximum is a
/// reduction right after the kernel that wrote its source. Identity targets
/// the rendered label and the back-quoted names, never prose.
#[test]
fn a_cuda_check_audits_the_cross_partition_syncs_of_a2() {
    let (stderr, code) = faber_check(
        &["--device", "cuda", "--partitions", "2"],
        &acceptance("a2-flood-stencil"),
    );
    assert_eq!(code, Some(0), "advice never fails the check:\n{stderr}");
    let audit: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("info[INFO001:avoidable_sync]"))
        .collect();
    for kernel in ["stage1", "stage2"] {
        assert!(
            audit.iter().any(|row| row.contains(&format!("`{kernel}`"))
                && row.contains("own-write-gather")
                && row.contains("unbounded-read")
                && row.contains("no read set")),
            "{kernel} gathers its writes whole:\n{stderr}"
        );
    }
    assert!(
        audit.iter().any(|row| row.contains("`wave_speed`")
            && row.contains("partials-all-gather")
            && row.contains("fusable-with-previous")
            && row.contains("(stage2)")),
        "wave_speed fuses into stage2:\n{stderr}"
    );
    // The reduction is the only fusable row, and a replicated single lane
    // kernel (advance) syncs nothing.
    assert_eq!(
        audit
            .iter()
            .filter(|row| row.contains("fusable-with-previous"))
            .count(),
        1,
        "{stderr}"
    );
    assert!(
        !audit.iter().any(|row| row.contains("`advance`")),
        "{stderr}"
    );
}

#[test]
fn a_metal_check_prints_no_sync_audit() {
    // Metal partitions share one memory: no gather, nothing to audit.
    let (stderr, code) = faber_check(
        &["--device", "metal", "--partitions", "2"],
        &acceptance("a2-flood-stencil"),
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert!(!stderr.contains("INFO001:avoidable_sync"), "{stderr}");
}

/// The placement table (need, S4): weights are a row slab, and a
/// lane output a single-lane kernel reads is a replica forced by that kernel.
#[test]
fn a_cuda_check_prints_the_placement_table() {
    let (stderr, code) = faber_check(
        &["--device", "cuda", "--partitions", "2"],
        &acceptance("c4-slab-placement"),
    );
    assert_eq!(code, Some(0), "advice never fails the check:\n{stderr}");
    let table: Vec<&str> = stderr
        .lines()
        .skip_while(|line| !line.starts_with("placement"))
        .skip(1)
        .map(str::trim)
        .collect();
    assert!(table.contains(&"w: slab axis 0 align 8"), "{stderr}");
    assert!(
        table
            .iter()
            .any(|line| line.starts_with("t: replica, forced_by bump")),
        "{stderr}"
    );
}

#[test]
fn a_check_without_a_spanned_cuda_device_prints_no_placement_table() {
    for args in [
        &["--device", "cuda"][..],
        &["--device", "metal", "--partitions", "2"][..],
    ] {
        let (stderr, code) = faber_check(args, &acceptance("c4-slab-placement"));
        assert_eq!(code, Some(0), "{stderr}");
        assert!(!stderr.contains("placement ("), "{stderr}");
    }
}
