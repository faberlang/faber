//! Run integration for the `norma:vector` shape-parameterized wrappers.
//!
//! `lib/norma/src/vector.fab` exposes `add`/`subtract`/`multiply`/`divide`/
//! `dot` over `<elem, size N>` and `cross` over a fixed width 3. On the
//! interpreted `faber run` path the call-site witness binds `N` from the
//! `vector<elem, size N>` argument shape; package instantiation must defer the
//! not-yet-interned substituted type to the finalize pass, which interns
//! `vector<elem, size N>` and re-substitutes the site. Before that deferral every
//! call failed validation with
//! `PKG001:invalid_mir_instantiated_generic_type_is_missing_semantic_metadata`.

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

/// The translated probe: `f32` add/subtract/multiply/divide/dot/cross followed
/// by the `int` dot. Runtime bindings use the landed `let` spelling.
const VECTOR_PROBE: &str = r#"+++
locale = "en"
+++
import from "norma:vector" vector

main {
    let vector<f32, size 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>
    let vector<f32, size 3> b ← [4.0, 5.0, 6.0] ↦ vector<f32, size 3>
    print vector.add(a, b)
    print vector.subtract(b, a)
    print vector.multiply(a, b)
    print vector.divide(b, a)
    print vector.dot(a, b)
    print vector.cross(a, b)
    let vector<int, size 2> p ← [3, 4] ↦ vector<int, size 2>
    let vector<int, size 2> q ← [1, 2] ↦ vector<int, size 2>
    print vector.dot(p, q)
}
"#;

/// Verified capture of the probe's stdout (display rules, never hand-written
/// from arithmetic).
const VECTOR_PROBE_STDOUT: &str = concat!(
    "[5.0, 7.0, 9.0]\n",
    "[3.0, 3.0, 3.0]\n",
    "[4.0, 10.0, 18.0]\n",
    "[4.0, 2.5, 2.0]\n",
    "32.0\n",
    "[-3.0, 6.0, -3.0]\n",
    "11\n",
);

fn temp_dir(label: &str) -> TempDir {
    TempDir::new("faber-norma-vector", label)
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

#[test]
fn vector_ops_run() {
    let dir = temp_dir("vector-all");
    let probe = dir.join("vector-all.fab");
    fs::write(&probe, VECTOR_PROBE).expect("write vector probe");

    let (stdout, stderr, ok) = run_faber(&["run", probe.to_str().expect("utf8 probe path")]);

    assert!(ok, "norma:vector probe should run:\n{stderr}");
    assert_eq!(
        stdout, VECTOR_PROBE_STDOUT,
        "unexpected probe stdout:\n{stdout}"
    );
}
