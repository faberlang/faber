//! Tests for `crate::hir::package`: package build/load round-trips,
//! determinism, and fail-closed loading.

use super::*;
use crate::codegen::{generate_from_analyzed, Target};
use crate::driver::{analyze_source, AnalyzeRequest, Config, Session};
use crate::locale::{latin_locale_pack, KeywordSurface};

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
        let unit = crate::hir::artifact::reconstruct_unit(module.unit)
            .expect("reconstruct legacy postcard unit");
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
    let nucleum =
        nucleum_metadata_from_loaded_modules(&modules).expect("legacy postcard nucleum metadata");
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
    main_input.local_links =
        vec![LocalLinkWire { binding: "aux".to_owned(), target: "aux.fab".to_owned() }];
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
        let postcard_output =
            generate_from_analyzed(Target::HirFaber, &postcard_module.unit, &surface).expect("canonical Faber (postcard)");
        let msgpack_output =
            generate_from_analyzed(Target::HirFaber, &msgpack_module.unit, &surface).expect("canonical Faber (msgpack)");
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
        let original_output =
            generate_from_analyzed(Target::HirFaber, original, &surface).expect("original emit");
        let original_faber = faber_code(&original_output);
        for loaded in [&loaded_postcard, &loaded_msgpack] {
            let module = loaded
                .modules
                .iter()
                .find(|module| module.relative_path == relative_path)
                .expect("module paired with original");
            let loaded_output =
                generate_from_analyzed(Target::HirFaber, &module.unit, &surface).expect("loaded emit");
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
    si x > 10 {
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

fn nucleum_source() -> &'static str {
    r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
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

#[test]
fn nucleum_metadata_records_role_and_only_entry_in_dispatch() {
    let unit = analyze("device.fab", nucleum_source());
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect nucleum metadata");
    assert_eq!(metadata.dispatch_and_reflection_names(), vec!["k".to_owned()]);
    assert_eq!(metadata.omitted_target_symbol_names(), vec!["helper".to_owned()]);
    let helper = metadata
        .items
        .iter()
        .find(|item| item.name == "helper")
        .expect("helper");
    assert_eq!(helper.role, crate::hir::NucleumRole::Fragment);
    #[cfg(feature = "mir-core")]
    assert_eq!(NUCLEUM_FRAGMENT_ABI_VERSION, crate::mir::HOST_CALL_ABI_VERSION);
    assert!(helper.has_hir_body);
    let body = helper.body.as_ref().expect("fragment body artifact");
    assert_eq!(body.compiler_version, NUCLEUM_COMPILER_VERSION);
    assert_eq!(body.abi_version, NUCLEUM_FRAGMENT_ABI_VERSION);
    assert!(!body.body_hash.is_empty());
    assert!(!body.type_constraints.is_empty());
}

#[test]
fn nucleum_package_round_trip_preserves_roles() {
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
    assert_eq!(loaded.nucleum.omitted_target_symbol_names(), vec!["helper".to_owned()]);
}

#[test]
fn signature_only_fragment_import_fails_closed() {
    let session = Session::new(Config::default().with_bodyless_functions());
    let Err(err) = analyze_source(
        &session,
        "iface.fab",
        "@ nucleum fragment\nfunctio helper(fractus<f32> x) → fractus<f32>\n",
        AnalyzeRequest::default(),
    ) else {
        panic!("signature-only fragment must fail at compile time");
    };
    assert!(
        err.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "NUCLEUM_FRAGMENT_EFFECT")
            && diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "reason" && arg.value == "missing_body")),
        "{err:?}"
    );

    let mut metadata = NucleumProgramMetadata::new();
    metadata.items.push(NucleumItemMetadata {
        name: "helper".to_owned(),
        relative_path: "iface.fab".to_owned(),
        def_id: 1,
        role: crate::hir::NucleumRole::Fragment,
        has_hir_body: false,
        body: None,
        fragment_callees: Vec::new(),
    });
    let err = metadata
        .require_importable_fragment_bodies()
        .expect_err("metadata fail-closed");
    assert!(matches!(err, NucleumPackageError::SignatureOnlyImport { name, .. } if name == "helper"));
}

#[test]
fn fragment_cache_key_includes_hash_compiler_abi_caps_and_bindings() {
    let unit = analyze("device.fab", nucleum_source());
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let body = metadata
        .items
        .iter()
        .find(|item| item.name == "helper")
        .and_then(|item| item.body.as_ref())
        .expect("body");
    let key = fragment_cache_key(body, &["metal", "wgsl"], &["f32"], &["4"]);
    assert!(key.starts_with("sha256:"));
    let other_caps = fragment_cache_key(body, &["llvm"], &["f32"], &["4"]);
    let other_types = fragment_cache_key(body, &["metal", "wgsl"], &["i32"], &["4"]);
    let other_shapes = fragment_cache_key(body, &["metal", "wgsl"], &["f32"], &["8"]);
    assert_ne!(key, other_caps);
    assert_ne!(key, other_types);
    assert_ne!(key, other_shapes);
}

#[test]
fn fragment_call_graph_rejects_cycle_after_import() {
    let ping = analyze(
        "ping.fab",
        r"
@ nucleum fragment
functio ping(fractus<f32> x) → fractus<f32> {
    redde pong(x)
}

@ nucleum fragment
functio pong(fractus<f32> x) → fractus<f32> {
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
fn fragment_bodies_are_omitted_from_target_symbols_after_composition() {
    let unit = analyze("device.fab", nucleum_source());
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let symbols = metadata.omitted_target_symbol_names();
    assert_eq!(symbols, vec!["helper".to_owned()]);
    assert!(!metadata
        .dispatch_and_reflection_names()
        .contains(&"helper".to_owned()));
}

#[cfg(feature = "mir-core")]
#[test]
fn importable_fragment_carries_canonical_mir_artifact() {
    let mut unit = analyze("device.fab", nucleum_source());
    let mut metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let lowered = crate::mir::lower_analyzed_unit(&mut unit, Default::default()).expect("lower fragment unit");
    metadata.attach_canonical_mir(lowered.validated.program(), "device.fab");
    metadata
        .require_canonical_mir()
        .expect("canonical MIR attached");
    let body = metadata
        .items
        .iter()
        .find(|item| item.name == "helper")
        .and_then(|item| item.body.as_ref())
        .expect("body");
    assert!(!body.mir.is_empty(), "OQ2: canonical MIR bytes required");
    let decoded: crate::mir::MirFunction = postcard::from_bytes(&body.mir).expect("decode MIR function");
    assert_eq!(decoded.source, Some(crate::hir::DefId(body_def_id(&metadata, "helper"))));
    let bytes = encode_nucleum_metadata(&metadata).expect("encode sidecar");
    let round = decode_nucleum_metadata(&bytes).expect("decode sidecar");
    assert_eq!(round.items.len(), metadata.items.len());
    assert_eq!(
        round
            .items
            .iter()
            .find(|item| item.name == "helper")
            .and_then(|item| item.body.as_ref())
            .map(|body| &body.mir),
        Some(&body.mir)
    );
}

fn body_def_id(metadata: &NucleumProgramMetadata, name: &str) -> u32 {
    metadata
        .items
        .iter()
        .find(|item| item.name == name)
        .expect("item")
        .def_id
}

#[test]
fn fragment_transfer_sink_is_recorded_as_fragment_callee() {
    // EOT-3W: `walk_expr_callees` covers `HirExpressionKind::Transfer` — a
    // fragment invoked as a `⇇` sink is a called fragment (once per payload),
    // so the post-import call graph records the edge instead of silently
    // dropping it through the catch-all.
    let unit = analyze(
        "device.fab",
        r"
@ nucleum fragment
functio emit(fractus<f32> x) → vacuum { tacet }

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    emit ⇇ x
    redde x
}
",
    );
    let metadata = collect_nucleum_metadata(&unit, "device.fab").expect("collect");
    let k = metadata
        .items
        .iter()
        .find(|item| item.name == "k")
        .expect("k entry");
    assert_eq!(k.fragment_callees, vec!["emit".to_owned()]);
}
