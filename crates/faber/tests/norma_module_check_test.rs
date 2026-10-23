//! `faber check` over every flat Norma standard-library module, one file at a
//! time, plus the executable `norma:sink` Collector exemplar.
//!
//! WHY: the embedded library is compiled as a whole and modules are resolved on
//! demand, so a defect that only appears when a module is checked by itself —
//! like `sorted_map.fab`'s generic empty-map default, whose `{} ∷ map<K, V>`
//! was rejected as a JSON literal — could ship without any gate noticing.
//! This test checks each flat `lib/norma/src/*.fab` module alone.
//!
//! A module that only passes `faber check` is not yet proven to execute, so the
//! Collector `sink.fab` declares is also run: it collects the items it is
//! given, reads them back with `to_list`, and `to_list` returns a copy that
//! changing cannot reach back into the collector.
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

/// The Collector probe: collect two items through the contract's `sink`, read
/// them back in order, then mutate the returned list and read again to show
/// `to_list` handed out a copy rather than the collector's own buffer.
const COLLECTOR_PROBE: &str = r#"+++
locale = "en"
+++

import from "norma:sink" sink

main {
    let sink.Collector collected ← sink.Collector { received = [] }
    collected.sink("one")
    collected.sink("two")
    print collected.to_list()
    mut list<string> copy_of ← collected.to_list()
    copy_of.append("three")
    print collected.to_list()
    print copy_of
}
"#;

/// The Collector probe's stdout, in the probe's own code locale.
const COLLECTOR_PROBE_STDOUT: &str = concat!(
    "[\"one\", \"two\"]\n",
    "[\"one\", \"two\"]\n",
    "[\"one\", \"two\", \"three\"]\n",
);

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

/// Run the `norma:sink` Collector exemplar end to end. The module-level check
/// above only parses and checks `sink.fab`; this proves the class it declares
/// collects, reads back and copies what it was given.
#[test]
fn norma_sink_collector_collects_reads_and_copies() {
    let dir = tempfile::tempdir().expect("create the Collector probe directory");
    let probe = dir.path().join("collector.fab");
    fs::write(&probe, COLLECTOR_PROBE).expect("write the Collector probe");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("run")
        .arg(&probe)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber run");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the norma:sink Collector probe should run:\n{stderr}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        COLLECTOR_PROBE_STDOUT,
        "unexpected Collector probe stdout"
    );
}
