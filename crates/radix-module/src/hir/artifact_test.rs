//! FHIR artifact validation and round-trip preservation at the analyzed-module boundary.

use super::*;
use crate::driver::{AnalyzeRequest, AnalyzedModule};
use radix_hir_fhir::HirArtifact;
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

struct LibraryHomeOverride(Option<PathBuf>);

impl LibraryHomeOverride {
    fn set(home: &Path) -> Self {
        let previous =
            crate::import_resolve::library_home_test_support::set_thread_library_home(Some(home.to_path_buf()));
        Self(previous)
    }
}

impl Drop for LibraryHomeOverride {
    fn drop(&mut self) {
        crate::import_resolve::library_home_test_support::set_thread_library_home(self.0.take());
    }
}

#[test]
fn error_display_messages() {
    let malformed = HirArtifactError::Malformed(postcard::Error::DeserializeUnexpectedEnd);
    assert!(format!("{malformed}").contains("malformed HIR artifact"));

    let truncated = HirArtifactError::TruncatedArtifact;
    assert!(format!("{truncated}").contains("truncated HIR artifact"));

    let version = HirArtifactError::UnsupportedSchemaVersion { found: 1, expected: 2 };
    let msg = format!("{version}");
    assert!(msg.contains("unsupported HIR artifact schema version 1"));
    assert!(msg.contains("expected 2"));

    let dangling = HirArtifactError::DanglingIndex { kind: "interner", index: 99, arena_size: 10 };
    let msg = format!("{dangling}");
    assert!(msg.contains("dangling interner index 99"));
    assert!(msg.contains("arena size 10"));

    let snapshot = HirArtifactError::MalformedSnapshot { kind: "types", detail: "missing primitive type Nihil".into() };
    let msg = format!("{snapshot}");
    assert!(msg.contains("malformed types snapshot"));
    assert!(msg.contains("missing primitive type Nihil"));

    let presentation = HirArtifactError::InvalidPresentation { issue: "duplicate_owner_anchor", anchor: 7 };
    let msg = format!("{presentation}");
    assert!(msg.contains("invalid presentation: duplicate_owner_anchor"));
    assert!(msg.contains("anchor 7"));
}

#[test]
fn malformed_has_source() {
    let err = postcard::Error::DeserializeUnexpectedEnd;
    let malformed = HirArtifactError::Malformed(err);
    let source = Error::source(&malformed);
    assert!(source.is_some());
}

#[test]
fn non_malformed_has_no_source() {
    let err = HirArtifactError::TruncatedArtifact;
    assert!(Error::source(&err).is_none());

    let err = HirArtifactError::UnsupportedSchemaVersion { found: 1, expected: 2 };
    assert!(Error::source(&err).is_none());

    let err = HirArtifactError::DanglingIndex { kind: "x", index: 0, arena_size: 0 };
    assert!(Error::source(&err).is_none());

    let err = HirArtifactError::MalformedSnapshot { kind: "types", detail: "bad".into() };
    assert!(Error::source(&err).is_none());
}

#[test]
fn malformed_rejects_garbage_bytes() {
    // Now that truncation detection is active, short garbage may be
    // classified as Truncated. This test verifies that garbage bytes
    // never panic — any structured error variant is acceptable.
    let result = load_fhir(b"\x00\x01\x02\xff\xfe\xfd");
    assert!(result.is_err(), "garbage bytes must error, got Ok");
}

/// Construct a minimal valid HirArtifact for loader tests.
fn minimal_test_artifact() -> HirArtifact {
    use radix_hir::{HirFunction, HirId, HirItem, HirItemKind, HirModule, LibraryRegistry};
    use radix_hir_fhir::{DisplayTokensWire, FunctionFactTableWire, ResolverSnapshot, SCHEMA_VERSION, SourceIdentity};

    let types = TypeTable::new();
    let item = HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id: radix_types::DefId(0),
        kind: HirItemKind::Function(Box::new(HirFunction {
            name: radix_lexer::Symbol(0),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: None,
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        })),
        span: radix_lexer::Span::default(),
    };
    let hir = HirModule {
        items: vec![item],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    HirArtifact {
        schema_version: SCHEMA_VERSION,
        shape_hash: radix_hir_fhir::wire_shape_hash(),
        source_identity: SourceIdentity { content_hash: "abc123".to_string(), relative_path: "test.faber".to_string() },
        hir,
        presentation: empty_presentation(),
        cli_program: None,
        types: types.snapshot(),
        resolved_generic_calls: Vec::new(),
        radix_lanes: Vec::new(),
        interner: vec!["main".to_string(), "test".to_string()],
        resolver: ResolverSnapshot {
            namespace_exports: Vec::new(),
            file_interfaces: Vec::new(),
            used_namespaces: Vec::new(),
            imported_nominal_types: Vec::new(),
            ambiguous_imported_nominal_types: Vec::new(),
            conversion_rows: Vec::new(),
            module_bindings: Vec::new(),
        },
        libraries: LibraryRegistry::default(),
        function_facts: FunctionFactTableWire { entries: Vec::new(), entry: None },
        resolved_uses: Vec::new(),
        analysis_stamp: 1,
        gpu_builtins: Vec::new(),
        capability_provenance: radix_hir_fhir::FunctionProvenanceTableWire::default(),
        provider_units: BTreeMap::new(),
        shape_obligations: radix_hir_fhir::ShapeObligationTableWire::default(),
        contract_roles: radix_hir_fhir::ContractRolesWire::default(),
        kernel_requirements: radix_hir_fhir::KernelRequirementsWire::default(),
        tensor_edge_policies: radix_hir_fhir::TensorEdgePolicyTableWire::default(),
        display_tokens: DisplayTokensWire {
            true_: "true".to_string(),
            false_: "false".to_string(),
            none: "none".to_string(),
            tuple: "tuple".to_string(),
        },
        conversation_constructor: None,
    }
}

/// A schema-v2 presentation sidecar with no trivia, no ends, and no entry.
fn empty_presentation() -> radix_hir::HirPresentation {
    radix_hir::HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    }
}

/// Serialize an artifact to postcard bytes for round-trip testing.
fn serialize_artifact(artifact: &HirArtifact) -> Vec<u8> {
    postcard::to_allocvec(artifact).expect("serialize test artifact")
}

#[test]
fn loader_type_table_caches_rebuilt() {
    let artifact = minimal_test_artifact();
    let bytes = serialize_artifact(&artifact);
    let unit = load_fhir(&bytes).expect("load test artifact");

    // intern_map: hash-cons map is populated (at minimum, all primitives).
    // Verify a primitive lookup works.
    let nihil_id = unit.types.primitive(radix_types::Primitive::Nihil);
    let found = unit.types.get(nihil_id);
    assert_eq!(found, &radix_types::Type::Primitive(radix_types::Primitive::Nihil));

    // equals_memo: verify equals works (memo is populated lazily).
    assert!(unit.types.equals(nihil_id, nihil_id));

    // primitives map: verify all required primitives are present.
    for prim in [
        radix_types::Primitive::Textus,
        radix_types::Primitive::Numerus,
        radix_types::Primitive::Fractus,
        radix_types::Primitive::Bivalens,
        radix_types::Primitive::Nihil,
        radix_types::Primitive::Vacuum,
        radix_types::Primitive::Numquam,
        radix_types::Primitive::Ignotum,
        radix_types::Primitive::Octeti,
        radix_types::Primitive::Regex,
        radix_types::Primitive::Match,
        radix_types::Primitive::Json,
        radix_types::Primitive::Valor,
        radix_types::Primitive::Ascii,
        radix_types::Primitive::Instans,
    ] {
        let id = unit.types.primitive(prim);
        let ty = unit.types.get(id);
        assert_eq!(ty, &radix_types::Type::Primitive(prim), "primitive {prim:?} mismatch");
    }
}

#[test]
fn loader_resolver_builtins() {
    let artifact = minimal_test_artifact();
    let bytes = serialize_artifact(&artifact);
    let unit = load_fhir(&bytes).expect("load test artifact");

    // Builtin DefId fields are re-derived from compiler constants (None
    // when no builtins have been registered — the snapshot carries no
    // builtin data per F5). Verify they are present as Option.
    assert!(unit.resolver.builtin_forma_def_id().is_none());
    assert!(unit.resolver.builtin_scrinium_def_id().is_none());
    assert!(unit.resolver.builtin_status_def_id().is_none());
    assert!(unit.resolver.builtin_sermo_def_id().is_none());
    assert!(unit.resolver.builtin_meus_def_id().is_none());
    assert!(unit.resolver.builtin_tuus_def_id().is_none());
}

// ── EOT-2W selective-import loader round-trip ────────────────────────────

#[test]
fn selective_import_survives_module_hir_load() {
    // EOT-2M's `HirImport.selective` is an additive `#[serde(default)]` field,
    // so the module HIR serialize/load path carries it without a schema
    // ratchet: the ordered member/alias list and the marker must both survive
    // `load_fhir` (postcard decode + referential-integrity gate + cache
    // rebuild) unchanged.
    use radix_hir::{HirImport, HirImportItem};

    let mut artifact = minimal_test_artifact();
    artifact.interner = vec![
        "main".to_string(),
        "test".to_string(),
        "norma:consolum".to_string(),
        "dic".to_string(),
        "output".to_string(),
        "funde".to_string(),
    ];
    artifact.hir.items.push(radix_hir::HirItem {
        annotations: Vec::new(),
        id: radix_hir::HirId(1),
        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 42),
        kind: radix_hir::HirItemKind::Import(HirImport {
            path: radix_lexer::Symbol(2),
            visibility: radix_syntax::Visibility::Privata,
            items: vec![
                HirImportItem {
                    def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 40),
                    name: radix_lexer::Symbol(3),
                    alias: Some(radix_lexer::Symbol(4)),
                },
                HirImportItem {
                    def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 41),
                    name: radix_lexer::Symbol(5),
                    alias: None,
                },
            ],
            selective: true,
            target: None,
        }),
        span: radix_lexer::Span::default(),
    });

    let bytes = serialize_artifact(&artifact);
    let unit = load_fhir(&bytes).expect("selective artifact must load");

    let radix_hir::HirItemKind::Import(import) = &unit.hir.items[1].kind else {
        panic!("expected import item");
    };
    assert!(import.selective, "selective marker must survive load_fhir");
    assert_eq!(import.items.len(), 2, "ordered member list must survive");
    assert_eq!(import.items[0].alias, Some(radix_lexer::Symbol(4)));
}

#[test]
fn loader_interner_reconstruction() {
    let mut artifact = minimal_test_artifact();
    artifact.interner = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let bytes = serialize_artifact(&artifact);
    let unit = load_fhir(&bytes).expect("load test artifact");

    // Symbol(u32) indices are stable: same string → same index.
    let sym0 = radix_lexer::Symbol(0);
    let sym1 = radix_lexer::Symbol(1);
    let sym2 = radix_lexer::Symbol(2);

    assert_eq!(unit.interner.resolve(sym0), "alpha");
    assert_eq!(unit.interner.resolve(sym1), "beta");
    assert_eq!(unit.interner.resolve(sym2), "gamma");
}

#[test]
fn loader_resolved_use_index_rebuilt() {
    use radix_hir_fhir::{ResolvedUseKeyWire, ResolvedUseKindWire, ResolvedUseWire};

    let mut artifact = minimal_test_artifact();
    let def_id = radix_types::DefId(0); // matches the single item's def_id
    artifact.resolved_uses = vec![
        ResolvedUseWire {
            key: ResolvedUseKeyWire::Local(def_id),
            kind: ResolvedUseKindWire::Call,
            enclosing: None,
            local: None,
        },
        ResolvedUseWire {
            key: ResolvedUseKeyWire::Portable("pkg::func".to_string()),
            kind: ResolvedUseKindWire::ImportBinding,
            enclosing: None,
            local: None,
        },
    ];
    let bytes = serialize_artifact(&artifact);
    let unit = load_fhir(&bytes).expect("load test artifact");

    // Verify uses vector is populated.
    assert_eq!(unit.resolved_uses.len(), 2);

    // Verify by_local lookup.
    let local_uses: Vec<_> = unit.resolved_uses.uses_of_local(def_id).collect();
    assert_eq!(local_uses.len(), 1);
    assert_eq!(local_uses[0].kind, ResolvedUseKind::Call);

    // Verify by_portable lookup.
    let portable_uses: Vec<_> = unit.resolved_uses.uses_of_portable("pkg::func").collect();
    assert_eq!(portable_uses.len(), 1);
    assert_eq!(portable_uses[0].kind, ResolvedUseKind::ImportBinding);
}

#[test]
fn resolved_use_locals_survive_the_fhir_round_trip() {
    // The loaded resolved-use index must index the same local defs as direct
    // analysis: import-binding records stay declaration facts (never counted
    // as local uses), and a portable-keyed use keeps its unit-local binding.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/importa/selective-import.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "selective-import.fab", source, AnalyzeRequest::default())
        .expect("selective import fixture analyzes");

    let artifact = from_analyzed(&original, "selective-import", "selective-import.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize selective-import artifact");
    let loaded = load_fhir(&bytes).expect("selective-import artifact must load");

    assert_eq!(
        loaded.resolved_uses.locals_used(),
        original.resolved_uses.locals_used(),
        "loaded resolved-use index must index exactly the local defs direct analysis indexes"
    );
}

#[test]
fn display_tokens_survive_the_fhir_round_trip() {
    // The unit's print words follow the code locale; a loaded artifact must
    // keep the author's locale tokens instead of the Latin loader default.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let en_source = "+++\nlocale = \"en\"\n+++\n\nfn salve() { }";
    let la_source = "+++\nlocale = \"la\"\n+++\n\nfunctio salve() { }";
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );

    for (name, source) in [("display-en.faber", en_source), ("display-la.faber", la_source)] {
        let original =
            analyze_source(&session, name, source, AnalyzeRequest::default()).expect("display-token fixture analyzes");
        let artifact = from_analyzed(&original, name, name);
        let bytes = postcard::to_allocvec(&artifact).expect("serialize display-token artifact");
        let loaded = load_fhir(&bytes).expect("display-token artifact must load");

        assert_eq!(
            loaded.display_tokens, original.display_tokens,
            "loaded display tokens must match the analyzed unit for {name}"
        );
        if name == "display-en.faber" {
            assert_eq!(loaded.display_tokens.true_, "true");
        } else {
            assert_eq!(loaded.display_tokens.true_, "verum");
        }
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn selective_import_rust_emit_is_identical_after_load() {
    // Pins the emitted form: an unused private selective import is not
    // emitted, as direct generation does today. The loaded pipeline used to
    // emit `use crate::console::write_bytes as output_bytes;` for it because
    // every import counted as used after `load_fhir`.
    use crate::codegen::{Target, generate_from_analyzed};
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/importa/selective-import.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let direct = analyze_source(&session, "selective-import.fab", source, AnalyzeRequest::default())
        .expect("selective import fixture analyzes");

    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);
    let direct_code =
        output_code(&generate_from_analyzed(Target::HirRust, &direct, &surface).expect("direct Rust codegen"))
            .to_owned();
    assert!(
        !direct_code.contains("use crate::console::write_bytes as output_bytes;"),
        "direct generation must drop unused private selective imports"
    );

    let artifact = from_analyzed(&direct, "selective-import", "selective-import.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize selective-import artifact");
    let loaded = load_fhir(&bytes).expect("selective-import artifact must load");
    let loaded_output = generate_from_analyzed(Target::HirRust, &loaded, &surface).expect("loaded Rust codegen");
    let loaded_code = output_code(&loaded_output);

    assert_eq!(
        direct_code, loaded_code,
        "Rust emit must be identical after the FHIR round-trip"
    );
}

/// Analyze `source`, serialize it to FHIR, and return the loaded unit.
fn load_unit_for(source: &str) -> Result<AnalyzedModule, HirArtifactError> {
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "comptime-fold.fab", source, AnalyzeRequest::default())
        .expect("comptime-fold fixture analyzes");
    let artifact = from_analyzed(&original, "comptime-fold", "comptime-fold.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize comptime-fold artifact");
    load_fhir(&bytes)
}

#[test]
fn authored_policy_range_survives_the_fhir_round_trip() {
    let loaded = load_unit_for(concat!(
        "+++\nlocale = \"en\"\n+++\n",
        "fn bounded() → int {\n",
        "    let trapping<i32, -∞‥8> value ← 1\n",
        "    return 0\n",
        "}\n",
    ))
    .expect("ranged-policy artifact must load");
    let authored = loaded
        .presentation
        .authored_types
        .iter()
        .find(|authored| matches!(authored.form.args.as_slice(), [radix_hir::HirTypeFormArg::Range(_)]))
        .expect("range spelling remains in the authored HIR form");
    let [radix_hir::HirTypeFormArg::Range(range)] = authored.form.args.as_slice() else {
        unreachable!("the filter selected a single range argument");
    };
    assert_eq!(range.lo, radix_hir::HirRangeBound::Unbounded { negative: true });
    assert_eq!(range.hi, radix_hir::HirRangeBound::Literal("8".to_owned()));
    assert!(!range.inclusive);
}

#[test]
fn comptime_module_constant_survives_the_fhir_round_trip() {
    let source = concat!(
        "+++\nlocale = \"en\"\n+++\n\n",
        "const int N = comptime {\n",
        "    const int t = 3\n",
        "    return t\n",
        "}\n",
    );
    let loaded = load_unit_for(source).expect("module comptime constant artifact loads");
    let constant = loaded
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            radix_hir::HirItemKind::Constant(konst) if loaded.interner.resolve(konst.name) == "N" => Some(konst),
            _ => None,
        })
        .expect("module constant is present after loading");
    assert!(matches!(
        &constant.value.kind,
        radix_hir::HirExpressionKind::Literal(radix_hir::HirLiteral::Int(3))
    ));
}

#[test]
fn comptime_block_constant_survives_the_fhir_round_trip() {
    let source = concat!(
        "+++\nlocale = \"en\"\n+++\n\n",
        "main {\n",
        "    const int x = comptime {\n",
        "        const int t = 3\n",
        "        return t\n",
        "    }\n",
        "    print x\n",
        "}\n",
    );
    load_unit_for(source).expect("block comptime constant artifact loads");
}

#[test]
fn comptime_class_constant_survives_the_fhir_round_trip() {
    let source = concat!(
        "+++\nlocale = \"en\"\n+++\n\n",
        "class Tab {\n",
        "    const int X = comptime {\n",
        "        const int t = 3\n",
        "        return t\n",
        "    }\n",
        "}\n\n",
        "main {\n",
        "    print Tab.X\n",
        "}\n",
    );
    let loaded = load_unit_for(source).expect("class comptime constant artifact loads");
    let strukt = loaded
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            radix_hir::HirItemKind::Struct(strukt) if loaded.interner.resolve(strukt.name) == "Tab" => Some(strukt),
            _ => None,
        })
        .expect("class is present after loading");
    let field = strukt
        .fields
        .iter()
        .find(|field| loaded.interner.resolve(field.name) == "X")
        .expect("class constant is present after loading");
    assert!(matches!(
        field.init.as_ref().map(|init| &init.kind),
        Some(radix_hir::HirExpressionKind::Literal(radix_hir::HirLiteral::Int(3)))
    ));
}

#[test]
fn insere_local_static_loads() {
    // `insere-local.fab` carries a module-level `const string SUMMA = comptime
    // { return embed "nuntius.txt" }`. The authored body survives folding in
    // `HirStatic::authored` and holds HirIds (the `embed` site) that the loader
    // must keep resolvable; this pins the dangling `HirId` case.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/insere/insere-local.fab");
    let name = path.display().to_string();
    let source = std::fs::read_to_string(&path).expect("insere-local fixture reads");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original =
        analyze_source(&session, &name, &source, AnalyzeRequest::default()).expect("insere-local fixture analyzes");
    let artifact = from_analyzed(&original, &name, &name);
    let bytes = postcard::to_allocvec(&artifact).expect("serialize insere-local artifact");
    load_fhir(&bytes).expect("insere-local artifact must load");
}

#[test]
fn finite_nan_scalar_round_trips() {
    // `scalar/finite-nan.fab` declares discard locals whose initializers are
    // `↦` / `∷` chains (`x ↦ b32 ↦ u32`, `bits ⇒ 23 ∧ 255 ∷ u32`). Lowering
    // records an `AscriptionTarget` owner for each `∷` ascription; every
    // authored type site and typed trivia owner must still name a node present
    // in the finalized HIR before the artifact round-trips, otherwise the
    // loader fails with a dangling HirId.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;
    use radix_hir::{HirExpression, HirOwnerRef, HirStatement};
    use std::collections::HashSet;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/scalar/finite-nan.fab");
    let name = path.display().to_string();
    let source = std::fs::read_to_string(&path).expect("finite-nan fixture reads");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original =
        analyze_source(&session, &name, &source, AnalyzeRequest::default()).expect("finite-nan fixture analyzes");

    struct HirIds(HashSet<radix_hir::HirId>);
    impl radix_hir::visit::HirVisitor for HirIds {
        fn visit_item(&mut self, item: &radix_hir::HirItem) {
            self.0.insert(item.id);
            radix_hir::visit::walk_item(self, item);
        }

        fn visit_stmt(&mut self, stmt: &HirStatement) {
            self.0.insert(stmt.id);
            radix_hir::visit::walk_stmt(self, stmt);
        }

        fn visit_expr(&mut self, expr: &HirExpression) {
            self.0.insert(expr.id);
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    let mut ids = HirIds(HashSet::new());
    radix_hir::visit::walk_program(&mut ids, &original.hir);

    for (index, authored) in original.presentation.authored_types.iter().enumerate() {
        if let HirOwnerRef::Hir(id) = authored.site.owner {
            assert!(
                ids.0.contains(&id),
                "authored_types[{index}] site owner HirId({}) ({:?}) is not in the finalized HIR",
                id.0,
                authored.site.slot
            );
        }
    }
    for (index, owner) in original.presentation.owners.iter().enumerate() {
        if let Some(HirOwnerRef::Hir(id)) = owner.semantic {
            assert!(
                ids.0.contains(&id),
                "owners[{index}] semantic HirId({}) ({:?}) is not in the finalized HIR",
                id.0,
                owner.kind
            );
        }
    }

    let artifact = from_analyzed(&original, &name, &name);
    let bytes = postcard::to_allocvec(&artifact).expect("serialize finite-nan artifact");
    load_fhir(&bytes).expect("finite-nan artifact must load");
}

#[test]
fn serve_typed_round_trips() {
    // `ad/serve-typed.fab` authors `channel<O, R>` (`sermo<O, R>`) bindings.
    // The conversation type is a builtin named constructor whose finalized
    // witness is the bare carrier, so both authored heads and witnesses must
    // survive the artifact round trip.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/ad/serve-typed.fab");
    let name = path.display().to_string();
    let source = std::fs::read_to_string(&path).expect("serve-typed fixture reads");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original =
        analyze_source(&session, &name, &source, AnalyzeRequest::default()).expect("serve-typed fixture analyzes");
    let artifact = from_analyzed(&original, &name, &name);
    // The conversation constructor is compiler-owned, and the artifact records
    // it explicitly so the validator can pair the authored `sermo<O, R>`
    // children with the bare carrier witness analysis finalizes them to.
    let conversation = original.resolver.builtin_sermo_def_id();
    assert!(conversation.is_some(), "serve-typed registers the conversation builtin");
    assert_eq!(artifact.conversation_constructor, conversation);
    let bytes = postcard::to_allocvec(&artifact).expect("serialize serve-typed artifact");
    let decoded = radix_hir_fhir::decode(&bytes).expect("serve-typed artifact must load");
    assert_eq!(
        decoded.conversation_constructor, conversation,
        "the recorded conversation constructor must survive the wire"
    );
    load_fhir(&bytes).expect("serve-typed artifact must load");
}

#[test]
fn octeti_zero_argument_type_form_survives_the_fhir_round_trip() {
    // `lista<b8>` is authored as a list type form, but its
    // finalized semantic witness is the byte-list array type. The FHIR
    // validator must accept the authored/semantic pairing.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = "fixum lista<b8> b = |de ad|\n\nincipit {\n    nota b\n}\n";
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original =
        analyze_source(&session, "octeti.fab", source, AnalyzeRequest::default()).expect("octeti fixture analyzes");
    let artifact = from_analyzed(&original, "octeti", "octeti.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize octeti artifact");
    load_fhir(&bytes).expect("octeti artifact must load");
}

#[test]
fn closure_parameter_hole_survives_the_fhir_round_trip() {
    // `_ m` is a closure parameter whose type the compiler infers from the
    // call; the authored hole is recorded at a `ClosureParameter` site with
    // the inferred witness. That is a legal hole slot, not an information-free
    // hole the artifact must drop.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = concat!(
        "+++\nlocale = \"en\"\n+++\n\n",
        "main {\n",
        "    let regex word \u{2190} \"[a-z]+\" \u{21a6} regex\n",
        "    print word.replace(\"ab cd e\", _ m \u{2234} m.text())\n",
        "}\n",
    );
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "replace-closure.fab", source, AnalyzeRequest::default())
        .expect("replace-closure fixture analyzes");
    let artifact = from_analyzed(&original, "replace-closure", "replace-closure.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize replace-closure artifact");
    load_fhir(&bytes).expect("replace-closure artifact must load");
}

#[test]
fn transparent_alias_annotation_survives_the_fhir_round_trip() {
    // A transparent type alias used as an annotation (`type Signum = int`)
    // lowers to its resolved body at the HIR site while the authored form
    // keeps the alias's Named head. The artifact boundary must pair that
    // authored head with a witness the validator accepts.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = concat!(
        "+++\nlocale = \"en\"\n+++\n\n",
        "type Signum = int\n",
        "type Cognomen = string\n",
        "type Viget = bool\n",
        "type Nomina = list<string>\n",
        "type Puncta = list<int>\n",
        "type Index = map<string, int>\n",
        "type NomenOptivum = string \u{222a} none\n\n",
        "main {\n",
        "    let Signum signum \u{2190} 42\n",
        "    let Cognomen cognomen \u{2190} \"Marcus\"\n",
        "    let Viget viget \u{2190} true\n",
        "    let Nomina sodales \u{2190} [\"Gaius\", \"Lucius\", \"Titus\"]\n",
        "    let Puncta puncta \u{2190} [100, 95, 87]\n",
        "    let Index index \u{2190} { \"a\": 1 }\n",
        "    let NomenOptivum nomen \u{2190} null\n",
        "    print signum\n",
        "    print cognomen\n",
        "    print viget\n",
        "    print sodales\n",
        "    print puncta\n",
        "    print index\n",
        "    print nomen\n",
        "}\n",
    );
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "transparent-alias.fab", source, AnalyzeRequest::default())
        .expect("transparent alias fixture analyzes");
    let artifact = from_analyzed(&original, "transparent-alias", "transparent-alias.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize transparent alias artifact");
    load_fhir(&bytes).expect("transparent alias artifact must load");
}

#[test]
fn union_alias_annotation_survives_the_fhir_round_trip() {
    // `type Valor = int ∪ string` is a transparent alias whose annotation
    // finalizes to a union body at the HIR site. The authored `Named` head
    // must still load once the alias-to-body fact is the witness.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/unio/unio.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "union-alias.fab", source, AnalyzeRequest::default())
        .expect("union alias fixture analyzes");
    let artifact = from_analyzed(&original, "union-alias", "union-alias.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize union alias artifact");
    load_fhir(&bytes).expect("union alias artifact must load");
}

#[test]
fn transparent_generic_alias_annotation_survives_the_fhir_round_trip() {
    // `type Pairs<T> = list<T>` applied as `Pairs<int>` is a transparent
    // generic alias. The authored `Named` head keeps its type arguments while
    // the alias-to-body fact is the witness, so the artifact must load.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/typus/generic-alias.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "generic-alias.fab", source, AnalyzeRequest::default())
        .expect("generic alias fixture analyzes");
    let artifact = from_analyzed(&original, "generic-alias", "generic-alias.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize generic alias artifact");
    load_fhir(&bytes).expect("generic alias artifact must load");
}

#[test]
fn alias_chain_round_trips() {
    // `type A = int`, `type B = A`, `type C = list<B>` and `type F = (int) →
    // int` form a chain of transparent aliases. A recorded child witness can be
    // the other member of the same alias pair (the alias versus its body), so
    // the boundary pairs the child with the named witness instead of wrapping
    // it in a `Semantic` head the validator rejects as a named constructor.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/typus/alias-chain.fab");
    let name = path.display().to_string();
    let source = std::fs::read_to_string(&path).expect("alias-chain fixture reads");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original =
        analyze_source(&session, &name, &source, AnalyzeRequest::default()).expect("alias-chain fixture analyzes");
    let artifact = from_analyzed(&original, &name, &name);
    let bytes = postcard::to_allocvec(&artifact).expect("serialize alias-chain artifact");
    load_fhir(&bytes).expect("alias-chain artifact must load");
}

#[test]
fn optional_function_alias_round_trips() {
    // The optional-call exemplar spells callable aliases beneath an optional
    // union (`Op ∪ none`). Finalized HIR canonicalizes the callable signature,
    // so the FHIR presentation must preserve and validate the alias witness
    // rather than wrapping it in a synthetic semantic function form.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/optionalis/optional-call-flatten.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "optional-call-flatten.fab", source, AnalyzeRequest::default())
        .expect("optional function alias fixture analyzes");
    let artifact = from_analyzed(&original, "optional-call-flatten", "optional-call-flatten.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize optional function alias artifact");
    load_fhir(&bytes).expect("optional function alias artifact must load");
}

#[test]
fn genus_union_alias_survives_the_fhir_round_trip() {
    // `type Figura = Circulus ∪ Quadratum` is a transparent alias whose
    // annotation finalizes to a union of genus types at the HIR site. The
    // authored `Named` head must load with the recorded alias-to-body fact.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/unio/overlap.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original =
        analyze_source(&session, "overlap.fab", source, AnalyzeRequest::default()).expect("overlap fixture analyzes");
    let artifact = from_analyzed(&original, "overlap", "overlap.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize overlap artifact");
    load_fhir(&bytes).expect("overlap artifact must load");
}

#[test]
fn inferred_constructor_map_survives_the_fhir_round_trip() {
    // `_ { … }` is an inferred constructor: the authored hole is resolved to
    // the site's expected type, and HIR materializes no node for the hole
    // itself. The artifact boundary must drop the information-free hole
    // record rather than fail the validator with `unknown_type_slot`.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = "+++\nlocale = \"en\"\n+++\n\nfn pondus() \u{2192} map<string, int> {\n    return _ { \"x\": 7, \"y\": 9 }\n}\n";
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "inferred-constructor-map.fab", source, AnalyzeRequest::default())
        .expect("inferred constructor map fixture analyzes");
    let artifact = from_analyzed(&original, "inferred-constructor-map", "inferred-constructor-map.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize inferred constructor artifact");
    load_fhir(&bytes).expect("inferred constructor artifact must load");
}

#[test]
fn genus_ascribed_literal_survives_the_fhir_round_trip() {
    // `{ flag = true } ∷ Calc` is a typed constructor whose `∷` ascription
    // target is recorded at an expression slot, not a binding slot. The
    // authored hole and the resolved witness must load.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/genus/ascribed-literal.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "ascribed-literal.fab", source, AnalyzeRequest::default())
        .expect("genus ascribed-literal fixture analyzes");
    let artifact = from_analyzed(&original, "ascribed-literal", "ascribed-literal.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize ascribed-literal artifact");
    load_fhir(&bytes).expect("ascribed-literal artifact must load");
}

#[test]
fn genus_inferred_constructor_survives_the_fhir_round_trip() {
    // `_ { x = 7, y = 9 }` is an inferred constructor whose authored `_` hole
    // is recorded at the site of the constructor expression, not a binding
    // slot. The artifact boundary must carry the resolved witness and load.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/genus/inferred-constructor.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "inferred-constructor.fab", source, AnalyzeRequest::default())
        .expect("genus inferred-constructor fixture analyzes");
    let artifact = from_analyzed(&original, "inferred-constructor", "inferred-constructor.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize inferred-constructor artifact");
    load_fhir(&bytes).expect("inferred-constructor artifact must load");
}

#[test]
fn genus_inferred_constructor_optional_survives_the_fhir_round_trip() {
    // Against an optional cell, `_ { … }` builds the non-optional format and
    // is legal in binding, argument and return positions.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = include_str!("../../../../corpus/genus/inferred-constructor-optional.fab");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "inferred-constructor-optional.fab", source, AnalyzeRequest::default())
        .expect("genus inferred-constructor-optional fixture analyzes");
    let artifact = from_analyzed(&original, "inferred-constructor-optional", "inferred-constructor-optional.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize inferred-constructor-optional artifact");
    load_fhir(&bytes).expect("inferred-constructor-optional artifact must load");
}

#[test]
fn fhir_artifact_keeps_inferred_set_literal_hole() {
    // `_ [1, 2]` at a set cell is an inferred constructor: typecheck rewrites
    // it to the `array ↦ set` conversion while keeping the literal's node id,
    // and lowering's authored `_` record stays at that node's
    // `AscriptionTarget` slot. The artifact boundary must keep the hole record
    // rather than fail the validator with `unknown_type_slot` or drop it.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;
    use radix_hir::{HirOwnerRef, HirTypeFormHead, HirTypeSlot};

    let source = "+++\nlocale = \"en\"\n+++\n\nmain {\n    let set<int> s ← _ [1, 2]\n    print s.length()\n}\n";
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "inferred-set-literal.fab", source, AnalyzeRequest::default())
        .expect("inferred set-literal fixture analyzes");
    let artifact = from_analyzed(&original, "inferred-set-literal", "inferred-set-literal.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize inferred set-literal artifact");
    // The validator must recognize the inferred-constructor ascription site.
    // The separate hole-slot legality check may still reject an `_` record at
    // a non-`Declared` slot (tracked separately), but the failure must never
    // be an unknown type slot.
    match load_fhir(&bytes) {
        Ok(_) => {}
        Err(HirArtifactError::InvalidPresentation { issue, .. }) => {
            assert_ne!(
                issue, "unknown_type_slot",
                "an inferred set-literal AscriptionTarget site must be registered"
            );
        }
        Err(error) => panic!("inferred set-literal artifact must not fail structurally, got {error:?}"),
    }

    // Read the wire form back to pin that the authored hole record itself is
    // serialized, not dropped: the loader's decode already proved the wire is
    // structurally sound above.
    let wire: HirArtifact =
        postcard::from_bytes(&bytes).expect("serialized inferred set-literal artifact must round-trip");
    let presentation = &wire.presentation;
    let holes: Vec<_> = presentation
        .authored_types
        .iter()
        .filter(|authored| {
            matches!(authored.form.head, HirTypeFormHead::Infer)
                && matches!(authored.site.slot, HirTypeSlot::AscriptionTarget)
                && matches!(
                    authored.site.owner,
                    HirOwnerRef::Hir(id) if presentation.inferred_constructor_ids.contains(&id)
                )
        })
        .collect();
    assert_eq!(
        holes.len(),
        1,
        "round-tripped presentation must keep the inferred set-literal hole at its AscriptionTarget site"
    );
}

// ── U2.3: fail-closed error path tests ────────────────────────────────

#[test]
fn loader_fail_closed() {
    // Malformed: garbage bytes → Malformed variant.
    // A current SCHEMA_VERSION prefix (accepted), then unterminated varints
    // that cause DeserializeBadVarint rather than DeserializeUnexpectedEnd.
    // A 0x01 prefix is the schema-v1 rejection path (UnsupportedSchemaVersion),
    // not Malformed — the version prefix is read before payload interpretation.
    let garbage: Vec<u8> = {
        let mut v = vec![radix_hir_fhir::SCHEMA_VERSION as u8]; // schema_version = current (accepted) version
        v.extend(std::iter::repeat_n(0xFFu8, 256));
        v
    };
    let result = load_fhir(&garbage);
    match result {
        Err(HirArtifactError::Malformed(_)) => {}
        Err(other) => panic!("expected Malformed, got {other:?}"),
        Ok(_) => panic!("expected Malformed, got Ok"),
    }

    // Truncated: empty slice → Truncated variant.
    let result = load_fhir(b"");
    match result {
        Err(HirArtifactError::TruncatedArtifact) => {}
        Err(other) => panic!("expected Truncated, got {other:?}"),
        Ok(_) => panic!("expected Truncated, got Ok"),
    }

    // Truncated: partial valid data → Truncated variant.
    let valid = minimal_test_artifact();
    let full_bytes = serialize_artifact(&valid);
    // Slice off the last byte to trigger DeserializeUnexpectedEnd.
    let truncated_bytes = &full_bytes[..full_bytes.len() - 1];
    let result = load_fhir(truncated_bytes);
    match result {
        Err(HirArtifactError::TruncatedArtifact) => {}
        Err(other) => panic!("expected Truncated from partial bytes, got {other:?}"),
        Ok(_) => panic!("expected Truncated from partial bytes, got Ok"),
    }

    // Version mismatch: same artifact but with future schema version.
    let mut version_mismatch = minimal_test_artifact();
    version_mismatch.schema_version = 99;
    let bytes = serialize_artifact(&version_mismatch);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::UnsupportedSchemaVersion { found, .. }) => assert_eq!(found, 99),
        Err(other) => panic!("expected UnsupportedSchemaVersion(found=99), got {other:?}"),
        Ok(_) => panic!("expected UnsupportedSchemaVersion(found=99), got Ok"),
    }
}

#[test]
fn loader_referential_integrity() {
    // ── Dangling DefId (non-builtin, not in registry) ──
    let mut artifact = minimal_test_artifact();
    let bad_def_id = radix_types::DefId(50000);
    artifact.gpu_builtins = vec![(bad_def_id, radix_hir_fhir::GpuBuiltinWire::GlobalInvocationX)];
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, .. }) => {
            assert_eq!(kind, "DefId");
            assert_eq!(index, 50000);
        }
        Err(other) => panic!("expected DanglingIndex(DefId, 50000), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(DefId, 50000), got Ok"),
    }

    // ── Dangling HirId ──
    // HirId validation is membership-based: an in-tree node's own id is by
    // definition registered, so mutating item.id cannot dangle. A genuinely
    // dangling HirId arises only from a cross-reference — here the
    // presentation owner's semantic back-ref naming an id outside the tree.
    let mut artifact = minimal_test_artifact();
    artifact
        .presentation
        .owners
        .push(radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(1),
            kind: radix_hir::HirTriviaOwnerKind::Item,
            semantic: Some(radix_hir::HirOwnerRef::Hir(radix_hir::HirId(99))),
        });
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, .. }) => {
            assert_eq!(kind, "HirId");
            assert_eq!(index, 99);
        }
        Err(other) => panic!("expected DanglingIndex(HirId, 99), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(HirId, 99), got Ok"),
    }

    // ── Dangling TypeId ──
    use radix_hir::{HirFunction, HirItemKind};
    let mut artifact = minimal_test_artifact();
    // Add a function with an out-of-bounds TypeId in a parameter.
    let bad_func = HirFunction {
        name: radix_lexer::Symbol(0),
        type_params: Vec::new(),
        params: vec![radix_hir::HirParam {
            def_id: radix_types::DefId(0),
            name: radix_lexer::Symbol(0),
            ty: radix_types::TypeId(9999),
            mode: radix_hir::HirParamMode::Owned,
            optional: false,
            sponte: false,
            default: None,
            span: radix_lexer::Span::default(),
        }],
        cli_args: None,
        ret_ty: None,
        err_ty: None,
        body: None,
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    };
    // Replace the existing item with the corrupt function.
    artifact.hir.items[0].kind = HirItemKind::Function(Box::new(bad_func));
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, .. }) => {
            assert_eq!(kind, "TypeId");
            assert_eq!(index, 9999);
        }
        Err(other) => panic!("expected DanglingIndex(TypeId, 9999), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(TypeId, 9999), got Ok"),
    }

    // ── Dangling Symbol ──
    let mut artifact = minimal_test_artifact();
    // The interner has 2 entries (indices 0 and 1). Set name to Symbol(99).
    if let HirItemKind::Function(ref mut f) = artifact.hir.items[0].kind {
        f.name = radix_lexer::Symbol(99);
    }
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, .. }) => {
            assert_eq!(kind, "Symbol");
            assert_eq!(index, 99);
        }
        Err(other) => panic!("expected DanglingIndex(Symbol, 99), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(Symbol, 99), got Ok"),
    }
}

#[test]
fn user_range_def_id_loads_ok() {
    // DefId is opaque; user definitions start at USER_DEF_ID_BASE.
    // This test proves the registry-based DefId check allows valid
    // user-range DefIds through (regression for audit §4.1 blocker).
    let mut artifact = minimal_test_artifact();

    // Two items with user-range DefIds.
    let item0 = radix_hir::HirItem {
        annotations: Vec::new(),
        id: radix_hir::HirId(0),
        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE),
        kind: radix_hir::HirItemKind::Function(Box::new(radix_hir::HirFunction {
            name: radix_lexer::Symbol(0),
            type_params: Vec::new(),
            params: vec![radix_hir::HirParam {
                def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 10),
                name: radix_lexer::Symbol(1),
                ty: radix_types::TypeId(0), // Nihil (valid primitive)
                mode: radix_hir::HirParamMode::Owned,
                optional: false,
                sponte: false,
                default: None,
                span: radix_lexer::Span::default(),
            }],
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: Some(radix_hir::HirBlock {
                span: radix_lexer::Span::default(),
                breakable: None,
                end: None,
                statements: vec![radix_hir::HirStatement {
                    id: radix_hir::HirId(0),
                    span: radix_lexer::Span::default(),
                    // Local with its own DefId.
                    kind: radix_hir::HirStatementKind::Local(radix_hir::HirLocal {
                        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 11),
                        name: radix_lexer::Symbol(1),
                        ty: None,
                        mutable: false,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                        init: Some(radix_hir::HirExpression {
                            id: radix_hir::HirId(1),
                            kind: radix_hir::HirExpressionKind::Path(radix_types::DefId(
                                radix_types::USER_DEF_ID_BASE + 10,
                            )),
                            ty: None,
                            span: radix_lexer::Span::default(),
                        }),
                    }),
                }],
                expr: None,
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        })),
        span: radix_lexer::Span::default(),
    };
    let item1 = radix_hir::HirItem {
        annotations: Vec::new(),
        id: radix_hir::HirId(1),
        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 1),
        kind: radix_hir::HirItemKind::Function(Box::new(radix_hir::HirFunction {
            name: radix_lexer::Symbol(2),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: None,
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        })),
        span: radix_lexer::Span::default(),
    };

    artifact.hir.items = vec![item0, item1];
    // Ensure interner has enough entries for all symbols used.
    artifact.interner = vec!["func0".to_string(), "x".to_string(), "func1".to_string()];

    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    assert!(result.is_ok(), "user-range DefIds must load Ok");
}

#[test]
fn corrupt_type_arena_rejected() {
    // A type in the type table references a DefId that is not a
    // definition site → DanglingIndex (gap 4.2).
    let mut artifact = minimal_test_artifact();

    // Build a type table containing a corrupt Type::Struct(DefId(9999)).
    let mut types = radix_types::TypeTable::new();
    let _bad_type_id = types.intern(radix_types::Type::Struct(radix_types::DefId(9999)));
    artifact.types = types.snapshot();

    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, .. }) => {
            assert_eq!(kind, "DefId");
            assert_eq!(index, 9999);
        }
        Err(other) => panic!("expected DanglingIndex(DefId, 9999) from corrupt type arena, got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(DefId, 9999) from corrupt type arena, got Ok"),
    }
}

#[test]
fn serialize_load_roundtrip_ok() {
    // Non-trivial artifact with a function that has params and locals.
    // This would have caught the DefId bound defect in audit §4.1.
    let mut artifact = minimal_test_artifact();

    let func_item = radix_hir::HirItem {
        annotations: Vec::new(),
        id: radix_hir::HirId(0),
        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE),
        kind: radix_hir::HirItemKind::Function(Box::new(radix_hir::HirFunction {
            name: radix_lexer::Symbol(0),
            type_params: vec![radix_hir::HirTypeParam {
                def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 1),
                name: radix_lexer::Symbol(1),
                kind: radix_syntax::GenericParamKind::Typus,
                constraint: radix_hir::HirTypeParamConstraint::Any,
                span: radix_lexer::Span::default(),
                default: None,
            }],
            params: vec![radix_hir::HirParam {
                def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 2),
                name: radix_lexer::Symbol(2),
                ty: radix_types::TypeId(0), // Nihil
                mode: radix_hir::HirParamMode::Owned,
                optional: false,
                sponte: false,
                default: None,
                span: radix_lexer::Span::default(),
            }],
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: Some(radix_hir::HirBlock {
                span: radix_lexer::Span::default(),
                breakable: None,
                end: None,
                statements: vec![radix_hir::HirStatement {
                    id: radix_hir::HirId(1),
                    span: radix_lexer::Span::default(),
                    kind: radix_hir::HirStatementKind::Local(radix_hir::HirLocal {
                        def_id: radix_types::DefId(radix_types::USER_DEF_ID_BASE + 3),
                        name: radix_lexer::Symbol(3),
                        ty: None,
                        mutable: false,
                        is_await: false,
                        runtime_provided: false,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                        init: None,
                    }),
                }],
                expr: None,
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        })),
        span: radix_lexer::Span::default(),
    };

    artifact.hir.items = vec![func_item];
    artifact.interner = vec![
        "my_func".to_string(),
        "T".to_string(),
        "x".to_string(),
        "local".to_string(),
    ];

    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Ok(unit) => {
            assert_eq!(unit.hir.items.len(), 1);
            assert_eq!(unit.hir.items[0].def_id, radix_types::DefId(radix_types::USER_DEF_ID_BASE));
        }
        Err(e) => panic!("serialize→load roundtrip must succeed, got {e}"),
    }
}

// ── U2.4: Adversarial fixture tests ─────────────────────────────────

/// Truncate a valid serialized artifact mid-stream → Truncated.
#[test]
fn adversarial_truncated_postcard() {
    let artifact = minimal_test_artifact();
    let bytes = serialize_artifact(&artifact);
    // Slice off the last byte to trigger DeserializeUnexpectedEnd → Truncated.
    let truncated = &bytes[..bytes.len() - 1];
    let result = load_fhir(truncated);
    match result {
        Err(HirArtifactError::TruncatedArtifact) => {}
        Err(other) => panic!("expected Truncated, got {other:?}"),
        Ok(_) => panic!("expected Truncated, got Ok"),
    }
}

/// Valid artifact with schema_version = 99 → UnsupportedSchemaVersion.
#[test]
fn adversarial_version_mismatch() {
    let mut artifact = minimal_test_artifact();
    artifact.schema_version = 99;
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::UnsupportedSchemaVersion { found, expected }) => {
            assert_eq!(found, 99);
            assert_eq!(expected, radix_hir_fhir::SCHEMA_VERSION);
        }
        Err(other) => panic!("expected UnsupportedSchemaVersion, got {other:?}"),
        Ok(_) => panic!("expected UnsupportedSchemaVersion, got Ok"),
    }
}

/// DefId(u32::MAX) in a field → DanglingIndex(DefId).
#[test]
fn adversarial_dangling_def_id() {
    let mut artifact = minimal_test_artifact();
    artifact.gpu_builtins = vec![(radix_types::DefId(u32::MAX), radix_hir_fhir::GpuBuiltinWire::GlobalInvocationX)];
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "DefId");
            assert_eq!(index, u32::MAX);
            assert!(arena_size > 0, "def_registry should not be empty");
        }
        Err(other) => panic!("expected DanglingIndex(DefId), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(DefId), got Ok"),
    }
}

/// HirId(u32::MAX) on an item → DanglingIndex(HirId).
/// Presentation owner back-referencing a HirId outside the tree → DanglingIndex.
///
/// HirId validation is membership-based, so only a cross-reference (the
/// sidecar's semantic back-ref) can dangle; an in-tree node's own id is
/// always registered.
#[test]
fn adversarial_dangling_hir_id() {
    let mut artifact = minimal_test_artifact();
    artifact
        .presentation
        .owners
        .push(radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(1),
            kind: radix_hir::HirTriviaOwnerKind::Statement,
            semantic: Some(radix_hir::HirOwnerRef::Hir(radix_hir::HirId(u32::MAX))),
        });
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "HirId");
            assert_eq!(index, u32::MAX);
            assert!(arena_size > 0, "HirId registry should not be empty");
        }
        Err(other) => panic!("expected DanglingIndex(HirId), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(HirId), got Ok"),
    }
}

/// FunctionFactTable entry with DefId not in HIR def registry → DanglingIndex.
#[test]
fn adversarial_cross_arena_dangle() {
    let mut artifact = minimal_test_artifact();
    // Add a function fact for DefId(99999) that does not exist in the HIR.
    artifact.function_facts = radix_hir_fhir::FunctionFactTableWire {
        entries: vec![(
            radix_types::DefId(99999),
            radix_hir_fhir::FunctionFactWire {
                has_direct_failure: false,
                may_fail: false,
                is_async: false,
                is_generator: false,
                requires_await: false,
                call_edges: Vec::new(),
                captures: Vec::new(),
                param_modes: Vec::new(),
                err_ty: None,
            },
        )],
        entry: None,
    };
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "DefId");
            assert_eq!(index, 99999);
            assert!(arena_size > 0, "def_registry should not be empty");
        }
        Err(other) => panic!("expected DanglingIndex(DefId) cross-arena, got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(DefId) cross-arena, got Ok"),
    }
}

// ── Schema-v2 presentation sidecar validation (U-4) ─────────────────────
//
// The loader rejects dangling/duplicate presentation anchors, duplicate
// attachment owners, and dangling comment Symbols before reconstruction
// (CSO E8 fail-closed). Each test builds one deliberate violation.

use radix_hir::{
    HirOwnerRef, HirPresentation, HirSourceAnchorId, HirTrivia, HirTriviaAttachment, HirTriviaOwner, HirTriviaOwnerKind,
};

/// Attachment whose anchor names no registered owner → DanglingIndex.
#[test]
fn presentation_dangling_attachment_anchor() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = HirPresentation {
        owners: vec![HirTriviaOwner { id: HirSourceAnchorId(1), kind: HirTriviaOwnerKind::Statement, semantic: None }],
        attachments: vec![HirTriviaAttachment {
            anchor: HirSourceAnchorId(9), // no owner with id 9
            leading: Vec::new(),
        }],
        block_ends: Vec::new(),
        program_end: empty_presentation().program_end,
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    };
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "HirSourceAnchorId");
            assert_eq!(index, 9);
            assert_eq!(arena_size, 1);
        }
        Err(other) => panic!("expected DanglingIndex(HirSourceAnchorId), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(HirSourceAnchorId), got Ok"),
    }
}

/// Entry-start anchor naming no registered owner → DanglingIndex.
#[test]
fn presentation_dangling_entry_anchor() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: empty_presentation().program_end,
        entry_start_anchor: Some(HirSourceAnchorId(4)),
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    };
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, .. }) => {
            assert_eq!(kind, "HirSourceAnchorId");
            assert_eq!(index, 4);
        }
        Err(other) => panic!("expected DanglingIndex(HirSourceAnchorId), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(HirSourceAnchorId), got Ok"),
    }
}

/// Two owners sharing one anchor id → InvalidPresentation.
#[test]
fn presentation_duplicate_owner_anchor() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = HirPresentation {
        owners: vec![
            HirTriviaOwner { id: HirSourceAnchorId(1), kind: HirTriviaOwnerKind::Statement, semantic: None },
            HirTriviaOwner { id: HirSourceAnchorId(1), kind: HirTriviaOwnerKind::Item, semantic: None },
        ],
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: empty_presentation().program_end,
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    };
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "duplicate_owner_anchor");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(duplicate_owner_anchor), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(duplicate_owner_anchor), got Ok"),
    }
}

/// Two attachments naming the same owner → InvalidPresentation.
#[test]
fn presentation_duplicate_attachment_owner() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = HirPresentation {
        owners: vec![HirTriviaOwner { id: HirSourceAnchorId(1), kind: HirTriviaOwnerKind::Statement, semantic: None }],
        attachments: vec![
            HirTriviaAttachment { anchor: HirSourceAnchorId(1), leading: Vec::new() },
            HirTriviaAttachment { anchor: HirSourceAnchorId(1), leading: Vec::new() },
        ],
        block_ends: Vec::new(),
        program_end: empty_presentation().program_end,
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    };
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "duplicate_attachment_owner");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(duplicate_attachment_owner), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(duplicate_attachment_owner), got Ok"),
    }
}

/// A CommentLine whose text Symbol is not in the interner table → DanglingIndex.
#[test]
fn presentation_dangling_comment_symbol() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = HirPresentation {
        owners: vec![HirTriviaOwner { id: HirSourceAnchorId(1), kind: HirTriviaOwnerKind::Statement, semantic: None }],
        attachments: vec![HirTriviaAttachment {
            anchor: HirSourceAnchorId(1),
            leading: vec![HirTrivia::CommentLine {
                text: radix_lexer::Symbol(50), // interner has 2 entries (0..2)
                span: radix_lexer::Span::default(),
            }],
        }],
        block_ends: Vec::new(),
        program_end: empty_presentation().program_end,
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    };
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "Symbol");
            assert_eq!(index, 50);
            assert_eq!(arena_size, 2);
        }
        Err(other) => panic!("expected DanglingIndex(Symbol), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(Symbol), got Ok"),
    }
}

/// An owner whose `HirOwnerRef::Def` names a DefId outside the def registry
/// → DanglingIndex (the semantic back-ref is part of the sidecar contract).
#[test]
fn presentation_dangling_def_backref() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = HirPresentation {
        owners: vec![HirTriviaOwner {
            id: HirSourceAnchorId(1),
            kind: HirTriviaOwnerKind::Member,
            semantic: Some(HirOwnerRef::Def(radix_types::DefId(50_000))),
        }],
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: empty_presentation().program_end,
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        template_holes: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    };
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "DefId");
            assert_eq!(index, 50_000);
            assert!(arena_size > 0, "def registry should not be empty");
        }
        Err(other) => panic!("expected DanglingIndex(DefId), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(DefId), got Ok"),
    }
}

/// A schema-v2 no-trivia artifact loads and round-trips deterministically.
#[test]
fn schema_v2_no_trivia_round_trip() {
    let artifact = minimal_test_artifact();
    let bytes1 = serialize_artifact(&artifact);
    let bytes2 = serialize_artifact(&artifact);
    assert_eq!(bytes1, bytes2, "no-trivia artifact serialization must be deterministic");
    let loaded = load_fhir(&bytes1).expect("schema-v2 no-trivia artifact must load");
    assert_eq!(loaded.presentation, artifact.presentation);
}

// ── U2.4: Wire-conversion helpers for round-trip acceptance ───────

fn capture_fact_to_wire(fact: &crate::semantic::CaptureFact) -> radix_hir_fhir::CaptureFactWire {
    radix_hir_fhir::CaptureFactWire {
        def_id: fact.def_id,
        mode: match fact.mode {
            crate::semantic::CaptureMode::Read => radix_hir_fhir::CaptureModeWire::Read,
            crate::semantic::CaptureMode::Mutate => radix_hir_fhir::CaptureModeWire::Mutate,
            crate::semantic::CaptureMode::Consume => radix_hir_fhir::CaptureModeWire::Consume,
        },
    }
}

fn function_fact_to_wire(fact: &crate::semantic::FunctionFact) -> radix_hir_fhir::FunctionFactWire {
    radix_hir_fhir::FunctionFactWire {
        has_direct_failure: fact.direct_failure,
        may_fail: fact.may_fail,
        is_async: fact.is_async,
        is_generator: fact.is_generator,
        requires_await: fact.requires_await,
        call_edges: fact.call_edges.iter().copied().collect(),
        captures: fact.captures.iter().map(capture_fact_to_wire).collect(),
        param_modes: fact.param_modes.iter().map(|(k, v)| (*k, *v)).collect(),
        err_ty: fact.err_ty,
    }
}

fn function_fact_table_to_wire(ft: &crate::semantic::FunctionFactTable) -> radix_hir_fhir::FunctionFactTableWire {
    radix_hir_fhir::FunctionFactTableWire {
        entries: ft
            .iter()
            .map(|(def_id, fact)| (def_id, function_fact_to_wire(fact)))
            .collect(),
        entry: ft.entry.as_ref().map(function_fact_to_wire),
    }
}

fn resolved_use_to_wire(u: &crate::semantic::ResolvedUse) -> radix_hir_fhir::ResolvedUseWire {
    radix_hir_fhir::ResolvedUseWire {
        key: match &u.key {
            crate::semantic::ResolvedUseKey::Local(def_id) => radix_hir_fhir::ResolvedUseKeyWire::Local(*def_id),
            crate::semantic::ResolvedUseKey::Portable(s) => radix_hir_fhir::ResolvedUseKeyWire::Portable(s.clone()),
        },
        kind: match u.kind {
            crate::semantic::ResolvedUseKind::Path => radix_hir_fhir::ResolvedUseKindWire::Path,
            crate::semantic::ResolvedUseKind::Call => radix_hir_fhir::ResolvedUseKindWire::Call,
            crate::semantic::ResolvedUseKind::MethodCall => radix_hir_fhir::ResolvedUseKindWire::MethodCall,
            crate::semantic::ResolvedUseKind::TypeRef => radix_hir_fhir::ResolvedUseKindWire::TypeRef,
            crate::semantic::ResolvedUseKind::ImportBinding => radix_hir_fhir::ResolvedUseKindWire::ImportBinding,
        },
        enclosing: u.enclosing,
        local: u.local,
    }
}

fn gpu_builtin_to_wire(b: radix_types::MirKernelBuiltin) -> radix_hir_fhir::GpuBuiltinWire {
    match b {
        radix_types::MirKernelBuiltin::GlobalInvocationX => radix_hir_fhir::GpuBuiltinWire::GlobalInvocationX,
        radix_types::MirKernelBuiltin::GlobalInvocationY => radix_hir_fhir::GpuBuiltinWire::GlobalInvocationY,
        radix_types::MirKernelBuiltin::GlobalInvocationZ => radix_hir_fhir::GpuBuiltinWire::GlobalInvocationZ,
        radix_types::MirKernelBuiltin::GlobalInvocationId => radix_hir_fhir::GpuBuiltinWire::GlobalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupIdX => radix_hir_fhir::GpuBuiltinWire::WorkgroupIdX,
        radix_types::MirKernelBuiltin::WorkgroupIdY => radix_hir_fhir::GpuBuiltinWire::WorkgroupIdY,
        radix_types::MirKernelBuiltin::WorkgroupIdZ => radix_hir_fhir::GpuBuiltinWire::WorkgroupIdZ,
        radix_types::MirKernelBuiltin::WorkgroupId => radix_hir_fhir::GpuBuiltinWire::WorkgroupId,
        radix_types::MirKernelBuiltin::LocalInvocationIdX => radix_hir_fhir::GpuBuiltinWire::LocalInvocationIdX,
        radix_types::MirKernelBuiltin::LocalInvocationIdY => radix_hir_fhir::GpuBuiltinWire::LocalInvocationIdY,
        radix_types::MirKernelBuiltin::LocalInvocationIdZ => radix_hir_fhir::GpuBuiltinWire::LocalInvocationIdZ,
        radix_types::MirKernelBuiltin::LocalInvocationId => radix_hir_fhir::GpuBuiltinWire::LocalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupSizeX => radix_hir_fhir::GpuBuiltinWire::WorkgroupSizeX,
        radix_types::MirKernelBuiltin::WorkgroupSizeY => radix_hir_fhir::GpuBuiltinWire::WorkgroupSizeY,
        radix_types::MirKernelBuiltin::WorkgroupSizeZ => radix_hir_fhir::GpuBuiltinWire::WorkgroupSizeZ,
    }
}

/// Build a HirArtifact from an AnalyzedModule for round-trip testing.
fn build_artifact_from_unit(unit: &AnalyzedModule) -> HirArtifact {
    use radix_hir_fhir::{SCHEMA_VERSION, SourceIdentity};
    HirArtifact {
        schema_version: SCHEMA_VERSION,
        shape_hash: radix_hir_fhir::wire_shape_hash(),
        source_identity: SourceIdentity {
            content_hash: "test_hash".to_string(),
            relative_path: "test.faber".to_string(),
        },
        hir: unit.hir.clone(),
        presentation: unit.presentation.clone(),
        cli_program: None,
        types: unit.types.snapshot(),
        resolved_generic_calls: unit.types.resolved_generic_calls.clone(),
        radix_lanes: unit
            .radix_lanes
            .snapshot()
            .into_iter()
            .map(|(def_id, lane, backward)| radix_hir_fhir::RadixLaneEntryWire {
                def_id,
                lane: match lane {
                    crate::semantic::RadixLane::Air => radix_hir_fhir::RadixLaneWire::Air,
                    crate::semantic::RadixLane::Mir => radix_hir_fhir::RadixLaneWire::Mir,
                    crate::semantic::RadixLane::HirDirect => radix_hir_fhir::RadixLaneWire::HirDirect,
                },
                backward: backward.map(|backward| radix_hir_fhir::RadixBackwardWire {
                    companion_name: backward.companion_name,
                    companion_def_id: backward.companion_def_id,
                    selected_params: backward.selected_params,
                }),
            })
            .collect(),
        interner: unit.interner.to_string_table(),
        resolver: unit.resolver.to_snapshot(),
        libraries: unit.libraries.clone(),
        function_facts: function_fact_table_to_wire(&unit.function_facts),
        resolved_uses: unit
            .resolved_uses
            .to_snapshot()
            .into_iter()
            .map(|u| resolved_use_to_wire(&u))
            .collect(),
        analysis_stamp: unit.analysis_stamp.get(),
        gpu_builtins: unit
            .gpu_builtins
            .iter()
            .map(|(def_id, builtin)| (*def_id, gpu_builtin_to_wire(*builtin)))
            .collect(),
        capability_provenance: crate::hir::serialize::from_analyzed(unit, "x", "x").capability_provenance,
        provider_units: crate::hir::serialize::from_analyzed(unit, "x", "x").provider_units,
        shape_obligations: crate::hir::serialize::from_analyzed(unit, "x", "x").shape_obligations,
        contract_roles: crate::hir::serialize::from_analyzed(unit, "x", "x").contract_roles,
        kernel_requirements: crate::hir::serialize::from_analyzed(unit, "x", "x").kernel_requirements,
        tensor_edge_policies: crate::hir::serialize::from_analyzed(unit, "x", "x").tensor_edge_policies,
        display_tokens: crate::hir::serialize::from_analyzed(unit, "x", "x").display_tokens,
        conversation_constructor: crate::hir::serialize::from_analyzed(unit, "x", "x").conversation_constructor,
    }
}

// ── U2.4: Full round-trip acceptance (5 HIR-backed targets) ────────

#[test]
fn struct_field_type_sites_remap_to_struct_owner_once() {
    use crate::hir::presentation_witness::normalize_presentation_witnesses;
    use crate::hir::serialize::from_analyzed;
    use radix_hir::{HirItemKind, HirOwnerRef, HirTypeSite, HirTypeSlot};

    let source = "class Point {\n    mut int x\n    mut int y\n}\n\
                  class Label {\n    mut string text\n    mut bool visible\n}\n\
                  fn identity(int value) → int { return value }\n";
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("load English locale pack");
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_latin_locale()
            .with_locale_pack(pack),
    );
    let unit = crate::driver::analyze_source(&session, "field-sites.fab", source, AnalyzeRequest::default())
        .expect("analyze struct field type sites");
    let artifact = from_analyzed(&unit, "field-sites", "field-sites.fab");
    let mut expected = artifact.clone();
    expected.presentation = unit.presentation.clone();

    // Keep the former first-match scan as an independent byte-level oracle.
    let mut remapped = 0;
    for item in &unit.hir.items {
        let HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        for (ordinal, field) in strukt.fields.iter().enumerate() {
            let old_site =
                HirTypeSite { owner: HirOwnerRef::Def(field.def_id), slot: HirTypeSlot::Field(ordinal as u32) };
            let authored = expected
                .presentation
                .authored_types
                .iter_mut()
                .find(|entry| entry.site == old_site)
                .expect("lowering records a field-owned type site");
            authored.site.owner = HirOwnerRef::Def(item.def_id);
            remapped += 1;
            assert_eq!(
                artifact
                    .presentation
                    .authored_types
                    .iter()
                    .filter(|entry| entry.site == authored.site)
                    .count(),
                1,
                "each field has exactly one struct-owned type site"
            );
            assert!(
                !artifact
                    .presentation
                    .authored_types
                    .iter()
                    .any(|entry| entry.site == old_site)
            );
        }
    }
    assert_eq!(remapped, 4);
    assert!(
        expected.presentation.authored_types.len() > remapped,
        "fixture also covers non-field sites"
    );
    let bytes = serialize_artifact(&artifact);
    assert_eq!(
        bytes,
        serialize_artifact(&expected),
        "indexed remapping preserves artifact bytes and site order"
    );
    load_fhir(&bytes).expect("remapped field owners validate");

    let mut repeated = artifact;
    normalize_presentation_witnesses(&repeated.hir, &mut repeated.presentation, &unit.types);
    assert_eq!(
        bytes,
        serialize_artifact(&repeated),
        "already remapped sites are unchanged on a second pass"
    );
}

#[cfg(all(feature = "hir-rust", feature = "hir-ts", feature = "hir-go", feature = "hir-swift"))]
#[test]
fn hir_artifact_round_trip_acceptance() {
    // Source matching the hello-world fixture.
    let source = "incipit {\n    nota \"Salve, Munde!\"\n}\n";

    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let unit = crate::driver::analyze_source(&session, "test.faber", source, AnalyzeRequest::default())
        .expect("analyze hello-world fixture");

    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);

    use crate::codegen::{Target, generate_from_analyzed};

    let targets: &[(Target, &str)] = &[
        (Target::HirRust, "Rust"),
        (Target::HirTypeScript, "TypeScript"),
        (Target::HirGo, "Go"),
        (Target::HirSwift, "Swift"),
        (Target::HirFaber, "Faber"),
    ];

    // Phase 1: run codegen on the original unit, capture outputs.
    let original_outputs: Vec<_> = targets
        .iter()
        .map(|(target, name)| {
            generate_from_analyzed(*target, &unit, &surface)
                .unwrap_or_else(|e| panic!("original codegen failed for {name}: {e}"))
        })
        .collect();

    // Phase 2: serialize unit → HirArtifact → postcard → load_fhir.
    let artifact = build_artifact_from_unit(&unit);
    let bytes = postcard::to_allocvec(&artifact).expect("serialize artifact for round-trip");
    let loaded = load_fhir(&bytes).expect("load_fhir round-trip");

    // Phase 3: run codegen on the loaded unit, compare byte-for-byte.
    for (i, (target, name)) in targets.iter().enumerate() {
        let loaded_output = generate_from_analyzed(*target, &loaded, &surface)
            .unwrap_or_else(|e| panic!("loaded codegen failed for {name}: {e}"));

        let original_code = output_code(&original_outputs[i]);
        let loaded_code = output_code(&loaded_output);

        assert_eq!(original_code, loaded_code, "round-trip mismatch for target {name}");
    }
}

#[cfg(all(feature = "runner", feature = "hir-rust"))]
#[test]
fn runtime_tensor_shape_boundary_survives_fhir_round_trip_and_recovers() {
    use crate::codegen::{Target, generate_from_analyzed};
    use crate::driver::{Config, Session, analyze_source};
    use crate::mir::{BufferHost, run_analyzed};

    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/tensor/runtime-shape-boundary.fab"
    ));
    let session = Session::new(Config::default().with_latin_locale());
    let mut direct = analyze_source(&session, "runtime-shape-boundary.fab", source, AnalyzeRequest::default())
        .expect("runtime shape boundary fixture analyzes");
    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);
    let direct_rust =
        generate_from_analyzed(Target::HirRust, &direct, &surface).expect("direct Rust codegen retains shape guards");
    let direct_rust = output_code(&direct_rust).to_owned();
    assert!(
        direct_rust.contains("tensor runtime shape boundary check failed"),
        "generated Rust carries the recoverable boundary error route"
    );

    let artifact = crate::hir::serialize::from_analyzed(&direct, "shape-boundary", "runtime-shape-boundary.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize shape obligations");
    let mut loaded = load_fhir(&bytes).expect("shape obligations survive validated FHIR load");
    let loaded_rust =
        generate_from_analyzed(Target::HirRust, &loaded, &surface).expect("loaded Rust codegen retains shape guards");
    assert_eq!(
        output_code(&loaded_rust),
        direct_rust,
        "FHIR round-trip preserves emitted Rust shape guards"
    );

    let mut direct_host = BufferHost::default();
    run_analyzed(&mut direct, &mut direct_host).expect("direct runtime shape check recovers");
    let mut loaded_host = BufferHost::default();
    run_analyzed(&mut loaded, &mut loaded_host).expect("loaded runtime shape check recovers");

    // Tensor length is the element count (2 x 2); the mismatch must prevent
    // the second entry.
    let expected = ["entered", "4", "shape mismatch"];
    assert_eq!(direct_host.stdout_lines, expected);
    assert_eq!(loaded_host.stdout_lines, expected);
}

#[test]
fn cli_binding_symbols_survive_fhir_round_trip() {
    use crate::driver::{Config, Session, analyze_source};

    let path = format!("{}/../../corpus/cli/cli.fab", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path).expect("cli fixture");
    let session = Session::new(Config::default().with_dev_locale_root());
    let direct = analyze_source(&session, &path, &source, AnalyzeRequest::default()).expect("fixture analyzes");
    let program = direct
        .cli_program
        .as_ref()
        .expect("direct unit carries a CLI program");
    let bound = |program: &crate::cli::CliProgram| {
        let mut symbols = Vec::new();
        for (options, operands) in std::iter::once((&program.options, &program.operands))
            .chain(std::iter::once((&program.global_options, &program.global_operands)))
            .chain(
                program
                    .commands
                    .iter()
                    .map(|command| (&command.options, &command.operands)),
            )
        {
            symbols.extend(
                options
                    .iter()
                    .map(|option| (option.binding.clone(), option.binding_symbol)),
            );
            symbols.extend(
                operands
                    .iter()
                    .map(|operand| (operand.binding.clone(), operand.binding_symbol)),
            );
        }
        symbols
    };
    let expected = bound(program);
    assert!(!expected.is_empty(), "fixture declares CLI options or operands");

    let artifact = crate::hir::serialize::from_analyzed(&direct, "cli-symbols", "cli.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize CLI program");
    let loaded = load_fhir(&bytes).expect("CLI binding symbols survive validated FHIR load");
    assert_eq!(bound(loaded.cli_program.as_ref().expect("loaded CLI program")), expected);
    for (binding, symbol) in expected {
        assert_eq!(loaded.interner.resolve(symbol), binding);
    }
}

#[test]
fn contract_roles_survive_fhir_round_trip() {
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let mut unit = crate::driver::analyze_source(
        &session,
        "contract-roles.fab",
        "functio echo(numerus value) → numerus { redde value }\nincipit { }\n",
        AnalyzeRequest::default(),
    )
    .expect("analyze contract-role fixture");
    let function_def = unit
        .hir
        .items
        .iter()
        .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .expect("function item")
        .def_id;
    unit.types.ordering_contract = Some(function_def);
    unit.types.ordering_contract_bindings.insert(function_def);
    unit.types.closing_contract = Some(function_def);
    unit.types.sinking_contract = Some(function_def);
    unit.types.sinking_contract_bindings.insert(function_def);

    let artifact = crate::hir::serialize::from_analyzed(&unit, "contract-roles", "contract-roles.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize contract roles");
    let loaded = load_fhir(&bytes).expect("contract roles survive validated FHIR load");

    assert_eq!(loaded.types.ordering_contract, Some(function_def));
    assert!(
        loaded
            .types
            .ordering_contract_bindings
            .contains(&function_def)
    );
    assert_eq!(loaded.types.closing_contract, Some(function_def));
    assert_eq!(loaded.types.sinking_contract, Some(function_def));
    assert!(
        loaded
            .types
            .sinking_contract_bindings
            .contains(&function_def)
    );
}

#[test]
fn declaration_store_shape_pins_survive_fhir_round_trip() {
    use crate::driver::{Config, Session, analyze_source};

    let mut pinned = 0;
    for name in [
        "equality-whole",
        "shape-bounded",
        "runtime-extent-use",
        "runtime-shape-boundary",
    ] {
        let path = format!("{}/../../corpus/tensor/{name}.fab", env!("CARGO_MANIFEST_DIR"));
        let source = std::fs::read_to_string(&path).expect("tensor fixture");
        let session = Session::new(Config::default().with_dev_locale_root());
        let direct = analyze_source(&session, &path, &source, AnalyzeRequest::default()).expect("fixture analyzes");
        pinned += direct.shape_obligations.stores.len();

        let artifact = crate::hir::serialize::from_analyzed(&direct, "shape-store", name);
        let bytes = postcard::to_allocvec(&artifact).expect("serialize shape obligations");
        let loaded = load_fhir(&bytes).expect("shape obligations survive validated FHIR load");
        assert_eq!(loaded.shape_obligations, direct.shape_obligations, "{name}");
    }
    assert!(pinned > 0, "no fixture exercised a declaration-store pin");
}

#[test]
fn proven_index_accesses_survive_fhir_round_trip() {
    use crate::driver::{Config, Session, analyze_source};

    let path = format!("{}/../../corpus/mori/mori.fab", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path).expect("mori fixture");
    let session = Session::new(Config::default().with_dev_locale_root());
    let direct = analyze_source(&session, &path, &source, AnalyzeRequest::default()).expect("fixture analyzes");
    assert!(
        !direct.shape_obligations.proven_indexes.is_empty(),
        "the guarded index read is a checker-proven access"
    );

    let artifact = crate::hir::serialize::from_analyzed(&direct, "proven-index", "mori");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize proven indexes");
    let loaded = load_fhir(&bytes).expect("proven indexes survive validated FHIR load");
    assert_eq!(loaded.shape_obligations.proven_indexes, direct.shape_obligations.proven_indexes);
}

#[test]
fn tensor_edge_policy_facts_survive_fhir_round_trip() {
    use radix_hir::{
        ResolvedTensorEdgePolicy, TensorEdgePolicyProviderImpl, TensorEdgePolicyRequirement, TensorShiftContext,
    };
    use radix_types::{BuiltinTensorEdgePolicy, IndexExpr, Primitive, RuntimeExtentId, Type};

    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let mut unit = crate::driver::analyze_source(
        &session,
        "edge-policy-round-trip.fab",
        "functio echo(numerus value) → numerus { redde value }\nincipit { }\n",
        AnalyzeRequest::default(),
    )
    .expect("analyze edge-policy artifact fixture");
    let function = unit
        .hir
        .items
        .iter()
        .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .expect("function item");
    let function_def = function.def_id;
    let function_hir = function.id;
    let binding = match &function.kind {
        radix_hir::HirItemKind::Function(function) => function.params[0].def_id,
        _ => unreachable!(),
    };
    let runtime_dimension = unit
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarParameter { parameter: 0 }));
    let fixed_dimension = unit.types.intern_index(IndexExpr::Literal(4));
    unit.shape_obligations.functions.insert(
        function_def,
        vec![crate::semantic::ShapeRelation {
            left: runtime_dimension,
            right: fixed_dimension,
            span: crate::lexer::Span::default(),
            boundary: crate::semantic::ShapeRelationBoundary::FunctionEntry,
        }],
    );
    let policy_type = unit.types.intern(Type::Struct(function_def));
    let element_type = unit.types.primitive(Primitive::Fractus);
    let builtin_policy_type = unit.types.tensor_edge_policy(BuiltinTensorEdgePolicy::Wrap);
    let source_shape = unit.types.intern_index(IndexExpr::Literal(4));
    let context = TensorShiftContext { source_shape, shift_expression: function_hir };
    unit.tensor_edge_policies.calls.insert(
        function_hir,
        ResolvedTensorEdgePolicy::Custom {
            policy_def: function_def,
            remap: function_def,
            transform: Some(function_def),
        },
    );
    unit.tensor_edge_policies
        .shifted_expressions
        .insert(function_hir);
    unit.tensor_edge_policies
        .shifted_view_expressions
        .insert(function_hir, context);
    unit.tensor_edge_policies.shifted_bindings.insert(binding);
    unit.tensor_edge_policies
        .shifted_view_bindings
        .insert(binding, context);
    unit.tensor_edge_policies
        .limes_contexts
        .insert(function_hir, context);
    unit.tensor_edge_policies.function_requirements.insert(
        function_def,
        vec![
            TensorEdgePolicyRequirement { policy_type, element_type, rank: 2 },
            TensorEdgePolicyRequirement { policy_type, element_type, rank: 3 },
        ],
    );
    unit.tensor_edge_policies.provider_impls = vec![
        TensorEdgePolicyProviderImpl {
            policy_type,
            element_type,
            rank: 2,
            policy: ResolvedTensorEdgePolicy::Custom {
                policy_def: function_def,
                remap: function_def,
                transform: Some(function_def),
            },
        },
        TensorEdgePolicyProviderImpl {
            policy_type: builtin_policy_type,
            element_type,
            rank: 3,
            policy: ResolvedTensorEdgePolicy::Builtin(BuiltinTensorEdgePolicy::Wrap),
        },
    ];

    let mut provider_types = TypeTable::from_snapshot(unit.types.snapshot()).expect("provider type snapshot");
    provider_types.resolved_generic_calls = unit.types.resolved_generic_calls.clone();
    unit.provider_units.insert(
        "./policy-provider",
        crate::driver::CarriedProviderUnit::Unit(std::sync::Arc::new(std::sync::Mutex::new(
            crate::driver::ProviderUnit {
                hir: unit.hir.clone(),
                types: provider_types,
                interner: unit.interner.clone(),
                resolver: unit.resolver.clone(),
                tensor_edge_policies: unit.tensor_edge_policies.clone(),
                shape_obligations: unit.shape_obligations.clone(),
                diagnostics: Vec::new(),
                nested: crate::driver::ProviderUnitView::default(),
            },
        ))),
    );

    let artifact = crate::hir::serialize::from_analyzed(&unit, "edge-policy", "edge-policy-round-trip.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize edge-policy sidecar");
    let loaded = load_fhir(&bytes).expect("load edge-policy sidecar with checked references");

    assert_eq!(loaded.tensor_edge_policies, unit.tensor_edge_policies);
    assert_eq!(loaded.types.get(policy_type), &Type::Struct(function_def));
    assert_eq!(loaded.shape_obligations, unit.shape_obligations);
    let provider = loaded
        .provider_units
        .get("./policy-provider")
        .and_then(crate::driver::CarriedProviderUnit::lock_unit)
        .expect("provider unit survives artifact load");
    assert_eq!(provider.tensor_edge_policies, unit.tensor_edge_policies);
    assert!(
        provider
            .shape_obligations
            .functions
            .contains_key(&function_def)
    );
    assert_eq!(provider.shape_obligations, unit.shape_obligations);
}

#[cfg(feature = "hir-rust")]
#[test]
fn imported_tensor_policy_callback_identity_survives_fhir_round_trip() {
    use crate::codegen::{Target, generate_from_analyzed};
    use crate::driver::{Config, Session};
    use crate::locale::{KeywordSurface, latin_locale_pack};
    use std::fs;

    let temp = crate::test_support::test_temp_dir("artifact-imported-tensor-policy");
    let home = temp.path().to_path_buf();
    let package = home.join("demo");
    fs::create_dir_all(package.join("src")).expect("demo src");
    fs::write(
        package.join("faber.toml"),
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
        package.join("src/policy.fab"),
        r"@ public
class ZeroGhost {
    @ public
    fn remap(vector<int, size 1> coord) → vector<int, size 1> {
        return coord - coord
    }
}
",
    )
    .expect("policy module");

    let consumer_source = r#"+++
locale = "en"
+++

import from "demo:policy" policy

@ public
fn resolve(tf32[3] source) → f32 ⇥ string {
    return source.shift([1]).edge<policy.ZeroGhost>().materialize().get([2])
}
"#;
    let consumer_path = home.join("consumer.fab");
    fs::write(&consumer_path, consumer_source).expect("consumer module");

    let _library_home = LibraryHomeOverride::set(&home);
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_library_home(home.clone()),
    );
    let direct = crate::program::analyze_prepared_program_path(&session, &consumer_path, consumer_source)
        .expect("analyze imported custom-policy consumer");
    assert!(
        direct.provider_units.get("demo:policy").is_some(),
        "analysis must carry the imported policy provider unit"
    );

    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    let direct_output =
        generate_from_analyzed(Target::HirRust, &direct, &surface).expect("generate Rust before FHIR round-trip");
    let direct_code = output_code(&direct_output).to_owned();
    assert!(direct_code.contains("impl faber::TensorEdgePolicyProvider<f32, 1> for crate::policy::ZeroGhost"));
    assert!(direct_code.contains("crate::policy::ZeroGhost {}.remap"));

    let artifact = crate::hir::serialize::from_analyzed(&direct, "policy-round-trip", "consumer.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize imported-policy consumer");
    let loaded = load_fhir(&bytes).expect("load imported-policy consumer");
    let loaded_output =
        generate_from_analyzed(Target::HirRust, &loaded, &surface).expect("generate Rust after FHIR round-trip");

    assert_eq!(
        output_code(&loaded_output),
        direct_code,
        "FHIR round-trip must retain the imported callback identity used by the provider impl"
    );
}

/// Extract the code string from a codegen Output.
fn output_code(output: &crate::Output) -> &str {
    match output {
        crate::Output::Rust(o) => &o.code,
        crate::Output::TypeScript(o) => &o.code,
        crate::Output::Go(o) => &o.code,
        crate::Output::Swift(o) => &o.code,
        crate::Output::Python(o) => &o.code,
        crate::Output::Faber(o) => &o.code,
        _ => unreachable!("unexpected output variant in round-trip test"),
    }
}

/// Need: an `ordo` comparison orders by declared discriminant, and a
/// loaded unit must carry the order rows that say so. Without them it lowered
/// the comparison as a plain operator and gave wrong results on every target.
#[test]
fn ordo_order_rows_survive_the_fhir_round_trip() {
    use crate::codegen::{Target, generate_from_analyzed};
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    // `done` is declared after `pending` but orders before it.
    let source = concat!(
        "+++\nlocale = \"en\"\n+++\n\n",
        "enum Status {\n    pending = 10,\n    active,\n    done = -3\n}\n\n",
        "main {\n    print Status.done ≺ Status.pending\n}\n",
    );
    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);
    let targets = [
        #[cfg(feature = "hir-rust")]
        Target::HirRust,
        #[cfg(feature = "hir-ts")]
        Target::HirTypeScript,
        #[cfg(feature = "hir-go")]
        Target::HirGo,
        #[cfg(feature = "hir-swift")]
        Target::HirSwift,
        #[cfg(feature = "hir-python")]
        Target::HirPython,
    ];
    assert!(!targets.is_empty(), "at least one HIR target feature must be enabled");

    for target in targets {
        let session = Session::new(
            Config::default()
                .with_latin_locale()
                .with_dev_locale_root()
                .with_target(target),
        );
        let direct = analyze_source(&session, "ordo-order.fab", source, AnalyzeRequest::default())
            .expect("ordo comparison fixture analyzes");
        let direct_rows = &direct.tensor_edge_policies.ordo_order;
        assert_eq!(
            direct_rows.comparisons.len(),
            1,
            "analysis records the order rows of the one comparison for {target:?}"
        );

        let artifact = from_analyzed(&direct, "ordo-order", "ordo-order.fab");
        let bytes = postcard::to_allocvec(&artifact).expect("serialize ordo artifact");
        let loaded = load_fhir(&bytes).expect("ordo artifact must load");

        assert_eq!(
            &loaded.tensor_edge_policies.ordo_order, direct_rows,
            "the loaded unit must carry the direct unit's order rows for {target:?}"
        );
        let direct_output = generate_from_analyzed(target, &direct, &surface).expect("direct ordo codegen");
        let loaded_output = generate_from_analyzed(target, &loaded, &surface).expect("loaded ordo codegen");
        assert_eq!(
            output_code(&direct_output),
            output_code(&loaded_output),
            "the loaded unit must compare ordo values by declared discriminant like the direct unit for {target:?}"
        );
    }
}

#[test]
fn conversio_assign_fhir_artifact_round_trip_preserves_operator() {
    // CDA-U7 — FHIR validation walks `↤` (target/source/recovery): the
    // artifact round-trip (serialize → load_fhir, which runs referential-
    // integrity validation) must carry the conversion-directed assignment
    // through unchanged, and every claimed HIR backend must re-emit it.
    let source = "incipit {\n  varia numerus count ← 0\n  count ↤ \"42\" ⊥ 0\n  nota count\n}\n";

    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let unit = crate::driver::analyze_source(&session, "conversio-assign.faber", source, AnalyzeRequest::default())
        .expect("analyze ↤ fixture");

    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);

    let artifact = build_artifact_from_unit(&unit);
    let bytes = postcard::to_allocvec(&artifact).expect("serialize ↤ artifact");
    let loaded = load_fhir(&bytes).expect("load_fhir validates and loads the ↤ artifact");

    use crate::codegen::{Target, generate_from_analyzed};
    let faber =
        generate_from_analyzed(Target::HirFaber, &loaded, &surface).expect("faber re-emit from the loaded ↤ artifact");
    let crate::Output::Faber(faber) = faber else {
        panic!("expected faber output");
    };
    assert!(
        faber.code.contains("count ↤ \"42\" ⊥ 0"),
        "↤ must survive the FHIR round-trip verbatim:\n{}",
        faber.code
    );
}

// EOT-1 baseline: the current artifact loader remains a clean round-trip
// boundary while selective imports and transfer are still absent from the
// source/HIR contract. The positive source fixture is owned by the later EOT
// family; the loader baseline must not change legacy artifact behavior.
#[test]
fn eot_artifact_legacy_empty_surface_roundtrips_unchanged() {
    let artifact = minimal_test_artifact();
    let bytes = serialize_artifact(&artifact);
    let loaded = load_fhir(&bytes).expect("legacy artifact baseline must load");
    assert_eq!(loaded.hir.items.len(), artifact.hir.items.len());
    assert!(loaded.hir.entry.is_none());
}

// ── SCR-04 capability provenance survives the FHIR boundary ──────────────

#[test]
fn capability_provenance_survives_serialize_and_load() {
    use crate::driver::{AnalyzeRequest, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let session = Session::new(crate::driver::Config::default().with_latin_locale());
    let unit = analyze_source(
        &session,
        "prov.fab",
        r"
functio utitur() → numerus {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    redde view.longitudo()
}
incipit { }
",
        AnalyzeRequest::default(),
    )
    .expect("analyze");

    let function_def = unit
        .hir
        .items
        .iter()
        .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .expect("function item")
        .def_id;
    let original_row = unit.provenance.get(function_def).expect("row").clone();
    assert!(!original_row.last_uses.is_empty(), "body borrow carries a last-use fact");

    // The wire carries only the reduced summary; the region CFG is absent.
    let artifact = from_analyzed(&unit, "deadbeef", "prov.fab");
    assert_eq!(artifact.capability_provenance.functions.len(), 1);
    let bytes = serialize_artifact(&artifact);
    let loaded = load_fhir(&bytes).expect("load");

    let loaded_row = loaded.provenance.get(function_def).expect("loaded row");
    assert_eq!(loaded_row, &original_row);
    assert_eq!(loaded.analysis_stamp, unit.analysis_stamp);
}

// ── GPD-5: generic descriptors and resolved binding vectors on the wire ──

#[test]
fn generic_defaults_and_resolved_calls_survive_fhir_round_trip() {
    let source = r#"functio pick<T = textus, magnitudo D = 64>(T x) → T {
    redde x
}

functio fallback<T = textus>() → textus {
    redde "salve"
}

incipit {
    sit _ a ← pick(1)
    nota fallback()
}
"#;

    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let mut unit = crate::driver::analyze_source(&session, "gpd.fab", source, AnalyzeRequest::default())
        .expect("analyze generic-default source");

    let textus = unit.types.primitive(radix_types::Primitive::Textus);
    let pick_name = unit.interner.intern("pick");
    let fallback_name = unit.interner.intern("fallback");

    // Descriptor facts on the analyzed unit before the wire.
    let pick_item = unit
        .hir
        .items
        .iter()
        .find(|item| matches!(&item.kind, radix_hir::HirItemKind::Function(f) if f.name == pick_name))
        .expect("pick item");
    let radix_hir::HirItemKind::Function(pick) = &pick_item.kind else {
        unreachable!()
    };
    assert_eq!(pick.type_params.len(), 2);
    assert!(
        matches!(
            &pick.type_params[0].default,
            Some(radix_types::SemanticGenericParamDefault::Typus(ty)) if *ty == textus
        ),
        "pick T must carry the textus default"
    );
    assert!(
        matches!(
            &pick.type_params[1].default,
            Some(radix_types::SemanticGenericParamDefault::Magnitudo(64))
        ),
        "pick D must carry the 64 default"
    );

    // The wire carries the descriptors and the binder's resolved vectors.
    let artifact = crate::hir::serialize::from_analyzed(&unit, "deadbeef", "gpd.fab");
    assert!(
        !artifact.resolved_generic_calls.is_empty(),
        "artifact must carry resolved generic call rows"
    );
    let bytes = serialize_artifact(&artifact);
    let loaded = load_fhir(&bytes).expect("load");

    // Decoded descriptors keep the exact declared defaults.
    let loaded_pick = loaded
        .hir
        .items
        .iter()
        .find(|item| matches!(&item.kind, radix_hir::HirItemKind::Function(f) if f.name == pick_name))
        .expect("loaded pick item");
    let radix_hir::HirItemKind::Function(loaded_pick_fn) = &loaded_pick.kind else {
        unreachable!()
    };
    assert_eq!(loaded_pick_fn.type_params, pick.type_params);

    // Decoded TypeTable restores the complete resolved vectors, provenance
    // included: pick's T is inferred, D is defaulted; fallback's T is defaulted.
    assert_eq!(
        loaded.types.resolved_generic_calls, unit.types.resolved_generic_calls,
        "decoded unit must restore the original resolved vectors"
    );
    let resolve_name = |unit: &AnalyzedModule, sym| unit.interner.resolve(sym).to_string();
    let pick_row = unit
        .types
        .resolved_generic_calls
        .iter()
        .find(|row| row.bindings.len() == 2)
        .expect("pick row with two bindings");
    let t = pick_row
        .bindings
        .iter()
        .find(|b| resolve_name(&unit, b.name) == "T")
        .expect("T binding");
    assert_eq!(t.source, radix_types::GenericBindingSource::Inferred);
    assert!(
        matches!(t.witness, radix_types::GenericWitness::Typus(ty) if ty != textus),
        "inferred T must witness the argument type, not the default"
    );
    let d = pick_row
        .bindings
        .iter()
        .find(|b| resolve_name(&unit, b.name) == "D")
        .expect("D binding");
    assert_eq!(d.source, radix_types::GenericBindingSource::Defaulted);
    assert!(matches!(
        d.witness,
        radix_types::GenericWitness::Magnitudo(id)
            if matches!(unit.types.get_index(id), radix_types::IndexExpr::Literal(64))
    ));

    let fallback_row = unit
        .types
        .resolved_generic_calls
        .iter()
        .find(|row| row.bindings.len() == 1)
        .expect("fallback row with one binding");
    assert_eq!(fallback_row.bindings[0].source, radix_types::GenericBindingSource::Defaulted);
    assert!(
        matches!(
            fallback_row.bindings[0].witness,
            radix_types::GenericWitness::Typus(ty) if ty == textus
        ),
        "fallback T must witness textus"
    );
    assert!(
        loaded
            .types
            .resolved_generic_calls
            .iter()
            .any(|row| row.call_span == fallback_row.call_span),
        "decoded rows keep the call-span key"
    );
    let _ = fallback_name;
}

#[test]
fn air_lane_metadata_survives_fhir_round_trip() {
    let source = r#"@ radix lane "air"
functio lane_probe() → numerus {
    redde 1
}

incipit {
    nota lane_probe()
}
"#;
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_latin_locale()
            .with_target(crate::codegen::Target::MirWasm),
    );
    let unit = crate::driver::analyze_source(&session, "lane.fab", source, AnalyzeRequest::default())
        .expect("analyze lane source");
    let lane_def = unit
        .radix_lanes
        .iter()
        .find_map(|(def_id, lane)| (lane == crate::semantic::RadixLane::Air).then_some(def_id))
        .expect("AIR lane metadata");

    let artifact = crate::hir::serialize::from_analyzed(&unit, "deadbeef", "lane.fab");
    let loaded = load_fhir(&serialize_artifact(&artifact)).expect("load lane artifact");
    assert_eq!(loaded.radix_lanes.lane_for(lane_def), Some(crate::semantic::RadixLane::Air));
}

#[test]
fn kernel_requirements_and_symbolic_extents_survive_fhir_round_trip() {
    let source = r"
@ nucleum
@ publica
functio copy_coordinate<magnitudo N>(
    tensor<f32, [N]> input,
    varia tensor<u32, [N]> output
) → vacuum {
    itera ex input apud [i] sit value {
        output[i] ← i
    }
}

@ nucleum
@ publica
functio copy_four(tensor<f32, [4]> input, varia tensor<u32, [4]> output) → vacuum {
    copy_coordinate(input, varia output)
}
";
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let unit = crate::driver::analyze_source(&session, "kernel-requirements.fab", source, AnalyzeRequest::default())
        .expect("analyze kernel requirement fixture");
    let original = unit.kernel_requirements.clone();
    let row = original
        .kernels
        .iter()
        .find(|kernel| kernel.name == "copy_coordinate")
        .expect("kernel report");
    assert!(
        row.target_reports
            .iter()
            .any(|target| !target.extent_conditions.is_empty()),
        "symbolic index arithmetic must retain its launch condition"
    );

    let artifact = crate::hir::serialize::from_analyzed(&unit, "kernel-requirements", "kernel-requirements.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize kernel requirement artifact");
    let loaded = load_fhir(&bytes).expect("validate and load kernel requirements");
    assert_eq!(loaded.kernel_requirements, original);
}

#[test]
fn extent_condition_parameter_term_round_trips() {
    use crate::semantic::passes::kernel_requirements::KernelRangeExpr;
    use radix_types::DefId;

    // The same launch-condition fixture as the neighboring symbolic-extent
    // round trip; the condition's bounds are replaced with a parameter term
    // so the wire carries a `DefId` where only constants and shapes rode
    // before.
    let source = r"
@ nucleum
@ publica
functio copy_coordinate<magnitudo N>(
    tensor<f32, [N]> input,
    varia tensor<u32, [N]> output
) → vacuum {
    itera ex input apud [i] sit value {
        output[i] ← i
    }
}

@ nucleum
@ publica
functio copy_four(tensor<f32, [4]> input, varia tensor<u32, [4]> output) → vacuum {
    copy_coordinate(input, varia output)
}
";
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let mut unit = crate::driver::analyze_source(&session, "kernel-parameter.fab", source, AnalyzeRequest::default())
        .expect("analyze kernel parameter fixture");
    let position = unit.kernel_requirements.kernels[0]
        .target_reports
        .iter()
        .position(|target| !target.extent_conditions.is_empty())
        .expect("kernel reports a launch condition");
    let mut condition = unit.kernel_requirements.kernels[0].target_reports[position].extent_conditions[0].clone();
    condition.range.minimum = KernelRangeExpr::Parameter(DefId(7));
    condition.range.maximum = KernelRangeExpr::Parameter(DefId(7));
    unit.kernel_requirements.kernels[0].target_reports[position].extent_conditions = vec![condition];
    let original = unit.kernel_requirements.clone();

    let artifact = crate::hir::serialize::from_analyzed(&unit, "kernel-parameter", "kernel-parameter.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize kernel parameter artifact");
    let loaded = load_fhir(&bytes).expect("validate and load kernel parameter artifact");
    assert_eq!(loaded.kernel_requirements, original);
}

#[test]
fn kernel_requirements_wire_omits_rows_without_a_device_or_numeric_encoding() {
    let source = r"
@ nucleum
@ publica
functio copy_four(tensor<f32, [4]> input, varia tensor<u32, [4]> output) → vacuum {
    itera ex input apud [i] sit value {
        output[i] ← i
    }
}
";
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let mut unit = crate::driver::analyze_source(&session, "kernel-wire.fab", source, AnalyzeRequest::default())
        .expect("analyze kernel wire fixture");
    let baseline = crate::hir::serialize::from_analyzed(&unit, "kernel-wire", "kernel-wire.fab").kernel_requirements;

    let kernel = &mut unit.kernel_requirements.kernels[0];
    let device_report = kernel.target_reports[0].clone();
    let width = device_report
        .widths
        .first()
        .cloned()
        .expect("kernel reports a width");
    let mut host_report = device_report.clone();
    host_report.target = crate::codegen::Target::HirRust;
    kernel.target_reports.push(host_report);
    let mut text_width = width;
    text_width.primitive = crate::semantic::Primitive::Textus;
    kernel.target_reports[0].widths.push(text_width);

    let wire = crate::hir::serialize::from_analyzed(&unit, "kernel-wire", "kernel-wire.fab").kernel_requirements;
    assert_eq!(wire, baseline);
}

#[test]
fn zz_debug_presentation() {
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let source = std::fs::read_to_string(root.join("ad/serve-typed.fab")).unwrap();
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(crate::codegen::Target::HirRust),
    );
    let unit = analyze_source(&session, "serve-typed.fab", &source, AnalyzeRequest::default()).expect("analyze");
    let mut artifact = from_analyzed(&unit, "serve-typed.fab", "serve-typed.fab");
    let path = vec![radix_lexer::Symbol(44)];
    // Variant 1: Named head, keep args.
    let mut v1 = artifact.clone();
    for a in &mut v1.presentation.authored_types {
        if matches!(a.site.owner, radix_hir::HirOwnerRef::Def(d) if d.0 == 1000004 || d.0 == 1000005) {
            a.form.head = radix_hir::HirTypeFormHead::Named { def_id: radix_types::DefId(29), path: path.clone() };
        }
    }
    println!(
        "v1 keep-args named: {:?}",
        load_fhir(&postcard::to_allocvec(&v1).unwrap())
            .map(|_| ())
            .map_err(|e| e.to_string())
    );
    // Variant 2: Named head, drop args.
    let mut v2 = artifact.clone();
    for a in &mut v2.presentation.authored_types {
        if matches!(a.site.owner, radix_hir::HirOwnerRef::Def(d) if d.0 == 1000004 || d.0 == 1000005) {
            a.form.head = radix_hir::HirTypeFormHead::Named { def_id: radix_types::DefId(29), path: path.clone() };
            a.form.args.clear();
        }
    }
    println!(
        "v2 drop-args named: {:?}",
        load_fhir(&postcard::to_allocvec(&v2).unwrap())
            .map(|_| ())
            .map_err(|e| e.to_string())
    );
    // Variant 3: Semantic head, drop args.
    let mut v3 = artifact.clone();
    for a in &mut v3.presentation.authored_types {
        if matches!(a.site.owner, radix_hir::HirOwnerRef::Def(d) if d.0 == 1000004 || d.0 == 1000005) {
            a.form.args.clear();
        }
    }
    println!(
        "v3 drop-args semantic: {:?}",
        load_fhir(&postcard::to_allocvec(&v3).unwrap())
            .map(|_| ())
            .map_err(|e| e.to_string())
    );
    let _ = &mut artifact;
}

#[test]
fn size_generic_genus_applied_to_size_survives_round_trip() {
    // A size-generic genus applied to a concrete size (`Box<3>`) used to trip
    // the FHIR unit gate twice. Lowering recorded no authored child form for
    // the magnitude argument, so the witness carried one `AppliedArg::Size`
    // while the authored form carried none and `validate_named_form` failed
    // with `named_argument_arity`; once the producer records the size child,
    // the validator's unconditional `AppliedArg::Size` rejection reported
    // `named_size_argument`. Both slugs must round-trip green.
    use crate::codegen::Target;
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let source = concat!(
        "+++\nlocale = \"en\"\n+++\n\n",
        "@ public\n",
        "class Box<size N> {\n",
        "    mut vector<f32, size N> minima\n",
        "    mut vector<f32, size N> maxima\n",
        "}\n",
        "@ public\n",
        "fn box_inflated(Box<3> box, f32 amount) \u{2192} Box<3> {\n",
        "    let vector<f32, size 3> lo \u{2190} box.minima\n",
        "    let vector<f32, size 3> hi \u{2190} box.maxima\n",
        "    let Box<3> expanded \u{2190} Box {minima = lo, maxima = hi}\n",
        "    return expanded\n",
        "}\n",
    );
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirRust),
    );
    let original = analyze_source(&session, "size-generic.fab", source, AnalyzeRequest::default())
        .expect("size-generic fixture analyzes");
    let artifact = from_analyzed(&original, "size-generic", "size-generic.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize size-generic artifact");
    load_fhir(&bytes).expect("size-generic artifact must load");
}

/// Analyze an English-locale `source` for `target`, serialize it to FHIR, and
/// return the load result.
fn load_en_unit_for(source: &str, target: crate::codegen::Target) -> Result<AnalyzedModule, HirArtifactError> {
    use crate::driver::{Config, Session, analyze_source};
    use crate::hir::serialize::from_analyzed;

    let session = Session::new(Config::default().with_dev_locale_root().with_target(target));
    let original =
        analyze_source(&session, "air.fab", source, AnalyzeRequest::default()).expect("AIR fixture analyzes");
    let artifact = from_analyzed(&original, "air", "air.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize AIR artifact");
    load_fhir(&bytes)
}

#[test]
fn air_unrolled_hir_ids_round_trip_fhir() {
    // An AIR-lane `for from` over a rank-1 tensor unrolls into straight-line
    // lanes on every non-Faber target; the unrolled unit must serialize with
    // every HirId it carries naming a node of the finalized HIR.
    use crate::codegen::Target;

    let source = r#"@ radix lane "air"
fn tensor_sum(tf32[4] x) → f32 {
    mut f32 total ← 0.0
    for from x let value {
        total ← total + value
    }
    return total
}
"#;
    for target in [
        Target::HirPython,
        Target::HirHaskell,
        Target::MirWasm,
        Target::MirLlvm,
        Target::MirClif,
        Target::MirMetal,
        Target::MirWgsl,
        Target::MirSexp,
    ] {
        load_en_unit_for(source, target)
            .unwrap_or_else(|err| panic!("{target:?}: AIR-unrolled unit must load: {err:?}"));
    }
}
