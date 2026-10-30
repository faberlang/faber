//! Provider-import contract tests (Triga SEM002 residual).

use super::import_contract_for_file;
use super::package_identity_facts_for_path;
use super::reader_locale_from_manifest;
use crate::driver::AnalyzeRequest;
use crate::file_interface::{
    FileExportKind, InterfaceBodyChannel, InterfaceDeviceRole, InterfaceDeviceVisibility, InterfaceIndexSnapshot,
    InterfaceParam, InterfaceTypeSnapshot,
};
use crate::import_resolve::library_home_test_support;
use crate::import_resolve::resolve_namespace_import;
use crate::semantic::{NumericWidth, Primitive, SemanticParamMode};
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn parse_reader_locale_accepts_toml_whitespace_and_inline_comment() {
    assert_eq!(
        reader_locale_from_manifest("[locale]\nlocale = \"en\" # package locale\n").as_deref(),
        Some("en")
    );
    assert_eq!(
        reader_locale_from_manifest("[ reader ]\nlocale = \"la\"\n").as_deref(),
        Some("la")
    );
}

fn english_config() -> crate::driver::Config {
    let path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(path).expect("load English locale pack");
    crate::driver::Config::default().with_resolved_locale_pack(pack)
}

fn temp_dir(label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(&format!("provider-{label}"));
    let path = dir.path().to_path_buf();
    (dir, path)
}

/// Thread-local library home override: isolates provider-import tests
/// without mutating a process-global library home,
/// so parallel vertex-source tests never see a transient temp path.
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

#[test]
fn emit_route_imported_generic_call_size_inference() {
    let (_td, home) = temp_dir("gmt-emit-inference");
    let provider = home.join("probevectors");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probevectors"
[library]
provider = "probevectors"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/vectors.fab"),
        r#"@ public
class Box<size N> {
    mut vector<f32, size N> minima
    mut vector<f32, size N> maxima
}

@ public
fn add<size N>(vector<f32, size N> a, vector<f32, size N> b) → vector<f32, size N> {
    return a.added(b)
}

@ public
fn dot<size N>(vector<f32, size N> a, vector<f32, size N> b) → f32 {
    return a.dot(b)
}

@ public
fn cross(vector<f32, size 3> a, vector<f32, size 3> b) → vector<f32, size 3> {
    return a.cross(b)
}

@ public
fn lane(vector<f32, size 3> a) → f32 {
    let vector<f32, size 3> swapped ← a.swizzle("zyx")
    return swapped.dot(a)
}

@ public
fn multiply(matrix<f32, [3, 2]> a, matrix<f32, [2, 3]> b) → matrix<f32, [3, 3]> {
    return a · b
}

@ public
fn box_minima<size N>(Box<N> b) → vector<f32, size N> {
    return b.minima
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "en"
+++

import from "probevectors:vectors" gmt

fn imported() → f32 {
    let vector<f32, size 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>
    let vector<f32, size 3> b ← [2.0, 4.0, 6.0] ↦ vector<f32, size 3>
    let vector<f32, size 3> s ← gmt.add(a, b)
    let vector<f32, size 3> x ← gmt.cross(a, b)
    return (s.dot(b) + x.dot(a)) + gmt.lane(a)
}

fn imported_signature() → vector<f32, size 4> {
    let vector<f32, size 4> a ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, size 4>
    let vector<f32, size 4> b ← [2.0, 4.0, 6.0, 8.0] ↦ vector<f32, size 4>
    return gmt.add(a, b)
}

fn imported_shapes() → matrix<f32, [3, 3]> {
    let vector<f32, size 3> s ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>
    let vector<f32, size 2> narrow ← [1.0, 2.0] ↦ vector<f32, size 2>
    let matrix<f32, [3, 2]> outer ← s ⊗ narrow
    let matrix<f32, [2, 3]> inner ← narrow ⊗ s
    return gmt.multiply(outer, inner)
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("gmt")
        .expect("gmt interface installed");

    // The emit-route snapshot must carry the size-parametric callables —
    // vector/matrix-typed exports were previously dropped from the interface,
    // so the consumer's `gmt.add` fell back to an Ignotum result and the
    // `SEM010`/`SEM011` cascade fired (S0-G6).
    for member in ["add", "dot", "cross", "lane", "multiply"] {
        assert!(
            interface.exports.contains_key(member),
            "export `{member}` must survive the emit-route snapshot, got {:?}",
            interface.exports.keys().collect::<Vec<_>>()
        );
    }
    let FileExportKind::Function(add) = &interface.exports["add"].kind else {
        panic!("add must snapshot as a function export");
    };
    let add_param_names: Vec<&str> = add
        .type_params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(add_param_names, vec!["N"]);
    assert!(
        add.type_params
            .iter()
            .all(|param| param.kind == crate::syntax::GenericParamKind::Magnitudo)
    );
    assert_eq!(
        add.params,
        vec![
            InterfaceParam {
                ty: InterfaceTypeSnapshot::Vector(
                    Box::new(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32,)),
                    InterfaceIndexSnapshot::Param("N".to_owned()),
                ),
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
                axis: false,
            },
            InterfaceParam {
                ty: InterfaceTypeSnapshot::Vector(
                    Box::new(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32,)),
                    InterfaceIndexSnapshot::Param("N".to_owned()),
                ),
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
                axis: false,
            },
        ]
    );
    assert_eq!(
        add.ret.as_ref(),
        &InterfaceTypeSnapshot::Vector(
            Box::new(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32,)),
            InterfaceIndexSnapshot::Param("N".to_owned()),
        )
    );
    // The `Box` nominal keeps its register-carrier fields instead of being
    // silently emptied by the snapshot (the same missing vector arm).
    let FileExportKind::Struct(strukt) = &interface.exports["Box"].kind else {
        panic!("Box must snapshot as a struct export");
    };
    assert_eq!(strukt.fields.len(), 2, "Box fields must survive the snapshot");
    assert_eq!(
        strukt.fields[0].ty,
        InterfaceTypeSnapshot::Vector(
            Box::new(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32,)),
            InterfaceIndexSnapshot::Param("N".to_owned()),
        )
    );

    // The emit-route analysis (single-file check through the same import
    // contract the `radix emit` driver builds) must accept the imported
    // generic call with a concrete-size result — identical to `faber check`.
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "emit route must accept the imported generic call, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result.diagnostics.iter().any(|d| {
            d.is_error()
                && matches!(
                    d.issue(),
                    Some(
                        "expression_type_mismatch"
                            | "initializer_annotation_mismatch"
                            | "return_type_mismatch"
                            | "numeric_operands_required"
                    )
                )
        }),
        "no SEM010/SEM011 cascade may survive the emit route: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolves_provider_module_via_library_home_manifest() {
    let (_td, home) = temp_dir("libhome");
    let provider = home.join("triga");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "triga"
[library]
provider = "triga"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/triga.fab"),
        "genus Vector3 { varia f32 x varia f32 y varia f32 z }\nfunctio vector3(f32 x, f32 y, f32 z) → Vector3 { redde Vector3 { x = x, y = y, z = z } }\n",
    )
    .expect("source");

    let _guard = LibraryHomeGuard::set(&home);
    let resolved = resolve_namespace_import(
        &crate::driver::Session::new(crate::driver::Config::default().with_latin_locale()),
        PathBuf::from("/tmp/app.fab").as_path(),
        "triga:triga",
        None,
    );
    let path = resolved.expect("triga:triga should resolve");
    assert!(path.ends_with("triga/src/triga.fab"), "got {}", path.display());
}

#[test]
fn import_contract_installs_struct_and_function_exports() {
    let (_td, home) = temp_dir("contract");
    let provider = home.join("triga");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "triga"
[library]
provider = "triga"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/triga.fab"),
        r#"+++
locale = "la"
+++
@ publica
genus Vector3 {
    varia f32 x
    varia f32 y
    varia f32 z
}
@ publica
genus Matrix4 {
    varia lista<f32> elements
}
@ publica
functio vector3(f32 x, f32 y, f32 z) → Vector3 {
    redde Vector3 { x = x, y = y, z = z }
}
@ publica
functio matrix4_inversa_affinis(de Matrix4 matrix) → Matrix4 ∪ nihil {
    redde nulla
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "triga:triga" triga
incipit {
  sit triga.Vector3 zero ← triga.vector3(0.0, 0.0, 0.0)
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);

    let interface = contract
        .file_interfaces
        .get("triga")
        .expect("triga interface installed");
    assert!(
        interface.exports.contains_key("Vector3"),
        "expected Vector3 export, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );
    assert!(interface.exports.contains_key("Matrix4"));
    assert!(interface.exports.contains_key("vector3"));
    assert!(interface.exports.contains_key("matrix4_inversa_affinis"));
}

#[test]
fn export_surface_preserves_name_tier_for_interna_and_publica() {
    // VM-U2 (CTO 29b629dc): the export surface carries {name, tier}, not an
    // untyped name set. `@ publica` and `@ interna` names both join the
    // surface with their tiers preserved for the VM-U4 seam check.
    let (_td, home) = temp_dir("surface-tiers");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio aperta() → vacuum {
}
@ interna
functio clausa() → vacuum {
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(&app, "+++\nlocale = \"la\"\n+++\nimporta ex \"probe:probe\" probe\n").expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let surface = contract
        .namespace_exports
        .get("probe")
        .expect("probe export surface");
    use crate::semantic::passes::visibility_annotation::VisibilityTier;
    assert_eq!(surface.get("aperta"), Some(&VisibilityTier::Publica));
    assert_eq!(surface.get("clausa"), Some(&VisibilityTier::Interna));
    assert_eq!(surface.len(), 2, "untyped name set would flatten interna away");
}

#[test]
fn interna_export_fails_closed_to_cross_package_consumer() {
    // VM-U2 (CTO 29b629dc): `@ interna` must fail closed to cross-package
    // consumers until VM-U4. The single-file tool path has no package context,
    // so every consumer is treated as cross-package.
    let (_td, home) = temp_dir("interna-fail-closed");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ interna
functio clausa() → vacuum {
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipit {
  probe.clausa()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        !result.success,
        "interna import must fail closed to a cross-package consumer: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("import_interna_cross_package")),
        "interna member must surface as a cross-package seam error: {:?}",
        result.diagnostics
    );
}

#[test]
fn publica_export_stays_importable_cross_package() {
    // Control for the fail-closed rule: `@ publica` members remain
    // importable to the same cross-package consumer.
    let (_td, home) = temp_dir("publica-importable");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio aperta() → vacuum {
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipit {
  probe.aperta()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "publica import must stay importable: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn interna_type_export_fails_closed_to_cross_package_consumer() {
    // VM-U2 (CTO 29b629dc): the fail-closed rule covers type members too —
    // an `@ interna` genus is not nameable cross-package.
    let (_td, home) = temp_dir("interna-type-fail-closed");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"
@ interna
genus Instrumentum {
    varia textus nomen = ""
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"
importa ex "probe:probe" probe
incipit {
  sit probe.Instrumentum m ← probe.Instrumentum { nomen = "x" }
  nota m.nomen
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        !result.success,
        "interna type import must fail closed cross-package: {:?}",
        result.diagnostics
    );
}

#[test]
fn publica_type_alias_is_importable_cross_package() {
    // MB-U1 / tela U5 parity control: an `@ publica` type alias joins the
    // import surface like a genus — the qualified alias resolves in both
    // return and parameter positions under a cross-package consumer.
    let (_td, home) = temp_dir("publica-alias-importable");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ publica
typus Nomen = textus
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
functio e(probe.Nomen nomen) → probe.Nomen {
    redde nomen
}
incipit {
  nota e("x")
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "publica type alias must stay importable: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("qualified_type_not_exported")),
        "SEM002.qualified_type_not_exported must not fire for an imported publica alias"
    );
}

#[test]
fn interna_type_alias_fails_closed_to_cross_package_consumer() {
    // VM-U2 mirror for aliases: an `@ interna` type alias is not nameable
    // cross-package (same-package consumers are allowed).
    let (_td, home) = temp_dir("interna-alias-fail-closed");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ interna
typus Nomen = textus
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
functio e() → probe.Nomen {
    redde "x"
}
incipit {
  nota e()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        !result.success,
        "interna type alias must fail closed to a cross-package consumer: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("import_interna_cross_package")),
        "interna alias must surface as a cross-package seam error: {:?}",
        result.diagnostics
    );
}

#[test]
fn unmarked_type_alias_is_module_private() {
    // An alias without a visibility annotation stays module-private: the
    // member is declared but never joins the export surface, so a consumer
    // reference is an import_module_private seam error.
    let (_td, home) = temp_dir("unmarked-alias-private");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
typus Nomen = textus
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
functio e() → probe.Nomen {
    redde "x"
}
incipit {
  nota e()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        !result.success,
        "unmarked type alias must be module-private: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("import_module_private")),
        "unmarked alias must surface as an import_module_private seam error: {:?}",
        result.diagnostics
    );
}

#[test]
fn wildcard_relative_import_resolves_publica_type_alias() {
    // MB-U1 / tela U5 repro shape: a consumer importing `./lib_alias` as `la`
    // resolves `la.Nomen` (an `@ publica` type alias) in return AND parameter
    // positions.
    let (_td, home) = temp_dir("wildcard-alias");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/lib_alias.fab");
    fs::write(home.join("lib_alias.fab"), fs::read_to_string(&fixture).expect("fixture")).expect("copy lib_alias");
    let app = home.join("consumer.fab");
    fs::write(
        &app,
        r#"import from "./lib_alias" * as la
fn e(la.Nomen nomen) → la.Nomen {
    return nomen
}
main {
    print "ok"
}
"#,
    )
    .expect("write consumer");
    let source = fs::read_to_string(&app).expect("read consumer");

    // Same seam as `radix check`: the en reader pack drives lexing of both the
    // consumer and its relative import target.
    let en_pack = en_locale_pack();
    let result = crate::services::check_diagnostics_for_source_with_locale_pack(
        &app.display().to_string(),
        &source,
        false,
        Some(&en_pack),
        None,
    );
    assert!(
        result.success,
        "wildcard import must resolve the publica type alias: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("qualified_type_not_exported")),
        "SEM002.qualified_type_not_exported must not fire for the imported alias"
    );
}

#[test]
fn import_contract_preserves_callable_posture_words_and_legacy_annotations() {
    let (_td, home) = temp_dir("posture-contract");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio later() fiet → numerus {
    redde 1
}

@ publica
functio stream() fiunt → numerus {
    cede 1
}

@ futura
@ publica
functio annotated_later() → numerus {
    redde 2
}

@ cursor
@ publica
functio annotated_stream() → numerus {
    cede 2
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipit {
  nota 1
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("probe")
        .expect("probe interface installed");

    let later = interface.exports.get("later").expect("later export");
    let crate::file_interface::FileExportKind::Function(later) = &later.kind else {
        panic!("later should export as function");
    };
    assert!(later.is_async);
    assert!(!later.is_generator);

    let annotated_later = interface
        .exports
        .get("annotated_later")
        .expect("annotated_later export");
    let crate::file_interface::FileExportKind::Function(annotated_later) = &annotated_later.kind else {
        panic!("annotated_later should export as function");
    };
    assert!(annotated_later.is_async);
    assert!(!annotated_later.is_generator);

    let stream = interface.exports.get("stream").expect("stream export");
    let crate::file_interface::FileExportKind::Function(stream) = &stream.kind else {
        panic!("stream should export as function");
    };
    assert!(!stream.is_async);
    assert!(stream.is_generator);

    let annotated_stream = interface
        .exports
        .get("annotated_stream")
        .expect("annotated_stream export");
    let crate::file_interface::FileExportKind::Function(annotated_stream) = &annotated_stream.kind else {
        panic!("annotated_stream should export as function");
    };
    assert!(!annotated_stream.is_async);
    assert!(annotated_stream.is_generator);
}

#[test]
fn check_accepts_tacebit_on_imported_legacy_futura_function() {
    let (_td, home) = temp_dir("legacy-futura-await");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ futura
@ publica
functio sleep() → vacuum {
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipiet {
  tacebit probe.sleep()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "legacy @ futura import must typecheck as promissum for tacebit: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_accepts_tacebit_on_imported_failable_futura_function() {
    let (_td, home) = temp_dir("failable-futura-await");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ futura
@ publica
functio sleep() → vacuum ⇥ textus {
    iace "bad"
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
functio caller() fiet → vacuum ⇥ textus {
  tacebit probe.sleep()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "imported failable @ futura function must expose promissum<T ⇥ E> for tacebit: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_resolves_imported_qualified_types_without_sem002() {
    let (_td, home) = temp_dir("check");
    let provider = home.join("triga");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "triga"
[library]
provider = "triga"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/triga.fab"),
        r#"+++
locale = "la"
+++
@ publica
genus Vector3 {
    varia f32 x
    varia f32 y
    varia f32 z
}
@ publica
functio vector3(f32 x, f32 y, f32 z) → Vector3 {
    redde Vector3 { x = x, y = y, z = z }
}
@ publica
functio vector3_normalizata(de Vector3 v) → Vector3 {
    redde v
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "triga:triga" triga
incipit {
  sit triga.Vector3 zero ← triga.vector3(0.0, 0.0, 0.0)
  sit triga.Vector3 n ← triga.vector3_normalizata(zero)
  adfirma n.x ≡ (0.0 ∷ f32) mori "ok"
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);

    assert!(
        result.success,
        "expected check success, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("unknown_qualified_type")),
        "SEM002.unknown_qualified_type must not fire for imported triga types"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn provider_emit_uses_rust_module_paths_not_unresolved_def() {
    let (_td, home) = temp_dir("emit");
    let provider = home.join("triga");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "triga"
[library]
provider = "triga"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/triga.fab"),
        r#"+++
locale = "la"
+++
@ publica
genus Vector3 {
    varia f32 x
    varia f32 y
    varia f32 z
}
@ publica
functio vector3(f32 x, f32 y, f32 z) → Vector3 {
    redde Vector3 { x = x, y = y, z = z }
}
@ publica
functio vector3_normalizata(de Vector3 v) → Vector3 {
    redde v
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "triga:triga" triga
incipit {
  sit triga.Vector3 zero ← triga.vector3(0.0, 0.0, 0.0)
  sit triga.Vector3 n ← triga.vector3_normalizata(zero)
  adfirma n.x ≡ (0.0 ∷ f32) mori "ok"
}
"#,
    )
    .expect("app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::program::compile_cli_path(&app, false, crate::codegen::Target::HirRust);

    assert!(
        result.success(),
        "emit should succeed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("pub mod triga"),
        "expected provider module, got:\n{}",
        output.code
    );
    assert!(
        output.code.contains("crate::triga::Vector3") || output.code.contains("crate::triga::vector3"),
        "expected crate::triga paths, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("unresolved_def"),
        "must not emit unresolved_def:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("triga.vector3"),
        "must not emit value-style triga. calls:\n{}",
        output.code
    );
}

/// SEM002 residual (correctness 859d68a): one unsupported/generic export must not
/// fail-all the provider namespace — plain siblings stay importable.
#[test]
fn generic_export_does_not_poison_sibling_plain_exports() {
    let (_td, home) = temp_dir("poison");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    // Generic lege + plain carpe — historically fail-all SEM002 on file interface install.
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ radix typus T in textus lista<textus> lista<b8>
@ publica
functio lege<T>(textus via) → T {
    mori "lege deferred in probe"
}
@ publica
functio carpe(textus via) → lista<textus> {
    redde [via]
}
@ publica
functio exstat(textus via) → bivalens {
    redde verum
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipit {
  sit bivalens ok ← probe.exstat("x")
  sit lista<textus> lines ← probe.carpe("y")
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("probe")
        .expect("probe interface");
    assert!(
        interface.exports.contains_key("carpe"),
        "carpe must export, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );
    assert!(
        interface.exports.contains_key("exstat"),
        "exstat must export, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );
    assert!(
        interface.exports.contains_key("lege"),
        "lege generic should snapshot with Param, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );

    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "check must succeed without SEM002.file_interface_import_failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result.diagnostics.iter().any(|d| {
            d.issue()
                .is_some_and(|issue| issue.starts_with("file_interface_import_failed"))
        }),
        "file_interface_import_failed must not fire: {:?}",
        result.diagnostics
    );
}

#[test]
fn unknown_nominal_param_export_is_skipped_not_fail_all() {
    let (_td, home) = temp_dir("json-poison");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    // Unknown applied type (not a formal primitive) still poisons one export only.
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio pange(ignotum_genus x) → textus {
    redde ""
}
@ publica
functio salve() → textus {
    redde "ok"
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipit {
  sit textus s ← probe.salve()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("probe")
        .expect("probe interface");
    // pange may still be present in the snapshot; install must skip it and keep salve.
    assert!(interface.exports.contains_key("salve"));

    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "salve must remain usable when pange is poison: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    // P2 residual: skipped poison export must be observable (WARN014), not silent.
    let skip = result
        .diagnostics
        .iter()
        .find(|d| d.issue() == Some("file_interface_export_skipped") && d.code == Some("WARN014"));
    assert!(
        skip.is_some(),
        "expected file_interface_export_skipped for pange, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let skip = skip.expect("skip diagnostic");
    assert!(
        skip.args
            .iter()
            .any(|arg| arg.name == "export" && arg.value == "pange"),
        "skip diagnostic should identify pange: {:?}",
        skip.args
    );
}

/// gradus S1 regression: a public callable whose signature references an
/// UNMARKED nominal of the same module (e.g. tensor.fab's `causa(TensorError
/// e) → textus` with unannotated `discretio TensorError`) must export cleanly
/// through the import-contract interface. The interface snapshot keeps the
/// legacy unmarked-exported surface (mirror of the package path's
/// `program_export_surface`); dropping the nominal poisoned the callable's
/// install (WARN014 skip) and produced false SEM010s during `faber convert`.
#[test]
fn unmarked_nominal_referenced_by_public_callable_exports_cleanly() {
    let (_td, home) = temp_dir("unmarked-nominal");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    // The error enum is UNMARKED (module-private default under the radix seam,
    // but the package path keeps unmarked declarations exported). The public
    // callables reference it, so the interface must still install them.
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
discretio ProbeError {
    Perperam { textus causa }
}

@ publica
functio perperam(textus causa) → ProbeError {
    redde finge Perperam { causa = causa }
}

@ publica
functio causa(ProbeError e) → textus {
    redde "err"
}
"#,
    )
    .expect("probe source");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipit {
  sit textus s ← probe.causa(probe.perperam("x"))
  nota s
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("probe")
        .expect("probe interface");
    assert!(
        interface.exports.contains_key("causa") && interface.exports.contains_key("perperam"),
        "callables referencing an unmarked nominal must still install, got: {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );

    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| { d.code == Some("WARN014") && d.issue() == Some("file_interface_export_skipped") }),
        "no export may be skipped when the referenced nominal is local-unmarked: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn direct_provider_interface_preserves_enum_params_and_qualified_fields() {
    let (_td, home) = temp_dir("scene-interface");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/math.fab"),
        r#"+++
locale = "la"
+++
@ publica
genus Matrix4 {
    varia f32 m00
}
"#,
    )
    .expect("math");
    fs::write(
        provider.join("src/scene.fab"),
        r#"+++
locale = "la"
+++
importa ex "probe:math" math

@ publica
discretio SceneNodeKind {
    SceneGroup { }
}

@ publica
genus SceneNode {
    varia textus name
    varia SceneNodeKind kind
    varia math.Matrix4 local_matrix
}

@ publica
functio scene_node(textus name, SceneNodeKind kind) → SceneNode {
    mori "deferred"
}

@ publica
functio scene_group(textus name) → SceneNode {
    mori "deferred"
}
"#,
    )
    .expect("scene");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:math" math
importa ex "probe:scene" scene

incipit {
  sit scene.SceneNode node ← scene.scene_group("root")
  sit math.Matrix4 local ← node.local_matrix
  nota node.name
  nota local.m00
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "provider scene interface should install enum params and qualified fields: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result.diagnostics.iter().any(|d| {
            d.code == Some("WARN014")
                && d.issue() == Some("file_interface_export_skipped")
                && d.args
                    .iter()
                    .any(|arg| arg.name == "export" && arg.value == "scene_node")
        }),
        "scene_node should no longer be skipped: {:?}",
        result.diagnostics
    );
}

/// cds-u6 G4 (browser.fab seam shape): a provider module that imports a
/// sibling and exports a callable whose public signature references the
/// imported sibling type (`fn mount(...) → Mounted ∪ null` with
/// `visus.Visus`/`visus.Thema` params) must export cleanly for a consumer
/// that imports the referenced sibling too — no WARN014 skip, the callable
/// usable, and both qualified type names nameable.
#[test]
fn imported_sibling_typed_callable_exports_cleanly() {
    let (_td, home) = temp_dir("g4-seam");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    // Sibling module: the imported view types (browser.fab's tela:visus shape).
    fs::write(
        provider.join("src/visus.fab"),
        r#"+++
locale = "la"
+++
@ publica
discretio Visus {
    Textus { textus valor }
}

@ publica
genus Thema {
    varia textus nomen
}

@ publica
functio textus_visus(textus valor) → Visus {
    mori "deferred"
}
"#,
    )
    .expect("visus");
    // The seam module: imports the sibling and exports a callable whose
    // signature references the imported types (browser.fab's mount shape).
    fs::write(
        provider.join("src/browser.fab"),
        r#"+++
locale = "la"
+++
importa ex "probe:visus" visus

@ publica
genus Scope {
    varia textus selector
}

@ publica
genus Mounted {
    varia Scope scopus
    varia visus.Visus visus
    varia visus.Thema thema
}

@ publica
functio mount(Scope scopus, visus.Visus visus, visus.Thema thema) → Mounted ∪ nihil {
    mori "deferred"
}
"#,
    )
    .expect("browser");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r##"+++
locale = "la"
+++
importa ex "probe:browser" browser
importa ex "probe:visus" visus

incipit {
  sit visus.Thema t ← visus.Thema { nomen = "lumen" }
  sit browser.Scope s ← browser.Scope { selector = "#root" }
  sit browser.Mounted ∪ nihil m ← browser.mount(s, visus.textus_visus("hi"), t)
  nota m
}
"##,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    // (a) the imported-sibling-typed export appears in the snapshot.
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("browser")
        .expect("browser interface");
    assert!(
        interface.exports.contains_key("mount"),
        "mount must appear in the snapshot, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );
    let crate::file_interface::FileExportKind::Function(mount) = &interface.exports["mount"].kind else {
        panic!("mount must be a function export");
    };
    let ret = mount.ret.as_ref();
    assert!(
        matches!(ret, crate::file_interface::InterfaceTypeSnapshot::Option(inner)
            if matches!(inner.as_ref(), crate::file_interface::InterfaceTypeSnapshot::Nominal(n)
                if n.name == "Mounted" && n.identity.is_some())),
        "mount ret must snapshot Mounted with a module identity, got {ret:?}"
    );

    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "browser seam shape must typecheck: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result.diagnostics.iter().any(|d| {
            d.code == Some("WARN014")
                && d.issue() == Some("file_interface_export_skipped")
                && d.args
                    .iter()
                    .any(|arg| arg.name == "export" && arg.value == "mount")
        }),
        "mount must not be export-skipped: {:?}",
        result.diagnostics
    );
}

/// WARN014 provider-identity residual (capsule+gguf_manifest+gguf A2 shape):
/// a provider module whose sibling imports are spelled in PACKAGE form
/// (source-root, no provider colon) still snapshots its exports with the
/// same canonical identities the provider-form spelling derives, and a
/// consumer that imports only the outer module installs the siblings
/// transitively — the callable must not be export-skipped (WARN014) nor
/// leave false SEM010s at its call site. The direct-import-everything
/// spelling is the commune-pin dodge and is covered there.
#[test]
fn package_form_sibling_typed_exports_clean_without_direct_imports() {
    let (_td, home) = temp_dir("a2-package-form");
    let provider = home.join("demo");
    fs::create_dir_all(provider.join("src")).expect("demo src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "demo"
[library]
provider = "demo"
[reader]
locale = "en"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/capsule.fab"),
        r"
@ public
class Capsule {
    mut string schema
}

@ public
fn construct_manifest(string schema) → Capsule ⇥ AdmissionError {
    return Capsule { schema = schema }
}

@ public
union AdmissionError {
    @ shared
    string message

    UnknownVersion,
    BadManifest
}
",
    )
    .expect("capsule");
    fs::write(
        provider.join("src/gguf_manifest.fab"),
        r"
@ public
union GgufManifestError {
    @ shared
    string message

    BadFormat,
    Truncated,
    BadWire
}
",
    )
    .expect("gguf_manifest");
    // The gguf module imports its siblings in PACKAGE form (bare
    // source-root spelling) — the shape the commune pin dodges.
    fs::write(
        provider.join("src/gguf.fab"),
        r#"
import from "capsule" capsule
import from "gguf_manifest" gguf_manifest

@ public
union GgufError {
    @ shared
    string message

    BadCapsule,
    ManifestError {
        gguf_manifest.GgufManifestError inner
    }
}

@ conversion
fn from_gguf_manifest_error(gguf_manifest.GgufManifestError err) → GgufError {
    return variant ManifestError {message = err.message, inner = err}
}

@ public
fn admit_capsule(string schema) → capsule.Capsule ⇥ GgufError {
    do {
        let capsule.Capsule c ← capsule.construct_manifest(schema)
        return c
    }
    catch err {
        throw variant BadCapsule {message = "capsule construction failed: §"(err.message)}
    }
}
"#,
    )
    .expect("gguf");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "en"
+++

import from "demo:gguf" gguf

fn use_admit() → string {
    do {
        let _ c ← gguf.admit_capsule("schema-2")
        return c.schema
    }
    catch err {
        return err.message
    }
}

main {
    print use_admit()
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    // (a) the gguf snapshot derives package-form edge identities from the
    // provider-form direct edge — the one package-form identity lookup.
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("gguf")
        .expect("gguf interface");
    assert!(
        interface.exports.contains_key("admit_capsule"),
        "admit_capsule must appear in the snapshot, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );
    let crate::file_interface::FileExportKind::Function(admit) = &interface.exports["admit_capsule"].kind else {
        panic!("admit_capsule must be a function export");
    };
    assert!(
        matches!(admit.ret.as_ref(), crate::file_interface::InterfaceTypeSnapshot::Nominal(n)
            if n.name == "Capsule" && n.identity.is_some()),
        "package-form qualified ref must carry the sibling identity, got {:?}",
        admit.ret
    );
    // (b) the package-form siblings install transitively under their
    // producer bindings so the consumer can remap them.
    for binding in ["capsule", "gguf_manifest"] {
        assert!(
            contract.file_interfaces.contains_key(binding),
            "package-form sibling {binding} must install transitively, got {:?}",
            contract.file_interfaces.keys().collect::<Vec<_>>()
        );
    }

    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "gguf-only consumer must typecheck against package-form siblings: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("WARN014") && d.issue() == Some("file_interface_export_skipped")),
        "no export may be skipped on the package-form sibling shape: {:?}",
        result.diagnostics
    );
    assert!(
        !result.diagnostics.iter().any(|d| d.code == Some("SEM010")),
        "no false SEM010 may surface on the package-form sibling shape: {:?}",
        result.diagnostics
    );
}

/// pumpfix-gfd594086-2.3 (Unit 2.1 regression): fs.fab names `fault.IoFault`
/// in `read_file_async`'s failure arm through its own provider-form
/// `norma:fault` edge. A consumer that imports only `norma:fs` must still
/// resolve that nominal — the transitive interface walk installs `fault`,
/// and check must not surface an unresolved/unknown-type diagnostic naming
/// `IoFault`.
#[test]
fn provider_edge_nominal_in_imported_signature_resolves_without_direct_import() {
    let (_td, home) = temp_dir("provider-edge-nominal");
    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "en"
+++

import from "norma:fs" charta

async_main {
    await_const string body ← charta.read_file_async<string>("x.txt")
    print body
}
"#,
    )
    .expect("write consumer");
    let source = fs::read_to_string(&app).expect("read consumer");

    let library_home = crate::import_resolve::in_tree_library_home().expect("<repo>/lib exists in the tree");
    let _guard = LibraryHomeGuard::set(&library_home);

    // (a) the imported fs interface's provider-form `norma:fault` edge must
    // install the fault interface under its producer binding; without the
    // transitive walk the signature nominal has no registration to resolve.
    let contract = import_contract_for_file(&app, &source, None, None);
    assert!(
        contract.file_interfaces.contains_key("fault"),
        "the fs.fab `norma:fault` edge must install transitively, got {:?}",
        contract.file_interfaces.keys().collect::<Vec<_>>()
    );

    // (b) check must resolve the imported `IoFault` without a direct import.
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "fs-only consumer must typecheck against the provider-form `norma:fault` edge: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| format!("{:?}", (d.code, d.issue(), d.args.clone())).contains("IoFault")),
        "IoFault in the imported fs signature must resolve without a direct \
         `norma:fault` import; diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

/// b494bfce-3: `norma:fs` exposes the durable atomic replace
/// (`solum:conde`) as `write_file_durably` / `write_bytes_durably`. A consumer
/// that imports only `norma:fs` must resolve both members — the test fails if
/// either wrapper is missing or malformed in the imported interface.
#[test]
fn durable_fs_write_wrapper_resolves_for_consumers() {
    let (_td, home) = temp_dir("durable-fs-write");
    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "en"
+++

import from "norma:fs" charta

main {
    charta.write_file_durably("checkpoint.txt", "weights-v1")
    charta.write_bytes_durably("checkpoint.bin", |00 ff 10|)
    print "durable wrappers ok"
}
"#,
    )
    .expect("write consumer");
    let source = fs::read_to_string(&app).expect("read consumer");

    let library_home = crate::import_resolve::in_tree_library_home().expect("<repo>/lib exists in the tree");
    let _guard = LibraryHomeGuard::set(&library_home);

    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "durable fs wrappers must typecheck for an fs-only consumer: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    for wrapper in ["write_file_durably", "write_bytes_durably"] {
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| format!("{:?}", (d.code, d.issue(), d.args.clone())).contains(wrapper)),
            "{wrapper} must resolve in the imported `norma:fs` interface; diagnostics={:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.issue(), d.args.clone()))
                .collect::<Vec<_>>()
        );
    }
}

/// cds-u6 snapshot-nomen-collision (extension-lib `Scopulum` shape): two
/// providers each export a same-named class and class-returning functions.
/// Same-named local classes must not cascade-skip both providers' exports
/// (WARN014), and each qualified type name must be nameable (SEM002 gone).
#[test]
fn same_named_local_classes_do_not_cascade_skip() {
    let (_td, home) = temp_dir("nomen-collision");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    // Kernel module: exports Scopulum + a class-returning function.
    fs::write(
        provider.join("src/tela.fab"),
        r#"+++
locale = "la"
+++
@ publica
genus Scopulum {
    varia textus nomen
    varia textus valor
}

@ publica
functio scopulum(textus nomen, textus valor) → Scopulum {
    mori "deferred"
}
"#,
    )
    .expect("tela");
    // Extension module: declares its OWN Scopulum (the original `Scopulum`
    // name that collided with the kernel's type in the shared snapshot).
    fs::write(
        provider.join("src/extension.fab"),
        r#"+++
locale = "la"
+++
@ publica
genus Scopulum {
    varia textus nomen
    varia textus valor
}

@ publica
functio chart_axis_muted() → Scopulum {
    mori "deferred"
}
"#,
    )
    .expect("extension");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:tela" tela
importa ex "probe:extension" ext

incipit {
  sit tela.Scopulum k ← tela.scopulum("a", "b")
  sit ext.Scopulum e ← ext.chart_axis_muted()
  nota k.nomen
  nota e.nomen
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "same-named local classes must typecheck: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let skipped = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some("WARN014") && d.issue() == Some("file_interface_export_skipped"))
        .map(|d| {
            d.args
                .iter()
                .find(|arg| arg.name == "export")
                .map_or_else(|| "?".to_owned(), |arg| arg.value.clone())
        })
        .collect::<Vec<_>>();
    assert!(
        skipped.is_empty(),
        "no export may be cascade-skipped on a name collision, got: {skipped:?}"
    );
}

/// Tool-path `norma:json` must install the formal-json facade (not hollow WARN014-all).
#[cfg(feature = "hir-rust")]
#[test]
fn norma_json_tool_path_installs_facade_exports() {
    let (_td_nj, dir_nj) = temp_dir("norma-json");
    let app = dir_nj.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "norma:json" json
incipit {
  sit json ∪ nihil maybe ← json.try_parse("{}")
  nota maybe
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");
    let home = crate::import_resolve::in_tree_library_home().expect("<repo>/lib exists in the tree");
    assert!(
        home.join("norma/src/json.fab").is_file(),
        "expected norma json under {}",
        home.display()
    );
    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("json")
        .expect("json file interface");
    for name in ["stringify", "parse", "try_parse"] {
        assert!(
            interface.exports.contains_key(name),
            "expected {name} in interface, got {:?}",
            interface.exports.keys().collect::<Vec<_>>()
        );
    }
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "norma:json facade must typecheck on tool path: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result.diagnostics.iter().any(|d| {
            d.issue()
                .is_some_and(|issue| issue.starts_with("file_interface_export_skipped"))
        }),
        "must not hollow-skip json exports: {:?}",
        result.diagnostics
    );
}

#[test]
fn import_contract_rejects_parent_traversal_before_read() {
    // P2-A regression: import_contract_for_file must reject `..` imports
    // without opening the target file.
    let (_td, home) = temp_dir("reject-dotdot");
    let app_file = home.join("app.fab");
    let sibling_file = home.join("sibling.fab");

    // Create a sibling file that would resolve if read were attempted.
    fs::write(&sibling_file, "functio salve() → textus { redde \"hi\" }\n").expect("write sibling");

    // Write the app with a `../` import that points to the sibling.
    fs::write(
        &app_file,
        r#"
importa ex "../sibling" sibling
importa ex "./good" good
incipit {}
"#,
    )
    .expect("write app");
    let source = fs::read_to_string(&app_file).expect("read app");

    let contract = import_contract_for_file(&app_file, &source, None, None);

    // The `../sibling` import must NOT be resolved — it's a `..` traversal.
    assert!(
        !contract.namespace_exports.contains_key("sibling"),
        ".. traversal import must be rejected before resolution"
    );
    assert!(
        !contract.file_interfaces.contains_key("sibling"),
        ".. traversal must not produce a file interface"
    );
    // `./good` is a valid relative import; its missing-file resolution is fine.
    assert!(!contract.namespace_exports.contains_key("good"));
}

#[test]
fn import_contract_for_nonexistent_file_returns_empty() {
    let (_td, home) = temp_dir("missing");
    let app = home.join("nonexistent.fab");
    // Non-existent file path but we still call contract — expect no panic, clean result.
    let contract = import_contract_for_file(&app, "", None, None);
    assert!(contract.file_interfaces.is_empty());
    assert!(contract.namespace_exports.is_empty());
}

#[test]
fn session_memos_parse_once_across_modules_one_driver_run() {
    // ARCH-U3 / S01-9 done_when (ii): two modules importing the same target
    // parse it once through one driver run. The run is one `Session` whose
    // per-module re-wraps share memo identity; deleting the target after the
    // first module's contract proves the second module was served from the
    // shared memo — no re-read, no re-parse — rather than re-resolving a file
    // that no longer exists.
    let (_td, home) = temp_dir("session-memos-parse-once");
    fs::write(home.join("util.fab"), "functio salve() → textus { redde \"salve\" }\n").expect("write util");
    let app_a = home.join("app_a.fab");
    let app_b = home.join("app_b.fab");
    for app in [&app_a, &app_b] {
        fs::write(
            app,
            r#"
importa ex "./util" util
incipit {}
"#,
        )
        .expect("write app");
    }
    let source_a = fs::read_to_string(&app_a).expect("read app a");
    let source_b = fs::read_to_string(&app_b).expect("read app b");

    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let prepared_a =
        crate::program::compile::prepare_program_module(&app_a, &source_a, &session).expect("prepare first module");
    // The second module enters through a re-wrap of the same run — the shape
    // the package driver uses per file — so the pin also proves re-wraps keep
    // memo identity instead of minting a fresh parse-once scope.
    let module_session = session.re_wrap(session.config.clone());
    fs::remove_file(home.join("util.fab")).expect("delete target");
    let prepared_b = crate::program::compile::prepare_program_module(&app_b, &source_b, &module_session)
        .expect("prepare second module from the shared memo");

    let binding_a = prepared_a
        .contract
        .namespace_exports
        .keys()
        .find(|binding| binding.as_str() == "util")
        .cloned();
    let binding_b = prepared_b
        .contract
        .namespace_exports
        .keys()
        .find(|binding| binding.as_str() == "util")
        .cloned();
    assert_eq!(binding_a.as_deref(), Some("util"), "first module installs the target");
    assert_eq!(
        binding_b, binding_a,
        "second module must serve the import binding through the shared session memo"
    );
}

/// P4 instans/json oracle reconciliation: a provider module whose exports
/// name the `instans` precision family (bare `instans` and `instans<ns>` in
/// params and returns) must snapshot its function exports instead of dropping
/// them, so importers typecheck `module.nunc()` against `instans<ns>`.
#[test]
fn import_contract_preserves_sized_and_bare_instans_exports() {
    let (_td, home) = temp_dir("instans-contract");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio nunc() → instans<ns> {
    redde ad 'probe:nunc' ↦ instans<ns>
}
@ publica
functio solve(textus wire) → instans {
    redde ad 'probe:solve' ↦ instans
}
@ publica
functio wire(instans<ns> t) → textus {
    redde t ↦ textus
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
incipit {
  sit instans<ns> nanos ← probe.nunc()
  sit instans parsed ← probe.solve("1979-05-27T07:32:00Z")
  sit textus w ← probe.wire(nanos)
  nota parsed
  nota w
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("probe")
        .expect("probe interface installed");
    for export in ["nunc", "solve", "wire"] {
        assert!(
            interface.exports.contains_key(export),
            "instans-typed export `{export}` must survive snapshot, got {:?}",
            interface.exports.keys().collect::<Vec<_>>()
        );
    }

    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "imported instans-typed exports must typecheck, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

// ── cds-u5-provider-locale ──
//
// Cross-package locale propagation (fix:web-dom-locale / fix:codegen001):
// a provider module is analyzed in ITS OWN reader surface — frontmatter
// locale > package `[reader] locale` > the importing module's pack > Latin
// default — and a provider directory may be located through its manifest's
// `[library] provider` segment (web:dom → the faber-web checkout).

fn en_locale_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load")
}

fn la_locale_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

/// The la-locale provider source, mirroring the faber-web/src/dom.fab seam
/// shape (Scope genus + scoped query/require).
const LA_DOM_SOURCE: &str = r#"@ publica
genus Scope {
    varia textus selector = ""
}

@ publica
genus Element {
    varia textus selector = ""
}

@ publica
functio scope(textus selector) → Scope {
    redde Scope { selector = selector }
}

@ publica
functio query(Scope scope, textus selector) → Element ∪ nihil {
    nota scope
    nota selector
    redde nulla
}

@ publica
functio require(Scope scope, textus selector) → Element {
    nota scope
    redde Element { selector = selector }
}
"#;

/// Write a package manifest declaring an `[library] provider` segment that
/// differs from the checkout directory name (the faber-web shape).
fn write_provider_manifest(dir: &Path, provider: &str, locale: &str) {
    fs::write(
        dir.join("faber.toml"),
        format!(
            r#"[package]
name = "provider-package"
[library]
provider = "{provider}"
[reader]
locale = "{locale}"
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

#[test]
fn resolve_provider_segment_via_manifest_provider_name() {
    // `web:dom` must resolve when the checkout directory is `faber-web` and
    // the manifest declares `[library] provider = "web"` — the directory-name
    // convention (`home/web`) misses, and the manifest scan must find it.
    crate::import_resolve::clear_provider_name_memo();
    let (_td, home) = temp_dir("provider-name");
    let provider = home.join("faber-web");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    write_provider_manifest(&provider, "web", "la");
    fs::write(provider.join("src/dom.fab"), LA_DOM_SOURCE).expect("source");

    let _guard = LibraryHomeGuard::set(&home);
    let resolved = resolve_namespace_import(
        &crate::driver::Session::new(crate::driver::Config::default().with_latin_locale()),
        PathBuf::from("/tmp/app.fab").as_path(),
        "web:dom",
        None,
    );
    let path = resolved.expect("web:dom should resolve via the manifest provider name");
    assert!(path.ends_with("faber-web/src/dom.fab"), "got {}", path.display());
}

#[test]
fn check_en_reader_imports_la_provider_cross_package() {
    // The tela/faber-web shape (done_when (a)): an en-locale module imports
    // the la-locale `web:dom` provider. The provider directory is `faber-web`
    // (manifest-declared `web` segment), the provider source is Latin, the
    // reader is en, and the reader consumes `web.Scope` in a class-field type
    // plus `web.scope(…)` at a call site. Must check green with no
    // SEM002.unknown_qualified_type.
    //
    // S2-U2 reconciliation: the reader-side app source uses the declared en
    // surface for its import keyword (`as`, not the Latin `ut`) — the en pack
    // owns `ut → as`. The provider source keeps its la identity: the
    // fix:web-dom-locale mechanism parses the target with its OWN package
    // locale (`[reader] locale = "la"`), so the Latin provider body claims
    // under la authority. Cross-package intent preserved on both sides.
    let (_td, home) = temp_dir("en-la");
    let tela = home.join("tela");
    fs::create_dir_all(tela.join("src")).expect("tela src");
    fs::write(
        tela.join("faber.toml"),
        r#"[package]
name = "tela"
[reader]
locale = "en"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    let web = home.join("faber-web");
    fs::create_dir_all(web.join("src")).expect("web src");
    write_provider_manifest(&web, "web", "la");
    fs::write(web.join("src/dom.fab"), LA_DOM_SOURCE).expect("source");

    let app = tela.join("src/reader.fab");
    fs::write(
        &app,
        r#"
import from "web:dom" public * as web

class Mounted {
    mut string selector
    mut web.Scope scopus
}

fn mount(string selector) → Mounted {
    return Mounted {
        selector = selector,
        scopus = web.scope(selector)
    }
}

fn probe(string selector) → string {
    mut web.Scope s ← web.scope(selector)
    return s.selector
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");
    let en_pack = en_locale_pack();

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source_with_locale_pack(
        &app.display().to_string(),
        &source,
        false,
        Some(&en_pack),
        None,
    );
    assert!(
        result.success,
        "en reader importing the la web:dom provider must check green, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("unknown_qualified_type")),
        "SEM002.unknown_qualified_type must not fire for the imported web.Scope type"
    );
}

#[test]
fn import_contract_parses_en_provider_under_own_package_locale() {
    // Target-locale propagation at the contract level: an en-surface provider
    // (English keywords, no frontmatter) in a package declaring
    // `[reader] locale = "en"`, imported by a Latin-default reader. The
    // target must be parsed with its OWN en pack, not the reader's Latin
    // default — otherwise `fn`/`return`/`string` fail to lex and no exports
    // install (the fix:web-dom-locale mechanism).
    let (_td, home) = temp_dir("en-target");
    let tela = home.join("tela");
    fs::create_dir_all(tela.join("src")).expect("tela src");
    fs::write(
        tela.join("faber.toml"),
        r#"[package]
name = "tela"
[reader]
locale = "en"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        tela.join("src/valida.fab"),
        r#"
@ public
fn valida_nomen_tag(string nomen) → bool {
    return nomen ≢ ""
}
"#,
    )
    .expect("provider source");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "tela:valida" valida
incipit {
  sit bivalens ok ← valida.valida_nomen_tag("a")
  nota ok
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("valida")
        .expect("en provider interface must install under its own package locale");
    assert!(
        interface.exports.contains_key("valida_nomen_tag"),
        "expected the en provider's export, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emit_across_import_uses_provider_package_locale() {
    // CODEGEN001 (done_when (b)): Rust emit of an import-bearing module
    // re-analyzes the imported provider. The provider is an en-surface module
    // in an en package; the re-analysis session must pick up the package's
    // `[reader] locale = "en"` instead of the Latin default, so the provider
    // emits as a `pub mod` instead of failing PARSE001.
    let (_td, home) = temp_dir("emit-locale");
    let tela = home.join("tela");
    fs::create_dir_all(tela.join("src")).expect("tela src");
    fs::write(
        tela.join("faber.toml"),
        r#"[package]
name = "tela"
[reader]
locale = "en"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        tela.join("src/valida.fab"),
        r#"
@ public
fn valida_nomen_tag(string nomen) → bool {
    return nomen ≢ ""
}
"#,
    )
    .expect("provider source");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "tela:valida" valida
incipit {
  sit bivalens ok ← valida.valida_nomen_tag("a")
  nota ok
}
"#,
    )
    .expect("app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::program::compile_cli_path(&app, false, crate::codegen::Target::HirRust);

    assert!(
        result.success(),
        "import-bearing Rust emit must succeed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("pub mod valida"),
        "expected the en provider module, got:\n{}",
        output.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emit_lowers_library_locale_surface_to_canonical() {
    // A Latin consumer may call library members by the library pack's
    // Latin surface. Generated Rust must name the library's English
    // exports: the provider module is emitted from native source.
    let (_td, home) = temp_dir("emit-surface-canonical");
    let lib = home.join("canvas");
    fs::create_dir_all(lib.join("src")).expect("canvas src");
    fs::create_dir_all(lib.join("locale/la")).expect("canvas locale");
    fs::write(
        lib.join("faber.toml"),
        r#"[package]
name = "canvas"
[library]
provider = "canvas"
[reader]
locale = "en"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        lib.join("src/canvas.fab"),
        r"
@ public
class Point {
    mut int x
}

@ public
fn make_point(int x) → Point {
    return Point { x = x }
}
",
    )
    .expect("provider source");
    fs::write(
        lib.join("locale/la/pack.toml"),
        r#"
[pack]
id = "la"
schema_version = 1

[[library_members]]
provider = "canvas"
module_path = ["canvas"]
kind = "type"
canonical = "Point"
surface = "Punctum"

[[library_members]]
provider = "canvas"
module_path = ["canvas"]
kind = "function"
canonical = "make_point"
surface = "fac_punctum"
"#,
    )
    .expect("library locale pack");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++

importa ex "canvas:canvas" * ut canvas

incipit {
  sit canvas.Punctum punctum ← canvas.fac_punctum(1)
  nota punctum
}
"#,
    )
    .expect("app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::program::compile_cli_path(&app, false, crate::codegen::Target::HirRust);

    assert!(
        result.success(),
        "Latin-surface library calls must rust-emit: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("make_point"),
        "expected canonical function name, got:\n{}",
        output.code
    );
    assert!(
        output.code.contains("Point"),
        "expected canonical type name, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("fac_punctum"),
        "consumer-locale function surface leaked into Rust:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("Punctum"),
        "consumer-locale type surface leaked into Rust:\n{}",
        output.code
    );
}

#[test]
fn package_identity_facts_registers_en_surface_local_imports() {
    // En-surface local sibling imports (`import from "./x"`) in a manifestless
    // package must register as SAME-PACKAGE identities: the re-parse resolves
    // the package's effective reader pack (product-default en) instead of
    // registry-direct Latin, so the English `import from` keyword lexes and
    // the sibling call stays an in-package call (never a provider call).
    let (_td, home) = temp_dir("identity-en");
    let package_dir = home.join("geminus");
    fs::create_dir_all(&package_dir).expect("package dir");
    fs::write(
        package_dir.join("adiutor.fab"),
        r#"fn greeting(string nomen) → string {
    return "Salve, §!"(nomen)
}
"#,
    )
    .expect("write sibling");
    let entry = package_dir.join("geminus.fab");
    fs::write(
        &entry,
        r#"import from "./adiutor" adiutor

main {
    print adiutor.greeting("Tullia")
}
"#,
    )
    .expect("write entry");

    let config = english_config();
    let identities = crate::program::package_identity_facts_for_path(&entry, &config)
        .expect("en-surface local imports must register identity facts");
    assert_eq!(identities.product, "geminus", "product derives from the package root");
    assert_eq!(
        identities.module_segments,
        vec!["geminus".to_owned()],
        "entry canonical module segments"
    );
    assert_eq!(
        identities.imports.get("adiutor"),
        Some(&vec!["adiutor".to_owned()]),
        "`import from \"./adiutor\"` must resolve to the sibling's same-package module segments"
    );
}

#[test]
fn package_identity_facts_keeps_registry_direct_latin_package() {
    // A package declaring the Latin reader locale (`[reader] locale = "la"`)
    // keeps registry-direct parsing: Latin local imports (`importa ex "./x"`)
    // still register same-package identities (no behavior change for
    // registry-direct packages).
    let (_td, home) = temp_dir("identity-la");
    let package_dir = home.join("adiutorium");
    fs::create_dir_all(&package_dir).expect("package dir");
    fs::write(
        package_dir.join("faber.toml"),
        r#"[package]
name = "adiutorium"
[reader]
locale = "la"
[paths]
source = "src"
"#,
    )
    .expect("manifest");
    let source_dir = package_dir.join("src");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::write(
        source_dir.join("adiutor.fab"),
        "functio salutatio(textus nomen) → textus {\n    redde \"Salve, §!\"(nomen)\n}\n",
    )
    .expect("write sibling");
    let entry = source_dir.join("adiutorium.fab");
    fs::write(
        &entry,
        r#"importa ex "./adiutor" adiutor

incipit {
    nota adiutor.salutatio("Tullia")
}
"#,
    )
    .expect("write entry");

    let config = crate::driver::Config::default().with_latin_locale();
    let identities = crate::program::package_identity_facts_for_path(&entry, &config)
        .expect("Latin local imports must register identity facts under the la pack");
    assert_eq!(identities.product, "adiutorium", "product derives from the package root");
    assert_eq!(
        identities.imports.get("adiutor"),
        Some(&vec!["src".to_owned(), "adiutor".to_owned()]),
        "`importa ex \"./adiutor\"` must resolve to the sibling's same-package module segments"
    );
}

// ── SEM014 en-surface contains inference (radix-lane residual) ──
//
// The tela validate.fab shape (check-compile): under the en reader pack the
// CLI check must infer `contains` with a NON-LITERAL string arg exactly like
// the canonical `continet` — the en surface spelling is rewritten to the
// canonical intrinsic before arg checking, so a method-call arg
// (`name.slice(0, 1)`) in a condition position must not leave an unresolved
// inference hole (SEM014.cannot_infer_expression_type).
#[test]
fn en_surface_contains_infers_with_non_literal_string_arg() {
    let source = r#"
fn valid_tag_name(string name) → bool {
    let string litterae ← "abcdefghijklmnopqrstuvwxyz"
    if name.length() ≡ 0 then return false
    if not litterae.contains(name.slice(0, 1) ⊥ "") then return false
    return true
}
"#;
    let en_pack = en_locale_pack();
    let result = crate::services::check_diagnostics_for_source_with_locale_pack(
        "en_contains_infer.fab",
        source,
        false,
        Some(&en_pack),
        None,
    );
    assert!(
        result.success,
        "en-surface `contains` with a non-literal string arg must check green: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result.diagnostics.iter().any(|d| d.code == Some("SEM014")),
        "no SEM014 may fire for the en-surface `contains` call: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn nested_local_import_prepared_analyze_does_not_frontend_fail() {
    // Mid imports leaf and names `leaf.Point` in a signature. Empty-contract
    // analyze_source fail-closes on the qualified type; the program helper
    // must discover leaf's interface so e2e/interpret paths do not
    // frontend-fail solely from missing contracts.
    let (_td, dir) = temp_dir("nested-local-analyze");
    let leaf = dir.join("leaf.fab");
    let mid = dir.join("mid.fab");
    fs::write(
        &leaf,
        r"
@ publica
genus Point {
    varia f32 x
    varia f32 y
}
",
    )
    .expect("leaf");
    fs::write(
        &mid,
        r#"
importa ex "./leaf" leaf

@ publica
functio via() → leaf.Point {
    redde leaf.Point { x = 0.0, y = 0.0 }
}
"#,
    )
    .expect("mid");
    let source = fs::read_to_string(&mid).expect("read mid");
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let empty = crate::driver::analyze_source(&session, &mid.display().to_string(), &source, AnalyzeRequest::default());
    assert!(
        empty.is_err(),
        "empty-contract analyze_source must frontend-fail nested imported type leaf.Point"
    );
    let unit = super::analyze_prepared_program_path(&session, &mid, &source)
        .expect("prepared analyze must not frontend-fail nested local import");
    assert!(
        !unit.diagnostics.iter().any(crate::Diagnostic::is_error),
        "prepared analyze diagnostics must be error-free: {:?}",
        unit.diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

fn write_demo_library_with_latin_pack(home: &Path) {
    let demo = home.join("demo");
    fs::create_dir_all(demo.join("src")).expect("demo src");
    fs::create_dir_all(demo.join("locale/la")).expect("demo locale");
    fs::write(
        demo.join("faber.toml"),
        r#"[package]
name = "demo"
[library]
provider = "demo"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("demo manifest");
    fs::write(
        demo.join("src/canvas.fab"),
        r#"
@ publica
genus Point {
    varia textus value = ""
}
@ publica
functio make_point(textus value) → Point {
    redde Point { value = value }
}
@ publica
functio show(Point p) → textus {
    redde p.value
}
"#,
    )
    .expect("demo source");
    fs::write(
        demo.join("locale/la/pack.toml"),
        r#"
[pack]
id = "la"
schema_version = 1

[[library_members]]
provider = "demo"
module_path = ["canvas"]
kind = "type"
canonical = "Point"
surface = "Punctum"

[[library_members]]
provider = "demo"
module_path = ["canvas"]
kind = "function"
canonical = "make_point"
surface = "fac_punctum"

[[library_members]]
provider = "demo"
module_path = ["canvas"]
kind = "function"
canonical = "show"
surface = "monstra"
"#,
    )
    .expect("demo latin pack");
}

#[test]
fn latin_consumer_resolves_library_owned_pack() {
    let (_td, home) = temp_dir("lib-locale");
    write_demo_library_with_latin_pack(&home);
    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"
importa ex "demo:canvas" canvas
incipit {
    sit canvas.Punctum p ← canvas.fac_punctum("salve")
    nota p
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");
    let la_pack = la_locale_pack();
    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source_with_locale_pack(
        &app.display().to_string(),
        &source,
        false,
        Some(&la_pack),
        None,
    );
    assert!(
        result.success,
        "latin consumer must resolve library locale/<id> members, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn latin_consumer_still_accepts_canonical_library_names() {
    let (_td, home) = temp_dir("lib-locale-canonical");
    write_demo_library_with_latin_pack(&home);
    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"
importa ex "demo:canvas" canvas
incipit {
    sit canvas.Point p ← canvas.make_point("salve")
    nota p
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");
    let la_pack = la_locale_pack();
    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source_with_locale_pack(
        &app.display().to_string(),
        &source,
        false,
        Some(&la_pack),
        None,
    );
    assert!(
        result.success,
        "canonical fallback must remain, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn latin_consumer_rejects_unmapped_library_surface() {
    let (_td, home) = temp_dir("lib-locale-invented");
    write_demo_library_with_latin_pack(&home);
    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"
importa ex "demo:canvas" canvas
incipit {
    sit canvas.Punctum fictum ← canvas.fac_ignotum("salve")
    nota fictum
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");
    let la_pack = la_locale_pack();
    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source_with_locale_pack(
        &app.display().to_string(),
        &source,
        false,
        Some(&la_pack),
        None,
    );
    assert!(!result.success, "invented library surface must fail");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("SEM002") || d.code == Some("SEM004")),
        "expected SEM002/SEM004, diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// S1-U2 convert-path repair regressions (variant projection)
// ---------------------------------------------------------------------------

#[test]
fn interface_snapshot_projects_union_and_enum_variants() {
    // S1-U2 defect: the convert-path interface snapshot (`file_interface_for_program`)
    // exported Union/Enum nominals with `variants: Vec::new()`, so imported union
    // patterns (`module.VARIANT` in `discerne` arms) failed SEM001. The snapshot
    // must project the variant list (name + payload fields) like the package path —
    // for identity-bearing targets (library/package imports). Identity-less
    // relative-import targets keep the legacy empty-variant surface the
    // error-masking diagnostics pin
    // (SEM001.unknown_variant / SEM008 on imported union refs).
    let (program, interner) = super::parse_program_for_namespace_exports(
        "producer.fab",
        r#"+++
locale = "la"
+++
@ publica
discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}

@ publica
ordo Gradus {
    Parvus,
    Magnus
}
"#,
        Some(&la_locale_pack()),
    )
    .expect("parse producer source");

    let identity = crate::file_interface::InterfaceLibraryIdentity {
        provider: "probe".to_owned(),
        package: None,
        module_path: vec!["eventum".to_owned()],
    };
    let interface = super::file_interface_for_program(&program, &interner, Some(&identity));

    let eventum = interface.exports.get("Eventum").expect("Eventum export");
    let crate::file_interface::FileExportKind::Enum(nominal) = &eventum.kind else {
        panic!("discretio must export as an Enum-kind nominal, got {:?}", eventum.kind);
    };
    assert_eq!(nominal.variants.len(), 2, "union variants must be projected");
    assert_eq!(nominal.variants[0].name, "Ictus");
    assert_eq!(nominal.variants[0].fields.len(), 1);
    assert_eq!(nominal.variants[0].fields[0].name, "vis");
    assert_eq!(
        nominal.variants[0].fields[0].ty,
        crate::file_interface::InterfaceTypeSnapshot::Primitive(crate::semantic::Primitive::Numerus)
    );
    assert_eq!(nominal.variants[1].name, "Sonitus");
    assert!(nominal.variants[1].fields.is_empty(), "unit variant carries no fields");

    let gradus = interface.exports.get("Gradus").expect("Gradus export");
    let crate::file_interface::FileExportKind::Enum(nominal) = &gradus.kind else {
        panic!("ordo must export as an Enum-kind nominal, got {:?}", gradus.kind);
    };
    assert_eq!(nominal.variants.len(), 2, "enum members must be projected");
    assert_eq!(nominal.variants[0].name, "Parvus");
    assert_eq!(nominal.variants[1].name, "Magnus");

    // Identity-less snapshot (relative-import target outside a package):
    // variants stay empty so the fail-closed surface is preserved.
    let legacy = super::file_interface_for_program(&program, &interner, None);
    let legacy_eventum = legacy.exports.get("Eventum").expect("Eventum export");
    let crate::file_interface::FileExportKind::Enum(legacy_nominal) = &legacy_eventum.kind else {
        panic!("discretio must export as an Enum-kind nominal, got {:?}", legacy_eventum.kind);
    };
    assert!(
        legacy_nominal.variants.is_empty(),
        "identity-less snapshots must keep the empty-variant fail-closed surface"
    );
}

#[test]
fn interface_snapshot_snaps_consts_and_tuple_returns() {
    // S1-U2 done_when surfaces: the convert-path interface must also carry
    // typed `fixum` consts (`module.CONST` values) and tuple return types
    // (`iuncta<A, B>`); dropping them blocked gradus gguf/safetensors/tensor_view
    // conversion with SEM010 at the call sites.
    let (program, interner) = super::parse_program_for_namespace_exports(
        "producer.fab",
        r#"+++
locale = "la"
+++
@ publica
sit textus schema_versio ← "2.0.0"

@ publica
functio limes() → iuncta<numerus, numerus> {
    redde iuncta<numerus, numerus> [1, 2]
}
"#,
        Some(&la_locale_pack()),
    )
    .expect("parse producer source");

    let interface = super::file_interface_for_program(&program, &interner, None);

    let schema_versio = interface
        .exports
        .get("schema_versio")
        .expect("const export");
    let crate::file_interface::FileExportKind::Const(snapshot, _) = &schema_versio.kind else {
        panic!("typed fixum must export as a Const snapshot, got {:?}", schema_versio.kind);
    };
    assert_eq!(
        *snapshot,
        crate::file_interface::InterfaceTypeSnapshot::Primitive(crate::semantic::Primitive::Textus)
    );

    let limes = interface.exports.get("limes").expect("function export");
    let crate::file_interface::FileExportKind::Function(callable) = &limes.kind else {
        panic!("functio must export as a Function, got {:?}", limes.kind);
    };
    assert_eq!(
        *callable.ret,
        crate::file_interface::InterfaceTypeSnapshot::Tuple(vec![
            crate::file_interface::InterfaceTypeSnapshot::Primitive(crate::semantic::Primitive::Numerus),
            crate::file_interface::InterfaceTypeSnapshot::Primitive(crate::semantic::Primitive::Numerus),
        ])
    );
}

#[test]
fn interface_snapshot_preserves_callable_and_storage_types() {
    // These signatures exercise portable type forms that cannot be inferred
    // from ordinary nominal/primitive exports: closures, two-channel promises,
    // atomics, and shaped sparse tensors.
    let (program, interner) = super::parse_program_for_namespace_exports(
        "producer.fab",
        r#"+++
locale = "la"
+++
@ publica
functio callback((numerus) → fractus handler) → numerus { redde 1 }

@ publica
functio pending(promissum<numerus ⇥ textus> value) → promissum<numerus ⇥ textus> { redde value }

@ publica
functio cells(atomic<numerus> cell, sparsa<fractus, [2, 3]> values) → fractus { redde 0.0 }
"#,
        Some(&la_locale_pack()),
    )
    .expect("parse producer source");
    let interface = super::file_interface_for_program(&program, &interner, None);

    let callback = interface.exports.get("callback").expect("callback export");
    let crate::file_interface::FileExportKind::Function(callback) = &callback.kind else {
        panic!("callback must export as a function");
    };
    let crate::file_interface::InterfaceTypeSnapshot::Func(handler) = &callback.params[0].ty else {
        panic!("function-typed parameter must retain its callable signature");
    };
    assert_eq!(handler.params.len(), 1);
    assert_eq!(
        handler.params[0].ty,
        crate::file_interface::InterfaceTypeSnapshot::Primitive(crate::semantic::Primitive::Numerus)
    );
    assert_eq!(
        *handler.ret,
        crate::file_interface::InterfaceTypeSnapshot::Primitive(crate::semantic::Primitive::Fractus)
    );

    let pending = interface.exports.get("pending").expect("pending export");
    let crate::file_interface::FileExportKind::Function(pending) = &pending.kind else {
        panic!("pending must export as a function");
    };
    let promise = crate::file_interface::InterfaceTypeSnapshot::PromissumFailable(
        Box::new(crate::file_interface::InterfaceTypeSnapshot::Primitive(
            crate::semantic::Primitive::Numerus,
        )),
        Box::new(crate::file_interface::InterfaceTypeSnapshot::Primitive(
            crate::semantic::Primitive::Textus,
        )),
    );
    assert_eq!(pending.params[0].ty, promise);
    assert_eq!(*pending.ret, promise);

    let cells = interface.exports.get("cells").expect("cells export");
    let crate::file_interface::FileExportKind::Function(cells) = &cells.kind else {
        panic!("cells must export as a function");
    };
    assert_eq!(
        cells.params[0].ty,
        crate::file_interface::InterfaceTypeSnapshot::Atomic(Box::new(
            crate::file_interface::InterfaceTypeSnapshot::Primitive(crate::semantic::Primitive::Numerus),
        ))
    );
    assert_eq!(
        cells.params[1].ty,
        crate::file_interface::InterfaceTypeSnapshot::Sparsa(
            Box::new(crate::file_interface::InterfaceTypeSnapshot::Primitive(
                crate::semantic::Primitive::Fractus,
            )),
            crate::file_interface::InterfaceIndexSnapshot::Tuple(vec![
                crate::file_interface::InterfaceIndexSnapshot::Literal(2),
                crate::file_interface::InterfaceIndexSnapshot::Literal(3),
            ]),
        )
    );
}

#[test]
fn converted_union_consumer_discernes_imported_variants() {
    // S1-U2 defect end-to-end: a consumer importing a provider `discretio`
    // must be able to match its variants (`casu provider.Union.Variant`) and
    // reference them as values. Before the variant projection, the empty
    // snapshot surfaced SEM001 unknown_identifier / unknown_variant cascades.
    let (_td, home) = temp_dir("union-interface");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/eventum.fab"),
        r#"+++
locale = "la"
+++
@ publica
discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}
"#,
    )
    .expect("eventum");
    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:eventum" eventum

functio describe(eventum.Eventum e) → textus {
    discerne e {
        casu eventum.Eventum.Ictus sit vis { redde vis ↦ textus }
        casu eventum.Eventum.Sonitus { redde "sonitus" }
    }
}

incipit {
    sit eventum.Eventum a ← finge eventum.Eventum.Ictus { vis = 3 } ∷ eventum.Eventum
    sit eventum.Eventum b ← eventum.Eventum.Sonitus
    nota describe(a) + describe(b)
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), &source, false);
    assert!(
        result.success,
        "imported union consumer must check green through the convert snapshot: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn import_contract_installs_transitive_relative_import_interfaces() {
    // MRL-4: a module's public surface can name sibling nominals through its
    // own relative imports (`list<p.Punctum>` in a `Catena` field). The
    // consumer's contract must carry the sibling module's interface under the
    // producer's binding or the field cannot remap cross-module.
    let (_td, home) = temp_dir("transitive-contract");
    fs::write(
        home.join("lib_punctum.fab"),
        r#"+++
locale = "en"
+++

@ public
class Punctum {
    mut u32 gradus
}
"#,
    )
    .expect("punctum");
    fs::write(
        home.join("lib_lista.fab"),
        r#"+++
locale = "en"
+++

import from "./lib_punctum" * as p

@ public
class Catena {
    mut list<p.Punctum> puncta
}
"#,
    )
    .expect("lista");
    let consumer = home.join("consumer.fab");
    fs::write(
        &consumer,
        r#"+++
locale = "en"
+++

import from "./lib_lista" * as l
"#,
    )
    .expect("consumer");
    let source = fs::read_to_string(&consumer).expect("read consumer");

    let contract = import_contract_for_file(&consumer, &source, None, None);
    let _lista = contract
        .file_interfaces
        .get("l")
        .expect("direct binding installed");
    let punctum = contract
        .file_interfaces
        .get("p")
        .expect("transitive sibling interface installed under the producer binding");
    assert!(
        punctum.exports.contains_key("Punctum"),
        "expected Punctum export, got {:?}",
        punctum.exports.keys().collect::<Vec<_>>()
    );

    // The consumer's own binding for the same name always wins: a direct
    // `* as p` of lib_lista must not be overwritten by the sibling install.
    let shadowing = home.join("shadowing.fab");
    fs::write(
        &shadowing,
        r#"+++
locale = "en"
+++

import from "./lib_lista" * as p
"#,
    )
    .expect("shadowing consumer");
    let shadow_source = fs::read_to_string(&shadowing).expect("read shadowing");
    let shadow_contract = import_contract_for_file(&shadowing, &shadow_source, None, None);
    let direct = shadow_contract
        .file_interfaces
        .get("p")
        .expect("consumer binding installed");
    assert!(
        direct.exports.contains_key("Catena"),
        "consumer's own binding must win over the transitive sibling install, got {:?}",
        direct.exports.keys().collect::<Vec<_>>()
    );
}

/// DFV2-3 (a, c): an imported `@ nucleum` declaration keeps its device role,
/// launch visibility, and explicit body channel in the consumer's file
/// interface. The role fact never travels without the body fact.
#[test]
fn import_contract_carries_imported_device_role_and_body_channel() {
    let (_td, home) = temp_dir("device-channel");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ nucleum
@ publica
functio scala(f32 x) → f32 {
    redde x + 1.0
}

@ nucleum
functio adiuvans(f32 x) → f32 {
    redde x + 2.0
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);

    let interface = contract
        .file_interfaces
        .get("probe")
        .expect("probe interface installed");

    let device_contract_of = |name: &str| {
        let export = interface
            .exports
            .get(name)
            .unwrap_or_else(|| panic!("{name} export"));
        let FileExportKind::Function(callable) = &export.kind else {
            panic!("{name} must be a function export");
        };
        callable
            .device
            .clone()
            .unwrap_or_else(|| panic!("{name} must carry a device contract across the import link"))
    };

    // Public `@ nucleum` with a body: launchable entry, source body available.
    let scala = device_contract_of("scala");
    assert_eq!(scala.role, InterfaceDeviceRole::Kernel);
    assert_eq!(scala.visibility, InterfaceDeviceVisibility::Public);
    assert_eq!(scala.body, InterfaceBodyChannel::Source);

    // Unmarked `@ nucleum` with a body: composable helper, never a launch
    // entry — the visibility gate rides with the role.
    let adiuvans = device_contract_of("adiuvans");
    assert_eq!(adiuvans.role, InterfaceDeviceRole::Kernel);
    assert_eq!(adiuvans.visibility, InterfaceDeviceVisibility::Private);
    assert_eq!(adiuvans.body, InterfaceBodyChannel::Source);
    assert!(adiuvans.is_composable());
}

/// DFV2-3 (a): a plain imported function carries no device contract at all —
/// the device fact is added for `@ nucleum` exports only, not smeared over
/// the whole import surface.
#[test]
fn import_contract_leaves_non_device_exports_without_device_contract() {
    let (_td, home) = temp_dir("device-channel-plain");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    fs::write(
        provider.join("src/probe.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio plana(f32 x) → f32 {
    redde x
}

@ nucleum fragment
@ publica
functio fragmentum(f32 x) → f32 {
    redde x
}
"#,
    )
    .expect("lib");

    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "probe:probe" probe
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("probe")
        .expect("probe interface installed");

    let plana = interface.exports.get("plana").expect("plana export");
    let FileExportKind::Function(callable) = &plana.kind else {
        panic!("plana must be a function export");
    };
    assert!(callable.device.is_none(), "non-device exports carry no device contract");

    // `@ nucleum fragment` keeps the composition Fragment role, distinct from
    // the v2 compute Kernel role.
    let fragmentum = interface
        .exports
        .get("fragmentum")
        .expect("fragmentum export");
    let FileExportKind::Function(callable) = &fragmentum.kind else {
        panic!("fragmentum must be a function export");
    };
    let device = callable
        .device
        .as_ref()
        .expect("fragment carries a device contract");
    assert_eq!(device.role, InterfaceDeviceRole::Fragment);
    assert_eq!(device.visibility, InterfaceDeviceVisibility::Public);
    assert_eq!(device.body, InterfaceBodyChannel::Source);
}

/// DFV2-3 provider-route source: the package-identity computation records the
/// device-bearing sibling interface for the consumer, keyed by the consumer's
/// module segments and import binding — the data the MIR provider route
/// carries into the consumer link.
#[test]
fn package_identity_facts_record_device_interface_for_the_consumer() {
    let (_td, home) = temp_dir("device-route");
    fs::write(
        home.join("faber.toml"),
        r#"[package]
name = "probe"
"#,
    )
    .expect("manifest");
    fs::write(
        home.join("kernels.fab"),
        r"
@ kernel
@ public
fn scala(f32 x) → f32 {
    return x + 1.0
}
",
    )
    .expect("sibling kernels");
    let app = home.join("app.fab");
    fs::write(
        &app,
        r#"
import from "./kernels" k
"#,
    )
    .expect("app");

    let config = english_config();
    let identities = package_identity_facts_for_path(&app, &config).expect("package identity facts for the consumer");
    assert_eq!(identities.module_segments, vec!["app".to_owned()]);
    assert_eq!(
        identities.imports.get("k").map(std::vec::Vec::as_slice),
        Some(&["kernels".to_owned()][..])
    );

    let interface = identities
        .device_interfaces
        .get("k")
        .expect("device-bearing sibling interface recorded for the consumer");
    let export = interface.exports.get("scala").expect("scala export");
    let FileExportKind::Function(callable) = &export.kind else {
        panic!("scala must be a function export");
    };
    let device = callable.device.as_ref().expect("device contract recorded");
    assert_eq!(device.role, InterfaceDeviceRole::Kernel);
    assert_eq!(device.visibility, InterfaceDeviceVisibility::Public);
    assert_eq!(device.body, InterfaceBodyChannel::Source);
}

/// KPC-WIRE done_when (i), advisory D2: an imported library that resolves and
/// reads but does not parse is a named import-site fact, never a silently
/// missing namespace surface. This is the pinned-gradus defect shape — the
/// consumer's namespace stops silently matching (no exports, no declarations,
/// no interface) and the compile fails closed instead of falling through the
/// ignotum route.
#[test]
fn import_contract_failcloses_on_unparsable_library() {
    let (_td, home) = temp_dir("import-site-failure");
    let provider = home.join("probe");
    fs::create_dir_all(provider.join("src")).expect("provider src");
    fs::write(
        provider.join("faber.toml"),
        r#"[package]
name = "probe"
[library]
provider = "probe"
[paths]
source = "src"
[build]
kind = "lib"
targets = ["rust"]
[reader]
locale = "la"
"#,
    )
    .expect("manifest");
    // Library content that cannot parse under any reader pack: the function
    // body never closes.
    fs::write(
        provider.join("src/probe.fab"),
        "+++\nlocale = \"la\"\n+++\nfunctio scala(f32 x) → f32 {\n    redde x + 1.0\n",
    )
    .expect("broken lib");

    let app = home.join("app.fab");
    fs::write(&app, "+++\nlocale = \"la\"\n+++\nimporta ex \"probe:probe\" probe\n").expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);

    assert_eq!(
        contract.import_site_failures.len(),
        1,
        "one named import-site fact per parse-failed library"
    );
    let failure = &contract.import_site_failures[0];
    assert_eq!(failure.binding, "probe");
    assert_eq!(failure.import_path, "probe:probe");
    assert!(
        failure.target_path.ends_with("probe.fab"),
        "fact names the resolved target, got {}",
        failure.target_path.display()
    );
    // The interfaceless namespace stops silently matching: no surface is
    // installed for the binding.
    assert!(!contract.namespace_exports.contains_key("probe"));
    assert!(!contract.namespace_declarations.contains_key("probe"));
    assert!(!contract.file_interfaces.contains_key("probe"));

    // The session memo deduplicates the failure: a second contract build for
    // the same target reports the same single fact, not a second one.
    let again = import_contract_for_file(&app, &source, None, None);
    assert_eq!(again.import_site_failures.len(), 1);
}

/// KPC-WIRE done_when (i), triaged against the KPC-SEM rubric: the gate fires
/// on parse failure with library content, never on surfaceless-by-design
/// namespaces. A provider namespace that resolves to no target stays silently
/// surfaceless — the 11 bare-driver/kernel-manifest regressions must hold.
#[test]
fn import_site_gate_ignores_surfaceless_namespaces() {
    let (_td, home) = temp_dir("import-site-surfaceless");
    let app = home.join("app.fab");
    fs::write(&app, "importa ex \"faber:processus\" processus\n").expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let contract = import_contract_for_file(&app, &source, None, None);

    assert!(
        contract.import_site_failures.is_empty(),
        "surfaceless namespace must not trip the fail-closed gate: {:?}",
        contract.import_site_failures
    );
    assert!(!contract.file_interfaces.contains_key("processus"));
}

/// KPC-WIRE done_when (i): the parse-failed library stops the program compile
/// with the named import-site diagnostic on the driver-side `Diagnostic`
/// channel (advisory D3 — no interface-type change).
#[test]
fn prepare_program_module_fails_closed_on_unparsable_library() {
    let (_td, home) = temp_dir("import-site-prepare");
    fs::write(home.join("math.fab"), "functio addere(f32 a, f32 b) → f32 {\n    redde a + b\n").expect("broken lib");
    let app = home.join("app.fab");
    fs::write(&app, "importa ex \"./math\" math\n").expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let diagnostic = match crate::program::compile::prepare_program_module(&app, &source, &session) {
        Ok(prepared) => panic!(
            "parse-failed library must fail the program compile closed, got contract bindings {:?}",
            prepared.contract.file_interfaces.keys().collect::<Vec<_>>()
        ),
        Err(diagnostic) => *diagnostic,
    };
    assert_eq!(diagnostic.code, Some("IMP001"));
    assert!(matches!(diagnostic.phase, crate::diagnostics::DiagnosticPhase::Parse));
    assert_eq!(diagnostic.issue(), Some("imported_library_parse_failure"));
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "binding" && arg.value == "math"),
        "diagnostic names the import site binding: {:?}",
        diagnostic.args
    );
}

/// ST3-2 residual: the sibling interface a single-file consumer installs must
/// carry the imported genus's static-field initializer, or the consumer's
/// `module.Tab.Z` read fails closed at MIR lowering.
#[test]
fn import_contract_carries_static_field_default_from_sibling() {
    let (_td, dir) = temp_dir("static-field-default");
    let lectio = dir.join("lectio.fab");
    fs::write(
        &lectio,
        r#"+++
locale = "la"
+++
@ publica
genus Tab {
    fixum numerus LIMES = 7
    varia numerus w = 1
}
"#,
    )
    .expect("lectio");
    let app = dir.join("app.fab");
    fs::write(
        &app,
        r#"+++
locale = "la"
+++
importa ex "./lectio" lectio
incipit {
    print lectio.Tab.LIMES
}
"#,
    )
    .expect("app");
    let source = fs::read_to_string(&app).expect("read app");

    let contract = import_contract_for_file(&app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("lectio")
        .expect("lectio interface installed");
    let tab = interface.exports.get("Tab").expect("Tab export");
    let FileExportKind::Struct(tab_export) = &tab.kind else {
        panic!("Tab must export as a struct snapshot, got {:?}", tab.kind);
    };
    let limes = tab_export
        .fields
        .iter()
        .find(|field| field.name == "LIMES")
        .expect("LIMES field crosses the interface");
    assert!(
        limes.default.is_some(),
        "static field default must ride the sibling interface, got {:?}",
        limes.default
    );
}

#[test]
fn interface_snapshot_carries_nondum_gates_in_every_spelling() {
    // The AST snapshot feeds sibling-file and library imports before full
    // analysis: an `@ nondum` function stays exported and its gate rides the
    // interface, so the importer refuses every use (SEM017).
    let (program, interner) = super::parse_program_for_namespace_exports(
        "producer.fab",
        r#"+++
locale = "la"
+++
@ publica
@ nondum "pending host dispatch"
functio later(textus s) → textus

@ publica
@ nondum rs
functio later_rs(textus s) → textus

@ publica
@ nondum { target = ts, ratio = "braced" }
functio later_ts(textus s) → textus

@ publica
functio nunc(textus s) → textus {
    redde s
}
"#,
        Some(&la_locale_pack()),
    )
    .expect("parse producer source");

    let interface = super::file_interface_for_program(&program, &interner, None);

    for name in ["later", "later_rs", "later_ts", "nunc"] {
        assert!(
            matches!(
                interface.exports.get(name).map(|export| &export.kind),
                Some(crate::file_interface::FileExportKind::Function(_))
            ),
            "{name} stays exported"
        );
    }
    let gate = |target: Option<&str>, reason: Option<&str>| {
        vec![crate::file_interface::InterfaceUnavailable {
            target: target.map(str::to_owned),
            reason: reason.map(str::to_owned),
        }]
    };
    assert_eq!(
        interface.unavailable.get("later"),
        Some(&gate(None, Some("pending host dispatch")))
    );
    assert_eq!(interface.unavailable.get("later_rs"), Some(&gate(Some("rs"), None)));
    assert_eq!(interface.unavailable.get("later_ts"), Some(&gate(Some("ts"), Some("braced"))));
    assert!(!interface.unavailable.contains_key("nunc"));
}

#[test]
fn interface_snapshot_carries_nondum_method_gates() {
    // A `genus` method marked `@ nondum` stays exported and its gate rides the
    // method record, so the importer refuses every call (SEM017).
    let (program, interner) = super::parse_program_for_namespace_exports(
        "producer.fab",
        r#"+++
locale = "la"
+++
@ publica
genus Nexus {
    varia numerus id

    @ nondum "pending host dispatch"
    functio claude() → vacuum

    @ nondum ts
    functio mitte() → vacuum {
        tacet
    }

    functio lege() → numerus {
        redde ego.id
    }
}
"#,
        Some(&la_locale_pack()),
    )
    .expect("parse producer source");

    let interface = super::file_interface_for_program(&program, &interner, None);

    let Some(crate::file_interface::FileExportKind::Struct(strukt)) =
        interface.exports.get("Nexus").map(|export| &export.kind)
    else {
        panic!("Nexus exports as a struct");
    };
    let gates = |name: &str| {
        strukt
            .methods
            .iter()
            .find(|method| method.name == name)
            .unwrap_or_else(|| panic!("method {name} stays exported"))
            .unavailable
            .clone()
    };
    let gate = |target: Option<&str>, reason: Option<&str>| {
        vec![crate::file_interface::InterfaceUnavailable {
            target: target.map(str::to_owned),
            reason: reason.map(str::to_owned),
        }]
    };
    assert_eq!(gates("claude"), gate(None, Some("pending host dispatch")));
    assert_eq!(gates("mitte"), gate(Some("ts"), None));
    assert!(gates("lege").is_empty());
}

#[test]
fn library_bindings_route_reports_missing_source_read() {
    let (_td, dir) = temp_dir("lib-bindings-missing-source");
    let missing = dir.join("absent.fab");
    let resolve = |_roots: &std::collections::BTreeSet<PathBuf>| crate::driver::TargetBindings::default();
    let result = super::compile_cli_input_with_library_bindings(
        &[missing.display().to_string()],
        false,
        crate::codegen::Target::HirRust,
        None,
        crate::codegen::OutputMode::Application,
        None,
        &resolve,
        None,
    );
    assert!(result.output.is_none(), "a missing source must not produce output");
    crate::test_support::assert_has_error_issue(&result.diagnostics, "IO001", "source_read_failed");
}

#[test]
fn unresolved_norma_import_is_unknown_library_module_before_emit() {
    let (_td, home) = temp_dir("unknown-norma-module");
    let app = home.join("main.fab");
    fs::write(
        &app,
        "+++\nlocale = \"en\"\n+++\n\nimport from \"norma:nonexistent\" nonexistent\n\nmain {\n    print \"unreachable\"\n}\n",
    )
    .expect("write app");

    for target in [crate::codegen::Target::HirGo, crate::codegen::Target::MirSexp] {
        let result = crate::program::compile_cli_path(&app, false, target);
        assert!(result.output.is_none(), "unknown provider import must not emit for {target:?}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.issue() == Some("unknown_library_module")
                    && diagnostic.code == Some("PKG001")
                    && diagnostic
                        .args
                        .iter()
                        .any(|arg| arg.name == "specifier" && arg.value == "norma:nonexistent")
            }),
            "expected PKG001 unknown_library_module for {target:?}, got {:?}",
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code, diagnostic.issue(), diagnostic.args.clone()))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn resolvable_norma_import_is_not_unknown_library_module() {
    let (_td, home) = temp_dir("known-norma-module");
    let app = home.join("main.fab");
    fs::write(
        &app,
        "+++\nlocale = \"la\"\n+++\nimporta ex \"norma:text\" text\n\nincipit {\n    nota \"ok\"\n}\n",
    )
    .expect("write app");

    let result = crate::program::compile_cli_path(&app, false, crate::codegen::Target::HirGo);
    assert!(
        result.success(),
        "resolvable Norma import should compile: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue(), diagnostic.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("unknown_library_module")),
        "resolvable Norma module must not be reported as unknown"
    );
}

// ── frontmatter locales fail closed and resolve on every program lane ──
//
// A declared `+++ locale = "…" +++` that cannot be resolved must stop the
// compile with its LOCALE003 diagnostic: the program layer
// used to swallow the resolver error, substitute the session pack, install it
// as an explicit override, and the driver then short-circuited past the
// malformed declaration — the source silently compiled as English. The
// discriminators below install that exact regression condition: a
// non-explicit session English pack over an English body that would compile
// cleanly if the error were swallowed again.

/// A config with no language-pack root and a loader serving only `en` and
/// `la`: the installed-binary condition, where only the embedded packs remain.
fn loader_only_config() -> crate::driver::Config {
    let en = en_locale_pack();
    let la = la_locale_pack();
    let mut config = crate::driver::Config::default();
    config.locale_root = None;
    config.with_locale_pack_loader(crate::driver::LocalePackLoader::new(move |locale_id| match locale_id {
        "en" => Ok(en.clone()),
        "la" => Ok(la.clone()),
        other => Err(format!("no pack for locale '{other}'")),
    }))
}

/// The discriminator condition of the swallowed-error bug: the loader-only
/// config plus a **non-explicit** session English pack. A declaration that
/// still resolves must win over it; a declaration that fails must stop the
/// compile instead of reading an English body under this fallback.
fn english_session_fallback() -> crate::driver::Config {
    loader_only_config().with_locale_pack(en_locale_pack())
}

/// Assert the compile stopped with no output on an error-severity `LOCALE003`
/// carrying the named issue: the previously swallowed resolver error, now the
/// leading failure.
fn assert_locale003_error(result: &crate::CompileResult, issue: &str, label: &str) {
    assert!(result.output.is_none(), "{label}: a failing declaration must not emit");
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.is_error() && diagnostic.code == Some("LOCALE003") && diagnostic.issue() == Some(issue)
        }),
        "{label}: expected error-severity LOCALE003/{issue}, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue(), diagnostic.severity))
            .collect::<Vec<_>>()
    );
}

fn compile_with_config(config: crate::driver::Config, source: &str) -> crate::CompileResult {
    let (_td, home) = temp_dir("locale-fail");
    let app = home.join("main.fab");
    fs::write(&app, source).expect("write app");
    crate::Compiler::new(config).compile(&app)
}

#[test]
fn program_compile_fails_closed_on_non_string_frontmatter_locale() {
    // With a non-explicit English session pack installed, the pre-fix
    // swallow read the English body under the session pack and compiled
    // silently; the fix must surface the LOCALE003 not_a_string error.
    let result = compile_with_config(english_session_fallback(), "+++\nlocale = 7\n+++\n\n@ public\nfn salve() { }\n");
    assert_locale003_error(&result, "not_a_string", "non-string locale");
}

#[test]
fn program_compile_fails_closed_on_invalid_frontmatter_locale_ids() {
    for (label, locale) in [
        ("empty", "\"\""),
        ("path shaped", "\"../escape\""),
        ("absolute", "\"/etc\""),
    ] {
        let source = format!("+++\nlocale = {locale}\n+++\n\n@ public\nfn salve() {{ }}\n");
        let result = compile_with_config(english_session_fallback(), &source);
        assert_locale003_error(&result, "invalid_id", label);
    }
}

#[test]
fn program_compile_fails_closed_on_unloadable_frontmatter_locale() {
    let result = compile_with_config(
        english_session_fallback(),
        "+++\nlocale = \"zh-Hant\"\n+++\n\n@ public\nfn salve() { }\n",
    );
    assert_locale003_error(&result, "load_failed", "unloadable locale");
}

#[test]
fn program_compile_fails_closed_without_any_pack_source() {
    // No root, no loader: the declared locale has no source at all — only a
    // non-explicit session English pack remains. The resolver's own
    // `no_root` diagnostic is the failure, never a silent English read.
    let mut config = crate::driver::Config::default();
    config.locale_root = None;
    let config = config.with_locale_pack(en_locale_pack());
    let result = compile_with_config(config, "+++\nlocale = \"la\"\n+++\n\n@ public\nfn salve() { }\n");
    assert_locale003_error(&result, "no_root", "no pack source");
}

#[test]
fn program_compile_fails_closed_on_malformed_root_pack() {
    // A present but malformed root pack fails loud over every later chain
    // step that could mask it: the configured loader loads `la` fine (the
    // root serves the id first, so the loader is never reached) and a
    // non-explicit session English pack sits over an English body.
    let (_td, home) = temp_dir("malformed-pack");
    let broken = home.join("la");
    fs::create_dir_all(&broken).expect("pack dir");
    fs::write(broken.join("pack.toml"), "not [ a valid pack").expect("malformed pack");
    let config = english_session_fallback().with_locale_root(home);
    let result = compile_with_config(config, "+++\nlocale = \"la\"\n+++\n\n@ public\nfn salve() { }\n");
    assert_locale003_error(&result, "load_failed", "malformed root pack");
}

#[test]
fn program_compile_still_honours_a_resolvable_frontmatter_locale() {
    // Declaration-over-session precedence: the loadable `la` declaration
    // wins against a non-explicit session English pack, so the Latin body
    // parses as Latin and the compile still emits.
    let config = english_session_fallback().with_target(crate::codegen::Target::HirRust);
    let result = compile_with_config(config, "+++\nlocale = \"la\"\n+++\n\n@ publica\nfunctio salve() { }\n");
    assert!(
        result.success(),
        "a loadable declared locale must still compile over a session fallback: {:?}",
        result.diagnostics
    );
    assert!(result.output.is_some(), "a loadable declared locale must emit output");
}

#[test]
fn program_compile_explicit_session_pack_overrides_frontmatter_declaration() {
    // Control for driver policy: an explicitly resolved session pack (CLI
    // `--locale en` semantics) is authoritative, so even an unloadable
    // declaration never reaches the resolver and the English body compiles.
    // The fail-closed tests above must not change this precedence.
    let config = loader_only_config().with_resolved_locale_pack(en_locale_pack());
    let result = compile_with_config(config, "+++\nlocale = \"zh-Hant\"\n+++\n\n@ public\nfn salve() { }\n");
    assert!(
        result.success(),
        "an explicit session pack must stay authoritative over the declaration: {:?}",
        result.diagnostics
    );
    assert!(result.output.is_some(), "an explicit session pack must emit output");
}

// ── provider analysis derives its locale policy from the session ──
//
// The provider-module analysis used to mint a fresh default config and
// re-discover a workspace language-pack root: a session whose
// root served the provider's declared locale (or a loader-installed installed
// binary) never reached the provider parse, which failed closed as a
// `provider_module_analysis_failed` marker at Rust emission.

/// Plant a language-pack root serving one re-identified locale: the workspace
/// `en` pack as `prov-en`, so only the session's configured root can read a
/// source declaring it (the from_toml_path validation needs the pack's
/// exemplar tree beside it).
fn plant_reidentified_pack(root: &Path, source_id: &str, planted_id: &str) {
    let workspace = crate::locale::workspace_locale_root().join(source_id);
    let destination = root.join(planted_id);
    copy_locale_dir(&workspace, &destination);
    let pack_path = destination.join("pack.toml");
    let pack_toml = fs::read_to_string(&pack_path).expect("planted pack");
    fs::write(
        &pack_path,
        pack_toml.replacen(&format!("id = \"{source_id}\""), &format!("id = \"{planted_id}\""), 1),
    )
    .expect("re-identified pack");
}

fn copy_locale_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("planted dir");
    for entry in fs::read_dir(source).expect("planted source") {
        let entry = entry.expect("planted entry").path();
        let target = destination.join(entry.file_name().expect("name"));
        if entry.is_dir() {
            copy_locale_dir(&entry, &target);
        } else {
            fs::copy(&entry, &target).expect("planted file");
        }
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn provider_unit_analysis_derives_locale_root_from_session_config() {
    // The provider declares a locale only the session's planted root serves.
    // A fresh-minted default config re-discovers the workspace root, misses
    // `prov-en`, parses the English provider on the built-in Latin surface,
    // and Rust emission fails with the provider-analysis marker; the derived
    // config reads the provider under its declared locale and emits it.
    let (_td, home) = temp_dir("provider-session-root");
    plant_reidentified_pack(&home, "en", "prov-en");
    let provider = home.join("valida.fab");
    fs::write(
        &provider,
        "+++\nlocale = \"prov-en\"\n+++\n\n@ public\nfn nomen(string s) → string {\n    return s\n}\n",
    )
    .expect("provider source");
    let app = home.join("app.fab");
    fs::write(
        &app,
        "+++\nlocale = \"prov-en\"\n+++\nimport from \"./valida\" valida\n\nmain {\n    print valida.nomen(\"salve\")\n}\n",
    )
    .expect("app source");

    let config = crate::driver::Config::default()
        .with_target(crate::codegen::Target::HirRust)
        .with_locale_root(home);
    let result = crate::Compiler::new(config).compile(&app);
    assert!(
        result.success(),
        "the session's locale root must serve the provider's declared locale: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("pub mod valida"),
        "expected the analyzed provider module, got:\n{}",
        output.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn provider_unit_analysis_resolves_declared_locale_through_loader_without_root() {
    // The installed-binary condition (need done_when): no language
    // pack root at all, the session's loader serves the provider's declared
    // frontmatter locale.
    let (_td, home) = temp_dir("provider-loader");
    let provider = home.join("valida.fab");
    fs::write(
        &provider,
        "+++\nlocale = \"la\"\n+++\n\n@ publica\nfunctio nomen(textus s) → textus {\n    redde s\n}\n",
    )
    .expect("provider source");
    let app = home.join("app.fab");
    fs::write(
        &app,
        "+++\nlocale = \"en\"\n+++\nimport from \"./valida\" valida\n\nmain {\n    print valida.nomen(\"salve\")\n}\n",
    )
    .expect("app source");

    let result = crate::Compiler::new(loader_only_config().with_target(crate::codegen::Target::HirRust)).compile(&app);
    assert!(
        result.success(),
        "the session's loader must serve the provider's declared locale: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("pub mod valida"),
        "expected the analyzed provider module, got:\n{}",
        output.code
    );
}

// ── manifest reader locales resolve through the session config ──
//
// `package_reader_locale_pack` used to resolve a package `[reader] locale`
// through a freshly minted default config: a session root or
// loader never reached the manifest locale.

#[test]
fn manifest_reader_locale_resolves_through_session_loader_without_root() {
    let (_td, home) = temp_dir("manifest-loader");
    let tela = home.join("tela");
    fs::create_dir_all(tela.join("src")).expect("tela src");
    write_provider_manifest(&tela, "tela", "la");
    fs::write(
        tela.join("src/valida.fab"),
        r#"
@ publica
functio nomen_tag(textus nomen) → bivalens {
    redde nomen ≢ ""
}
"#,
    )
    .expect("provider source");
    let valida = tela.join("src/valida.fab");

    let session = crate::driver::Session::new(loader_only_config());
    let pack = super::package_reader_locale_pack(&session, &valida)
        .expect("the session's loader must serve the manifest locale");
    assert_eq!(pack.metadata.id, "la", "the manifest-selected pack is the la pack");

    // No loader and no root: the manifest locale resolves to nothing and the
    // documented fallback (the importing module's pack) applies — asserted
    // here as the intentional `None`.
    let mut bare = crate::driver::Config::default();
    bare.locale_root = None;
    let bare_session = crate::driver::Session::new(bare);
    assert!(
        super::package_reader_locale_pack(&bare_session, &valida).is_none(),
        "an unresolvable manifest locale falls back, it does not error"
    );
}

#[test]
fn manifest_reader_locale_resolves_through_session_root() {
    let (_td, home) = temp_dir("manifest-root");
    plant_reidentified_pack(&home, "la", "prov-la");
    let tela = home.join("tela");
    fs::create_dir_all(tela.join("src")).expect("tela src");
    fs::write(
        tela.join("faber.toml"),
        "[package]\nname = \"tela\"\n[reader]\nlocale = \"prov-la\"\n[paths]\nsource = \"src\"\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n",
    )
    .expect("manifest");
    let valida = tela.join("src/valida.fab");
    fs::write(&valida, "@ publica\nfunctio salve() { }\n").expect("provider source");

    let config = crate::driver::Config::default().with_locale_root(home);
    let session = crate::driver::Session::new(config);
    let pack = super::package_reader_locale_pack(&session, &valida)
        .expect("the session's configured root must serve the manifest locale");
    assert_eq!(pack.metadata.id, "prov-la");
}

#[test]
fn contract_walk_resolves_manifest_locale_through_session_loader_without_root() {
    // End to end (need done_when): an en consumer imports a Latin
    // provider whose package manifest selects `la`; with no workspace root the
    // session's loader serves the manifest locale and the interface installs.
    let (_td, home) = temp_dir("manifest-loader-contract");
    let tela = home.join("tela");
    fs::create_dir_all(tela.join("src")).expect("tela src");
    write_provider_manifest(&tela, "tela", "la");
    fs::write(
        tela.join("src/valida.fab"),
        r#"
@ publica
functio nomen_tag(textus nomen) → bivalens {
    redde nomen ≢ ""
}
"#,
    )
    .expect("provider source");
    let app = home.join("app.fab");
    fs::write(
        &app,
        "+++\nlocale = \"en\"\n+++\nimport from \"tela:valida\" valida\n\nmain {\n    print valida.nomen_tag(\"a\")\n}\n",
    )
    .expect("app source");
    let source = fs::read_to_string(&app).expect("read app");

    let _guard = LibraryHomeGuard::set(&home);
    let session = crate::driver::Session::new(loader_only_config());
    let contract = super::import_contract_for_session(&session, &app, &source, None, None);
    let interface = contract
        .file_interfaces
        .get("valida")
        .expect("the manifest-selected locale must parse the provider");
    assert!(
        interface.exports.contains_key("nomen_tag"),
        "expected the Latin provider's export, got {:?}",
        interface.exports.keys().collect::<Vec<_>>()
    );
}
