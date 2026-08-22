//! Tests for `crate::hir::package`: package build/load round-trips,
//! determinism, and fail-closed loading.

use super::*;
use crate::codegen::{generate_from_analyzed, Target};
use crate::driver::{analyze_source, Config, Session};
use crate::locale::{latin_locale_pack, KeywordSurface};

fn analyze(name: &str, source: &str) -> AnalyzedModule {
    analyze_with(name, source, Config::default())
}

fn analyze_with(name: &str, source: &str, config: Config) -> AnalyzedModule {
    let session = Session::new(config);
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
    let loaded = load_package(&encode_package(&pkg).expect("encode")).expect("load");
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
