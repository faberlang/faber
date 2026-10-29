//! `faber run --target runner`: the named default engine. Its behaviour,
//! output, exit codes and trap stderr must be identical to a plain
//! `faber run` (the no-`--target` route), and `faber build`/`emit` refuse the
//! spelling by name, as they do for `clif-jit`.

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

#[test]
fn runner_target_is_identical_to_the_default_route() {
    let dir = tempfile::tempdir().expect("scratch");
    for (name, text, status) in cases() {
        let source = write(dir.path(), name, text);
        let default = outcome(faber().arg("run").arg(&source));
        let runner = outcome(faber().args(["run", "--target", "runner"]).arg(&source));
        assert_eq!(default.status, Some(status), "{name}: default status");
        assert!(
            !default.stdout.is_empty() || status != 0,
            "{name}: the default route printed nothing"
        );
        assert_eq!(runner.stdout, default.stdout, "{name}: stdout");
        assert_eq!(runner.stderr, default.stderr, "{name}: stderr");
        assert_eq!(runner.status, default.status, "{name}: exit status");
    }
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
