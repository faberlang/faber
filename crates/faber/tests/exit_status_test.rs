//! The tool's own exit statuses (`faber_cli::exit_status`): 0 success, 64 bad
//! arguments, 65 the source has error diagnostics, 78 a bad manifest key or a
//! locale pack that fails to load, 1 only a failure with no known class.
//!
//! These are statuses of the tool, not of the program it runs; a program's trap
//! status is covered by the run tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GOOD: &str = "main {\n    print 1\n}\n";
const BAD_SOURCE: &str = "main {\n    print undefined_name\n}\n";
const WARNING_ONLY: &str = "main {\n    let int unused ← 1\n    print 2\n}\n";

const USAGE: i32 = 64;
const DATA_ERROR: i32 = 65;
const CONFIG: i32 = 78;

fn faber(args: &[&str]) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn source(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, text).expect("write source");
    path.display().to_string()
}

/// A bin package whose entry is `src/main.fab`.
fn package(dir: &Path, name: &str, manifest_extra: &str, entry: &str) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(&root).expect("package dir");
    fs::write(
        root.join("faber.toml"),
        format!(
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"1\"\n\n[paths]\nentry = \"main.fab\"\n\n{manifest_extra}"
        ),
    )
    .expect("write manifest");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(root.join("src/main.fab"), entry).expect("write entry");
    root
}

#[test]
fn success_and_warnings_alone_exit_zero() {
    let dir = tempfile::tempdir().expect("scratch");
    let good = source(dir.path(), "good.fab", GOOD);
    let warning = source(dir.path(), "warning.fab", WARNING_ONLY);
    let (code, stderr) = faber(&["check", &good]);
    assert_eq!(code, Some(0), "{stderr}");
    let (code, stderr) = faber(&["check", &warning]);
    assert_eq!(code, Some(0), "a warning alone is success:\n{stderr}");
}

#[test]
fn source_with_error_diagnostics_exits_65_on_every_source_command() {
    let dir = tempfile::tempdir().expect("scratch");
    let bad = source(dir.path(), "bad.fab", BAD_SOURCE);
    for args in [
        vec!["check", &bad],
        vec!["emit", "--target", "rust", &bad],
        vec![
            "build",
            "--out-dir",
            dir.path().to_str().expect("utf8"),
            &bad,
        ],
        vec!["run", &bad],
        vec!["test", &bad],
        vec!["format", &bad],
        vec!["verify", &bad],
    ] {
        let (code, stderr) = faber(&args);
        assert_eq!(code, Some(DATA_ERROR), "{args:?}:\n{stderr}");
    }
}

#[test]
fn package_source_errors_exit_65() {
    let dir = tempfile::tempdir().expect("scratch");
    let root = package(dir.path(), "bad-package", "", BAD_SOURCE);
    let root = root.to_str().expect("utf8");
    for command in ["check", "build", "run", "test"] {
        let (code, stderr) = faber(&[command, root]);
        assert_eq!(code, Some(DATA_ERROR), "{command}:\n{stderr}");
    }
}

#[test]
fn promoted_warnings_exit_65_and_plain_warnings_exit_0() {
    let dir = tempfile::tempdir().expect("scratch");
    let warning = source(dir.path(), "warning.fab", WARNING_ONLY);
    let (code, stderr) = faber(&["check", "--deny-warnings", &warning]);
    assert_eq!(code, Some(DATA_ERROR), "{stderr}");
    let (code, stderr) = faber(&["check", &warning]);
    assert_eq!(code, Some(0), "{stderr}");

    let root = package(dir.path(), "warning-package", "", WARNING_ONLY);
    let root = root.to_str().expect("utf8");
    let (code, stderr) = faber(&["check", "--deny-warnings", root]);
    assert_eq!(code, Some(DATA_ERROR), "{stderr}");
    let (code, stderr) = faber(&["check", root]);
    assert_eq!(code, Some(0), "{stderr}");
}

#[test]
fn bad_arguments_exit_64() {
    let dir = tempfile::tempdir().expect("scratch");
    let good = source(dir.path(), "good.fab", GOOD);
    for args in [
        vec!["check", "--no-such-flag", &good],
        vec!["check", "--deny", "NOSUCH999", &good],
        vec!["emit", "--target", "no-such-target", &good],
        vec!["build", "--target", "no-such-target", &good],
        vec!["no-such-command"],
        vec![],
        vec!["explain"],
        vec!["format", "--write", "--check", &good],
    ] {
        let (code, stderr) = faber(&args);
        assert_eq!(code, Some(USAGE), "{args:?}:\n{stderr}");
    }
}

#[test]
fn help_and_version_still_exit_zero() {
    for args in [["--help"], ["--version"]] {
        let (code, stderr) = faber(&args);
        assert_eq!(code, Some(0), "{args:?}:\n{stderr}");
    }
}

#[test]
fn bad_manifest_key_exits_78() {
    let dir = tempfile::tempdir().expect("scratch");
    let root = package(
        dir.path(),
        "bad-manifest",
        "[build]\nkind = \"no-such-kind\"\n",
        GOOD,
    );
    let root = root.to_str().expect("utf8");
    for command in ["check", "build", "run", "test"] {
        let (code, stderr) = faber(&[command, root]);
        assert_eq!(code, Some(CONFIG), "{command}:\n{stderr}");
    }
}

#[test]
fn locale_pack_that_fails_to_load_exits_78() {
    let dir = tempfile::tempdir().expect("scratch");
    let root = package(
        dir.path(),
        "missing-pack",
        "[locale]\nlocale = \"en\"\npack = \"no-such-pack.toml\"\n",
        GOOD,
    );
    let root = root.to_str().expect("utf8");
    for command in ["check", "run"] {
        let (code, stderr) = faber(&[command, root]);
        assert_eq!(code, Some(CONFIG), "{command}:\n{stderr}");
    }

    let good = source(dir.path(), "good.fab", GOOD);
    let (code, stderr) = faber(&["check", "--locale", "no-such-locale", &good]);
    assert_eq!(code, Some(CONFIG), "{stderr}");
}

#[test]
fn failure_with_no_known_class_exits_1() {
    let dir = tempfile::tempdir().expect("scratch");
    let missing = dir.path().join("missing.fab");
    let (code, stderr) = faber(&["check", missing.to_str().expect("utf8")]);
    assert_eq!(code, Some(1), "{stderr}");
}
