use super::*;
use crate::discovery::PackageSpec;
use crate::library::{LibraryResolver, LockedLibraryPackage};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "radix-program-import-graph-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

fn parse_file(path: PathBuf, source: &str) -> PackageFile {
    let lex = radix_module::lexer::lex(source);
    assert!(lex.success(), "lex failed for {path:?}");
    let parse = radix_module::parser::parse_with_options(
        lex,
        radix_module::parser::ParseOptions {
            allow_bodyless_functions: false,
        },
    );
    assert!(parse.success(), "parse failed for {path:?}");
    PackageFile {
        path,
        module_segments: Vec::new(),
        raw_source: source.to_owned(),
        source: source.to_owned(),
        body_byte_offset: 0,
        frontmatter: None,
        locale_pack: None,
        program: parse.program.expect("program"),
        interner: parse.interner,
        library_imports: Vec::new(),
        expanded_library_imports: Vec::new(),
    }
}

fn spec(source_root: PathBuf, entry: PathBuf) -> PackageSpec {
    PackageSpec {
        package_root: source_root.clone(),
        source_root,
        entry,
        templates: BTreeMap::new(),
        manifest_backed: false,
    }
}

// ── resolve_import ─────────────────────────────────────────────────────────

#[test]
fn resolve_import_local_relative_module() {
    let root = temp_root("local-relative");
    fs::write(root.join("lib.fab"), "functio f() → vacuum { tacet }\n").unwrap();
    let from_file = root.join("main.fab");
    let package = spec(root.clone(), from_file.clone());
    let resolver = LibraryResolver::with_optional_home(None);

    match resolve_import(&package, &resolver, &from_file, "./lib") {
        ImportResolution::Local(target) => {
            assert!(target.ends_with("lib.fab"), "target: {target:?}")
        }
        other => panic!("expected local resolution, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_local_source_root_form() {
    let root = temp_root("local-source-root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/math.fab"),
        "functio f() → vacuum { tacet }\n",
    )
    .unwrap();
    let from_file = root.join("src/main.fab");
    let package = spec(root.join("src"), from_file.clone());
    let resolver = LibraryResolver::with_optional_home(None);

    match resolve_import(&package, &resolver, &from_file, "math") {
        ImportResolution::Local(target) => {
            assert!(target.ends_with("src/math.fab"), "target: {target:?}")
        }
        other => panic!("expected local resolution, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_self_provider_module_is_local() {
    let root = temp_root("self-provider");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/dom.fab"), "fn f() → vacuum { tacet }\n").unwrap();
    let from_file = root.join("src/app.fab");
    let package = spec(root.join("src"), from_file.clone());
    let mut locked = BTreeMap::new();
    locked.insert(
        "web".to_owned(),
        LockedLibraryPackage {
            name: "web".to_owned(),
            version: "0.1.0".to_owned(),
            interface_root: root.join("src"),
        },
    );
    let resolver =
        LibraryResolver::with_optional_home(None).with_package_lock(BTreeMap::new(), locked);

    match resolve_import(&package, &resolver, &from_file, "web:dom") {
        ImportResolution::Local(target) => {
            assert!(
                target.ends_with("src/dom.fab"),
                "self-provider import must resolve in-tree: {target:?}"
            );
        }
        other => panic!("expected local self-import, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_external_provider_stays_library() {
    let root = temp_root("external-provider");
    fs::create_dir_all(root.join("prov/src")).unwrap();
    fs::create_dir_all(root.join("app/src")).unwrap();
    fs::write(root.join("prov/src/mod.fab"), "fn f() → vacuum { tacet }\n").unwrap();
    let from_file = root.join("app/src/main.fab");
    let package = spec(root.join("app/src"), from_file.clone());
    let resolver = LibraryResolver::new(&root);

    match resolve_import(&package, &resolver, &from_file, "prov:mod") {
        ImportResolution::Library(module) => {
            assert_eq!(module.package, "prov");
            assert!(module.interface_path.ends_with("prov/src/mod.fab"));
        }
        other => panic!("expected external library resolution, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_nested_library_under_consumer_dir_stays_library() {
    // Manifestless consumer at `<root>/main.fab` with `with_library_home(<root>/lib)`
    // used to prefix-match `testlib` as a self-import because source_root is
    // `<root>`. The library has its own faber.toml and must stay Library.
    let root = temp_root("nested-lib-under-consumer");
    fs::create_dir_all(root.join("lib/testlib/src")).unwrap();
    fs::write(
        root.join("lib/testlib/faber.toml"),
        r#"[package]
name = "testlib"
version = "0.1.0"
edition = "2026"

[library]
provider = "testlib"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]
"#,
    )
    .unwrap();
    fs::write(
        root.join("lib/testlib/src/outer.fab"),
        "functio computa(numerus value) → numerus {\n    redde value + 1\n}\n",
    )
    .unwrap();
    let from_file = root.join("main.fab");
    let package = spec(root.clone(), from_file.clone());
    let resolver = LibraryResolver::new(root.join("lib"));

    match resolve_import(&package, &resolver, &from_file, "testlib:outer") {
        ImportResolution::Library(module) => {
            assert_eq!(module.package, "testlib");
            assert!(
                module.interface_path.ends_with("lib/testlib/src/outer.fab"),
                "interface: {:?}",
                module.interface_path
            );
        }
        other => panic!("expected external nested library, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_library_provider() {
    let root = temp_root("library-provider");
    fs::create_dir_all(root.join("prov/src")).unwrap();
    fs::write(
        root.join("prov/src/mod.fab"),
        "functio f() → vacuum { tacet }\n",
    )
    .unwrap();
    let package = spec(root.join("app"), root.join("app/main.fab"));
    let resolver = LibraryResolver::new(&root);

    match resolve_import(&package, &resolver, &root.join("app/main.fab"), "prov:mod") {
        ImportResolution::Library(module) => {
            assert_eq!(module.package, "prov");
            assert!(module.interface_path.ends_with("prov/src/mod.fab"));
        }
        other => panic!("expected library resolution, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_unsupported_at_prefixed_and_url() {
    let root = temp_root("unsupported");
    fs::create_dir_all(root.join("src")).unwrap();
    let package = spec(root.join("src"), root.join("src/main.fab"));
    let resolver = LibraryResolver::with_optional_home(None);
    let from_file = root.join("src/main.fab");

    assert!(matches!(
        resolve_import(&package, &resolver, &from_file, "@imperia"),
        ImportResolution::Unsupported
    ));
    // `://` specifiers are provider-shaped: the provider segment parses, the
    // module path is malformed, so the resolver reports an invalid specifier.
    assert!(matches!(
        resolve_import(&package, &resolver, &from_file, "https://example.com/x"),
        ImportResolution::Error(_)
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_proba_specifier_is_forbidden() {
    let root = temp_root("proba-forbidden");
    let package = spec(root.clone(), root.join("main.fab"));
    let resolver = LibraryResolver::with_optional_home(None);
    let from_file = root.join("main.fab");

    match resolve_import(&package, &resolver, &from_file, "helpers.proba") {
        ImportResolution::Error(diag) => {
            assert_eq!(diag.issue(), Some("proba_import_forbidden"))
        }
        other => panic!("expected proba-import error, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_proba_local_target_is_forbidden() {
    let root = temp_root("proba-local-target");
    fs::write(root.join("helpers.proba"), "probandum \"h\" {}\n").unwrap();
    let package = spec(root.clone(), root.join("main.fab"));
    let resolver = LibraryResolver::with_optional_home(None);
    let from_file = root.join("main.fab");

    // An explicit `.proba` specifier that names a real on-disk test source is
    // rejected before any local resolution runs.
    match resolve_import(&package, &resolver, &from_file, "./helpers.proba") {
        ImportResolution::Error(diag) => {
            assert_eq!(diag.issue(), Some("proba_import_forbidden"))
        }
        other => panic!("expected proba-import error, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_kernel_import_is_script_mode_only() {
    let root = temp_root("kernel");
    let package = spec(root.clone(), root.join("main.fab"));
    let resolver = LibraryResolver::with_optional_home(None);
    let from_file = root.join("main.fab");

    match resolve_import(&package, &resolver, &from_file, "faber:kernel") {
        ImportResolution::Error(diag) => {
            assert_eq!(diag.issue(), Some("kernel_import_script_mode_only"))
        }
        other => panic!("expected kernel-import error, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_import_library_resolve_error_is_diagnostic() {
    let root = temp_root("library-error");
    let package = spec(root.clone(), root.join("main.fab"));
    let resolver = LibraryResolver::with_optional_home(None);
    let from_file = root.join("main.fab");

    match resolve_import(&package, &resolver, &from_file, "prov:mod") {
        ImportResolution::Error(diag) => {
            assert_eq!(diag.issue(), Some("missing_library_home"))
        }
        other => panic!("expected library resolve error, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

// ── cycle detection ────────────────────────────────────────────────────────

#[test]
fn detect_import_cycles_reports_cycle_with_diagnostic() {
    let root = temp_root("cycle");
    fs::write(root.join("a.fab"), "importa ex \"./b\" b\n").unwrap();
    fs::write(root.join("b.fab"), "importa ex \"./a\" a\n").unwrap();
    let package = spec(root.clone(), root.join("a.fab"));
    let files = vec![
        parse_file(root.join("a.fab"), "importa ex \"./b\" b\n"),
        parse_file(root.join("b.fab"), "importa ex \"./a\" a\n"),
    ];

    let diagnostics = detect_import_cycles(&package, &files);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("package_import_cycle")),
        "expected an import-cycle diagnostic, got {:?}",
        diagnostics
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn detect_import_cycles_clean_graph_has_no_diagnostics() {
    let root = temp_root("clean");
    fs::write(root.join("a.fab"), "importa ex \"./b\" b\n").unwrap();
    fs::write(root.join("b.fab"), "functio f() → vacuum { tacet }\n").unwrap();
    let package = spec(root.clone(), root.join("a.fab"));
    let files = vec![
        parse_file(root.join("a.fab"), "importa ex \"./b\" b\n"),
        parse_file(root.join("b.fab"), "functio f() → vacuum { tacet }\n"),
    ];

    assert!(detect_import_cycles(&package, &files).is_empty());
    let _ = fs::remove_dir_all(&root);
}
