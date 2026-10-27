//! `faber check --device D --partitions N`: "serialized by the backend"
//! advice (`INFO001`, info severity) on an acceptance package. The check is
//! static (the device image is built, nothing runs), so no GPU is needed.
//!
//! Identity assertions target the rendered label (`INFO001:<issue>`) and the
//! back-quoted kernel and entry names, never prose.
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
fn partitions_need_a_device_that_can_span() {
    let (stderr, code) = faber_check(
        &["--device", "cuda", "--partitions", "2"],
        &acceptance("a1-rungs"),
    );
    assert_eq!(code, Some(64), "{stderr}");
    assert!(advice(&stderr).is_empty(), "{stderr}");
}
