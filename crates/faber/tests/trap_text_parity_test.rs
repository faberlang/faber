//! One Faber trap, one stderr line: the runner and the native clif lane print
//! byte-identical trap text and exit with the same status.
//!
//! The words come from `radix_runtime_contract::trap::text`, shared by both
//! lanes. Each program here traps through a different collection check; the
//! test runs it on the runner (`faber run --target runner`) and as a clif
//! native binary (`faber build -t clif`, which builds the `runtime/clif`
//! archive on first use) and compares the two.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const LIST_HEAD: &str =
    "let list<int> xs ← [10, 20, 30]\n    mut int n ← 9\n    let int low ← -1\n";

/// `(name, source, expected trap line)`.
fn cases() -> Vec<(&'static str, String, &'static str)> {
    let list =
        |tail: &str| format!("main {{\n    {LIST_HEAD}    print \"before\"\n    {tail}\n}}\n");
    let text = |tail: &str| {
        format!(
            "main {{\n    let string s ← \"abc\"\n    mut int n ← 9\n    let int low ← -1\n    print \"before\"\n    {tail}\n}}\n"
        )
    };
    vec![
        (
            "list_window_past_end",
            list("print xs[0‥n]"),
            "error: list range 0‥9 outside 0‥3",
        ),
        (
            "list_window_reversed_interval",
            list("print xs[2‥1]"),
            "error: interval bounds must be monotonic (start ≤ end)",
        ),
        (
            "list_window_negative",
            list("print xs[low‥2]"),
            "error: expected non-negative numerus index",
        ),
        (
            "list_index_past_end",
            list("print xs[n]"),
            "error: array index out of bounds",
        ),
        (
            "list_index_negative",
            list("print xs[low]"),
            "error: expected non-negative numerus index",
        ),
        (
            "text_window_past_end",
            text("print s[0‥n]"),
            "error: text range 0‥9 outside 0‥3",
        ),
        (
            "text_index_past_end",
            text("print s[n]"),
            "error: text index out of bounds",
        ),
        (
            "text_index_negative",
            text("print s[low]"),
            "error: expected non-negative numerus index",
        ),
        (
            "map_missing_key",
            "main {\n    mut map<string, int> m ← empty\n    m.put(\"a\", 1)\n    print \"before\"\n    print m[\"z\"]\n}\n"
                .to_owned(),
            "error: tabula missing key",
        ),
    ]
}

fn first_stderr_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .find(|line| !line.starts_with("warning") && !line.starts_with("  "))
        .unwrap_or_default()
        .to_owned()
}

fn run_on_runner(source: &Path) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["run", "--target", "runner"])
        .arg(source)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber run");
    (output.status.code(), first_stderr_line(&output.stderr))
}

fn run_on_clif(dir: &Path, name: &str, source: &Path) -> (Option<i32>, String) {
    let out_dir = dir.join(format!("{name}-out"));
    let build = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["build", "-t", "clif", "--out-dir"])
        .arg(&out_dir)
        .arg(source)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber build");
    assert!(
        build.status.success(),
        "{name}: clif build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = out_dir.join(source.file_stem().expect("source stem"));
    let output = Command::new(&binary)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("{name}: run {}: {error}", binary.display()));
    (output.status.code(), first_stderr_line(&output.stderr))
}

#[test]
fn the_runner_and_clif_print_the_same_trap_text_and_exit_with_the_same_status() {
    let dir = tempfile::tempdir().expect("scratch");
    for (name, text, expected) in cases() {
        let source = dir.path().join(format!("{name}.fab"));
        fs::write(&source, text).expect("write source");
        let runner = run_on_runner(&source);
        let clif = run_on_clif(dir.path(), name, &source);
        assert_eq!(runner, (Some(70), expected.to_owned()), "{name}: runner");
        assert_eq!(clif, runner, "{name}: clif differs from the runner");
    }
}
