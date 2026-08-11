//! Subprocess checks for the FHIR package surface (`faber build --target
//! fhir`, portable default, capability rows, clean-room run).

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

fn temp_dir(label: &str) -> TempDir {
    TempDir::new("faber-fhir-integration", label)
}

fn run_faber(args: &[&str]) -> (String, String, bool) {
    let (stdout, stderr, status) = run_faber_status(args);
    (stdout, stderr, status.success())
}

fn run_faber_status(args: &[&str]) -> (String, String, std::process::ExitStatus) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn faber");

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read stdout");

    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let status = child.wait().expect("wait");
    (stdout, stderr, status)
}

fn run_faber_clean_room(args: &[&str]) -> (String, String, bool) {
    // The child inherits this process's environment; the clean-room gate must
    // not see a dev library home or Rust toolchain on PATH.
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("FABER_LIBRARY_HOME")
        .env("PATH", "/usr/bin:/bin");
    let mut child = command.spawn().expect("spawn faber in clean room");

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read stdout");

    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let status = child.wait().expect("wait");
    (stdout, stderr, status.success())
}

#[test]
fn targets_reports_fhir_package_capabilities() {
    let (stdout, stderr, ok) = run_faber(&["targets"]);
    assert!(ok, "faber targets failed:\n{stderr}");

    let row = stdout
        .lines()
        .find(|line| line.starts_with("fhir "))
        .unwrap_or_else(|| panic!("missing fhir row:\n{stdout}"));
    assert!(
        row.contains("check=yes build=yes run=yes package=yes"),
        "FHIR package row must show package build/load/run truth:\n{row}"
    );
    assert!(
        row.contains("faber build --target fhir"),
        "FHIR row must point at the Faber package build surface:\n{row}"
    );
}

#[test]
fn build_target_fhir_writes_package_artifact() {
    let package = temp_dir("fhir-build");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "fhir-build-test"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(src.join("main.fab"), "incipit { nota \"salve\" }\n").expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "build",
        "--target",
        "fhir",
        package.to_str().expect("utf8 package path"),
    ]);
    assert!(ok, "fhir build failed:\n{stderr}");

    let artifact = PathBuf::from(stdout.trim());
    assert_eq!(
        artifact,
        package.join("target/faber-fhir/package.fhirpkg"),
        "fhir build must print the package artifact path:\n{stdout}"
    );
    assert!(artifact.is_file(), "package artifact must exist");
    // The portable route must not emit a generated Rust crate.
    assert!(
        !package.join("target/faber/Cargo.toml").exists(),
        "fhir build must not emit generated Rust"
    );
}

#[test]
fn run_target_fhir_loads_and_runs_in_process() {
    let package = temp_dir("fhir-run");
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "fhir-run-test"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(src.join("main.fab"), "incipit { nota \"salve\" }\n").expect("write entry");

    let (stdout, stderr, ok) = run_faber(&[
        "run",
        "--target",
        "fhir",
        package.to_str().expect("utf8 package path"),
    ]);
    assert!(ok, "fhir run failed:\n{stderr}");
    assert_eq!(
        stdout, "salve\n",
        "FMIR run from loaded FHIR must print the program output"
    );
}

#[test]
#[ignore = "clean-room gate: scrubs PATH and spawns faber; run in periodic end-to-end coverage"]
fn portable_default_init_check_build_run_clean_room() {
    let package = temp_dir("fhir-clean-room");
    let package_str = package.to_str().expect("utf8 package path");

    // `faber init` creates a fresh package with the implicit portable default
    // (no explicit [build] target).
    let (_stdout, stderr, ok) = run_faber_clean_room(&["init", package_str]);
    assert!(ok, "clean-room init failed:\n{stderr}");
    let manifest = fs::read_to_string(package.join("faber.toml")).expect("read init manifest");
    assert!(
        !manifest.contains("target ="),
        "init manifest must leave the build target implicit (portable default):\n{manifest}"
    );

    // check → build → run with no cargo, no rustc, no sibling checkout.
    let (_stdout, stderr, ok) = run_faber_clean_room(&["check", package_str]);
    assert!(ok, "clean-room check failed:\n{stderr}");

    let (build_stdout, stderr, ok) = run_faber_clean_room(&["build", package_str]);
    assert!(ok, "clean-room build failed:\n{stderr}");
    let artifact = PathBuf::from(build_stdout.trim());
    assert!(
        artifact.is_file(),
        "clean-room build must produce the FHIR package artifact:\n{build_stdout}"
    );

    let (run_stdout, stderr, ok) = run_faber_clean_room(&["run", package_str]);
    assert!(ok, "clean-room run failed:\n{stderr}");
    assert_eq!(
        run_stdout, "Salve, munde!\n",
        "clean-room run must execute the fresh package through FMIR from loaded FHIR"
    );
}
