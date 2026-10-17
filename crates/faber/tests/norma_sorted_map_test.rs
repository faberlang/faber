//! Run integration for `norma:sorted_map`.
//!
//! `lib/norma/src/sorted_map.fab` keeps its rebalancing helpers (`insert`,
//! `remove`, `equals`, `remove_at`) at file scope as generic free functions and
//! calls them from the `SortedMap<K, V>` genus methods through an explicit
//! turbofish. A genus method carries its genus's type parameters rather than a
//! per-genus instantiation, so the nested call has no concrete witness to
//! instantiate against: before the seed guard, unit monomorphization scanned
//! the param-bearing method and minted a param-bearing helper instance, which
//! the survivor net rejected with
//! `invalid_mir_function_fNN_contains_unresolved_generic_type_parameter`.
//! The call now stays a `Definition` callee resolving to the retained template,
//! the same way the genus method itself executes.

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

/// The translated probe: construct, insert, overwrite, project, probe, delete.
/// Runtime bindings use the landed `mut` spelling.
const SORTED_MAP_PROBE: &str = r#"+++
locale = "en"
+++
import from "norma:sorted_map" sorted_map

main {
    mut sorted_map.SortedMap<string, int> m ← sorted_map.empty<string, int>()
    print m.is_empty()
    m.put("b", 2)
    m.put("a", 1)
    m.put("c", 3)
    m.put("a", 10)
    print m.keys()
    print m.values()
    print m.length()
    print m.get("a")
    print m.get("zz")
    print m.has("b")
    m.delete("b")
    print m.has("b")
    print m.keys()
}
"#;

/// Verified capture of the probe's stdout (display rules, never hand-written).
const SORTED_MAP_PROBE_STDOUT: &str = concat!(
    "true\n",
    "[\"a\", \"b\", \"c\"]\n",
    "[10, 2, 3]\n",
    "3\n",
    "10\n",
    "none\n",
    "true\n",
    "false\n",
    "[\"a\", \"c\"]\n",
);

fn temp_dir(label: &str) -> TempDir {
    TempDir::new("faber-norma-sorted-map", label)
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
fn sorted_map_basic_runs() {
    let dir = temp_dir("sorted-map-all");
    let probe = dir.join("sorted-map-all.fab");
    fs::write(&probe, SORTED_MAP_PROBE).expect("write sorted_map probe");

    let (stdout, stderr, ok) = run_faber(&["run", probe.to_str().expect("utf8 probe path")]);

    assert!(ok, "norma:sorted_map probe should run:\n{stderr}");
    assert_eq!(
        stdout, SORTED_MAP_PROBE_STDOUT,
        "unexpected probe stdout:\n{stdout}"
    );
}
