//! An imported function's optional defaults run from any package caller.
//!
//! A sibling unit's exported function may declare `optional coalesce`
//! parameters. The entry or another sibling caller omits the argument; the
//! package link carries only the callee's synthetic definition id, so the
//! omitted argument must be filled from linked callee metadata before the
//! runner sees a call-argument count mismatch.

use super::*;
use radix_mir_runner::BufferHost;

const UTIL: &str = r#"@ public
fn plus(int a optional coalesce 5) → int {
    return a + 1
}

@ public
fn greet(string t optional coalesce "salve") → string {
    return t
}
"#;

const MAIN: &str = r#"import from "./util" util

main {
    print util.plus()
    print util.plus(1)
    print util.greet()
    print util.greet("yo")
}
"#;

fn write_package(dir: &std::path::Path, util: &str, main: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "imported-optional-default"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("util.fab"), util).expect("write util.fab");
    fs::write(src.join("main.fab"), main).expect("write main.fab");
    src.join("main.fab")
}

fn write_package_with_mid(
    dir: &std::path::Path,
    util: &str,
    mid: &str,
    main: &str,
) -> std::path::PathBuf {
    let entry = write_package(dir, util, main);
    let source_dir = entry.parent().expect("package source directory");
    fs::write(source_dir.join("mid.fab"), mid).expect("write mid.fab");
    entry
}

fn issues(diagnostics: &[Diagnostic]) -> Vec<(Option<&str>, Option<&str>)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
        .collect()
}

#[test]
fn imported_sibling_default_call_runs() {
    let dir = crate::test_support::test_temp_dir("imported-sibling-default");
    let mid = r#"import from "./util" util
@ public
fn twice() → int { return util.plus() + util.plus() }
"#;
    let main = r#"import from "./mid" mid
main {
    print mid.twice() }
"#;
    let entry = write_package_with_mid(dir.as_ref(), UTIL, mid, main);
    let mut host = BufferHost::default();
    if let Err(diagnostics) = run_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    ) {
        panic!(
            "an imported optional-default call from a sibling must run, got {:?}",
            issues(&diagnostics)
        );
    }
    assert_eq!(host.stdout_lines, vec!["12"]);
}

#[test]
fn imported_optional_default_scalar_and_text_run() {
    let dir = crate::test_support::test_temp_dir("imported-optional-default");
    let entry = write_package(dir.as_ref(), UTIL, MAIN);
    let mut host = BufferHost::default();
    if let Err(diagnostics) = run_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    ) {
        panic!(
            "an imported optional-default call must run from the package entry, got {:?}",
            issues(&diagnostics)
        );
    }
    assert_eq!(
        host.stdout_lines,
        vec![
            "6".to_owned(),
            "2".to_owned(),
            "salve".to_owned(),
            "yo".to_owned()
        ]
    );
}
