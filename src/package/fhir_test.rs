//! Tests for FHIR package assembly, loading, loaded-package adaptation, and
//! HIR target parity (Rust, canonical Faber, FMIR run) from a loaded artifact.

use super::*;
use crate::package::codegen::assemble_crate;
use crate::package::compile::generate_package_rust;
use crate::package::library_resolver_from_config;
use crate::package::run_package_mir;
use crate::package::test_support::test_temp_dir;
use radix::codegen::rust::RustFieldNamePolicy;
use radix::codegen::{generate_from_analyzed, Target};
use radix::driver::Config;
use radix::mir::BufferHost;
use std::fs;
use std::path::{Path, PathBuf};

fn dev_norma_library_home() -> PathBuf {
    if let Some(home) = std::env::var_os("FABER_LIBRARY_HOME")
        .map(PathBuf::from)
        .filter(|path| path.join("norma/src").exists())
    {
        return home;
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("faberlang container root");
    for candidate in workspace.ancestors() {
        if candidate.join("norma/src").exists() {
            return candidate.to_path_buf();
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

const LOCAL_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\n\nfunctio run() → textus {\n    redde utilModule.salutare()\n}\n\nincipit {\n    nota utilModule.salutare()\n}\n";
const LOCAL_UTIL: &str = "functio salutare() → textus {\n    redde \"salve\"\n}\n";

/// Two-module package with a local import and a namespace call in the entry.
fn write_local_package(dir: &std::path::Path) -> std::path::PathBuf {
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
    fs::write(src.join("util.fab"), LOCAL_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), LOCAL_MAIN).expect("write main.fab");
    src.join("main.fab")
}

/// Single-module package exercising generics and `discerne`/`casu` patterns.
fn write_generics_patterns_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-generics"
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
        src.join("main.fab"),
        r#"discretio Forma {
    Numerus { numerus v },
    Textus { textus v }
}

functio describe(Forma f) → textus {
    discerne f {
        casu Numerus fixum v { redde "n: §"(v) }
        casu Textus fixum v { redde "t: §"(v) }
    }
}

functio idem<T>(T x) → T {
    redde x
}

incipit {
    nota describe(finge Numerus { v = 7 } ∷ Forma)
    nota idem(42)
}
"#,
    )
    .expect("write main.fab");
    src.join("main.fab")
}

/// Library-import package (imports `norma:solum`). `declared` controls whether
/// the manifest declares the dependency record the envelope must carry (a
/// declared dependency also writes the matching `faber.lock`, as `faber
/// install` would).
fn write_library_package(dir: &std::path::Path, declared: bool) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    let mut manifest = String::from(
        r#"
[package]
name = "fhir-library"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    );
    if declared {
        manifest.push_str("\n[dependencies]\nnorma = \"1.0.0\"\n");
        let home = dev_norma_library_home();
        let home = home.display();
        let lock = format!(
            "[[package]]\nname = \"norma\"\nversion = \"1.0.0\"\nsource = \"path:{home}/norma\"\npackage_root = \"{home}/norma\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{home}/norma/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
        );
        fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    }
    fs::write(dir.join("faber.toml"), manifest).expect("write faber.toml");
    fs::write(
        src.join("main.fab"),
        "importa ex \"norma:solum\" privata solum\n\nfunctio scinde(textus s) → numerus {\n    fixum lista<textus> partes ← solum.carpe(s)\n    redde partes.longitudo()\n}\n\nincipit {\n    nota scinde(\"a,b,c\")\n}\n",
    )
    .expect("write main.fab");
    src.join("main.fab")
}

/// Extract the code string from a codegen [`radix::Output`].
fn output_code(output: &radix::Output) -> &str {
    match output {
        radix::Output::Rust(o) => &o.code,
        radix::Output::Faber(o) => &o.code,
        _ => panic!("unexpected output variant in FHIR parity test"),
    }
}

// ---------------------------------------------------------------------------
// Stage 2: build/load round trip
// ---------------------------------------------------------------------------

#[test]
fn build_load_round_trip_with_local_import() {
    let dir = test_temp_dir("fhir-pkg");
    let entry = write_local_package(&dir);
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
fn build_is_deterministic_byte_identical() {
    let dir = test_temp_dir("fhir-determinism");
    let entry = write_local_package(&dir);
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

// ---------------------------------------------------------------------------
// Stage 3: HIR target parity from a package artifact
// ---------------------------------------------------------------------------

#[test]
fn loaded_package_rust_parity_matches_direct() {
    let dir = test_temp_dir("fhir-parity");
    let entry = write_local_package(&dir);
    let config = Config::default();
    let resolver = library_resolver_from_config(&config);

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
fn loaded_package_rust_parity_generics_patterns() {
    let dir = test_temp_dir("fhir-generics-parity");
    let entry = write_generics_patterns_package(&dir);
    let config = Config::default();
    let resolver = library_resolver_from_config(&config);

    let mut direct = analyze_package(&config, &entry).expect("direct analysis");
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
        "loaded generics/patterns Rust must match direct"
    );
}

#[test]
fn loaded_package_canonical_faber_parity() {
    let dir = test_temp_dir("fhir-faber-parity");
    let entry = write_local_package(&dir);
    let config = Config::default();

    let direct = analyze_package(&config, &entry).expect("direct analysis");
    let artifact = build_package_fhir(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    assert_eq!(direct.units.len(), loaded.modules.len());

    // Direct units are in analysis (dependency-first) order; loaded modules
    // are sorted by canonical path — align by package-relative path.
    let package_root = &direct.spec.package_root;
    let direct_by_path = direct
        .units
        .iter()
        .map(|unit| {
            (
                unit.path
                    .strip_prefix(package_root)
                    .unwrap_or(&unit.path)
                    .to_string_lossy()
                    .into_owned(),
                &unit.analysis,
            )
        })
        .collect::<BTreeMap<_, _>>();

    let latin = radix::reader_locale::latin_reader_pack();
    let surface = radix::reader_locale::KeywordSurface::new(&latin);
    for loaded_module in &loaded.modules {
        let direct_unit = direct_by_path
            .get(&loaded_module.relative_path)
            .unwrap_or_else(|| panic!("no direct unit for {}", loaded_module.relative_path));
        let direct_code = output_code(
            &generate_from_analyzed(Target::Faber, direct_unit, &surface)
                .expect("direct canonical Faber emit"),
        )
        .to_owned();
        let loaded_code = output_code(
            &generate_from_analyzed(Target::Faber, &loaded_module.unit, &surface)
                .expect("loaded canonical Faber emit"),
        )
        .to_owned();
        assert_eq!(
            loaded_code, direct_code,
            "canonical Faber re-emission mismatch for {}",
            loaded_module.relative_path
        );
    }
}

#[test]
fn loaded_package_fmir_run_parity_matches_direct() {
    let dir = test_temp_dir("fhir-fmir-parity");
    let entry = write_local_package(&dir);
    let config = Config::default();

    let mut direct_host = BufferHost::default();
    run_package_mir(&config, &entry, &mut direct_host).expect("direct FMIR run");
    let direct_lines = direct_host.stdout_lines.clone();

    let artifact = build_package_fhir(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    let mut loaded_host = BufferHost::default();
    run_loaded_package_fhir(&config, loaded, &artifact.root, &mut loaded_host)
        .expect("loaded FMIR run");

    assert_eq!(
        loaded_host.stdout_lines, direct_lines,
        "loaded FHIR package FMIR run must match direct run"
    );
}

#[test]
fn loaded_package_library_import_unit_parity() {
    let dir = test_temp_dir("fhir-library-parity");
    let entry = write_library_package(&dir, true);
    let config = Config::default().with_stdlib(dev_norma_library_home());

    let direct = analyze_package(&config, &entry).expect("direct analysis with norma");
    assert!(
        direct.diagnostics.iter().all(|diag| !diag.is_error()),
        "direct analysis diagnostics: {:?}",
        direct.diagnostics
    );

    let artifact = build_package_fhir(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    assert_eq!(loaded.modules.len(), 1);
    assert_eq!(
        loaded.modules[0].library_imports.len(),
        1,
        "norma:solum library import survives the envelope"
    );

    // Per-unit codegen parity (Rust + canonical Faber) with the library
    // import intact. Full crate assembly inlines library module bodies, which
    // is the store-backed Stage 5 path; the unit surface is source-free here.
    let latin = radix::reader_locale::latin_reader_pack();
    let surface = radix::reader_locale::KeywordSurface::new(&latin);
    for target in [Target::Rust, Target::Faber] {
        let direct_code = output_code(
            &generate_from_analyzed(target, &direct.units[0].analysis, &surface)
                .expect("direct emit"),
        )
        .to_owned();
        let loaded_code = output_code(
            &generate_from_analyzed(target, &loaded.modules[0].unit, &surface)
                .expect("loaded emit"),
        )
        .to_owned();
        assert_eq!(loaded_code, direct_code, "library-import parity for {target}");
    }
}

#[test]
fn loaded_package_undeclared_library_import_fails_before_codegen() {
    let dir = test_temp_dir("fhir-undeclared");
    let entry = write_library_package(&dir, false);
    let config = Config::default().with_stdlib(dev_norma_library_home());

    // Analysis succeeds (norma resolves via the dev library home) and the
    // envelope builds, but the manifest declares no dependency record for
    // norma — the adapter must reject the load before any codegen.
    let artifact = build_package_fhir(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    assert!(loaded.dependencies.is_empty());
    assert!(!loaded.modules[0].library_imports.is_empty());

    let result = loaded_package_to_analyzed(loaded, &artifact.root);
    match result {
        Err(diagnostics) => {
            assert!(
                diagnostics
                    .iter()
                    .any(|diag| crate::package::test_support::diagnostic_has_issue(
                        diag,
                        "fhir_dependency_unresolved"
                    )),
                "expected fhir_dependency_unresolved, got {diagnostics:?}"
            );
        }
        Ok(_) => panic!("undeclared library import must fail before codegen"),
    }
}
