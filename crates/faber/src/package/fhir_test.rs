//! Tests for FHIR package assembly, loading, loaded-package adaptation, and
//! HIR target parity (Rust, canonical Faber, FMIR run) from a loaded artifact.

use super::*;
use crate::package::codegen::assemble_crate;
use crate::package::library_resolver_from_config;
use crate::package::run_package_mir;
use crate::package::rust_target::generate_package_rust;
use crate::package::test_support::test_temp_dir;
use faber_hir_rust::RustFieldNamePolicy;
use radix::codegen::{generate_from_analyzed, Target};
use radix::driver::Config;
use radix::hir::{LibraryItemKind, LibraryProvider};
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

const CONST_MEMBER_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\n\nfunctio run() → textus {\n    redde utilModule.VALUE\n}\n\nincipit {\n    nota utilModule.VALUE\n}\n";
const CONST_MEMBER_UTIL: &str = "const textus VALUE ← \"salve\"\n";

const LIBRARY_CONST_MAIN: &str = "importa ex \"constlib:vals\" privata vals\n\nfunctio run() → textus {\n    redde vals.LIBRARY_VALUE\n}\n\nincipit {\n    nota vals.LIBRARY_VALUE\n}\n";
const LIBRARY_CONST_VALS: &str = "const textus LIBRARY_VALUE ← \"salve\"\n";

/// Entry that CALLS a const data member — an unsupported shape that must
/// keep the non-function fail-closed diagnostic (U1 done_when 4).
const CONST_MEMBER_CALL_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\n\nincipit {\n    nota utilModule.VALUE()\n}\n";

const LOCAL_LOCALE_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\n\nfunctio run() → textus {\n    redde utilModule.greet()\n}\n\nincipit {\n    nota utilModule.greet()\n}\n";
const LOCAL_LOCALE_UTIL: &str = "functio salutare() → textus {\n    redde \"salve\"\n}\n";

const EXTERNAL_LIBRARY_LOCALE_MAIN: &str = "import from \"triga:math\" private math\nimport from \"norma:chorda\" private chorda\n\nfn run() → string {\n    const math.Vector3 canonical ← math.vector3(1.0, 0.0, 0.0)\n    const math.Vector3 canonical_shifted ← canonical.addita(math.vector3(0.0, 1.0, 0.0))\n    const float canonical_length ← canonical_shifted.longitudo()\n    const math.Vec3 original ← math.make_vector(1.0, 0.0, 0.0)\n    const math.Vec3 shifted ← original.add(math.make_vector(0.0, 2.0, 0.0))\n    const math.Vec3 normalized ← shifted.normalize()\n    const float length ← normalized.length()\n    const string reversed ← chorda.reverse(\"abc\")\n    print canonical_length\n    print length\n    return reversed\n}\n\nmain {\n    print run()\n}\n";
const NORMA_LATIN_COMPATIBILITY_MAIN: &str = "importa ex \"norma:chorda\" privata chorda\n\nfunctio run() → textus {\n    redde chorda.retorta(\"abc\")\n}\n\nincipit {\n    nota run()\n}\n";

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

/// Two-module package whose sibling exports a const data member the entry
/// references through its namespace binding (U1 s1-data-member-abi mutation).
fn write_const_member_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-const-member"
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
    fs::write(src.join("util.fab"), CONST_MEMBER_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), CONST_MEMBER_MAIN).expect("write main.fab");
    src.join("main.fab")
}

/// Package whose entry imports a const data member from a lock-resolved
/// library fixture (`constlib:vals`), proving library const members link and
/// execute through package MIR (U1 done_when 2). The library lives under a
/// config library home (`libhome/constlib/src`) so both the analysis phase
/// (lock resolver) and the package-MIR link phase (config home resolver)
/// resolve `constlib:vals`.
fn write_library_const_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("constlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-const-library"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
constlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"constlib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(
        library.join("src/vals.fab"),
        LIBRARY_CONST_VALS,
    )
    .expect("write library const module");
    fs::write(src.join("main.fab"), LIBRARY_CONST_MAIN).expect("write main.fab");
    src.join("main.fab")
}

fn write_local_locale_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create local locale package src");
    fs::create_dir_all(dir.join("locale")).expect("create local locale packs");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "local-locale-fixture"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write local locale manifest");
    fs::write(
        dir.join("locale/en-local-library.toml"),
        r#"
[pack]
id = "en-local-library"
schema_version = 1
fallback = ["la"]

[[library_members]]
provider = "package"
package = "local-locale-fixture"
module_path = ["util"]
kind = "function"
canonical = "salutare"
surface = "greet"

[llm]
system_prompt_snippet = "Emit local-library locale fixture source"
exemplars = ["local-library.fab"]
"#,
    )
    .expect("write local library locale pack");
    fs::write(dir.join("locale/local-library.fab"), "incipit {}\n")
        .expect("write local library locale exemplar");
    fs::write(src.join("util.fab"), LOCAL_LOCALE_UTIL).expect("write local locale util");
    fs::write(src.join("main.fab"), LOCAL_LOCALE_MAIN).expect("write local locale main");
    src.join("main.fab")
}

fn write_external_library_locale_package(
    dir: &std::path::Path,
    source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create external library package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "external-library-locale-fixture"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write external library manifest");
    fs::write(src.join("main.fab"), source).expect("write external library consumer");
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
fn local_library_locale_surface_resolves_through_package_loader() {
    let dir = test_temp_dir("fhir-local-locale");
    let entry = write_local_locale_package(&dir);
    let (config, _diagnostic_pack) = crate::package::config_with_locale(
        radix::codegen::Target::HirRust,
        &entry,
        Some("en-local-library"),
        None,
    )
    .expect("load local library locale through package selection");
    assert_eq!(
        config
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("en-local-library")
    );
    let package = analyze_package(&config, &entry).expect("analyze localized local package");

    assert!(
        package
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "localized local package diagnostics: {:?}",
        package.diagnostics
    );

    let util = package
        .units
        .iter()
        .find(|unit| unit.module_segments == ["util".to_owned()])
        .expect("localized local utility unit");
    assert_eq!(
        util.file_interface.identity,
        Some(radix::file_interface::InterfaceLibraryIdentity {
            provider: "package".to_owned(),
            package: Some("local-locale-fixture".to_owned()),
            module_path: vec!["util".to_owned()],
        })
    );
    assert!(
        util.file_interface.exports.contains_key("salutare"),
        "local interface must retain canonical export names"
    );
}

#[test]
fn english_pack_resolves_real_triga_and_norma_library_surfaces() {
    let dir = test_temp_dir("fhir-external-library-locale");
    let entry = write_external_library_locale_package(&dir, EXTERNAL_LIBRARY_LOCALE_MAIN);
    let (config, _diagnostic_pack) = crate::package::config_with_locale(
        radix::codegen::Target::HirRust,
        &entry,
        Some("en"),
        None,
    )
    .expect("load installed English library locale");
    let config = config.with_stdlib(dev_norma_library_home());
    let package = analyze_package(&config, &entry).expect("analyze localized external libraries");

    assert!(
        package
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "localized external library diagnostics: {:?}",
        package.diagnostics
    );
    let entry_unit = package.entry_unit().expect("external library entry unit");
    assert_eq!(
        entry_unit.expanded_library_imports.len(),
        2,
        "the consumer must resolve both real source-library imports"
    );
    let library_items = entry_unit
        .analysis
        .libraries
        .items
        .values()
        .collect::<Vec<_>>();
    assert!(
        library_items.iter().any(|item| {
            item.identity.provider == LibraryProvider::Package("triga".to_owned())
                && item.identity.module_path == ["math".to_owned()]
                && item.exported_name == "vector3"
                && item.kind == LibraryItemKind::Function
        }),
        "library provenance: {library_items:?}"
    );
    assert!(
        library_items.iter().any(|item| {
            item.identity.provider == LibraryProvider::Package("triga".to_owned())
                && item.identity.module_path == ["math".to_owned()]
                && item.exported_name == "Vector3"
                && item.kind == LibraryItemKind::Struct
        }),
        "library provenance: {library_items:?}"
    );
    assert!(
        library_items.iter().any(|item| {
            item.identity.provider == LibraryProvider::Builtin("norma".to_owned())
                && item.identity.module_path == ["chorda".to_owned()]
                && item.exported_name == "retorta"
                && item.kind == LibraryItemKind::Function
        }),
        "library provenance: {library_items:?}"
    );
}

#[test]
fn latin_norma_consumer_keeps_canonical_surface_compatibility() {
    let dir = test_temp_dir("fhir-norma-latin-compatibility");
    let entry = write_external_library_locale_package(&dir, NORMA_LATIN_COMPATIBILITY_MAIN);
    let config = Config::default().with_stdlib(dev_norma_library_home());
    let package =
        analyze_package(&config, &entry).expect("analyze Latin Norma compatibility consumer");

    assert!(
        package
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "Latin Norma compatibility diagnostics: {:?}",
        package.diagnostics
    );
    let entry_unit = package.entry_unit().expect("Latin Norma entry unit");
    assert_eq!(
        entry_unit.expanded_library_imports.len(),
        1,
        "the Latin consumer must resolve the canonical Norma import"
    );
    assert!(
        entry_unit.analysis.libraries.items.values().any(|item| {
            item.identity.provider == LibraryProvider::Builtin("norma".to_owned())
                && item.identity.module_path == ["chorda".to_owned()]
                && item.exported_name == "retorta"
                && item.kind == LibraryItemKind::Function
        }),
        "library provenance: {:?}",
        entry_unit
            .analysis
            .libraries
            .items
            .values()
            .collect::<Vec<_>>()
    );
}

#[test]
fn build_is_deterministic_byte_identical() {
    let dir = test_temp_dir("fhir-determinism");
    let entry = write_local_package(&dir);
    let first = build_package_fhir(&Config::default(), &entry).expect("first build");
    let first_bytes = fs::read(&first.package_path).expect("read first artifact");
    let second = build_package_fhir(&Config::default(), &entry).expect("second build");
    let second_bytes = fs::read(&second.package_path).expect("read second artifact");
    assert_eq!(
        first_bytes, second_bytes,
        "repeated builds must be byte-identical"
    );
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

    let latin = radix::locale::latin_locale_pack();
    let surface = radix::locale::KeywordSurface::new(&latin);
    for loaded_module in &loaded.modules {
        let direct_unit = direct_by_path
            .get(&loaded_module.relative_path)
            .unwrap_or_else(|| panic!("no direct unit for {}", loaded_module.relative_path));
        let direct_code = output_code(
            &generate_from_analyzed(Target::HirFaber, direct_unit, &surface)
                .expect("direct canonical Faber emit"),
        )
        .to_owned();
        let loaded_code = output_code(
            &generate_from_analyzed(Target::HirFaber, &loaded_module.unit, &surface)
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
fn loaded_package_const_data_member_parity() {
    let dir = test_temp_dir("fhir-const-member");
    let entry = write_const_member_package(&dir);
    let config = Config::default();

    // S1 data-member ABI: the entry references `utilModule.VALUE` (a const
    // data member exported by the sibling `util.fab`). The direct and loaded
    // package-MIR paths must both link the const member, lower it through the
    // merged program, and match the direct-execution oracle.
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
        "loaded FHIR package const-member FMIR run must match direct run"
    );
    assert_eq!(
        direct_lines,
        vec!["salve".to_owned()],
        "const data member must print its value oracle"
    );
}

#[test]
fn loaded_package_library_const_data_member_runs() {
    let dir = test_temp_dir("fhir-library-const");
    let entry = write_library_const_package(&dir);
    // The package-MIR link phase resolves library imports through the config
    // library home; the fixture library lives under `libhome/constlib/src`.
    let config = Config::default().with_stdlib(dir.join("libhome"));

    // U1 done_when (2): a linked-library const data member links and executes
    // through the direct package-MIR path.
    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("library const FMIR run");
    assert_eq!(
        host.stdout_lines,
        vec!["salve".to_owned()],
        "library const data member must print its value oracle"
    );
}

#[test]
fn package_mir_non_function_member_call_fails_closed() {
    let dir = test_temp_dir("fhir-non-function-shape");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-non-function"
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
    fs::write(src.join("util.fab"), CONST_MEMBER_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), CONST_MEMBER_CALL_MAIN).expect("write main.fab");

    // U1 done_when (4): calling a const data member (`utilModule.VALUE()`) is
    // an unsupported shape and must keep the non-function fail-closed
    // diagnostic instead of linking or passing silently. The const member is
    // never a callable verb: it lives only in the data-member target table.
    let mut host = BufferHost::default();
    let result = run_package_mir(&Config::default(), &src.join("main.fab"), &mut host);
    match result {
        Err(diagnostics) => {
            assert!(
                diagnostics.iter().any(|diag| {
                    diag.message
                        .contains("package MIR does not yet support non-function namespace member `VALUE`")
                }),
                "expected non-function namespace member diagnostic, got {diagnostics:?}"
            );
        }
        Ok(()) => panic!("call to a const data member must fail closed"),
    }
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
    let latin = radix::locale::latin_locale_pack();
    let surface = radix::locale::KeywordSurface::new(&latin);
    for target in [Target::HirRust, Target::HirFaber] {
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
        assert_eq!(
            loaded_code, direct_code,
            "library-import parity for {target}"
        );
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
