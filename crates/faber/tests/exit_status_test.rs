//! The tool's own exit statuses (`faber_cli::exit_status`): 0 success, 64 bad
//! arguments, 65 the source has error diagnostics, 66 an input that cannot be
//! read, 69 a feature this build lacks, 70 an internal render or serialization
//! failure, 73 an output that cannot be written, 78 a bad manifest key or a
//! locale pack that fails to load, 1 only a failure with no known class or an
//! outcome status (format drift, a failing test case, `update --check`).
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
const NO_INPUT: i32 = 66;
#[allow(
    dead_code,
    reason = "used only by the tests that lack a default feature"
)]
const UNAVAILABLE: i32 = 69;
const CANT_CREATE: i32 = 73;

fn faber_with_stdin(args: &[&str], stdin: Stdio) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .stdin(stdin)
        .output()
        .expect("spawn faber");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

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
    let mut commands = vec![
        vec!["check", &bad],
        vec!["run", &bad],
        vec!["test", &bad],
        vec!["format", &bad],
        vec!["verify", &bad],
    ];
    // The rust-target legs need the private rust leaf: a build without it
    // (the public cut) refuses them with PKG001 before reading the source.
    #[cfg(feature = "hir-rust")]
    commands.extend([
        vec!["emit", "--target", "rust", &bad],
        vec![
            "build",
            "--out-dir",
            dir.path().to_str().expect("utf8"),
            &bad,
        ],
    ]);
    for args in &commands {
        let (code, stderr) = faber(args);
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
fn missing_input_file_or_path_exits_66() {
    let dir = tempfile::tempdir().expect("scratch");
    let missing = dir.path().join("missing.fab");
    let missing = missing.to_str().expect("utf8");
    let missing_package = dir.path().join("no-such-package");
    let missing_package = missing_package.to_str().expect("utf8");
    let mut commands = vec![
        vec!["check", missing],
        vec!["check", missing_package],
        vec!["lex", missing],
        vec!["parse", missing],
        vec!["hir", missing],
        vec!["run", missing],
        vec!["format", "--stdout", missing],
        vec!["convert", "--to", "la", "--stdout", missing],
        vec!["model", "inspect", missing],
    ];
    // The rust-target leg needs the private rust leaf: a build without it
    // (the public cut) refuses `emit -t rust` with PKG001 before the missing
    // input is ever read.
    #[cfg(feature = "hir-rust")]
    commands.push(vec!["emit", "-t", "rust", missing]);
    for args in &commands {
        let (code, stderr) = faber(args);
        assert_eq!(code, Some(NO_INPUT), "{args:?}:\n{stderr}");
    }
}

#[test]
fn unreadable_stdin_exits_66() {
    // A directory opened for reading fails every read with EISDIR.
    let dir = tempfile::tempdir().expect("scratch");
    for args in [
        vec!["check", "-"],
        vec!["format", "--stdin"],
        vec!["convert", "--to", "la", "--stdin"],
    ] {
        let stdin = fs::File::open(dir.path()).expect("open directory");
        let (code, stderr) = faber_with_stdin(&args, Stdio::from(stdin));
        assert_eq!(code, Some(NO_INPUT), "{args:?}:\n{stderr}");
    }
}

#[test]
fn a_directory_with_no_fab_files_exits_66_and_a_non_fab_path_exits_64() {
    let dir = tempfile::tempdir().expect("scratch");
    let empty = dir.path().join("empty");
    fs::create_dir_all(&empty).expect("dir");
    let (code, stderr) = faber(&["format", empty.to_str().expect("utf8")]);
    assert_eq!(code, Some(NO_INPUT), "{stderr}");
    let text = dir.path().join("notes.txt");
    fs::write(&text, "x").expect("file");
    let (code, stderr) = faber(&["format", text.to_str().expect("utf8")]);
    assert_eq!(code, Some(USAGE), "{stderr}");
}

#[test]
fn source_errors_stay_65_not_66() {
    let dir = tempfile::tempdir().expect("scratch");
    let bad = source(dir.path(), "bad.fab", BAD_SOURCE);
    let (code, stderr) = faber(&["check", &bad]);
    assert_eq!(code, Some(DATA_ERROR), "{stderr}");
}

#[test]
fn write_failures_exit_73() {
    let dir = tempfile::tempdir().expect("scratch");
    let good = source(dir.path(), "good.fab", GOOD);
    let blocker = source(dir.path(), "blocker", "not a directory");
    // A parent that is a file cannot become a directory.
    // The rust- and ts-target legs need those private leaves: a build without
    // them (the public cut) refuses with PKG001 before any output is written.
    #[cfg(feature = "hir-rust")]
    {
        let under_file = format!("{blocker}/out.rs");
        // A directory cannot be written as a file.
        let as_dir = dir.path().display().to_string();
        for output in [&under_file, &as_dir] {
            let (code, stderr) = faber(&["emit", "-t", "rust", "-o", output, &good]);
            assert_eq!(code, Some(CANT_CREATE), "emit -o {output}:\n{stderr}");
        }
    }
    #[cfg(feature = "hir-ts")]
    {
        let under_file = format!("{blocker}/out.rs");
        let (code, stderr) = faber(&["build", "-t", "ts", "-o", &under_file, &good]);
        assert_eq!(code, Some(CANT_CREATE), "build -o:\n{stderr}");
    }
    let (code, stderr) = faber(&["init", &format!("{blocker}/pkg")]);
    assert_eq!(code, Some(CANT_CREATE), "init:\n{stderr}");
}

// The probe succeeds through `emit -t rust`, so like the other rust-target
// probes it compiles out of builds without the rust leaf (the public cut).
#[cfg(feature = "hir-rust")]
#[test]
fn successful_output_file_is_unchanged() {
    let dir = tempfile::tempdir().expect("scratch");
    let good = source(dir.path(), "good.fab", GOOD);
    let out = dir.path().join("nested/out.rs");
    let (code, stderr) = faber(&[
        "emit",
        "-t",
        "rust",
        "-o",
        out.to_str().expect("utf8"),
        &good,
    ]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(out.exists());
}

#[cfg(feature = "runner")]
#[test]
fn run_statistics_write_failure_exits_73() {
    let dir = tempfile::tempdir().expect("scratch");
    let root = package(dir.path(), "stats-package", "", GOOD);
    let blocker = source(dir.path(), "blocker", "not a directory");
    let stats = format!("{blocker}/stats.json");
    let (code, stderr) = faber(&["run", "--stats-file", &stats, root.to_str().expect("utf8")]);
    assert_eq!(code, Some(CANT_CREATE), "{stderr}");
}

#[cfg(unix)]
#[cfg(feature = "hir-faber")]
#[test]
fn format_write_failure_exits_73_and_drift_exits_1() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("scratch");
    let drifting = source(dir.path(), "drift.fab", "main {\n      print 1\n}\n");
    let (code, stderr) = faber(&["format", "--check", &drifting]);
    assert_eq!(
        code,
        Some(1),
        "drift is a report, not a classed failure:\n{stderr}"
    );

    fs::set_permissions(&drifting, fs::Permissions::from_mode(0o444)).expect("read-only");
    // A superuser can write a read-only file; the test only means to force a
    // failed write, so skip when the write still succeeds.
    if fs::OpenOptions::new().write(true).open(&drifting).is_ok() {
        return;
    }
    let (code, stderr) = faber(&["format", "--write", &drifting]);
    assert_eq!(code, Some(CANT_CREATE), "{stderr}");
}

#[cfg(not(feature = "runner"))]
#[test]
fn runner_commands_without_the_runner_exit_69() {
    let dir = tempfile::tempdir().expect("scratch");
    let good = source(dir.path(), "good.fab", GOOD);
    for args in [vec!["run", &good], vec!["test", &good]] {
        let (code, stderr) = faber(&args);
        assert_eq!(code, Some(UNAVAILABLE), "{args:?}:\n{stderr}");
    }
}

#[cfg(not(feature = "hir-faber"))]
#[test]
fn format_without_hir_faber_exits_69() {
    let dir = tempfile::tempdir().expect("scratch");
    let good = source(dir.path(), "good.fab", GOOD);
    let (code, stderr) = faber(&["format", "--stdout", &good]);
    assert_eq!(code, Some(UNAVAILABLE), "{stderr}");
}

#[cfg(not(feature = "hir-ts"))]
#[test]
fn target_missing_from_the_build_exits_69_not_64() {
    let dir = tempfile::tempdir().expect("scratch");
    let good = source(dir.path(), "good.fab", GOOD);
    let (code, stderr) = faber(&["emit", "-t", "ts", &good]);
    assert_eq!(code, Some(UNAVAILABLE), "{stderr}");
    let (code, stderr) = faber(&["emit", "-t", "no-such-target", &good]);
    assert_eq!(code, Some(USAGE), "{stderr}");
}
