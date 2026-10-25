//! `faber check` over the NA1 and NA2 Norma integration acceptance packages.
//!
//! WHY: the acceptance packages prove the whole Norma stack end to end, but
//! nothing else in the ladder checks them as packages, so any change in their
//! check status could land unnoticed. Each package is checked exactly as a
//! user would run it, with only the product binary installed: no sibling
//! repos, no package fetch.
//!
//! `KNOWN_FAILING` lists packages that fail `faber check` today with their
//! first error diagnostic identity. A listed package that starts passing, or
//! whose first error identity changes, fails the test; the allowlist can only
//! shrink or change with intent.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Packages that fail `faber check` when checked as a whole, as (package
/// directory, first error diagnostic identity).
const KNOWN_FAILING: &[(&str, &str)] = &[
    ("na1-csv-https", "SEM011:failable_call_requires_handler"),
    ("na2-https-stack", "SEM011:failable_call_requires_handler"),
];

fn integration_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/norma/proof/integration")
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

/// The first `error[CODE:issue]` identity in the diagnostic stream, if any.
fn first_error_identity(stderr: &str) -> Option<&str> {
    stderr.lines().find_map(|line| {
        let rest = line.strip_prefix("error[")?;
        rest.split_once(']').map(|(identity, _)| identity)
    })
}

#[test]
fn norma_integration_packages_pass_faber_check() {
    let root = integration_root();
    let mut failures = Vec::new();
    for name in KNOWN_FAILING.iter().map(|(name, _)| *name) {
        let package = root.join(name);
        assert!(
            package.join("faber.toml").is_file(),
            "{} is not a package (no faber.toml)",
            package.display()
        );
        let known = KNOWN_FAILING.iter().find(|(dir, _)| *dir == name);
        let (stderr, status) = check_package(&package);
        match (status, known) {
            (Some(0), None) => {}
            (Some(0), Some((dir, identity))) => failures.push(format!(
                "{dir}: listed in KNOWN_FAILING ({identity}) but now passes `faber check`"
            )),
            (_, Some((dir, identity))) => {
                let actual = first_error_identity(&stderr).unwrap_or("<no error identity>");
                if actual != *identity {
                    failures.push(format!(
                        "{dir}: KNOWN_FAILING lists {identity}, but the first error is {actual} (exit {status:?})\n{stderr}"
                    ));
                }
            }
            (_, None) => {
                let actual = first_error_identity(&stderr).unwrap_or("<no error identity>");
                failures.push(format!(
                    "{name} (exit {status:?}, first error {actual}):\n{stderr}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Norma integration packages failing the `faber check` gate:\n{}",
        failures.join("\n")
    );
}
