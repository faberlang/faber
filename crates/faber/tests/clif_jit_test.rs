//! `faber run --target clif-jit`: the program is compiled in memory and run in
//! this process, with the same stdout, stderr and exit status as the runner
//! and as a `--target clif` executable.
//!
//! Each case is compared against the runner (`faber run`); the runner and the
//! linked clif executable agree on these programs (see
//! `trap_text_parity_test.rs`), so agreement with the runner is agreement with
//! both without linking anything here. The refusal cases compare against
//! `faber build --target clif`, whose compile step is the same route.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn faber() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command.stdin(Stdio::null());
    command
}

struct Outcome {
    stdout: String,
    stderr: String,
    status: Option<i32>,
}

fn outcome(command: &mut Command) -> Outcome {
    let output = command.output().expect("spawn faber");
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code(),
    }
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(format!("{name}.fab"));
    fs::write(&path, text).expect("write source");
    path
}

/// `(name, source, expected exit status)`: hello, arithmetic, a list loop, a
/// class instance, a decimal, a `main exit` status, and the traps that end the
/// run mid-program. The status is pinned so a source that fails to compile on
/// both engines cannot pass by agreeing on emptiness.
fn cases() -> Vec<(&'static str, &'static str, i32)> {
    vec![
        ("hello", "main {\n    print \"hello\"\n}\n", 0),
        (
            "arithmetic",
            "main {\n    let int a ← 7\n    let int b ← 5\n    let int c ← (a + b) / 2\n    print a * b + 3\n    print c\n    print 1.5 + 2.25\n}\n",
            0,
        ),
        (
            "list_loop",
            "main {\n    mut int total ← 0\n    let list<int> xs ← [1, 2, 3, 4]\n    for from xs let x {\n        total ← total + x\n    }\n    print total\n    print xs\n}\n",
            0,
        ),
        (
            "class_instance",
            "class Point {\n    let int x = 0\n    let int y = 0\n}\n\nmain {\n    let Point p ← Point { x = 3, y = 4 }\n    print p.x + p.y\n    print p\n}\n",
            0,
        ),
        (
            "d64",
            "main {\n    let d64 a ← 1.25\n    let d64 b ← 2.5\n    print a + b\n    print a * b\n}\n",
            0,
        ),
        ("exit_status", "main exit 3 {\n    print \"before\"\n}\n", 3),
        (
            "trap_index",
            "main {\n    let list<int> xs ← [10, 20, 30]\n    mut int n ← 9\n    print \"before\"\n    print xs[n]\n}\n",
            70,
        ),
        (
            "trap_d64_range",
            "main {\n    let d64 top ← 92233720368.54775807\n    print top\n    let d64 over ← top + 0.00000001\n}\n",
            70,
        ),
    ]
}

#[test]
fn clif_jit_matches_the_runner_on_stdout_stderr_and_status() {
    let dir = tempfile::tempdir().expect("scratch");
    for (name, text, status) in cases() {
        let source = write(dir.path(), name, text);
        let runner = outcome(faber().arg("run").arg(&source));
        let jit = outcome(faber().args(["run", "--target", "clif-jit"]).arg(&source));
        assert_eq!(runner.status, Some(status), "{name}: runner status");
        assert!(
            !runner.stdout.is_empty(),
            "{name}: the runner printed nothing"
        );
        assert_eq!(jit.stdout, runner.stdout, "{name}: stdout");
        assert_eq!(jit.status, runner.status, "{name}: exit status");
        let first = |text: &str| {
            text.lines()
                .find(|line| line.starts_with("error:"))
                .map(str::to_owned)
        };
        assert_eq!(
            first(&jit.stderr),
            first(&runner.stderr),
            "{name}: error line"
        );
    }
}

/// A lone `.fab` file that imports a sibling module runs on `clif-jit` with
/// the runner's output. The file is package-shaped — a single-file object
/// carries no linked sibling code, so the imported call would decline as
/// CODEGEN001 `external definition call` — so both routes link the import
/// graph through the package machinery.
#[test]
fn clif_jit_runs_an_importing_lone_file_like_the_runner() {
    let dir = tempfile::tempdir().expect("scratch");
    let source = write(
        dir.path(),
        "main",
        "import from \"./auxilium\" auxilium\n\nmain {\n    print auxilium.saluta(\"Marcus\")\n}\n",
    );
    write(
        dir.path(),
        "auxilium",
        "@ public\nfn saluta(string nomen) → string {\n    return \"Salve, §!\"(nomen)\n}\n\nmain {}\n",
    );
    let runner = outcome(faber().arg("run").arg(&source));
    let jit = outcome(faber().args(["run", "--target", "clif-jit"]).arg(&source));
    assert_eq!(runner.stdout, "Salve, Marcus!\n");
    assert_eq!(jit.stdout, runner.stdout, "stdout");
    assert_eq!(jit.status, runner.status, "exit status");
}

#[test]
fn clif_jit_refuses_what_clif_refuses_with_the_same_diagnostics() {
    let declined = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/ad/sermo-channel.fab")
        .canonicalize()
        .expect("corpus exemplar");
    let dir = tempfile::tempdir().expect("scratch");
    let built = outcome(
        faber()
            .args(["build", "--target", "clif", "--out-dir"])
            .arg(dir.path())
            .arg(&declined),
    );
    let jit = outcome(faber().args(["run", "--target", "clif-jit"]).arg(&declined));
    assert_ne!(built.status, Some(0), "clif must decline the exemplar");
    assert!(
        built.stderr.contains("clif target declines"),
        "unexpected clif refusal:\n{}",
        built.stderr
    );
    assert_eq!(jit.stdout, built.stdout);
    assert_eq!(jit.stderr, built.stderr);
    assert_eq!(jit.status, built.status);
}

#[test]
fn clif_jit_builds_nothing_and_says_so_by_name() {
    let dir = tempfile::tempdir().expect("scratch");
    let source = write(dir.path(), "hello", "main {\n    print \"hello\"\n}\n");
    for subcommand in ["build", "emit"] {
        let result = outcome(
            faber()
                .args([subcommand, "--target", "clif-jit"])
                .arg(&source),
        );
        assert_eq!(result.status, Some(64), "{subcommand}: usage status");
        assert!(
            result
                .stderr
                .contains("clif-jit runs in memory; use --target clif to build an executable"),
            "{subcommand}: {}",
            result.stderr
        );
    }
}
