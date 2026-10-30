//! `faber run --target runner`: the named interpreted route. A plain
//! `faber run` defaults to the clif-jit lane now (falling back to the runner
//! on a compile-time decline — see `run_default_engine_test.rs`), so this
//! file pins that the explicit spelling still interprets, and that
//! `faber build`/`emit` refuse the spelling by name, as they do for
//! `clif-jit`.

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

/// `(name, source, expected exit status)`: stdout, an `main exit` status, and
/// a trap that ends the run mid-program. The status is pinned so a source
/// that fails to compile on both routes cannot pass by agreeing on emptiness.
fn cases() -> Vec<(&'static str, &'static str, i32)> {
    vec![
        (
            "stdout",
            "main {\n    print \"hello\"\n    print 1 + 2\n}\n",
            0,
        ),
        ("exit_status", "main exit 3 {\n    print \"before\"\n}\n", 3),
        (
            "trap_index",
            "main {\n    let list<int> xs ← [10, 20, 30]\n    mut int n ← 9\n    print \"before\"\n    print xs[n]\n}\n",
            70,
        ),
    ]
}

/// The explicit runner target interprets: these programs run with the
/// interpreter's own stream shape — no compile-lane diagnostics ride along
/// (the clif lanes print analysis warnings the interpreter does not), and the
/// trap ends the run with the shared trap line.
#[test]
fn runner_target_interprets_the_program() {
    let dir = tempfile::tempdir().expect("scratch");
    for (name, text, status) in cases() {
        let source = write(dir.path(), name, text);
        let runner = outcome(faber().args(["run", "--target", "runner"]).arg(&source));
        assert_eq!(runner.status, Some(status), "{name}: runner status");
        assert!(
            !runner.stdout.is_empty() || status != 0,
            "{name}: the runner printed nothing"
        );
        assert!(
            !runner.stderr.contains("warning["),
            "{name}: the interpreter prints no compile-lane warnings: {}",
            runner.stderr
        );
        if name == "trap_index" {
            assert_eq!(
                runner.stderr.trim_end(),
                "error: array index out of bounds",
                "{name}: trap line"
            );
        }
    }
}

/// `--target runner` is the interpreter by name, not an alias of the default
/// engine: an AIR exemplar the clif lanes accept is refused by the runner's
/// MIR-backed lane restriction, exactly as before the clif-jit default.
#[test]
fn runner_target_still_refuses_an_air_exemplar_the_jit_runs() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/genus/air-struct-literal.fab")
        .canonicalize()
        .expect("corpus exemplar");
    let runner = outcome(faber().args(["run", "--target", "runner"]).arg(&source));
    assert_eq!(runner.status, Some(65), "{:?}", runner.stderr);
    assert!(
        runner.stderr.contains("TARGETLANE001"),
        "the runner's own lane restriction: {}",
        runner.stderr
    );
}

#[test]
fn build_and_emit_refuse_the_runner_by_name() {
    let dir = tempfile::tempdir().expect("scratch");
    let source = write(dir.path(), "hello", "main {\n    print \"hello\"\n}\n");
    for subcommand in ["build", "emit"] {
        let result = outcome(
            faber()
                .args([subcommand, "--target", "runner"])
                .arg(&source),
        );
        assert_eq!(result.status, Some(64), "{subcommand}: usage status");
        assert!(
            result
                .stderr
                .contains("runner is the in-memory interpreter"),
            "{subcommand}: {}",
            result.stderr
        );
    }
}
