//! A missing-key read through `norma:value` yields null and prints, not traps.
//!
//! A single-file program whose only imports are `norma:*` modules runs through
//! the embedded package route (`run_as_package_source` → `run_package_mir` →
//! `run_fmir_package_image`). `value.get` on a missing key and `value.find` on
//! a missing dotted path both yield null, so `is_null` prints `true` twice and
//! the run succeeds instead of trapping.

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

/// The audit probe `norma/toml-value-missing.fab` translated to current
/// binding words (the retired run-time `const` binder is a named PARSE010, so
/// the binder is `let`): both reads miss (`get` on a bare key, `find` on a
/// dotted path) and each must print `true` rather than raise a missing-key
/// trap. Inside the Rust raw string the `\"` and `\n` stay literal — they are
/// the Faber source's own escapes.
const PROBE: &str = r#"+++
locale = "en"
+++
import from "norma:toml" toml
import from "norma:value" value

main {
    let value doc ← toml.parse("title = \"x\"\nn = 42\nf = 1.5\nb = true\narr = [1, 2, 3]\n[server]\nport = 8080\nname = \"s\"\n[[items]]\nid = 1\n[[items]]\nid = 2\n")
    print value.is_null(value.get(doc, "missing"))
    print value.is_null(value.find(doc, "server.nope"))
}
"#;

fn run_probe(label: &str, source: &str) -> (String, String, bool) {
    let dir = TempDir::new("faber-norma-value-missing", label);
    let probe = dir.join("probe.fab");
    fs::write(&probe, source).expect("write probe");

    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("run")
        .arg(probe.to_str().expect("utf8 probe path"))
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
fn value_get_missing_key_prints_true_true() {
    let (stdout, stderr, ok) = run_probe("value-get-missing", PROBE);

    assert!(
        ok,
        "the missing-key reads must succeed, not trap:\n{stderr}"
    );
    assert_eq!(
        stdout, "true\ntrue\n",
        "get on a missing key and find on a missing dotted path both yield null; is_null prints true (en locale):\nstderr:\n{stderr}"
    );
}
