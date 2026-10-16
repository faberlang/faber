//! A failable function exported by a sibling package file keeps its declared
//! success type at a caller that never imports the fault's home library.
//!
//! The sibling's `⇥ fault.IoFault` names a `norma:fault` nominal. The caller's
//! import contract must install that library transitively, or the canonical
//! nominal lookup misses, the whole export is skipped (WARN014) and the call
//! types as unresolved. The second probe covers the merge path: a sibling unit
//! plus a failable generic library call (`fs.read_file<string>`) whose
//! template names a helper of its own library.

use super::*;
use radix_mir_runner::BufferHost;

const LOADER_UTIL: &str = r#"import from "norma:fault" fault

@ public
fn make(int n) → list<f32> ⇥ fault.IoFault {
    mut list<f32> out ← []
    for range 0‥n let k {
        out.append(1.5)
    }
    return out
}
"#;

const LOADER_LIST_MAIN: &str = r#"import from "./util" util

main {
    let list<f32> g ← util.make(3)
    print g.length()
}
"#;

const LOADER_TENSOR_MAIN: &str = r#"import from "./util" util

main {
    const size H ← 2
    const size W ← 3
    let tensor<f32, [H, W]> t ← util.make(H * W) ↦ tensor<f32, [H, W]>
    print t
}
"#;

const PLAIN_UTIL: &str = r#"@ public
fn make(int n) → int {
    return n + 1
}
"#;

const SIBLING_AND_LIBRARY_GENERIC_MAIN: &str = r#"import from "norma:fs" fs
import from "./util" util

main {
    let int n ← util.make(2)
    let string text ← fs.read_file<string>("/definitely/not/here")
    print n
    print text
}
"#;

fn write_package(dir: &std::path::Path, util: &str, main: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "imported-failable-call"
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

fn issues(diagnostics: &[Diagnostic]) -> Vec<(Option<&str>, Option<&str>)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
        .collect()
}

#[test]
fn imported_failable_call_has_its_declared_success_type() {
    let dir = crate::test_support::test_temp_dir("imported-failable-list");
    let entry = write_package(dir.as_ref(), LOADER_UTIL, LOADER_LIST_MAIN);
    let mut host = BufferHost::default();
    if let Err(diagnostics) = run_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    ) {
        panic!(
            "caller of an imported failable function must type-check, got {:?}",
            issues(&diagnostics)
        );
    }
    assert_eq!(host.stdout_lines, vec!["3".to_owned()]);
}

#[test]
fn imported_failable_call_converts_into_a_tensor() {
    let dir = crate::test_support::test_temp_dir("imported-failable-tensor");
    let entry = write_package(dir.as_ref(), LOADER_UTIL, LOADER_TENSOR_MAIN);
    let mut host = BufferHost::default();
    if let Err(diagnostics) = run_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    ) {
        panic!(
            "tensor conversion of an imported failable call must lower, got {:?}",
            issues(&diagnostics)
        );
    }
    assert_eq!(host.stdout_lines.len(), 1, "{:?}", host.stdout_lines);
}

#[test]
fn sibling_unit_with_failable_generic_library_call_validates() {
    let dir = crate::test_support::test_temp_dir("sibling-and-library-generic");
    let entry = write_package(dir.as_ref(), PLAIN_UTIL, SIBLING_AND_LIBRARY_GENERIC_MAIN);
    let result = with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |_| {},
    );
    if let Err(diagnostics) = &result {
        panic!(
            "sibling unit plus fs.read_file<string> must lower, got {:?}",
            issues(diagnostics)
        );
    }
}
