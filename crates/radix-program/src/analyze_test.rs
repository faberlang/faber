use super::*;
use crate::discovery::PackageSpec;
use crate::library::LibraryResolver;
use radix_module::file_interface::FileExportKind;
use radix_module::semantic::passes::visibility_annotation::VisibilityTier;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "radix-program-analyze-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

// Test seams: no product manifest validation, no library home, no linked
// crates — the compiler-owned analysis path exercised standalone (faber
// supplies the real product seams in production). The locale seam mirrors the
// faber product default: the product-default English pack is loaded when the
// caller selects no locale (pack-less registry-direct lexing is not the
// package analysis route).
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

#[test]
fn package_test_analysis_deadline_fails_closed() {
    let dir = temp_root("analysis-deadline");
    let entry = dir.join("main.fab");
    fs::write(&entry, "functio value() → numerus { redde 1 }\n").expect("write source");
    let spec = discover_package(&entry).expect("discover package");
    let resolver = LibraryResolver::with_optional_home(None);
    let expired = Instant::now() - Duration::from_secs(1);
    let config = Config::default().with_locale_pack(radix_module::locale::latin_locale_pack());

    let diagnostics = analyze_package_spec_with_deadline(
        &config,
        spec,
        &resolver,
        true,
        None,
        Some(expired),
        &test_seams(),
    );
    let diagnostics = match diagnostics {
        Ok(_) => panic!("expired package test analysis must fail closed"),
        Err(diagnostics) => diagnostics,
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("package_analysis_budget_exceeded")),
        "unexpected diagnostics: {diagnostics:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Moved from `crates/faber/src/package_test.rs`
/// (`package_analysis_api_exposes_multifile_units_without_rust_emit`): the
/// production analysis API exposes multi-unit shape (entry identity, module
/// segments, export surface, namespace exports) without any backend emit.
#[test]
fn analyze_package_exposes_multifile_units_without_rust_emit() {
    let dir = temp_root("analysis-api");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    let module = dir.join("thing.fab");
    fs::write(
        &module,
        r#"
@ public
fn label() → string {
  return "ok"
}
"#,
    )
    .expect("write module");

    let package =
        analyze_package(&Config::default(), &entry, &test_seams()).expect("analyze package");

    assert!(
        !package
            .diagnostics
            .iter()
            .any(radix_module::Diagnostic::is_error),
        "expected no analysis errors, got {:?}",
        package
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(package.nodes.len(), 2);
    assert_eq!(package.roots.len(), 1);
    assert!(package.entry.is_some());
    let entry_unit = package.entry_node().expect("entry unit");
    assert_eq!(entry_unit.path, entry);
    assert!(entry_unit.analysis.hir.entry.is_some());
    assert_eq!(
        entry_unit.namespace_exports.get("thing"),
        Some(&BTreeMap::from([(
            "label".to_owned(),
            VisibilityTier::Publica
        )]))
    );

    let module_unit = package
        .nodes
        .values()
        .find(|unit| !unit.is_entry)
        .expect("module unit");
    assert_eq!(module_unit.path, module);
    assert_eq!(module_unit.module_segments, vec!["thing".to_owned()]);
    assert!(module_unit.analysis.hir.entry.is_none());
    assert_eq!(module_unit.export_names, vec!["label".to_owned()]);
    assert!(matches!(
        module_unit
            .file_interface
            .exports
            .get("label")
            .map(|export| &export.kind),
        Some(FileExportKind::Function(_))
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// The loader mints provider carriage for every resolved import edge
/// (ARCH-U4): the entry unit's view carries the local sibling under its
/// import-path spelling. The package declares its reader locale because the
/// provider re-analysis reads the target package's locale, not the session's
/// (see `provider_unit_for_target`).
#[test]
fn analyze_package_carries_provider_unit_for_local_import() {
    let dir = temp_root("provider-carriage");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "providercarriage"
version = "0.1.0"

[locale]
locale = "en"

[paths]
source = "."
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
@ public
fn label() → string {
  return "ok"
}
"#,
    )
    .expect("write module");

    let package =
        analyze_package(&Config::default(), &entry, &test_seams()).expect("analyze package");
    let entry_unit = package.entry_node().expect("entry unit");
    let carried = entry_unit
        .analysis
        .provider_units
        .get("./thing")
        .expect("local import edge carries its provider unit");
    match carried {
        CarriedProviderUnit::Unit(_) => {}
        CarriedProviderUnit::Failed { message } => panic!("provider carriage failed: {message}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Negative parity for the analysis diagnostics: a semantic error in a
/// sibling unit surfaces through multi-unit analysis as a package failure
/// (cross-file diagnostic propagation, not a silent skip).
#[test]
fn analyze_package_surfaces_sibling_unit_diagnostics() {
    let dir = temp_root("sibling-diag");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./lib" lib

main {
  print lib.broken()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("lib.fab"),
        r"
@ public
fn broken() → int {
  return nonexistent_value
}
",
    )
    .expect("write module");

    let entry = dir.join("main.fab");
    let Err(diagnostics) = analyze_package(&Config::default(), &entry, &test_seams()) else {
        panic!("sibling semantic error must fail multi-unit analysis");
    };
    assert!(
        diagnostics.iter().any(radix_module::Diagnostic::is_error),
        "expected the sibling error diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Binary programs expose one entry ModuleId, one root, and local import edges.
#[test]
fn analyze_package_records_entry_roots_and_local_import_edges() {
    let dir = temp_root("program-graph");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
@ public
fn label() → string {
  return "ok"
}
"#,
    )
    .expect("write module");

    let program =
        analyze_package(&Config::default(), &entry, &test_seams()).expect("analyze package");
    let entry_id = program.entry.clone().expect("binary entry");
    assert_eq!(program.roots, vec![entry_id.clone()]);
    assert!(program.nodes.get(&entry_id).expect("entry node").is_entry);
    let thing_id = program
        .nodes
        .iter()
        .find(|(_, node)| node.module_segments == ["thing".to_owned()])
        .map(|(id, _)| id.clone())
        .expect("thing node");
    let local = program.imports.local_edges(&entry_id);
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].0, "thing");
    assert_eq!(local[0].1, thing_id);
    let _ = fs::remove_dir_all(&dir);
}

/// Entry-less library packages keep multiple roots and no executable entry.
#[test]
fn analyze_package_library_roots_have_no_entry() {
    let dir = temp_root("library-roots");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "libroots"
version = "0.1.0"

[library]
provider = "libroots"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fhir"]
"#,
    )
    .expect("write manifest");
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::write(
        dir.join("src/alpha.fab"),
        r"
@ public
fn alpha() → int {
  return 1
}
",
    )
    .expect("write alpha");
    fs::write(
        dir.join("src/beta.fab"),
        r"
@ public
fn beta() → int {
  return 2
}
",
    )
    .expect("write beta");

    let program =
        analyze_package(&Config::default(), &dir, &test_seams()).expect("analyze library");
    assert!(program.entry.is_none(), "library has no executable entry");
    assert!(
        program.roots.len() >= 2,
        "library keeps multiple roots, got {:?}",
        program.roots
    );
    assert!(program.nodes.values().all(|node| !node.is_entry));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn analyze_package_for_tests_named_proba_skips_sibling() {
    let dir = temp_root("named-proba");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "namedproba"
version = "0.1.0"

[library]
provider = "namedproba"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fhir"]
"#,
    )
    .expect("write manifest");
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::write(
        dir.join("src/lib.fab"),
        r"
@ public
fn n() → int {
  return 1
}
",
    )
    .expect("write lib");
    fs::write(
        dir.join("src/cache.proba"),
        r#"
test "cache" {
  assert 1 ≡ 1
}
"#,
    )
    .expect("write cache.proba");
    fs::write(
        dir.join("src/decode.proba"),
        r#"
test "decode" {
  assert 1 ≡ 1
}
"#,
    )
    .expect("write decode.proba");

    let program = analyze_package_for_tests(
        &Config::default(),
        &dir.join("src/cache.proba"),
        None,
        &test_seams(),
    )
    .expect("analyze named proba");
    let proba_names: Vec<_> = program
        .nodes
        .values()
        .filter_map(|node| {
            node.path
                .extension()
                .and_then(|ext| ext.to_str())
                .filter(|ext| *ext == "proba")
                .and_then(|_| node.path.file_name())
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(proba_names, vec!["cache.proba".to_owned()]);
    let _ = fs::remove_dir_all(&dir);
}

/// PKG-ADM green half: package analysis registers the unit's own publica
/// export surface as kernel-template admission before the per-unit orphan
/// check, so a package library module exporting a size-generic
/// `@ kernel @ public` template with no same-unit caller analyzes clean —
/// the package API is not mistaken for an orphan.
#[test]
fn analyze_package_admits_exported_generic_kernel_template() {
    let dir = temp_root("pkg-adm-admit");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "pkgadmadmit"
version = "0.1.0"

[library]
provider = "pkgadmadmit"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fhir"]
"#,
    )
    .expect("write manifest");
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::write(
        dir.join("src/math.fab"),
        r"
@ kernel
@ public
fn add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
  return a + b
}
",
    )
    .expect("write math");

    let program = analyze_package(
        &Config::default().with_target(radix_module::Target::MirWgsl),
        &dir,
        &test_seams(),
    )
    .expect("exported generic kernel template is demand-admitted");

    assert!(
        !program
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("nucleum_orphaned_annotation")),
        "no ADM001 for the exported template: {:?}",
        program.diagnostics
    );
    let math_unit = program
        .nodes
        .values()
        .find(|unit| unit.path.ends_with("math.fab"))
        .expect("math unit analyzed");
    assert_eq!(math_unit.export_names, vec!["add".to_owned()]);
    assert!(math_unit.analysis.hir.entry.is_none());
    let _ = fs::remove_dir_all(&dir);
}

/// PKG-ADM converse: admission is the package export fact, not a blanket
/// public-annotation exemption — a `@ private` (module-private) size-generic
/// kernel with no same-unit caller is absent from the export surface and
/// stays an ADM001 orphan through package analysis.
#[test]
fn analyze_package_keeps_unexported_generic_kernel_orphaned() {
    let dir = temp_root("pkg-adm-orphan");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "pkgadmorphan"
version = "0.1.0"

[library]
provider = "pkgadmorphan"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fhir"]
"#,
    )
    .expect("write manifest");
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::write(
        dir.join("src/math.fab"),
        r"
@ kernel
@ private
fn add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
  return a + b
}
",
    )
    .expect("write math");

    let diagnostics = match analyze_package(
        &Config::default().with_target(radix_module::Target::MirWgsl),
        &dir,
        &test_seams(),
    ) {
        Err(diagnostics) => diagnostics,
        Ok(_) => panic!("unexported generic kernel stays an orphan"),
    };
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("nucleum_orphaned_annotation")),
        "ADM001 must survive for the unexported template: {diagnostics:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Resolver whose only provider (`web`) is locked to an empty interface root,
/// so `web:nope` deterministically yields `PKG001:unknown_library_module`
/// without touching a library home.
fn locked_empty_provider_resolver(root: &Path) -> LibraryResolver {
    let interface_root = root.join("vendor");
    fs::create_dir_all(&interface_root).expect("mkdir vendor");
    let mut locked = BTreeMap::new();
    locked.insert(
        "web".to_owned(),
        crate::library::LockedLibraryPackage {
            name: "web".to_owned(),
            version: "0.1.0".to_owned(),
            interface_root,
        },
    );
    LibraryResolver::with_optional_home(None).with_package_lock(BTreeMap::new(), locked)
}

fn unknown_library_module_diagnostic(source: &str, label: &str) -> Diagnostic {
    let dir = temp_root(label);
    let entry = dir.join("main.fab");
    fs::write(&entry, source).expect("write entry");
    let spec = discover_package(&entry).expect("discover package");
    let resolver = locked_empty_provider_resolver(&dir);

    let diagnostics = match analyze_package_spec_with_deadline(
        &Config::default(),
        spec,
        &resolver,
        false,
        None,
        None,
        &test_seams(),
    ) {
        Ok(_) => panic!("unknown library module must fail package analysis"),
        Err(diagnostics) => diagnostics,
    };
    let _ = fs::remove_dir_all(&dir);
    diagnostics
        .into_iter()
        .find(|diag| diag.issue() == Some("unknown_library_module"))
        .expect("unknown_library_module diagnostic")
}

/// The compact renderer prints `file:line`; a bare byte span printed
/// `file:<byte offset>` (`main.fab:474` for a 100-line file).
#[test]
fn unknown_library_module_diagnostic_names_the_import_line() {
    let diagnostic = unknown_library_module_diagnostic(
        "\n\nimport from \"web:nope\" nope\nmain {}\n",
        "unknown-module-line",
    );

    assert_eq!(diagnostic.code, Some("PKG001"));
    assert_eq!(diagnostic.line, Some(3), "{diagnostic:?}");
    assert_eq!(
        diagnostic.display_location(),
        format!("{}:3", diagnostic.file)
    );
}

/// Spans are body-relative; frontmatter lines must be added back so the
/// reported line is the physical line in the file.
#[test]
fn unknown_library_module_diagnostic_line_counts_frontmatter() {
    let diagnostic = unknown_library_module_diagnostic(
        "+++\nlocale = \"en\"\n+++\n\nimport from \"web:nope\" nope\nmain {}\n",
        "unknown-module-frontmatter-line",
    );

    assert_eq!(diagnostic.code, Some("PKG001"));
    assert_eq!(diagnostic.line, Some(5), "{diagnostic:?}");
    assert_eq!(
        diagnostic.display_location(),
        format!("{}:5", diagnostic.file)
    );
}
