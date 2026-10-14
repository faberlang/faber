//! Six-cell seam matrix: 2 packages × 3 tiers.
//!
//! Policy cells prove [`super::importability`] / [`super::violation_issue`].
//! Integration cells compile a real importer against a real target and assert
//! `code` / `issue()` / args on every diagnostic.

use super::import_seam::{
    ISSUE_IMPORT_INTERNA_CROSS_PACKAGE, ISSUE_IMPORT_MODULE_PRIVATE, ImportSeam, Importability, importability,
    seam_from_flag, violation_issue,
};
use crate::diagnostics::Diagnostic;
use crate::driver::AnalyzeRequest;
use crate::import_resolve::library_home_test_support;
use crate::semantic::SemanticErrorKind;
use crate::semantic::passes::visibility_annotation::VisibilityTier;
use crate::services::check_diagnostics_for_source_with_locale_pack;
use crate::test_support::args_have_no_legacy_message;
use std::fs;
use std::path::{Path, PathBuf};

/// The built-in Latin (`la`) reader pack: the canonical surface these tests
/// are written in.
fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

fn temp_dir(label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(&format!("import-seam-{label}"));
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

fn write_lib_manifest(root: &Path, name: &str) {
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(
        root.join("faber.toml"),
        format!(
            r#"[package]
name = "{name}"
[library]
provider = "{name}"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#
        ),
    )
    .expect("manifest");
}

const TARGET_SOURCE: &str = r"
@ publica
functio aperta() → vacuum {
}

@ interna
functio clausa() → vacuum {
}

@ privata
functio secretum() → vacuum {
}

functio occultum() → vacuum {
}
";

const CONSUMER_TEMPLATE: &str = r#"
importa ex "{import}" {binding}
incipit {{
  {binding}.{member}()
}}
"#;

fn write_target_lib(root: &Path, file_stem: &str) {
    fs::write(root.join("src").join(format!("{file_stem}.fab")), TARGET_SOURCE).expect("lib");
}

fn consumer_source(import: &str, binding: &str, member: &str) -> String {
    CONSUMER_TEMPLATE
        .replace("{import}", import)
        .replace("{binding}", binding)
        .replace("{member}", member)
}

fn expect_structured(diag: &Diagnostic, code: &str, issue: &str, name: &str) {
    assert_eq!(diag.code, Some(code), "code: {diag:?}");
    assert_eq!(diag.issue(), Some(issue), "issue: {diag:?}");
    assert!(
        diag.args
            .iter()
            .any(|arg| arg.name == "name" && arg.value == name),
        "name arg {name}: {diag:?}"
    );
    assert!(
        args_have_no_legacy_message(&diag.args),
        "seam diagnostic must not choose prose at emit site: {diag:?}"
    );
}

fn find_issue<'a>(diagnostics: &'a [Diagnostic], issue: &str) -> &'a Diagnostic {
    diagnostics
        .iter()
        .find(|diag| diag.issue() == Some(issue))
        .unwrap_or_else(|| panic!("missing issue {issue}: {diagnostics:?}"))
}

// ──── Policy matrix (2 seams × 3 tiers) ────

#[test]
fn seam_matrix_policy_six_cells() {
    let cells = [
        (
            Some(VisibilityTier::Publica),
            ImportSeam::SamePackage,
            Importability::Allowed,
            None,
        ),
        (
            Some(VisibilityTier::Interna),
            ImportSeam::SamePackage,
            Importability::Allowed,
            None,
        ),
        (
            Some(VisibilityTier::Privata),
            ImportSeam::SamePackage,
            Importability::ModulePrivate,
            Some(ISSUE_IMPORT_MODULE_PRIVATE),
        ),
        (
            Some(VisibilityTier::Publica),
            ImportSeam::CrossPackage,
            Importability::Allowed,
            None,
        ),
        (
            Some(VisibilityTier::Interna),
            ImportSeam::CrossPackage,
            Importability::InternaCrossPackage,
            Some(ISSUE_IMPORT_INTERNA_CROSS_PACKAGE),
        ),
        (
            Some(VisibilityTier::Privata),
            ImportSeam::CrossPackage,
            Importability::ModulePrivate,
            Some(ISSUE_IMPORT_MODULE_PRIVATE),
        ),
    ];
    for (tier, seam, expected, issue) in cells {
        assert_eq!(importability(tier, seam), expected, "importability {tier:?} {seam:?}");
        assert_eq!(violation_issue(tier, seam), issue, "issue {tier:?} {seam:?}");
    }
}

#[test]
fn unmarked_name_is_module_private() {
    assert_eq!(
        importability(Some(VisibilityTier::Privata), ImportSeam::SamePackage),
        Importability::ModulePrivate
    );
    assert_eq!(importability(None, ImportSeam::SamePackage), Importability::Missing);
}

#[test]
fn same_package_root_is_same_seam() {
    let (_td, root) = temp_dir("seam-root");
    write_lib_manifest(&root, "probe");
    write_target_lib(&root, "probe");
    let from = root.join("src/consumer.fab");
    let target = root.join("src/probe.fab");
    fs::write(&from, "importa ex \"./probe\" probe\n").expect("consumer");
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    assert!(crate::import_resolve::same_package(&session, &from, &target));
    assert_eq!(seam_from_flag(true), ImportSeam::SamePackage);
}

#[test]
fn different_package_roots_are_cross_seam() {
    let (_td, home) = temp_dir("cross-root");
    let probe = home.join("probe");
    write_lib_manifest(&probe, "probe");
    write_target_lib(&probe, "probe");
    let app = home.join("app.fab");
    fs::write(&app, "importa ex \"probe:probe\" probe\n").expect("app");
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    assert!(!crate::import_resolve::same_package(
        &session,
        &app,
        &probe.join("src/probe.fab")
    ));
    assert_eq!(seam_from_flag(false), ImportSeam::CrossPackage);
}

#[cfg(feature = "hir-rust")]
#[test]
fn module_analysis_does_not_derive_contracts_from_path_name() {
    let (_td, root) = temp_dir("fail-closed");
    write_lib_manifest(&root, "probe");
    write_target_lib(&root, "probe");
    let consumer = root.join("src/consumer.fab");
    fs::write(&consumer, consumer_source("./probe", "probe", "aperta")).expect("consumer");
    let source = fs::read_to_string(&consumer).expect("read");
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let prepared = crate::program::compile::prepare_program_module(&consumer, &source, &session)
        .expect("program helper builds contracts from the path");
    assert!(
        prepared.contract.file_interfaces.contains_key("probe"),
        "program/discovery helper still supplies contracts: {:?}",
        prepared.contract.file_interfaces.keys().collect::<Vec<_>>()
    );
    let mut unit =
        crate::driver::analyze_source(&session, &consumer.display().to_string(), &source, AnalyzeRequest::default())
            .expect("empty contracts stay unrestricted; they must not be derived");
    let probe = unit.interner.intern("probe");
    let aperta = unit.interner.intern("aperta");
    assert!(
        !unit.resolver.namespace_has_file_interface(probe),
        "path-shaped analyze_source must not derive file interfaces"
    );
    assert_eq!(
        unit.resolver.namespace_export_tier(probe, aperta),
        None,
        "path-shaped analyze_source must not install namespace exports"
    );
    let compile_str = crate::Compiler::new(crate::driver::Config::default().with_latin_locale())
        .compile_str(&consumer.display().to_string(), &source);
    match &compile_str.output {
        Some(crate::Output::Rust(out)) => {
            assert!(
                !out.code.contains("pub mod probe"),
                "compile_str must not install file interfaces/namespace exports for a path-shaped name"
            );
        }
        other => panic!(
            "compile_str fail-closed must emit consumer Rust without derived contracts, got output={} diagnostics={:?}",
            other.is_some(),
            compile_str
                .diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        ),
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn nested_norma_import_single_file_hir_rust_emits_provider() {
    let (_td, dir) = temp_dir("nested-valor");
    let app = dir.join("app.fab");
    fs::write(
        &app,
        r#"
importa ex "norma:value" valor
incipit {
  nota "ok"
}
"#,
    )
    .expect("app");
    let home = crate::import_resolve::in_tree_library_home().expect("<repo>/lib exists in the tree");
    assert!(
        home.join("norma/src/value.fab").is_file(),
        "expected lib/norma/src/value.fab under {}",
        home.display()
    );
    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::Compiler::new(crate::driver::Config::default().with_latin_locale()).compile(&app);
    assert!(
        result.success(),
        "nested norma:valor single-file HirRust must compile: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let crate::Output::Rust(out) = result.output.as_ref().expect("rust output") else {
        panic!("expected HirRust output");
    };
    assert!(
        out.code.contains("pub mod valor"),
        "provider re-analysis must emit the nested Norma module: {}",
        out.code
    );
}

// ──── Integration matrix ────

fn check_same_package(member: &str) -> crate::services::CheckDiagnosticsResult {
    let (_td, root) = temp_dir(&format!("same-{member}"));
    write_lib_manifest(&root, "probe");
    write_target_lib(&root, "probe");
    let consumer = root.join("src/consumer.fab");
    fs::write(&consumer, consumer_source("./probe", "probe", member)).expect("consumer");
    let source = fs::read_to_string(&consumer).expect("read");
    check_diagnostics_for_source_with_locale_pack(
        &consumer.display().to_string(),
        &source,
        false,
        Some(&la_pack()),
        None,
    )
}

fn check_cross_package(member: &str) -> crate::services::CheckDiagnosticsResult {
    let (_td, home) = temp_dir(&format!("cross-{member}"));
    let probe = home.join("probe");
    write_lib_manifest(&probe, "probe");
    write_target_lib(&probe, "probe");
    let app = home.join("app.fab");
    fs::write(&app, consumer_source("probe:probe", "probe", member)).expect("app");
    let source = fs::read_to_string(&app).expect("read");
    let _guard = LibraryHomeGuard::set(&home);
    check_diagnostics_for_source_with_locale_pack(&app.display().to_string(), &source, false, Some(&la_pack()), None)
}

#[test]
fn same_package_publica_resolves() {
    let result = check_same_package("aperta");
    assert!(result.success, "same-package @ publica must resolve: {:?}", result.diagnostics);
}

#[test]
fn same_package_interna_resolves() {
    let result = check_same_package("clausa");
    assert!(result.success, "same-package @ interna must resolve: {:?}", result.diagnostics);
}

#[test]
fn same_package_privata_errors() {
    let result = check_same_package("secretum");
    assert!(!result.success, "same-package @ privata must error: {:?}", result.diagnostics);
    let diag = find_issue(&result.diagnostics, ISSUE_IMPORT_MODULE_PRIVATE);
    expect_structured(diag, "SEM006", ISSUE_IMPORT_MODULE_PRIVATE, "secretum");
}

#[test]
fn same_package_unmarked_errors() {
    let result = check_same_package("occultum");
    assert!(
        !result.success,
        "same-package unmarked name must error: {:?}",
        result.diagnostics
    );
    let diag = find_issue(&result.diagnostics, ISSUE_IMPORT_MODULE_PRIVATE);
    expect_structured(diag, "SEM006", ISSUE_IMPORT_MODULE_PRIVATE, "occultum");
}

#[test]
fn cross_package_publica_resolves() {
    let result = check_cross_package("aperta");
    assert!(result.success, "cross-package @ publica must resolve: {:?}", result.diagnostics);
}

#[test]
fn cross_package_interna_errors() {
    let result = check_cross_package("clausa");
    assert!(!result.success, "cross-package @ interna must error: {:?}", result.diagnostics);
    let diag = find_issue(&result.diagnostics, ISSUE_IMPORT_INTERNA_CROSS_PACKAGE);
    expect_structured(diag, "SEM006", ISSUE_IMPORT_INTERNA_CROSS_PACKAGE, "clausa");
}

#[test]
fn cross_package_privata_errors() {
    let result = check_cross_package("secretum");
    assert!(!result.success, "cross-package @ privata must error: {:?}", result.diagnostics);
    let diag = find_issue(&result.diagnostics, ISSUE_IMPORT_MODULE_PRIVATE);
    expect_structured(diag, "SEM006", ISSUE_IMPORT_MODULE_PRIVATE, "secretum");
}

#[test]
fn diagnostic_kind_is_import_not_found() {
    assert_eq!(
        crate::diagnostics::semantic_spec(SemanticErrorKind::ImportAbsent).code,
        "SEM006"
    );
}
