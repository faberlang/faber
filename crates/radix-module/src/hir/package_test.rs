//! Tests for `crate::hir::package`: package build/load round-trips,
//! determinism, and fail-closed loading.

use super::*;
use crate::codegen::{generate_from_analyzed, Target};
use crate::driver::{analyze_source, Config, Session};
use crate::locale::{latin_locale_pack, KeywordSurface};

fn analyze(name: &str, source: &str) -> AnalyzedModule {
    let session = Session::new(Config::default());
    analyze_source(&session, name, source).expect("analyze test source")
}

fn input<'a>(relative_path: &'a str, is_entry: bool, analysis: &'a AnalyzedModule) -> PackageUnitInput<'a> {
    PackageUnitInput {
        relative_path: relative_path.to_owned(),
        module_segments: vec![relative_path.to_owned()],
        is_entry,
        export_names: vec!["greet".to_owned()],
        local_links: vec![],
        library_imports: vec![],
        content_hash: format!("sha256:{relative_path}"),
        analysis,
    }
}

fn sample_source(name: &str) -> String {
    format!("incipit {{\n    nota \"{name}\"\n}}\n")
}

fn sample_dependencies() -> Vec<PackageDependencyWire> {
    vec![PackageDependencyWire {
        name: "norma".to_owned(),
        version: "1.0.0".to_owned(),
        lock_identity: "path:/tmp/norma".to_owned(),
        checksum: None,
    }]
}

#[test]
fn package_round_trip_two_modules() {
    let main = analyze("main.fab", &sample_source("salve"));
    let aux = analyze("aux.fab", &sample_source("vale"));
    let pkg = build_package(
        PackageIdentityWire { name: "exemplum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![input("aux.fab", false, &aux), input("main.fab", true, &main)],
        sample_dependencies(),
    )
    .expect("build package");
    let bytes = encode_package(&pkg).expect("encode package");

    let loaded = load_package(&bytes).expect("load package");
    assert_eq!(loaded.identity.name, "exemplum");
    assert_eq!(loaded.entry_path, "main.fab");
    assert_eq!(loaded.modules.len(), 2);
    assert_eq!(loaded.dependencies.len(), 1);

    let entry = loaded
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    assert_eq!(entry.relative_path, "main.fab");

    // Loaded units must codegen identically to the originals (per-unit
    // source → fhir → target == source → target).
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    for (loaded_module, original) in loaded.modules.iter().zip([&aux, &main]) {
        let loaded_output =
            generate_from_analyzed(Target::HirRust, &loaded_module.unit, &surface).expect("loaded codegen");
        let original_output = generate_from_analyzed(Target::HirRust, original, &surface).expect("original codegen");
        assert_eq!(output_code(&loaded_output), output_code(&original_output));
    }
}

/// Extract the code string from a codegen [`crate::Output`] (Rust lane).
fn output_code(output: &crate::Output) -> &str {
    match output {
        crate::Output::Rust(o) => &o.code,
        _ => panic!("unexpected non-Rust output variant in package test"),
    }
}

#[test]
fn package_build_is_deterministic_across_input_order() {
    let main = analyze("main.fab", &sample_source("salve"));
    let aux = analyze("aux.fab", &sample_source("vale"));

    let build = |modules: Vec<PackageUnitInput<'_>>| {
        build_package(
            PackageIdentityWire {
                name: "exemplum".to_owned(),
                version: "1.0.0".to_owned(),
                edition: "2026".to_owned(),
            },
            "main.fab",
            None,
            modules,
            sample_dependencies(),
        )
        .expect("build package")
    };

    let forward = encode_package(&build(vec![input("aux.fab", false, &aux), input("main.fab", true, &main)]))
        .expect("encode forward");

    let reversed = encode_package(&build(vec![input("main.fab", true, &main), input("aux.fab", false, &aux)]))
        .expect("encode reversed");

    assert_eq!(forward, reversed, "input order must not affect package bytes");
}

#[test]
fn package_load_from_disk_round_trip() {
    let unit = analyze("main.fab", &sample_source("salve"));
    let pkg = build_package(
        PackageIdentityWire { name: "exemplum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![input("main.fab", true, &unit)],
        vec![],
    )
    .expect("build package");
    let bytes = encode_package(&pkg).expect("encode package");

    // Write and read back from disk: the loader must not need the source or
    // any in-memory analysis state (source-independent re-entry).
    let path = std::env::temp_dir().join(format!("radix-fhir-package-test-{}.fhirpkg", std::process::id()));
    std::fs::write(&path, &bytes).expect("write package artifact");
    let from_disk = std::fs::read(&path).expect("read package artifact");
    let _ = std::fs::remove_file(&path);

    let loaded = load_package(&from_disk).expect("load package from disk");
    assert_eq!(loaded.modules.len(), 1);
    assert!(loaded.modules[0].is_entry);
}

#[test]
fn package_load_rejects_tampered_unit_payload() {
    let unit = analyze("main.fab", &sample_source("salve"));
    let pkg = build_package(
        PackageIdentityWire { name: "exemplum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![input("main.fab", true, &unit)],
        vec![],
    )
    .expect("build package");
    let mut bytes = encode_package(&pkg).expect("encode package");
    // Flip a byte in the unit payload region (past the version prefix + a
    // couple of fields). Any decode outcome must be a structured error —
    // never a panic, never a silently truncated package.
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    assert!(load_package(&bytes).is_err(), "tampered unit payload must fail closed");
}

#[test]
fn package_entry_flag_is_single_and_matches_entry_path() {
    let main = analyze("main.fab", &sample_source("salve"));
    let aux = analyze("aux.fab", &sample_source("vale"));
    let pkg = build_package(
        PackageIdentityWire { name: "exemplum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![input("aux.fab", false, &aux), input("main.fab", true, &main)],
        vec![],
    )
    .expect("build package");
    let loaded = load_package(&encode_package(&pkg).expect("encode package")).expect("load package");
    let flagged = loaded
        .modules
        .iter()
        .filter(|module| module.is_entry)
        .collect::<Vec<_>>();
    assert_eq!(flagged.len(), 1);
    assert_eq!(flagged[0].relative_path, loaded.entry_path);
}

#[test]
fn package_dependencies_survive_round_trip() {
    let unit = analyze("main.fab", &sample_source("salve"));
    let pkg = build_package(
        PackageIdentityWire { name: "exemplum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![input("main.fab", true, &unit)],
        sample_dependencies(),
    )
    .expect("build package");
    let loaded = load_package(&encode_package(&pkg).expect("encode package")).expect("load package");
    assert_eq!(loaded.dependencies.len(), 1);
    assert_eq!(loaded.dependencies[0].name, "norma");
    assert_eq!(loaded.dependencies[0].lock_identity, "path:/tmp/norma");
}
