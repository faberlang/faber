//! Tests for FHIR package assembly, loading, loaded-package adaptation, and
//! HIR target parity (Rust, canonical Faber, FMIR run) from a loaded artifact.

use super::*;
use crate::package::library_resolver_from_config;
use crate::package::test_support::{diagnostic_has_arg, test_temp_dir};
use radix::program::analyze::AnalyzedProgram;
use radix::program::codegen::assemble_crate;
use radix::program::rust_target::generate_package_rust;

fn analyze_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<AnalyzedProgram, Vec<radix::module::diagnostics::Diagnostic>> {
    radix::program::analyze::analyze_package(config, input, &crate::package::faber_analysis_seams())
}

fn build_fhir_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<radix::program::fhir::PackageFhir, Vec<radix::module::diagnostics::Diagnostic>> {
    radix::program::fhir::build_package_fhir(config, input, &crate::package::faber_analysis_seams())
}

fn run_mir_with_faber_seams<H: radix::module::mir::Host + ?Sized>(
    config: &Config,
    input: &Path,
    host: &mut H,
) -> Result<(), Vec<radix::module::diagnostics::Diagnostic>> {
    radix::program::mir::run_package_mir(
        config,
        input,
        &crate::package::mir::faber_mir_seams(),
        host,
    )
}

use faber_hir_rust::RustFieldNamePolicy;
use radix::module::codegen::{Target, generate_from_analyzed};
use radix::module::driver::Config;
use radix::module::hir::{LibraryItemKind, LibraryProvider};
use radix::module::locale::LocalePack;
use radix::module::mir::BufferHost;
use std::fs;
use std::path::{Path, PathBuf};

const LOCAL_MAIN: &str = "import from \"./util\" * as utilModule\n\nfn run() → string {\n    return utilModule.salutare()\n}\n\nmain {\n    print utilModule.salutare()\n}\n";
const LOCAL_UTIL: &str = "@ public\nfn salutare() → string {\n    return \"salve\"\n}\n";

const CONST_MEMBER_MAIN: &str = "import from \"./util\" * as utilModule\n\nfn run() → string {\n    return utilModule.VALUE\n}\n\nmain {\n    print utilModule.VALUE\n}\n";
const CONST_MEMBER_UTIL: &str = "@ public\nconst string VALUE = \"salve\"\n";

const LIBRARY_CONST_MAIN: &str = "import from \"constlib:vals\" vals\n\nfn run() → string {\n    return vals.LIBRARY_VALUE\n}\n\nmain {\n    print vals.LIBRARY_VALUE\n}\n";
const LIBRARY_CONST_VALS: &str =
    "+++\nlocale = \"en\"\n+++\n@ public\nconst string LIBRARY_VALUE = \"salve\"\n";

/// Entry that CALLS a const data member — an unsupported shape that must
/// keep the non-function fail-closed diagnostic (U1 `done_when` 4).
const CONST_MEMBER_CALL_MAIN: &str =
    "import from \"./util\" * as utilModule\n\nmain {\n    print utilModule.VALUE()\n}\n";

const LOCAL_LOCALE_MAIN: &str = "import from \"./util\" * as utilModule\n\nfn run() → string {\n    return utilModule.greet()\n}\n\nmain {\n    print utilModule.greet()\n}\n";
const LOCAL_LOCALE_UTIL: &str = "@ public\nfn salutare() → string {\n    return \"salve\"\n}\n";

// The `triga:` provider is an inline fixture library (see
// `fixture_library_home`), not the live triga sibling, so a sibling's API
// churn cannot break this test. The fixture names are source-canonical.
const EXTERNAL_LIBRARY_LOCALE_MAIN: &str = "import from \"triga:math\" math\nimport from \"norma:text\" text\n\nfn run() → string {\n    const math.Point3 origin ← math.point3(1.0, 0.0, 0.0)\n    const string reversed ← text.reverse(\"abc\")\n    const string joined ← text.join([reversed, \"abc\"], \",\")\n    print joined\n    return joined\n}\n\nmain {\n    print run()\n}\n";
const NORMA_LATIN_COMPATIBILITY_MAIN: &str = "importa ex \"norma:text\" chorda\n\nfunctio run() → textus {\n    redde chorda.retorta(\"abc\")\n}\n\nincipit {\n    nota run()\n}\n";

/// A library home with the real Norma stdlib (symlinked) and an inline
/// `triga` source library exporting a stable `Point3` genus and `point3`
/// constructor. The test proves the source-library projection, not triga's
/// live public surface.
fn fixture_library_home(dir: &Path) -> PathBuf {
    let home = dir.join("library-home");
    let triga_src = home.join("triga/src");
    fs::create_dir_all(&triga_src).expect("create fixture triga src");
    fs::write(
        home.join("triga/faber.toml"),
        "[package]\nname = \"triga\"\nversion = \"0.0.0\"\nedition = \"2026\"\n\n[library]\nprovider = \"triga\"\n\n[paths]\nsource = \"src\"\n\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n\n[locale]\nlocale = \"en\"\n",
    )
    .expect("write fixture triga manifest");
    fs::write(
        triga_src.join("math.fab"),
        "@ public\nclass Point3 {\n    var f32 x\n    var f32 y\n    var f32 z\n}\n\n@ public\nfn point3(f32 x, f32 y, f32 z) → Point3 {\n    return Point3 { x = x, y = y, z = z }\n}\n",
    )
    .expect("write fixture triga math");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        radix::program::library_home()
            .expect("library home resolves in-tree")
            .join("norma"),
        home.join("norma"),
    )
    .expect("link norma into the fixture library home");
    home
}

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
/// execute through package MIR (U1 `done_when` 2). The library lives under a
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
    fs::write(library.join("src/vals.fab"), LIBRARY_CONST_VALS)
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

# S2-U2 pack-authoritative gate: a non-la reader pack owns its keyword and
# type surfaces explicitly. The fixtures below use the en surfaces, so the
# test-local pack declares them (the test's intent is the `[[library_members]]`
# surface row `salutare` → `greet`, not keyword gating).
[keywords]
publica = "public"
importa = "import"
ex = "from"
privata = "private"
ut = "as"
functio = "fn"
redde = "return"
incipit = "main"
nota = "print"

[types]
textus = "string"

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
    fs::write(dir.join("locale/local-library.fab"), "main {}\n")
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
        r#"union Forma {
    Numerus { int v },
    Textus { string v }
}

fn describe(Forma f) → string {
    match f {
        case Numerus const v { return "n: §"(v) }
        case Textus const v { return "t: §"(v) }
    }
}

fn idem<T>(T x) → T {
    return x
}

main {
    print describe(variant Numerus { v = 7 } ∷ Forma)
    print idem(42)
}
"#,
    )
    .expect("write main.fab");
    src.join("main.fab")
}

/// Library-import package (imports `norma:fs`). `declared` controls whether
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
        let home = radix::program::library_home().expect("library home resolves in-tree");
        let home = home.display();
        let lock = format!(
            "[[package]]\nname = \"norma\"\nversion = \"1.0.0\"\nsource = \"path:{home}/norma\"\npackage_root = \"{home}/norma\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{home}/norma/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
        );
        fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    }
    fs::write(dir.join("faber.toml"), manifest).expect("write faber.toml");
    fs::write(
        src.join("main.fab"),
        "import from \"norma:fs\" fs\n\nfn scinde(string s) → bool {\n    return fs.exists(s)\n}\n\nmain {\n    print scinde(\"a,b,c\")\n}\n",
    )
    .expect("write main.fab");
    src.join("main.fab")
}

/// Extract the code string from a codegen [`radix::module::Output`].
fn output_code(output: &radix::module::Output) -> &str {
    match output {
        radix::module::Output::Rust(o) => &o.code,
        radix::module::Output::Faber(o) => &o.code,
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
    let artifact =
        build_fhir_with_faber_seams(&Config::default(), &entry).expect("build FHIR package");
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
    // D5.1: the entry's unmarked `fn run()` is module-private, so the entry
    // exports nothing; the imported module's `@ public fn salutare()` is the
    // export surface the round trip must preserve.
    assert!(main.export_names.is_empty());

    let util = loaded
        .modules
        .iter()
        .find(|module| module.relative_path == "src/util.fab")
        .expect("util module");
    assert!(!util.is_entry);
    assert!(util.local_links.is_empty());
    assert_eq!(util.export_names, vec!["salutare".to_owned()]);
}

#[test]
fn local_library_locale_surface_resolves_through_package_loader() {
    let dir = test_temp_dir("fhir-local-locale");
    let entry = write_local_locale_package(&dir);
    let (config, _diagnostic_pack) = crate::package::config_with_locale(
        radix::module::codegen::Target::HirRust,
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
    let mut package =
        analyze_with_faber_seams(&config, &entry).expect("analyze localized local package");

    assert!(
        package
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "localized local package diagnostics: {:?}",
        package.diagnostics
    );

    let util = package
        .nodes
        .values()
        .find(|unit| unit.module_segments == ["util".to_owned()])
        .expect("localized local utility unit");
    assert_eq!(
        util.file_interface.identity,
        Some(radix::module::file_interface::InterfaceLibraryIdentity {
            provider: "package".to_owned(),
            package: Some("local-locale-fixture".to_owned()),
            module_path: vec!["util".to_owned()],
        })
    );
    assert!(
        util.file_interface.exports.contains_key("salutare"),
        "local interface must retain canonical export names"
    );

    // The locale projection must stay atomic through Rust emission: the
    // entry unit calls the canonical `salutare` implementation, never the
    // consumer-locale surface spelling `greet`.
    let resolver = library_resolver_from_config(&config);
    let generated = generate_package_rust(
        &mut package,
        &resolver,
        None,
        RustFieldNamePolicy::Preserve,
        None,
        crate::package::locale::load_locale_pack_for_input,
    );
    assert!(
        generated.diagnostics.iter().all(|diag| !diag.is_error()),
        "localized package Rust generation failed: {:?}",
        generated.diagnostics
    );
    let entry_code = generated.entry_code.expect("localized entry code");
    assert!(
        entry_code.contains("salutare"),
        "emitted Rust must call the canonical `salutare` implementation:\n{entry_code}"
    );
    assert!(
        !entry_code.contains("greet"),
        "consumer-locale surface spelling must not leak into emitted Rust:\n{entry_code}"
    );
    let assembled = assemble_crate(&entry_code, &generated.module_tree.render(0));
    assert!(
        assembled.contains("salutare"),
        "assembled crate must carry the canonical `salutare` implementation:\n{assembled}"
    );
}

#[test]
fn strict_pack_rejects_unlisted_canonical_member_spelling() {
    let dir = test_temp_dir("fhir-local-locale-strict");
    let entry = write_local_locale_package(&dir);
    // The strict `en-local-library` pack lists only the `greet` surface for
    // the canonical `salutare` member. The canonical spelling must not be
    // accepted as an implicit alias at the source boundary.
    fs::write(
        dir.join("src/main.fab"),
        "import from \"./util\" * as utilModule\n\nfn run() → string {\n    return utilModule.salutare()\n}\n\nmain {\n    print utilModule.salutare()\n}\n",
    )
    .expect("write canonical-spelling consumer");
    let (config, _diagnostic_pack) = crate::package::config_with_locale(
        radix::module::codegen::Target::HirRust,
        &entry,
        Some("en-local-library"),
        None,
    )
    .expect("load local library locale through package selection");
    let diagnostics = match analyze_with_faber_seams(&config, &entry) {
        Ok(package) => package.diagnostics,
        Err(diagnostics) => diagnostics,
    };
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == Some("SEM004")
                && diag.issue() == Some("namespace_missing_export")),
        "canonical spelling must be rejected under the strict pack: {:?}",
        diagnostics
            .iter()
            .map(|d| (d.code, d.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn english_pack_resolves_source_library_and_norma_surfaces() {
    // 80b59a6c(a) / CTO e2ca2b57 F2: en pack has zero triga library_members;
    // analysis projects the exports of a source library under the `triga`
    // provider (an inline fixture, so the live triga surface cannot break
    // this). Norma surfaces stay the en identity rows.
    let dir = test_temp_dir("fhir-external-library-locale");
    let entry = write_external_library_locale_package(&dir, EXTERNAL_LIBRARY_LOCALE_MAIN);
    let (config, _diagnostic_pack) = crate::package::config_with_locale(
        radix::module::codegen::Target::HirRust,
        &entry,
        Some("en"),
        None,
    )
    .expect("load installed English library locale");
    let config = config.with_library_home(fixture_library_home(&dir));
    let en_pack = config
        .locale_pack
        .as_ref()
        .expect("en pack selected for analysis");
    assert!(
        en_pack
            .library_members
            .keys()
            .all(|identity| identity.package.as_deref() != Some("triga")),
        "ruling 80b59a6c(a): en pack must have zero triga library_members"
    );
    assert!(
        en_pack.library_members.is_empty(),
        "language packs must not embed library members"
    );
    let package =
        analyze_with_faber_seams(&config, &entry).expect("analyze ruled external library surfaces");

    assert!(
        package
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "ruled external library diagnostics: {:?}",
        package.diagnostics
    );
    let entry_unit = package.entry_node().expect("external library entry unit");
    assert_eq!(
        entry_unit.expanded_library_imports.len(),
        2,
        "the consumer must resolve both source-library imports"
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
                && item.exported_name == "point3"
                && item.kind == LibraryItemKind::Function
        }),
        "projected triga point3: {library_items:?}"
    );
    assert!(
        library_items.iter().any(|item| {
            item.identity.provider == LibraryProvider::Package("triga".to_owned())
                && item.identity.module_path == ["math".to_owned()]
                && item.exported_name == "Point3"
                && item.kind == LibraryItemKind::Struct
        }),
        "projected triga Point3: {library_items:?}"
    );
    assert!(
        library_items.iter().any(|item| {
            item.identity.provider == LibraryProvider::Builtin("norma".to_owned())
                && item.identity.module_path == ["text".to_owned()]
                && item.exported_name == "reverse"
                && item.kind == LibraryItemKind::Function
        }),
        "en-pack norma reverse: {library_items:?}"
    );
}

#[test]
fn latin_norma_consumer_keeps_canonical_surface_compatibility() {
    let dir = test_temp_dir("fhir-norma-latin-compatibility");
    let entry = write_external_library_locale_package(&dir, NORMA_LATIN_COMPATIBILITY_MAIN);
    // The fixture is a Latin-identity consumer (canonical `norma:text`
    // surface): compile it under the explicit la code surface — the la pack's
    // registry-direct Latin authority — mirroring the R3 conversio-assign
    // precedent (Latin sources declare la identity, they are not en-migrated).
    let la_pack_path = radix::module::locale::workspace_locale_pack_path("la");
    let la_pack = LocalePack::from_toml_path(la_pack_path).expect("load la locale pack");
    let config = Config::default().with_locale_pack(la_pack);
    let package = analyze_with_faber_seams(&config, &entry)
        .expect("analyze Latin Norma compatibility consumer");

    assert!(
        package
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "Latin Norma compatibility diagnostics: {:?}",
        package.diagnostics
    );
    let entry_unit = package.entry_node().expect("Latin Norma entry unit");
    assert_eq!(
        entry_unit.expanded_library_imports.len(),
        1,
        "the Latin consumer must resolve the canonical Norma import"
    );
    assert!(
        entry_unit.analysis.libraries.items.values().any(|item| {
            item.identity.provider == LibraryProvider::Builtin("norma".to_owned())
                && item.identity.module_path == ["text".to_owned()]
                && item.exported_name == "reverse"
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
    let first = build_fhir_with_faber_seams(&Config::default(), &entry).expect("first build");
    let first_bytes = fs::read(&first.package_path).expect("read first artifact");
    let second = build_fhir_with_faber_seams(&Config::default(), &entry).expect("second build");
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

    let mut direct = analyze_with_faber_seams(&config, &entry).expect("direct package analysis");
    let generated = generate_package_rust(
        &mut direct,
        &resolver,
        None,
        RustFieldNamePolicy::Preserve,
        None,
        crate::package::locale::load_locale_pack_for_input,
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

    let artifact = build_fhir_with_faber_seams(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    let links = loaded_links_by_unit_path(&loaded, &artifact.root);
    let mut adapted =
        loaded_package_to_analyzed(loaded, &artifact.root).expect("adapt loaded package");
    let generated_loaded = generate_package_rust(
        &mut adapted,
        &crate::library::default_library_resolver(),
        None,
        RustFieldNamePolicy::Preserve,
        Some(&links),
        crate::package::locale::load_locale_pack_for_input,
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

    let mut direct = analyze_with_faber_seams(&config, &entry).expect("direct analysis");
    let generated = generate_package_rust(
        &mut direct,
        &resolver,
        None,
        RustFieldNamePolicy::Preserve,
        None,
        crate::package::locale::load_locale_pack_for_input,
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

    let artifact = build_fhir_with_faber_seams(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    let links = loaded_links_by_unit_path(&loaded, &artifact.root);
    let mut adapted =
        loaded_package_to_analyzed(loaded, &artifact.root).expect("adapt loaded package");
    let generated_loaded = generate_package_rust(
        &mut adapted,
        &crate::library::default_library_resolver(),
        None,
        RustFieldNamePolicy::Preserve,
        Some(&links),
        crate::package::locale::load_locale_pack_for_input,
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

    let direct = analyze_with_faber_seams(&config, &entry).expect("direct analysis");
    let artifact = build_fhir_with_faber_seams(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    assert_eq!(direct.nodes.len(), loaded.modules.len());

    // Direct units are in analysis (dependency-first) order; loaded modules
    // are sorted by canonical path — align by package-relative path.
    let package_root = &direct.spec.package_root;
    let direct_by_path = direct
        .nodes
        .values()
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

    let latin = radix::module::locale::latin_locale_pack();
    let surface = radix::module::locale::KeywordSurface::new(&latin);
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
    run_mir_with_faber_seams(&config, &entry, &mut direct_host).expect("direct FMIR run");
    let direct_lines = direct_host.stdout_lines.clone();

    let artifact = build_fhir_with_faber_seams(&config, &entry).expect("build FHIR package");
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
    run_mir_with_faber_seams(&config, &entry, &mut direct_host).expect("direct FMIR run");
    let direct_lines = direct_host.stdout_lines.clone();

    let artifact = build_fhir_with_faber_seams(&config, &entry).expect("build FHIR package");
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
    let config = Config::default().with_library_home(dir.join("libhome"));

    // U1 done_when (2): a linked-library const data member links and executes
    // through the direct package-MIR path.
    let mut host = BufferHost::default();
    run_mir_with_faber_seams(&config, &entry, &mut host).expect("library const FMIR run");
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
    let result = run_mir_with_faber_seams(&Config::default(), &src.join("main.fab"), &mut host);
    match result {
        Err(diagnostics) => {
            assert!(
                diagnostics.iter().any(|diag| {
                    diagnostic_has_arg(diag, "reason", "non_function_namespace_member")
                        && diagnostic_has_arg(diag, "member", "VALUE")
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
    let config = Config::default();

    let direct = analyze_with_faber_seams(&config, &entry).expect("direct analysis with norma");
    assert!(
        direct.diagnostics.iter().all(|diag| !diag.is_error()),
        "direct analysis diagnostics: {:?}",
        direct.diagnostics
    );

    let artifact = build_fhir_with_faber_seams(&config, &entry).expect("build FHIR package");
    let loaded = load_package_fhir(&artifact.package_path).expect("load FHIR package");
    assert_eq!(loaded.modules.len(), 1);
    assert_eq!(
        loaded.modules[0].library_imports.len(),
        1,
        "norma:fs library import survives the envelope"
    );

    // Per-unit codegen parity (Rust + canonical Faber) with the library
    // import intact. Full crate assembly inlines library module bodies, which
    // is the store-backed Stage 5 path; the unit surface is source-free here.
    let latin = radix::module::locale::latin_locale_pack();
    let surface = radix::module::locale::KeywordSurface::new(&latin);
    for target in [Target::HirRust, Target::HirFaber] {
        let direct_code = output_code(
            &generate_from_analyzed(
                target,
                &direct.nodes.values().next().expect("node").analysis,
                &surface,
            )
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
    let config = Config::default();

    // Analysis succeeds (norma resolves via the dev library home) and the
    // envelope builds, but the manifest declares no dependency record for
    // norma — the adapter must reject the load before any codegen.
    let artifact = build_fhir_with_faber_seams(&config, &entry).expect("build FHIR package");
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
