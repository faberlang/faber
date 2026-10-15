//! `faber run` on local compile-time constants (`fixum T x = e`, DECISIONS
//! D5.10): the constant runs as an ordinary immutable local, and a value that
//! is not a compile-time fact is a compile error.

use std::fs;
use std::process::Command;

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

fn run(label: &str, source: &str) -> (String, String, bool) {
    let dir = TempDir::new("faber-compile-time-local", label);
    let path = dir.join("main.fab");
    fs::write(&path, format!("+++\nlocale = \"en\"\n+++\n{source}")).expect("write source");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("run")
        .arg(&path)
        .output()
        .expect("run faber");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

#[test]
fn constants_run_as_immutable_locals() {
    let (stdout, stderr, ok) = run(
        "runs",
        "fn f(int p) → int {\n    const int k = 3 * 4\n    const int m = k + 1\n    const _ n = 7\n    return p + m + n\n}\n\nmain {\n    print f(1)\n}\n",
    );
    assert!(ok, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "21");
}

#[test]
fn a_runtime_value_is_rejected_with_the_local_constant_issue() {
    let (_, stderr, ok) = run(
        "runtime",
        "fn f(int p) → int {\n    const int k = p + 1\n    return k\n}\n\nmain {\n    print f(1)\n}\n",
    );
    assert!(!ok);
    assert!(
        stderr.contains("constant_initializer_not_constant"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_constant_that_does_not_fit_its_type_is_rejected() {
    let (_, stderr, ok) = run("overflow", "main {\n    const u8 d = 300\n    print d\n}\n");
    assert!(!ok);
    assert!(
        stderr.contains("store_constant_out_of_range"),
        "stderr: {stderr}"
    );
}

#[test]
fn varia_with_equals_is_rejected() {
    let (_, stderr, ok) = run("varia", "main {\n    mut int n = 3\n    print n\n}\n");
    assert!(!ok);
    assert!(
        stderr.contains("varia_compile_time_initializer"),
        "stderr: {stderr}"
    );
}
