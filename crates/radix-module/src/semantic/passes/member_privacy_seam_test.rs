//! Genus member visibility across real file and package seams (D5.3, D5.4).
//!
//! A `genus` exported from another file keeps its member visibility through
//! the file interface. `@ privata` members stay private to the type in every
//! importer; `@ interna` members admit same-package importers only. Any
//! importer may still set every field in a construction literal.

use crate::import_resolve::library_home_test_support;
use crate::services::{CheckDiagnosticsResult, check_diagnostics_for_source};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(&format!("member-privacy-{label}"));
    let path = dir.path().to_path_buf();
    (dir, path)
}

struct LibraryHomeGuard {
    previous: Option<PathBuf>,
}

impl LibraryHomeGuard {
    fn set(home: &Path) -> Self {
        let previous = library_home_test_support::set_thread_library_home(Some(home.to_path_buf()));
        Self { previous }
    }
}

impl Drop for LibraryHomeGuard {
    fn drop(&mut self) {
        library_home_test_support::set_thread_library_home(self.previous.take());
    }
}

const TARGET_SOURCE: &str = r"
@ publica
genus Arca {
    @ privata
    varia numerus secretum
    @ interna
    varia numerus domesticum
    varia numerus apertum

    @ interna
    functio domestica() → vacuum {
    }
}
";

fn write_lib(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(
        root.join("faber.toml"),
        "[package]\nname = \"probe\"\n[library]\nprovider = \"probe\"\n[paths]\nsource = \"src\"\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n",
    )
    .expect("manifest");
    fs::write(root.join("src/probe.fab"), TARGET_SOURCE).expect("lib");
}

fn consumer(import: &str, line: &str) -> String {
    format!(
        "importa ex \"{import}\" probe\nincipit {{\n  varia probe.Arca a ← probe.Arca {{ secretum = 1, domesticum = 2, apertum = 3 }}\n  {line}\n}}\n"
    )
}

fn check_same_package(line: &str) -> CheckDiagnosticsResult {
    let (_td, root) = temp_dir("same");
    write_lib(&root);
    let path = root.join("src/consumer.fab");
    let source = consumer("./probe", line);
    fs::write(&path, &source).expect("consumer");
    check_diagnostics_for_source(&path.display().to_string(), &source, false)
}

fn check_cross_package(line: &str) -> CheckDiagnosticsResult {
    let (_td, home) = temp_dir("cross");
    write_lib(&home.join("probe"));
    let path = home.join("app.fab");
    let source = consumer("probe:probe", line);
    fs::write(&path, &source).expect("app");
    let _guard = LibraryHomeGuard::set(&home);
    check_diagnostics_for_source(&path.display().to_string(), &source, false)
}

fn assert_issue(result: &CheckDiagnosticsResult, issue: &str) {
    assert!(!result.success, "expected {issue}: {:?}", result.diagnostics);
    let diag = result
        .diagnostics
        .iter()
        .find(|diag| diag.issue() == Some(issue))
        .unwrap_or_else(|| panic!("missing {issue}: {:?}", result.diagnostics));
    assert_eq!(diag.code, Some("SEM062"), "{diag:?}");
    assert!(
        diag.args
            .iter()
            .any(|arg| arg.name == "owner" && arg.value == "Arca"),
        "{diag:?}"
    );
}

fn assert_clean(result: &CheckDiagnosticsResult) {
    assert!(result.success, "{:?}", result.diagnostics);
}

#[test]
fn construction_sets_every_field_from_either_seam() {
    assert_clean(&check_same_package("nota a.apertum"));
    assert_clean(&check_cross_package("nota a.apertum"));
}

#[test]
fn interna_members_admit_the_same_package() {
    assert_clean(&check_same_package("a.domesticum ← 4\n  nota a.domesticum\n  a.domestica()"));
}

#[test]
fn interna_members_reject_another_package() {
    assert_issue(&check_cross_package("nota a.domesticum"), "member_interna_read");
    assert_issue(&check_cross_package("a.domesticum ← 4"), "member_interna_write");
    assert_issue(&check_cross_package("a.domestica()"), "member_interna_call");
}

#[test]
fn private_members_reject_every_importer() {
    assert_issue(&check_same_package("nota a.secretum"), "member_private_read");
    assert_issue(&check_cross_package("a.secretum ← 4"), "member_private_write");
}
