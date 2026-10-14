use super::*;
use radix_module::program::program_declaration_surface;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "radix-program-library-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

// ── specifier shape ────────────────────────────────────────────────────────

#[test]
fn resolve_returns_none_for_non_provider_specifiers() {
    let resolver = LibraryResolver::with_optional_home(None);
    assert!(matches!(resolver.resolve("./lib"), Ok(None)));
    assert!(matches!(resolver.resolve("../lib"), Ok(None)));
    assert!(matches!(resolver.resolve("plain/module"), Ok(None)));
}

#[test]
fn resolve_rejects_old_builtin_norma_slash_forms() {
    let resolver = LibraryResolver::with_optional_home(None);
    let err = resolver
        .resolve("norma/json")
        .expect_err("norma slash must be rejected");
    assert!(matches!(
        err,
        LibraryResolveError::OldBuiltinNormaSpecifier { replacement, .. }
            if replacement == "norma:json"
    ));
}

#[test]
fn resolve_rejects_malformed_provider_specifiers() {
    let resolver = LibraryResolver::with_optional_home(None);
    for (specifier, reason) in [
        (":mod", "provider segment must not be empty"),
        (
            "bad provider:mod",
            "provider segment must contain only ASCII letters, numbers, underscore, or hyphen",
        ),
        ("prov:", "module path segment must not be empty"),
        (
            "prov:a:b",
            "library specifier must contain exactly one provider separator",
        ),
        (
            "prov:a/../b",
            "module path must not contain empty, dot, or dot-dot segments",
        ),
    ] {
        let err = resolver.resolve(specifier).expect_err(specifier);
        assert!(
            matches!(&err, LibraryResolveError::ProviderSpecifierInvalid { specifier: s, reason: r } if s == specifier && r == reason),
            "unexpected error for {specifier}: {err:?}"
        );
    }
}

// ── home / provider resolution ─────────────────────────────────────────────

#[test]
fn resolve_missing_library_home_when_no_home() {
    let resolver = LibraryResolver::with_optional_home(None);
    let err = resolver.resolve("prov:mod").expect_err("no home must fail");
    assert!(matches!(
        err,
        LibraryResolveError::LibraryHomeMissing { hint, .. }
            if hint.contains("--library-home")
    ));
}

#[test]
fn resolve_unknown_provider_from_empty_home() {
    let root = temp_root("unknown-provider");
    let resolver = LibraryResolver::new(&root);
    let err = resolver
        .resolve("prov:mod")
        .expect_err("empty home must fail");
    assert!(matches!(
        err,
        LibraryResolveError::ProviderUnknown { provider, .. } if provider == "prov"
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_undeclared_provider_when_declarations_exist() {
    let root = temp_root("undeclared-provider");
    let resolver = LibraryResolver::new(&root).with_package_lock(
        BTreeMap::from([("other".to_owned(), "1.0.0".to_owned())]),
        BTreeMap::new(),
    );
    let err = resolver
        .resolve("prov:mod")
        .expect_err("undeclared must fail");
    assert!(matches!(
        err,
        LibraryResolveError::ProviderUndeclared { provider, .. } if provider == "prov"
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_installed_provider_from_home_manifest() {
    let root = temp_root("installed-provider");
    let provider_repo = root.join("prov");
    fs::create_dir_all(provider_repo.join("src")).expect("mkdir src");
    fs::write(
        provider_repo.join("faber.toml"),
        "[package]\nname = \"prov\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[library]\nprovider = \"prov\"\n\n[paths]\nsource = \"src\"\n\n[build]\nkind = \"lib\"\ntargets = [\"fhir\"]\n",
    )
    .expect("write manifest");
    fs::write(
        provider_repo.join("src/mod.fab"),
        "functio f() → vacuum { tacet }\n",
    )
    .unwrap();

    let resolver = LibraryResolver::new(&root);
    let resolved = resolver
        .resolve("prov:mod")
        .expect("resolve")
        .expect("provider must resolve");
    assert_eq!(resolved.package, "prov");
    assert_eq!(resolved.module_path, vec!["mod"]);
    assert!(resolved.interface_path.ends_with("prov/src/mod.fab"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_installed_provider_falls_back_to_src_layout() {
    // A provider repo without a top-level faber.toml but with `src/` still
    // resolves (legacy library-home layout).
    let root = temp_root("src-fallback");
    fs::create_dir_all(root.join("prov/src")).expect("mkdir src");
    fs::write(
        root.join("prov/src/mod.fab"),
        "functio f() → vacuum { tacet }\n",
    )
    .unwrap();

    let resolver = LibraryResolver::new(&root);
    let resolved = resolver
        .resolve("prov:mod")
        .expect("resolve")
        .expect("provider must resolve");
    assert!(resolved.interface_path.ends_with("prov/src/mod.fab"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_unknown_module_lists_known_modules() {
    let root = temp_root("unknown-module");
    let provider_repo = root.join("prov");
    fs::create_dir_all(provider_repo.join("src")).expect("mkdir src");
    fs::write(
        provider_repo.join("src/known.fab"),
        "functio f() → vacuum { tacet }\n",
    )
    .unwrap();

    let resolver = LibraryResolver::new(&root);
    let err = resolver
        .resolve("prov:missing")
        .expect_err("missing module must fail");
    assert!(matches!(
        err,
        LibraryResolveError::ModuleUnknown { package, known_modules, .. }
            if package == "prov" && known_modules == vec!["known".to_owned()]
    ));
    let _ = fs::remove_dir_all(&root);
}

// ── lock / declared resolution ─────────────────────────────────────────────

#[test]
fn resolve_locked_package_preferred_over_home() {
    let root = temp_root("locked-preferred");
    let interface_root = root.join("pkg/src");
    fs::create_dir_all(&interface_root).expect("mkdir interface");
    fs::write(
        interface_root.join("mod.fab"),
        "functio f() → vacuum { tacet }\n",
    )
    .unwrap();

    let locked = BTreeMap::from([(
        "pkg".to_owned(),
        LockedLibraryPackage {
            name: "pkg".to_owned(),
            version: "1.0.0".to_owned(),
            interface_root: interface_root.clone(),
        },
    )]);
    let resolver = LibraryResolver::new(&root).with_package_lock(BTreeMap::new(), locked);
    let resolved = resolver
        .resolve("pkg:mod")
        .expect("resolve")
        .expect("locked package must resolve");
    assert_eq!(resolved.package, "pkg");
    assert!(resolved.interface_path.ends_with("pkg/src/mod.fab"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_unknown_module_for_locked_package() {
    let root = temp_root("locked-unknown");
    let interface_root = root.join("pkg/src");
    fs::create_dir_all(&interface_root).expect("mkdir interface");
    fs::write(
        interface_root.join("known.fab"),
        "functio f() → vacuum { tacet }\n",
    )
    .unwrap();

    let locked = BTreeMap::from([(
        "pkg".to_owned(),
        LockedLibraryPackage {
            name: "pkg".to_owned(),
            version: "1.0.0".to_owned(),
            interface_root: interface_root.clone(),
        },
    )]);
    let resolver =
        LibraryResolver::with_optional_home(None).with_package_lock(BTreeMap::new(), locked);
    let err = resolver
        .resolve("pkg:missing")
        .expect_err("missing locked module must fail");
    assert!(matches!(
        err,
        LibraryResolveError::ModuleUnknown { package, known_modules, .. }
            if package == "pkg" && known_modules == vec!["known".to_owned()]
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_declared_but_unlocked_package_is_missing_locked_diagnostic() {
    let resolver = LibraryResolver::with_optional_home(None).with_package_lock(
        BTreeMap::from([("pkg".to_owned(), "2.0.0".to_owned())]),
        BTreeMap::new(),
    );
    let err = resolver
        .resolve("pkg:mod")
        .expect_err("declared without lock must fail");
    assert!(matches!(
        err,
        LibraryResolveError::LockedPackageMissing { provider, version, .. }
            if provider == "pkg" && version.as_deref() == Some("2.0.0")
    ));
}

// ── provider segment validation ────────────────────────────────────────────

#[test]
fn is_valid_provider_segment_accepts_alphanumeric_underscore_hyphen() {
    assert!(is_valid_provider_segment("norma"));
    assert!(is_valid_provider_segment("my-provider_2"));
    assert!(!is_valid_provider_segment(""));
    assert!(!is_valid_provider_segment("bad provider"));
    assert!(!is_valid_provider_segment("bad.provider"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn norma_http_generates_ordinary_rust_module() {
    // The drifted `is_builtin_norma_http` special-case mapped Latin verbs to
    // `faber::http::*` (which does not exist) and skipped rust-module emit.
    // norma:http now rides the ordinary generated-library path.
    let root = temp_root("norma-http-ordinary");
    fs::create_dir_all(root.join("norma/src")).expect("mkdir norma");
    fs::write(
        root.join("norma/src/http.fab"),
        "functio get(textus url) → numerus { redde 0 }\n",
    )
    .expect("write http.fab");

    let resolver = LibraryResolver::new(&root);
    let module = resolver
        .resolve("norma:http")
        .expect("resolve")
        .expect("norma:http must resolve");
    let import = crate::LibraryImportBinding {
        binding: "http".to_owned(),
        visibility: radix_module::syntax::Visibility::Publica,
        import_span: radix_module::lexer::Span::new(0, 0),
        module,
    };
    let mut cache =
        LibraryInterfaceCache::default().with_locale_loader(crate::test_support::test_locale_pack);
    assert!(
        library_generates_rust_module(&import, &mut cache).expect("generate decision"),
        "norma:http must generate a rust module after the special-case deletion"
    );
    let _ = fs::remove_dir_all(&root);
}

// ── export-visibility resolver (D5.1) ──────────────────────────────────────

fn library_surfaces_for(
    label: &str,
    source: &str,
) -> (
    BTreeMap<String, VisibilityTier>,
    BTreeMap<String, VisibilityTier>,
) {
    let root = temp_root(label);
    fs::create_dir_all(root.join("norma/src")).expect("mkdir norma");
    fs::write(root.join("norma/src/visibilis.fab"), source).expect("write visibilis.fab");
    let resolver = LibraryResolver::new(&root);
    let module = resolver
        .resolve("norma:visibilis")
        .expect("resolve")
        .expect("norma:visibilis must resolve");
    let import = crate::LibraryImportBinding {
        binding: "visibilis".to_owned(),
        visibility: radix_module::syntax::Visibility::Publica,
        import_span: radix_module::lexer::Span::new(0, 0),
        module,
    };
    let mut cache =
        LibraryInterfaceCache::default().with_locale_loader(crate::test_support::test_locale_pack);
    let exports =
        library_interface_export_surface(&import, &resolver, &mut cache).expect("export surface");
    let cached = load_cached_library_interface(&import.module, &mut cache).expect("cached");
    let declarations = program_declaration_surface(&cached.program, &cached.interner);
    let _ = fs::remove_dir_all(&root);
    (exports, declarations)
}

const VISIBILITY_TIERS_SOURCE: &str = "
@ publica
functio aperta() → vacuum { }

@ interna
functio clausa() → vacuum { }

@ privata
functio secretum() → vacuum { }

functio occultum() → vacuum { }
";

#[test]
fn unmarked_library_declaration_is_module_private_in_export_surface() {
    let (exports, _) = library_surfaces_for("unmarked-export", VISIBILITY_TIERS_SOURCE);
    assert_eq!(
        exports,
        BTreeMap::from([
            ("aperta".to_owned(), VisibilityTier::Publica),
            ("clausa".to_owned(), VisibilityTier::Interna),
        ]),
        "unmarked and `@ privata` declarations must be absent from the export surface"
    );
}

#[test]
fn unmarked_library_declaration_is_module_private_in_declaration_surface() {
    let (_, declarations) = library_surfaces_for("unmarked-declaration", VISIBILITY_TIERS_SOURCE);
    assert_eq!(
        declarations.get("occultum"),
        Some(&VisibilityTier::Privata),
        "an unmarked declaration exists but is module-private"
    );
    assert_eq!(declarations.get("secretum"), Some(&VisibilityTier::Privata));
    assert_eq!(declarations.get("aperta"), Some(&VisibilityTier::Publica));
    assert_eq!(declarations.get("clausa"), Some(&VisibilityTier::Interna));
}

/// Bodyless binding declarations (bound by a target binding manifest) follow
/// the same D5.1 rule as bodied ones: `@ publica` exports them, unmarked keeps
/// them module-private. There is no by-construction export exception.
const BODYLESS_BINDING_SOURCE: &str = "
@ publica
functio ligata(textus via) → textus ⇥ textus

functio adiutrix(textus via) → textus
";

#[test]
fn bodyless_binding_declaration_exports_only_when_marked() {
    let (exports, declarations) = library_surfaces_for("bodyless-binding", BODYLESS_BINDING_SOURCE);
    assert_eq!(
        exports,
        BTreeMap::from([("ligata".to_owned(), VisibilityTier::Publica)]),
        "a marked bodyless binding declaration is exported; an unmarked one is not"
    );
    assert_eq!(declarations.get("adiutrix"), Some(&VisibilityTier::Privata));
}
