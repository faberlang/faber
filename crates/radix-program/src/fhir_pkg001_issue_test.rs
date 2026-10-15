//! FHIR builder PKG001 issue naming (need, item 2).
//!
//! `build_package_fhir` had four failure arms that built issue-less `PKG001`
//! diagnostics, so they rendered `error[PKG001]: <path>` with no variant
//! identity. Each arm now carries a stable issue slug
//! (`fhir_artifact_dir_failed`, `fhir_artifact_write_failed`, …) so the
//! failure is machine-identifiable.
//!
//! These tests assert the issue code only — never rendered message text.

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;

use crate::analyze::AnalysisSeams;
use crate::discovery::PackageSpec;
use crate::library::LibraryResolver;
use std::fs;
use std::path::{Path, PathBuf};

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "radix-program-fhir-pkg001-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

fn default_locale_pack(
    _input: &Path,
    _locale: Option<&str>,
) -> Result<Option<radix_module::locale::LocalePack>, Box<Diagnostic>> {
    let path = radix_module::locale::workspace_locale_pack_path("en");
    let pack = radix_module::locale::LocalePack::from_toml_path(path).map_err(|err| {
        Box::new(
            Diagnostic::error(format!("failed to load English locale pack: {err}"))
                .with_phase(radix_module::diagnostics::DiagnosticPhase::Lex),
        )
    })?;
    Ok(Some(pack))
}

fn accept_manifest(_spec: &PackageSpec) -> Result<(), Box<Diagnostic>> {
    Ok(())
}

fn empty_resolver_for_package(
    _config: &Config,
    _package_root: &Path,
) -> Result<LibraryResolver, Vec<Diagnostic>> {
    Ok(LibraryResolver::with_optional_home(None))
}

fn test_seams() -> AnalysisSeams {
    AnalysisSeams {
        locale_loader: default_locale_pack,
        manifest_validator: accept_manifest,
        resolver_for_package: empty_resolver_for_package,
    }
}

/// Minimal manifest-backed `bin` package with a single line-level entry.
fn write_package(label: &str) -> (PathBuf, PathBuf) {
    let dir = temp_root(label);
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-pkg001-fixture"
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
    fs::write(src.join("main.fab"), "main {\n    print \"salve\"\n}\n").expect("write main.fab");
    (dir, src.join("main.fab"))
}

fn has_issue(diagnostics: &[Diagnostic], code: &str, issue: &str) -> bool {
    diagnostics
        .iter()
        .any(|diag| diag.code == Some(code) && diag.issue() == Some(issue))
}

/// A plain file where the artifact root directory must be created fails the
/// `create_dir_all` arm, which must name itself.
#[test]
fn fhir_artifact_dir_failed_names_the_arm() {
    let (dir, entry) = write_package("dir");
    // A plain FILE named `target` makes `target/faber-fhir` uncreatable.
    fs::write(dir.join("target"), b"not a directory").expect("write blocking file");

    let result = crate::fhir::build_package_fhir(&Config::default(), &entry, &test_seams());
    let _ = fs::remove_dir_all(&dir);

    let diagnostics = result.expect_err("artifact dir creation must fail");
    assert!(
        has_issue(&diagnostics, "PKG001", "fhir_artifact_dir_failed"),
        "the create_dir_all arm must carry its issue slug: {diagnostics:?}"
    );
}

/// Pre-creating `target/faber-fhir/package.fhirpkg` as a directory makes
/// `fs::write` fail; the arm must name itself.
#[test]
fn fhir_artifact_write_failed_names_the_arm() {
    let (dir, entry) = write_package("write");
    // The artifact path itself is a directory, so `fs::write` cannot succeed.
    fs::create_dir_all(dir.join("target/faber-fhir/package.fhirpkg")).expect("block artifact path");

    let result = crate::fhir::build_package_fhir(&Config::default(), &entry, &test_seams());
    let _ = fs::remove_dir_all(&dir);

    let diagnostics = result.expect_err("artifact write must fail");
    assert!(
        has_issue(&diagnostics, "PKG001", "fhir_artifact_write_failed"),
        "the fs::write arm must carry its issue slug: {diagnostics:?}"
    );
}
