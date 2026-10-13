//! Tests for `crate::hir::package`: package build/load round-trips,
//! determinism, and fail-closed loading.

use super::*;
use crate::codegen::{Target, generate_from_analyzed};
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::locale::{KeywordSurface, latin_locale_pack};
use crate::mir::MirLoweringOptions;

fn analyze(name: &str, source: &str) -> AnalyzedModule {
    analyze_with(name, source, Config::default())
}

fn analyze_with(name: &str, source: &str, config: Config) -> AnalyzedModule {
    let session = Session::new(config);
    analyze_source(&session, name, source, AnalyzeRequest::default()).expect("analyze test source")
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
    let bytes = encode_package_msgpack(&pkg).expect("encode package");

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

/// Extract the code string from a codegen [`crate::Output`] (Faber lane).
fn faber_code(output: &crate::Output) -> &str {
    match output {
        crate::Output::Faber(o) => &o.code,
        _ => panic!("unexpected non-Faber output variant in package test"),
    }
}

/// Loads a legacy v1 postcard envelope through the legacy codec and the
/// same reconstruction steps as [`load_package`] (FPF-4 proof seam: the
/// retired wire, exercised without the product loader).
fn load_package_legacy_postcard(bytes: &[u8]) -> LoadedHirPackage {
    let decoded = decode_package(bytes).expect("decode legacy postcard package");
    let mut modules = Vec::with_capacity(decoded.modules.len());
    for module in decoded.modules {
        let unit = crate::hir::artifact::reconstruct_unit(module.unit).expect("reconstruct legacy postcard unit");
        modules.push(LoadedHirModule {
            relative_path: module.path,
            module_segments: module.module_segments,
            is_entry: module.is_entry,
            export_names: module.export_names,
            local_links: module.local_links,
            library_imports: module.library_imports,
            unit,
        });
    }
    let nucleum = nucleum_metadata_from_loaded_modules(&modules).expect("legacy postcard nucleum metadata");
    LoadedHirPackage {
        identity: decoded.identity,
        entry_path: decoded.entry_path,
        entry_frontmatter: decoded.entry_frontmatter,
        modules,
        dependencies: decoded.dependencies,
        nucleum,
    }
}

#[test]
fn both_codec_loaded_package_equivalence() {
    // FPF-4 (post-FPF-5): one real analyzed package, encoded through both
    // codecs (the live MessagePack wire + the retired legacy postcard v1
    // envelope), loaded through the matching decode paths, must reconstruct
    // equal `LoadedHirPackage`s and emit identical canonical Faber from the
    // loaded HIR. The product loader (`load_package`) takes only the
    // MessagePack wire; the postcard side rides the legacy codec directly.
    let main = analyze("main.fab", main_equivalence_source());
    let aux = analyze("aux.fab", aux_equivalence_source());

    let mut main_input = input("main.fab", true, &main);
    main_input.export_names = vec!["computa".to_owned()];
    main_input.local_links = vec![LocalLinkWire { binding: "aux".to_owned(), target: "aux.fab".to_owned() }];
    main_input.library_imports = vec![LibraryImportWire {
        binding: "mathesis".to_owned(),
        package: "norma".to_owned(),
        module: vec!["mathesis".to_owned()],
    }];
    let mut aux_input = input("aux.fab", false, &aux);
    aux_input.export_names = vec!["auxilia".to_owned()];

    let mut pkg = build_package(
        PackageIdentityWire { name: "exemplum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![aux_input, main_input],
        sample_dependencies(),
    )
    .expect("build package");

    // Live wire (FPF-5): `.fhirpkg` carries the v2 MessagePack envelope and
    // `load_package` decodes it directly.
    let msgpack_bytes = encode_package_msgpack(&pkg).expect("encode package (msgpack)");
    let loaded_msgpack = load_package(&msgpack_bytes).expect("load package (msgpack)");

    // Legacy v1 postcard envelope: still decodable through the legacy codec
    // for the equivalence proof, but rejected by the product loader with the
    // named legacy error (no fallback decode).
    pkg.package_schema_version = LEGACY_PACKAGE_SCHEMA_VERSION;
    let postcard_bytes = encode_package(&pkg).expect("encode package (legacy postcard)");
    assert_ne!(msgpack_bytes, postcard_bytes, "the two codecs must produce distinct wires");
    match load_package(&postcard_bytes) {
        Err(FhirPackageError::LegacyPostcardPackage { found: 1, expected: 2 }) => {}
        Err(other) => panic!("expected the named legacy rejection, got {other:?}"),
        Ok(loaded) => panic!("legacy postcard bytes must not load, got {} modules", loaded.modules.len()),
    }
    let loaded_postcard = load_package_legacy_postcard(&postcard_bytes);

    // LoadedHirPackage equality: envelope, module table, dependencies, and
    // nucleum metadata all agree between the two load paths.
    assert_eq!(loaded_postcard.identity, loaded_msgpack.identity);
    assert_eq!(loaded_postcard.entry_path, loaded_msgpack.entry_path);
    assert_eq!(loaded_postcard.entry_frontmatter, loaded_msgpack.entry_frontmatter);
    assert_eq!(loaded_postcard.dependencies, loaded_msgpack.dependencies);
    assert_eq!(loaded_postcard.nucleum, loaded_msgpack.nucleum);
    assert_eq!(loaded_postcard.modules.len(), loaded_msgpack.modules.len());

    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    for (postcard_module, msgpack_module) in loaded_postcard.modules.iter().zip(&loaded_msgpack.modules) {
        assert_eq!(postcard_module.relative_path, msgpack_module.relative_path);
        assert_eq!(postcard_module.module_segments, msgpack_module.module_segments);
        assert_eq!(postcard_module.is_entry, msgpack_module.is_entry);
        assert_eq!(postcard_module.export_names, msgpack_module.export_names);
        assert_eq!(postcard_module.local_links, msgpack_module.local_links);
        assert_eq!(postcard_module.library_imports, msgpack_module.library_imports);

        // Reconstructed AnalyzedModule equality: both loads run the same
        // `reconstruct_unit`; equality of the re-serialized unit artifacts
        // proves every loader-carried fact family (HIR, presentation, types,
        // interner, resolver, libraries, facts, uses) agrees.
        let postcard_artifact = crate::hir::serialize::from_analyzed(
            &postcard_module.unit,
            &format!("sha256:{}", postcard_module.relative_path),
            &postcard_module.relative_path,
        );
        let msgpack_artifact = crate::hir::serialize::from_analyzed(
            &msgpack_module.unit,
            &format!("sha256:{}", msgpack_module.relative_path),
            &msgpack_module.relative_path,
        );
        assert_eq!(
            postcard_artifact, msgpack_artifact,
            "unit `{}` must reconstruct identically from both codecs",
            postcard_module.relative_path
        );

        // Canonical-Faber emit equality from the loaded HIR.
        let postcard_output = generate_from_analyzed(Target::HirFaber, &postcard_module.unit, &surface)
            .expect("canonical Faber (postcard)");
        let msgpack_output = generate_from_analyzed(Target::HirFaber, &msgpack_module.unit, &surface)
            .expect("canonical Faber (msgpack)");
        let postcard_faber = faber_code(&postcard_output);
        let msgpack_faber = faber_code(&msgpack_output);
        assert_eq!(
            postcard_faber, msgpack_faber,
            "canonical Faber emit from unit `{}` must be codec-independent",
            postcard_module.relative_path
        );
        assert!(!postcard_faber.is_empty(), "canonical Faber emit must not be empty");
    }

    // Both loaded programs emit the same canonical Faber as the original
    // in-session analysis (loaded HIR is as capable as the analyzed HIR).
    for (relative_path, original) in [("aux.fab", &aux), ("main.fab", &main)] {
        let original_output = generate_from_analyzed(Target::HirFaber, original, &surface).expect("original emit");
        let original_faber = faber_code(&original_output);
        for loaded in [&loaded_postcard, &loaded_msgpack] {
            let module = loaded
                .modules
                .iter()
                .find(|module| module.relative_path == relative_path)
                .expect("module paired with original");
            let loaded_output = generate_from_analyzed(Target::HirFaber, &module.unit, &surface).expect("loaded emit");
            let loaded_faber = faber_code(&loaded_output);
            assert_eq!(
                loaded_faber, original_faber,
                "loaded unit `{relative_path}` must emit canonical Faber as the original"
            );
        }
    }
}

fn main_equivalence_source() -> &'static str {
    r#"
discretio Event {
    Click { numerus x },
    Quit
}

@ publica
functio computa(numerus x) → numerus {
    si x ≻ 10 {
        redde x * 2
    }
    redde x + 1
}

incipit {
    fixum Event event ← finge Click { x = 7 }
    discerne event {
        casu Click fixum x { nota computa(x) }
        casu Quit { nota "quit" }
    }
}
"#
}

fn aux_equivalence_source() -> &'static str {
    r"
@ publica
functio auxilia(numerus x) → numerus {
    fixum numerus duplum ← x * 2
    redde duplum + 1
}
"
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

    let forward = encode_package_msgpack(&build(vec![input("aux.fab", false, &aux), input("main.fab", true, &main)]))
        .expect("encode forward");

    let reversed = encode_package_msgpack(&build(vec![input("main.fab", true, &main), input("aux.fab", false, &aux)]))
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
    let bytes = encode_package_msgpack(&pkg).expect("encode package");

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
    let mut bytes = encode_package_msgpack(&pkg).expect("encode package");
    // Flip a byte in the unit payload region (past the version prefix + a
    // couple of fields). Any decode outcome must be a structured error —
    // never a panic, never a silently truncated package.
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    assert!(load_package(&bytes).is_err(), "tampered unit payload must fail closed");
}

#[test]
fn load_package_rejects_legacy_postcard_fail_closed() {
    // FPF-5: the v1 postcard envelope is retired. The product loader reads
    // only the v2 MessagePack wire and rejects postcard bytes with the named
    // legacy error naming the ratchet — no fallback decode, no dual writer.
    let unit = analyze("main.fab", &sample_source("salve"));
    let mut pkg = build_package(
        PackageIdentityWire { name: "exemplum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![input("main.fab", true, &unit)],
        vec![],
    )
    .expect("build package");
    pkg.package_schema_version = LEGACY_PACKAGE_SCHEMA_VERSION;
    let legacy_bytes = encode_package(&pkg).expect("encode legacy postcard package");

    match load_package(&legacy_bytes) {
        Err(FhirPackageError::LegacyPostcardPackage { found, expected }) => {
            assert_eq!(found, 1);
            assert_eq!(expected, 2);
            let text = FhirPackageError::LegacyPostcardPackage { found, expected }.to_string();
            assert!(text.contains("legacy"), "the rejection must name the legacy wire: {text}");
            assert!(text.contains("v2"), "the rejection must name the ratchet target: {text}");
        }
        Err(other) => panic!("expected the named legacy rejection, got {other:?}"),
        Ok(loaded) => panic!("legacy postcard bytes must not load, got {} modules", loaded.modules.len()),
    }
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
    let loaded = load_package(&encode_package_msgpack(&pkg).expect("encode package")).expect("load package");
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
    let loaded = load_package(&encode_package_msgpack(&pkg).expect("encode package")).expect("load package");
    assert_eq!(loaded.dependencies.len(), 1);
    assert_eq!(loaded.dependencies[0].name, "norma");
    assert_eq!(loaded.dependencies[0].lock_identity, "path:/tmp/norma");
}

/// v2 device source: a private fragment helper, a public `@ nucleum` entry,
/// and a private `@ nucleum` helper — visibility is the launch surface.
fn nucleum_source() -> &'static str {
    r"
@ nucleum fragment
functio helper(f32 x) → f32 {
    redde x
}

@ publica
@ nucleum
functio k(f32 x) → f32 {
    redde helper(x)
}

@ nucleum
functio secret(f32 x) → f32 {
    redde x
}
"
}

fn nucleum_input<'a>(relative_path: &'a str, analysis: &'a AnalyzedModule) -> PackageUnitInput<'a> {
    PackageUnitInput {
        relative_path: relative_path.to_owned(),
        module_segments: vec![relative_path.to_owned()],
        is_entry: true,
        export_names: vec!["helper".to_owned(), "k".to_owned()],
        local_links: vec![],
        library_imports: vec![],
        content_hash: format!("sha256:{relative_path}"),
        analysis,
    }
}

fn nucleum_item<'a>(metadata: &'a NucleumProgramMetadata, name: &str) -> &'a NucleumItemMetadata {
    metadata
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("item {name}"))
}

#[test]
fn nucleum_metadata_filters_launch_surface_by_public_visibility() {
    // DFV2-5: dispatch/reflection metadata admits only `@ nucleum @ publica`.
    // A private kernel and a fragment helper stay composable but never get a
    // launch row.
    let unit = analyze("device.fab", nucleum_source());
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect nucleum metadata");
    assert_eq!(metadata.dispatch_and_reflection_names(), vec!["k".to_owned()]);
    assert_eq!(
        metadata.omitted_target_symbol_names(),
        vec!["helper".to_owned(), "secret".to_owned()]
    );

    let k = nucleum_item(&metadata, "k");
    assert_eq!(k.role, crate::hir::NucleumRole::Entry);
    assert_eq!(k.visibility, NucleumVisibility::Public);
    assert!(k.is_launch_entry());

    let secret = nucleum_item(&metadata, "secret");
    assert_eq!(secret.role, crate::hir::NucleumRole::Entry);
    assert_eq!(secret.visibility, NucleumVisibility::Private);
    assert!(!secret.is_launch_entry());

    let helper = nucleum_item(&metadata, "helper");
    assert_eq!(helper.role, crate::hir::NucleumRole::Fragment);
    assert_eq!(helper.visibility, NucleumVisibility::Private);
    assert!(!helper.is_launch_entry());
    assert!(helper.has_hir_body);
}

#[test]
fn nucleum_metadata_records_device_body_identity() {
    // DFV2-5 (b): both roles carry the artifact skeleton — constraints,
    // effect summary slot, compiler/ABI identity. The body hash is honest
    // build state: empty until canonical MIR attaches.
    let unit = analyze("device.fab", nucleum_source());
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect nucleum metadata");
    #[cfg(feature = "mir-core")]
    assert_eq!(NUCLEUM_DEVICE_ABI_VERSION, crate::mir::HOST_CALL_ABI_VERSION);
    for name in ["helper", "k", "secret"] {
        let body = nucleum_item(&metadata, name)
            .body
            .as_ref()
            .unwrap_or_else(|| panic!("device body artifact for {name}"));
        assert_eq!(body.compiler_version, NUCLEUM_COMPILER_VERSION);
        assert_eq!(body.abi_version, NUCLEUM_DEVICE_ABI_VERSION);
        assert!(body.mir.is_empty(), "canonical bytes attach from the lowered unit");
        assert!(
            body.body_hash.is_empty(),
            "the hash covers the serialized bytes, not a fingerprint"
        );
        assert!(!body.type_constraints.is_empty());
    }
}

#[test]
fn nucleum_package_round_trip_preserves_visibility_roles() {
    let unit = analyze("device.fab", nucleum_source());
    let pkg = build_package(
        PackageIdentityWire { name: "nucleum".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "device.fab",
        None,
        vec![nucleum_input("device.fab", &unit)],
        vec![],
    )
    .expect("build package");
    let loaded = load_package(&encode_package_msgpack(&pkg).expect("encode")).expect("load");
    assert_eq!(loaded.nucleum.dispatch_and_reflection_names(), vec!["k".to_owned()]);
    assert_eq!(
        loaded.nucleum.omitted_target_symbol_names(),
        vec!["helper".to_owned(), "secret".to_owned()]
    );
    assert_eq!(nucleum_item(&loaded.nucleum, "k").visibility, NucleumVisibility::Public);
    assert_eq!(nucleum_item(&loaded.nucleum, "secret").visibility, NucleumVisibility::Private);
}

#[test]
fn signature_only_device_import_fails_closed() {
    let session = Session::new(Config::default().with_bodyless_functions());
    let Err(err) = analyze_source(
        &session,
        "iface.fab",
        "@ nucleum fragment\nfunctio helper(f32 x) → f32\n",
        AnalyzeRequest::default(),
    ) else {
        panic!("signature-only fragment must fail at compile time");
    };
    assert!(
        err.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "nucleum_fragment_effect")
            && diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "reason" && arg.value == "missing_body")),
        "{err:?}"
    );

    // Metadata-level gate: an importable item (fragment at any visibility,
    // or a public device function) without any body fails closed; a private
    // kernel helper stays module-internal and is enforced at composition.
    let item = |name: &str, role: crate::hir::NucleumRole, visibility: NucleumVisibility| NucleumItemMetadata {
        name: name.to_owned(),
        relative_path: "iface.fab".to_owned(),
        def_id: 1,
        role,
        visibility,
        has_hir_body: false,
        body: None,
        device_callees: Vec::new(),
    };
    let mut metadata = NucleumProgramMetadata::new();
    metadata.items = vec![
        item("helper", crate::hir::NucleumRole::Fragment, NucleumVisibility::Private),
        item("open", crate::hir::NucleumRole::Entry, NucleumVisibility::Public),
    ];
    let err = metadata
        .require_importable_fragment_bodies()
        .expect_err("importable signature-only items must fail");
    assert!(matches!(&err, NucleumPackageError::SignatureOnlyImport { name, .. } if name == "helper"));

    metadata.items = vec![item("open", crate::hir::NucleumRole::Entry, NucleumVisibility::Public)];
    let err = metadata
        .require_importable_fragment_bodies()
        .expect_err("signature-only public entry must fail");
    assert!(matches!(&err, NucleumPackageError::SignatureOnlyImport { name, .. } if name == "open"));

    metadata.items = vec![item(
        "secret",
        crate::hir::NucleumRole::Entry,
        NucleumVisibility::Private,
    )];
    metadata
        .require_importable_fragment_bodies()
        .expect("private helpers are not on the import surface");
}

/// Lower the unit and attach canonical MIR bodies to `metadata`.
#[cfg(feature = "mir-core")]
fn attach_bodies(metadata: &mut NucleumProgramMetadata, unit: &mut AnalyzedModule) {
    let lowered = crate::mir::lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("lower device unit");
    metadata.attach_canonical_mir(lowered.validated.program(), "device.fab");
}

#[test]
fn device_body_cache_key_tracks_hash_compiler_abi_caps_and_bindings() {
    let mut unit = analyze("device.fab", nucleum_source());
    let mut metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    #[cfg(feature = "mir-core")]
    attach_bodies(&mut metadata, &mut unit);
    let body = nucleum_item(&metadata, "helper")
        .body
        .as_ref()
        .expect("body");
    let key = device_body_cache_key(body, &["metal", "wgsl"], &["f32"], &["4"]);
    assert!(key.starts_with("sha256:"));
    let other_caps = device_body_cache_key(body, &["llvm"], &["f32"], &["4"]);
    let other_types = device_body_cache_key(body, &["metal", "wgsl"], &["i32"], &["4"]);
    let other_shapes = device_body_cache_key(body, &["metal", "wgsl"], &["f32"], &["8"]);
    assert_ne!(key, other_caps);
    assert_ne!(key, other_types);
    assert_ne!(key, other_shapes);

    // Cache identity moves with the body hash, the compiler, and the ABI:
    // a re-hashed body, a rebuilt compiler, and an ABI bump are three
    // different cache subjects.
    let mut rehashed = body.clone();
    rehashed.body_hash = "sha256:other".to_owned();
    assert_ne!(key, device_body_cache_key(&rehashed, &["metal", "wgsl"], &["f32"], &["4"]));
    let mut other_compiler = body.clone();
    other_compiler.compiler_version = "0.0.0-test".to_owned();
    assert_ne!(
        key,
        device_body_cache_key(&other_compiler, &["metal", "wgsl"], &["f32"], &["4"])
    );
    let mut other_abi = body.clone();
    other_abi.abi_version += 1;
    assert_ne!(key, device_body_cache_key(&other_abi, &["metal", "wgsl"], &["f32"], &["4"]));
}

#[test]
fn device_call_graph_rejects_cycle_after_import() {
    let ping = analyze(
        "ping.fab",
        r"
@ nucleum fragment
functio ping(f32 x) → f32 {
    redde pong(x)
}

@ nucleum fragment
functio pong(f32 x) → f32 {
    redde ping(x)
}
",
    );
    let err = collect_nucleum_metadata(&ping, "ping.fab").expect_err("cycle");
    match err {
        NucleumPackageError::CyclicCallGraph { cycle } => {
            assert!(cycle.contains("ping"), "{cycle}");
            assert!(cycle.contains("pong"), "{cycle}");
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn device_call_graph_rejects_kernel_cycle() {
    // v2 composition covers entry-to-entry calls, so the package call graph
    // must see kernel-to-kernel edges too, not only fragment edges.
    let unit = analyze(
        "cycle.fab",
        r"
@ publica
@ nucleum
functio k1(f32 x) → f32 {
    redde k2(x)
}

@ publica
@ nucleum
functio k2(f32 x) → f32 {
    redde k1(x)
}
",
    );
    let err = collect_nucleum_metadata(&unit, "cycle.fab").expect_err("kernel cycle");
    match err {
        NucleumPackageError::CyclicCallGraph { cycle } => {
            assert!(cycle.contains("k1"), "{cycle}");
            assert!(cycle.contains("k2"), "{cycle}");
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn device_call_graph_keeps_same_named_functions_of_different_modules_apart() {
    // `a.fab` has `f → g` and `b.fab` has `g → f`, but the two `f`s and the two
    // `g`s are different functions. Keyed by bare name the merged package graph
    // reads `f → g → f`; keyed by module and name it has no cycle.
    let a = analyze(
        "a.fab",
        r"
@ nucleum fragment
functio g(f32 x) → f32 {
    redde x
}

@ nucleum fragment
functio f(f32 x) → f32 {
    redde g(x)
}
",
    );
    let b = analyze(
        "b.fab",
        r"
@ nucleum fragment
functio f(f32 x) → f32 {
    redde x
}

@ nucleum fragment
functio g(f32 x) → f32 {
    redde f(x)
}
",
    );
    let metadata = nucleum_metadata_from_unit_inputs(&[nucleum_input("a.fab", &a), nucleum_input("b.fab", &b)])
        .expect("same-named functions of different modules are not a call cycle");
    assert_eq!(metadata.items.len(), 4);
}

#[test]
fn non_launch_device_bodies_are_omitted_from_target_symbols() {
    let unit = analyze("device.fab", nucleum_source());
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let symbols = metadata.omitted_target_symbol_names();
    assert_eq!(symbols, vec!["helper".to_owned(), "secret".to_owned()]);
    for omitted in &symbols {
        assert!(!metadata.dispatch_and_reflection_names().contains(omitted));
    }
}

#[cfg(feature = "mir-core")]
#[test]
fn importable_device_bodies_carry_canonical_mir_artifact() {
    let mut unit = analyze("device.fab", nucleum_source());
    let mut metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    attach_bodies(&mut metadata, &mut unit);
    metadata
        .require_canonical_mir()
        .expect("canonical MIR attached");
    metadata
        .verify_importable_device_bodies()
        .expect("attached bodies verify");

    // The public entry's artifact carries the actual serialized body —
    // decodable postcard MIR for that exact definition, hashed over the
    // bytes that travel.
    let k = nucleum_item(&metadata, "k");
    let body = k.body.as_ref().expect("entry body artifact");
    assert!(!body.mir.is_empty(), "the launch entry's canonical body is required for import");
    let decoded: crate::mir::MirFunction = postcard::from_bytes(&body.mir).expect("decode MIR function");
    assert_eq!(decoded.source, Some(crate::hir::DefId(k.def_id)));
    assert_eq!(body.body_hash, format!("sha256:{}", handroll::sha256_hex(&body.mir)));

    let bytes = encode_nucleum_metadata(&metadata).expect("encode sidecar");
    let round = decode_nucleum_metadata(&bytes).expect("decode sidecar");
    assert_eq!(round.items.len(), metadata.items.len());
    assert_eq!(round.items, metadata.items);
}

#[cfg(feature = "mir-core")]
#[test]
fn device_body_import_gate_fails_closed_on_missing_tampered_and_stale_bodies() {
    let mut unit = analyze("device.fab", nucleum_source());
    let mut metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");

    // Missing: the pre-attach artifact has no canonical bytes.
    let err = metadata
        .verify_importable_device_bodies()
        .expect_err("pre-attach bodies are not importable");
    assert!(matches!(err, NucleumPackageError::MirBodyMissing { .. }));
    assert_eq!(err.reason(), "missing_body");

    attach_bodies(&mut metadata, &mut unit);
    metadata
        .verify_importable_device_bodies()
        .expect("attached bodies verify");

    // Tampered: bytes that no longer match the recorded hash.
    let mut tampered = metadata.clone();
    tampered
        .items
        .iter_mut()
        .find(|item| item.name == "k")
        .and_then(|item| item.body.as_mut())
        .expect("k body")
        .mir[0] ^= 0xff;
    let err = tampered
        .verify_importable_device_bodies()
        .expect_err("tampered body must fail");
    assert!(matches!(&err, NucleumPackageError::BodyHashMismatch { name, .. } if name == "k"));
    assert_eq!(err.reason(), "body_hash_mismatch");

    // Stale: an artifact from another compiler/ABI generation.
    let mut stale = metadata.clone();
    {
        let body = stale
            .items
            .iter_mut()
            .find(|item| item.name == "k")
            .and_then(|item| item.body.as_mut())
            .expect("k body");
        body.compiler_version = "0.0.0-old".to_owned();
        body.abi_version -= 1;
    }
    let err = stale
        .verify_importable_device_bodies()
        .expect_err("stale body must fail");
    assert!(matches!(&err, NucleumPackageError::IncompatibleBodyVersion { name, .. } if name == "k"));
    assert_eq!(err.reason(), "stale_body_version");

    // The transport bridge degrades every failure to the fail-closed
    // `Missing` channel — never a signature-only callable.
    let contract = |metadata: &NucleumProgramMetadata, name: &str| nucleum_item(metadata, name).device_contract();
    assert!(matches!(
        contract(&metadata, "k").body,
        crate::file_interface::InterfaceBodyChannel::Canonical(_)
    ));
    assert!(matches!(
        contract(&tampered, "k").body,
        crate::file_interface::InterfaceBodyChannel::Missing
    ));
    assert!(matches!(
        contract(&stale, "k").body,
        crate::file_interface::InterfaceBodyChannel::Missing
    ));
    let pre_attach = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    assert!(matches!(
        contract(&pre_attach, "k").body,
        crate::file_interface::InterfaceBodyChannel::Missing
    ));
}

#[cfg(feature = "mir-core")]
#[test]
fn device_contract_bridge_carries_role_visibility_and_template() {
    // DFV2-5 (a): the package artifact is usable by the DFV2-3/DFV2-4
    // transport — role and launch visibility map through, and the canonical
    // template is the serialized body bytes.
    let mut unit = analyze("device.fab", nucleum_source());
    let mut metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    attach_bodies(&mut metadata, &mut unit);

    let k = nucleum_item(&metadata, "k").clone();
    let contract = k.device_contract();
    assert_eq!(contract.role, crate::file_interface::InterfaceDeviceRole::Kernel);
    assert_eq!(contract.visibility, crate::file_interface::InterfaceDeviceVisibility::Public);
    let crate::file_interface::InterfaceBodyChannel::Canonical(canonical) = &contract.body else {
        panic!("attached public entry must carry a canonical body");
    };
    assert_eq!(canonical.template, k.body.as_ref().expect("body").mir);
    assert!(!canonical.body_hash.is_empty());
    assert!(contract.is_composable());

    let secret = nucleum_item(&metadata, "secret").clone();
    let contract = secret.device_contract();
    assert_eq!(contract.visibility, crate::file_interface::InterfaceDeviceVisibility::Private);
}

#[test]
fn nucleum_sidecar_is_deterministic_and_rejects_foreign_bytes() {
    // DFV2-5 (f): rebuilding the same unit yields identical sidecar bytes.
    let unit = analyze("device.fab", nucleum_source());
    let first = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let second = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let first_bytes = encode_nucleum_metadata(&first).expect("encode");
    let second_bytes = encode_nucleum_metadata(&second).expect("encode");
    assert_eq!(first_bytes, second_bytes, "sidecar bytes must be deterministic");

    // A payload that is not this metadata shape fails closed on decode —
    // pre-DFV2-5 sidecars (no visibility field) cannot load silently.
    assert!(decode_nucleum_metadata(&[0xde, 0xad, 0xbe, 0xef]).is_err());
}

#[test]
fn device_callee_in_a_copy_into_value_is_recorded() {
    // `walk_expr_callees` covers `HirExpressionKind::CopyInto`: a device
    // function called in the value of `target ⇇ value` is a called device
    // callee, so the post-import call graph records the edge instead of
    // silently dropping it through the catch-all.
    let unit = analyze(
        "device.fab",
        r"
@ nucleum fragment
functio emit(tf32[4] x) → tf32[4] { redde x }

@ nucleum
functio k(tf32[4] x, in tf32[4] out) → vacuum {
    out ⇇ emit(x)
}
",
    );
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let k = metadata
        .items
        .iter()
        .find(|item| item.name == "k")
        .expect("k entry");
    assert_eq!(k.device_callees, vec!["emit".to_owned()]);
}
