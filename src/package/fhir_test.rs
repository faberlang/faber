//! Tests for FHIR package assembly, loading, and loaded-package adaptation.

use super::*;
use crate::package::codegen::assemble_crate;
use crate::package::compile::generate_package_rust;
use crate::package::library_resolver_from_config;
use crate::package::test_support::test_temp_dir;
use radix::codegen::rust::RustFieldNamePolicy;
use std::fs;

fn write_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-fixture"
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
    fs::write(
        src.join("util.fab"),
        "functio salutare() → textus {\n    redde \"salve\"\n}\n",
    )
    .expect("write util.fab");
    fs::write(
        src.join("main.fab"),
        "importa ex \"./util\" privata * ut utilModule\n\nfunctio run() → textus {\n    redde utilModule.salutare()\n}\n",
    )
    .expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn build_load_round_trip_with_local_import() {
    let dir = test_temp_dir("fhir-pkg");
    let entry = write_package(&dir);
    let artifact = build_package_fhir(&Config::default(), &entry).expect("build FHIR package");
    assert!(artifact.package_path.is_file(), "package artifact written");

    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    assert_eq!(loaded.identity.name, "fhir-fixture");
    assert_eq!(loaded.identity.version, "1.0.0");
    assert_eq!(loaded.entry_path, "src/main.fab");
    assert_eq!(loaded.modules.len(), 2, "entry + imported module");

    let main = loaded
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    assert_eq!(main.relative_path, "src/main.fab");
    assert_eq!(main.local_links.len(), 1);
    assert_eq!(main.local_links[0].binding, "utilModule");
    assert_eq!(main.local_links[0].target, "src/util.fab");
    assert_eq!(main.export_names, vec!["run".to_owned()]);

    let util = loaded
        .modules
        .iter()
        .find(|module| module.relative_path == "src/util.fab")
        .expect("util module");
    assert!(!util.is_entry);
    assert!(util.local_links.is_empty());
}

#[test]
fn loaded_package_rust_parity_matches_direct() {
    let dir = test_temp_dir("fhir-parity");
    let entry = write_package(&dir);
    let config = Config::default();
    let resolver = library_resolver_from_config(&config);

    // Direct: analyze → generate package Rust.
    let mut direct = analyze_package(&config, &entry).expect("direct package analysis");
    let generated = generate_package_rust(
        &mut direct,
        &resolver,
        None,
        RustFieldNamePolicy::Preserve,
        None,
    );
    assert!(
        generated.diagnostics.iter().all(|diag| !diag.is_error()),
        "direct Rust generation failed: {:?}",
        generated.diagnostics
    );
    let direct_code = assemble_crate(
        &generated.entry_code.expect("direct entry code"),
        &generated.module_tree.render(0),
    );

    // Loaded: build FHIR → load → adapt → generate package Rust from the
    // envelope's explicit link table (no source on disk).
    let artifact = build_package_fhir(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    let links = loaded_links_by_unit_path(&loaded, &artifact.root);
    let mut adapted =
        loaded_package_to_analyzed(loaded, &artifact.root).expect("adapt loaded package");
    let generated_loaded = generate_package_rust(
        &mut adapted,
        &crate::library::LibraryResolver::default(),
        None,
        RustFieldNamePolicy::Preserve,
        Some(&links),
    );
    assert!(
        generated_loaded
            .diagnostics
            .iter()
            .all(|diag| !diag.is_error()),
        "loaded Rust generation failed: {:?}",
        generated_loaded.diagnostics
    );
    let loaded_code = assemble_crate(
        &generated_loaded.entry_code.expect("loaded entry code"),
        &generated_loaded.module_tree.render(0),
    );

    assert_eq!(
        loaded_code, direct_code,
        "loaded FHIR package Rust must match direct package Rust"
    );
}

#[test]
fn build_is_deterministic_byte_identical() {
    let dir = test_temp_dir("fhir-determinism");
    let entry = write_package(&dir);
    let first = build_package_fhir(&Config::default(), &entry).expect("first build");
    let first_bytes = fs::read(&first.package_path).expect("read first artifact");
    let second = build_package_fhir(&Config::default(), &entry).expect("second build");
    let second_bytes = fs::read(&second.package_path).expect("read second artifact");
    assert_eq!(first_bytes, second_bytes, "repeated builds must be byte-identical");
}

#[test]
fn load_rejects_missing_file_fail_closed() {
    let dir = test_temp_dir("fhir-missing");
    let result = load_package_fhir(&dir.join("absent.fhirpkg"));
    assert!(result.is_err(), "missing artifact must fail closed");
}
