//! `faber check` over every flat Norma standard-library module, one file at a
//! time.
//!
//! WHY: the embedded library is compiled as a whole and modules are resolved on
//! demand, so a defect that only appears when a module is checked by itself —
//! like `sorted_map.fab`'s generic empty-map default, whose `{} ∷ map<K, V>`
//! was rejected as a JSON literal — could ship without any gate noticing.
//! This test checks each flat `lib/norma/src/*.fab` module alone.
//!
//! `KNOWN_FAILING` lists modules that already fail today with their first error
//! diagnostic code. A listed module that starts passing, or a new failure,
//! fails the test; the allowlist can only shrink or change with intent.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Modules that fail `faber check` when checked alone, as (file name, first
/// error diagnostic code). Empty today: every flat module passes.
const KNOWN_FAILING: &[(&str, &str)] = &[];

fn norma_src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/norma/src")
}

/// Flat `.fab` modules directly under `lib/norma/src`; the per-module
/// subdirectories (`fs/`, `http/`, `json/`, `net/`, `toml/`) are not visited.
fn flat_modules(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let entries = fs::read_dir(root).expect("read lib/norma/src");
    for entry in entries {
        let path = entry.expect("read lib/norma/src entry").path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("fab") {
            found.push(path);
        }
    }
    found.sort();
    found
}

fn check_module(module: &Path) -> (String, Option<i32>) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("check")
        .arg(module)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber check");
    (
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

/// The first `error[CODE:issue]` identity in the diagnostic stream, if any.
fn first_error_code(stderr: &str) -> Option<&str> {
    stderr.lines().find_map(|line| {
        let rest = line.strip_prefix("error[")?;
        rest.split_once(']').map(|(code, _)| code)
    })
}

#[test]
fn norma_modules_pass_faber_check_individually() {
    let root = norma_src_root();
    let modules = flat_modules(&root);
    assert!(
        !modules.is_empty(),
        "no flat .fab modules found under {}",
        root.display()
    );

    let mut failures = Vec::new();
    for module in &modules {
        let name = module
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let known = KNOWN_FAILING.iter().find(|(file, _)| *file == name);
        let (stderr, status) = check_module(module);
        match (status, known) {
            (Some(0), None) => {}
            (Some(0), Some((file, code))) => failures.push(format!(
                "{file}: listed in KNOWN_FAILING ({code}) but now passes `faber check`"
            )),
            (_, Some((file, code))) => {
                let actual = first_error_code(&stderr).unwrap_or("<no error code>");
                if actual != *code {
                    failures.push(format!(
                        "{file}: KNOWN_FAILING lists {code}, but the first error is {actual} (exit {status:?})"
                    ));
                }
            }
            (_, None) => {
                let actual = first_error_code(&stderr).unwrap_or("<no error code>");
                failures.push(format!(
                    "{name} (exit {status:?}, first error {actual}):\n{stderr}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Norma modules failing individual `faber check`:\n{}",
        failures.join("\n")
    );
}
