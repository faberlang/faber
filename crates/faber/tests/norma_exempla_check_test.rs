//! `faber check` over the in-tree Norma exemplar packages.
//!
//! WHY: the packages under `lib/norma/exempla/` are the only
//! package-shaped consumers of several Norma modules (`norma:crypto`,
//! `norma:process`, `norma:http/*`). Nothing else compiled them, so a module
//! rename left three of them importing names that no longer exist
//! (`PKG001:unknown_library_module`) without any gate noticing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Packages the harness must find, so an empty or relocated exemplar tree
/// cannot pass by checking nothing.
const REQUIRED_PACKAGES: [&str; 3] = ["crypta-sha2", "http/client-io", "http/server-io"];

fn exempla_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/norma/exempla")
}

/// Directories holding a `faber.toml`, at most two levels below `root`
/// (`http/client-io` is nested one level).
fn exemplar_packages(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_packages(root, 2, &mut found);
    found.sort();
    found
}

fn collect_packages(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if dir.join("faber.toml").is_file() {
        found.push(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let entries = fs::read_dir(dir).expect("read exempla directory");
    for entry in entries {
        let path = entry.expect("read exempla entry").path();
        if path.is_dir() {
            collect_packages(&path, depth - 1, found);
        }
    }
}

fn check_package(package: &Path) -> (String, Option<i32>) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("check")
        .arg(package)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber check");
    (
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

#[test]
fn norma_exempla_packages_pass_faber_check() {
    let root = exempla_root();
    let packages = exemplar_packages(&root);
    for required in REQUIRED_PACKAGES {
        assert!(
            packages.iter().any(|p| p.ends_with(required)),
            "exemplar package `{required}` not found under {}",
            root.display()
        );
    }

    let mut failures = Vec::new();
    for package in &packages {
        let (stderr, code) = check_package(package);
        if code != Some(0) {
            failures.push(format!("{} (exit {code:?}):\n{stderr}", package.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "exemplar packages failing `faber check`:\n{}",
        failures.join("\n")
    );
}
