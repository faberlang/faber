//! Runtime trap text raised inside a Norma library frame survives `faber run`.
//!
//! A single-file program whose only import is a `norma:*` module runs through
//! the package route (`run_as_package_source` → `run_package_mir` →
//! `run_fmir_package_image`). A trap raised while executing the library frame
//! must reach stderr with its own text, not render as a bare `error[PKG001]`
//! with an empty message.

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

/// The retry probe: the shift overflows the `int` return cell at the store.
/// Numeric math itself is unbounded, so the trap belongs to the store.
const MATH_SHIFT_OVERFLOW_PROBE: &str = r#"+++
locale = "en"
+++
import from "norma:math" math

main {
    print math.shift_left(1, 64)
}
"#;

/// A leaf failure inside `norma:random`: the range is rejected, not clamped.
const RANDOM_REJECT_PROBE: &str = r#"+++
locale = "en"
+++
import from "norma:random" random
main {
    print random.random_int(5, 1)
    print random.random_bytes(-1).length()
}
"#;

fn temp_dir(label: &str) -> TempDir {
    TempDir::new("faber-norma-trap", label)
}

fn run_faber(args: &[&str]) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber run");

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

fn run_probe(label: &str, source: &str) -> (String, String, bool) {
    let dir = temp_dir(label);
    let probe = dir.join("probe.fab");
    fs::write(&probe, source).expect("write probe");
    run_faber(&["run", probe.to_str().expect("utf8 probe path")])
}

#[test]
fn shift_overflow_message_from_norma_frame() {
    let (_stdout, stderr, ok) = run_probe("math-shift-overflow", MATH_SHIFT_OVERFLOW_PROBE);

    assert!(!ok, "the shift overflow must fail:\n{stderr}");
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: the trap text raised inside the library
    // frame is the contract this regression pins; a bare PKG001 with an empty
    // message is the defect.
    assert!(
        stderr.contains("numerus overflow"),
        "trap text from the norma:math frame must survive to stderr:\n{stderr}"
    );
}

#[test]
fn random_reject_message_from_norma_frame() {
    let (_stdout, stderr, ok) = run_probe("random-reject", RANDOM_REJECT_PROBE);

    assert!(!ok, "the rejected range must fail:\n{stderr}");
    assert!(
        !stderr.trim().is_empty(),
        "the leaf failure must render a diagnostic, not nothing"
    );
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: the leaf failure kind is the stable
    // identity of the fixed message; the empty PKG001 form is the defect.
    assert!(
        stderr.contains("InvalidArgument"),
        "leaf failure kind from the norma:random frame must survive to stderr:\n{stderr}"
    );
}
