//! End-to-end proofs for `faber check --air` (ALA-1): three verdicts, unroll
//! note, advisory-only exit parity, annotated diagnostics byte-identity, and
//! unchanged default `check` output.

use std::process::Command;

// File mode (`cmd_check_file`, single-file pipeline) uses the canonical Latin
// surface by default, so the fixtures use `functio`/`fixum`/`varia`/`itera ab`.
const MIXED_SOURCE: &str = r#"functio computus_gyrum() → fractus {
    fixum fractus total ← 0.0
    itera ab 0‥10 per 2 fixum i {
        total ← total + 1.0
    }
    redde total
}

functio gradus_passus(numerus x) → numerus {
    varia numerus total ← x
    total ← total + 1
    redde total
}

functio salve(textus nomen) → textus {
    redde nomen
}
"#;

const ANNOTATED_IMPURE: &str = r#"@ radix lane "air"
functio gradus_passus(numerus x) → numerus {
    varia numerus total ← x
    total ← total + 1
    redde total
}
"#;

fn faber_binary() -> std::path::PathBuf {
    super::format_test::faber_binary()
}

fn write_fixture(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create fixture dir");
    let path = dir.path().join("fixture.fab");
    std::fs::write(&path, content).expect("write fixture");
    (dir, path)
}

fn run_faber(args: &[&str]) -> std::process::Output {
    Command::new(faber_binary())
        .args(args)
        .output()
        .expect("run faber")
}

/// Run both invocations through stdin so plain check and `--air` take the
/// identical single-file pipeline (plain `check <file.fab>` routes through the
/// package loader with a different reader surface).
fn run_faber_stdin(source: &str, air: bool) -> std::process::Output {
    let mut args = vec!["check"];
    if air {
        args.push("--air");
    }
    args.push("-");
    let mut command = Command::new(faber_binary());
    command
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().expect("spawn faber");
    {
        let mut stdin = child.stdin.take().expect("stdin pipe");
        use std::io::Write;
        stdin.write_all(source.as_bytes()).expect("write stdin source");
    }
    child.wait_with_output().expect("run faber")
}

/// The rendered report row for `name` (single line), if present.
fn report_row<'a>(stdout: &'a str, name: &str) -> Option<&'a str> {
    stdout.lines().find(|line| line.starts_with(name))
}

/// (a) unannotated pure numeric fn → ADMISSIBLE with unroll note within
/// budget 64; (b) unannotated impure fn → WOULD-REJECT with own-body rule
/// ids and line spans; (c) no numeric/tensor signature → NOT-A-TARGET.
#[test]
fn air_report_renders_three_verdicts() {
    let (_dir, path) = write_fixture(MIXED_SOURCE);
    let output = run_faber(&["check", "--air", path.to_str().expect("utf8 path")]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let row = report_row(&stdout, "computus_gyrum")
        .unwrap_or_else(|| panic!("pure numeric fn must have a report row:\n{stdout}"));
    assert!(
        row.contains("ADMISSIBLE"),
        "pure numeric fn must be ADMISSIBLE:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "itera ab unrolls (budget {})",
            radix::module::semantic::passes::air_unroll::UNROLL_BUDGET
        )),
        "admitted itera ab loop must carry the unroll note:\n{stdout}"
    );
    let row = report_row(&stdout, "gradus_passus")
        .unwrap_or_else(|| panic!("impure fn must have a report row:\n{stdout}"));
    assert!(
        row.contains("WOULD-REJECT"),
        "impure fn must be WOULD-REJECT:\n{stdout}"
    );
    assert!(
        stdout.contains("mutable_varia (l."),
        "would-reject rows must list own-body rule ids with line spans:\n{stdout}"
    );
    assert!(
        stdout.contains("assignment_mutation (l."),
        "assignment rejection must be listed with a line span:\n{stdout}"
    );
    let row = report_row(&stdout, "salve")
        .unwrap_or_else(|| panic!("non-numeric fn must have a report row:\n{stdout}"));
    assert!(
        row.contains("NOT-A-TARGET"),
        "non-numeric fn must be NOT-A-TARGET:\n{stdout}"
    );
    // Advisory rows never change the exit status: parity with plain check on
    // the same source (the fixum-rebind loop shape is rejected by plain check
    // pre-unroll, so both sides exit 1 here).
    let plain = run_faber_stdin(MIXED_SOURCE, false);
    assert_eq!(
        plain.status.code(),
        output.status.code(),
        "--air exit code must equal plain check"
    );
}

/// Annotated functions appear with their real-gate verdict and the annotated
/// marker.
#[test]
fn air_report_marks_annotated_functions() {
    let output = run_faber_stdin(ANNOTATED_IMPURE, true);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let row = report_row(&stdout, "gradus_passus")
        .unwrap_or_else(|| panic!("annotated fn must have a report row:\n{stdout}"));
    assert!(
        row.contains("WOULD-REJECT") && row.contains("annotated"),
        "annotated fn must render with the annotated marker:\n{stdout}"
    );
}

/// (d) annotated fn plain-check diagnostics are byte-identical with and
/// without `--air` (the report goes to stdout, never stderr).
#[test]
fn annotated_diagnostics_byte_identical_with_and_without_air() {
    let plain = run_faber_stdin(ANNOTATED_IMPURE, false);
    let air = run_faber_stdin(ANNOTATED_IMPURE, true);

    assert!(
        !plain.stderr.is_empty(),
        "fixture must produce hard AIR-gate diagnostics under plain check:\n{}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert_eq!(
        plain.stderr, air.stderr,
        "diagnostics must be byte-identical with --air"
    );
    assert_eq!(plain.status.code(), air.status.code());
}

/// (e) `--air` exit code equals plain check on the same source.
#[test]
fn air_exit_parity_with_plain_check() {
    for (source, label) in [(MIXED_SOURCE, "clean"), (ANNOTATED_IMPURE, "gate-failing")] {
        let plain = run_faber_stdin(source, false);
        let air = run_faber_stdin(source, true);
        assert_eq!(
            plain.status.code(),
            air.status.code(),
            "exit codes must match between plain check and --air ({label})"
        );
    }
}

/// (f) default `check` output is unchanged when the flag is absent.
#[test]
fn default_check_output_unchanged_without_flag() {
    let plain = run_faber_stdin(MIXED_SOURCE, false);
    let stdout = String::from_utf8_lossy(&plain.stdout);
    let stderr = String::from_utf8_lossy(&plain.stderr);

    assert!(
        !stdout.contains("ADMISSIBLE")
            && !stderr.contains("ADMISSIBLE")
            && !stdout.contains(super::check_air::AIR_REPORT_HEADER)
            && !stderr.contains(super::check_air::AIR_REPORT_HEADER),
        "plain check must not print advisory rows:\n{stdout}\n{stderr}"
    );
}

/// Package-shaped input (directory or manifest) with `--air` prints an
/// explicit unsupported message.
#[test]
fn package_shaped_input_with_air_is_unsupported() {
    let root = tempfile::tempdir().expect("create package fixture root");
    std::fs::write(
        root.path().join("faber.toml"),
        r#"
[package]
name = "air-pkg"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    std::fs::create_dir_all(root.path().join("src")).expect("create src");
    std::fs::write(root.path().join("src/main.fab"), "main {}\n").expect("write source");

    // Explicit `--package` forces package shape regardless of file input.
    let output = run_faber(&[
        "check",
        "--air",
        "--package",
        root.path().to_str().expect("utf8 path"),
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("`faber check --air` package mode is not supported"),
        "package input with --air must print the unsupported message:\n{stderr}"
    );
    assert_eq!(Some(1), output.status.code());
}
