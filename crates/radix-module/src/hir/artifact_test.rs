use super::*;
use crate::driver::AnalyzeRequest;
use radix_hir_fhir::HirArtifact;
use std::error::Error;

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
    use radix_hir_fhir::{FunctionFactTableWire, ResolverSnapshot, SourceIdentity, SCHEMA_VERSION};

    let types = TypeTable::new();
    let item = HirItem {
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
            annotations: Vec::new(),
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
        source_identity: SourceIdentity { content_hash: "abc123".to_string(), relative_path: "test.faber".to_string() },
        hir,
        presentation: empty_presentation(),
        cli_program: None,
        types: types.snapshot(),
        interner: vec!["main".to_string(), "test".to_string()],
        resolver: ResolverSnapshot {
            namespace_exports: Vec::new(),
            file_interfaces: Vec::new(),
            used_namespaces: Vec::new(),
            imported_nominal_types: Vec::new(),
            ambiguous_imported_nominal_types: Vec::new(),
        },
        libraries: LibraryRegistry::default(),
        function_facts: FunctionFactTableWire { entries: Vec::new(), entry: None },
        resolved_uses: Vec::new(),
        analysis_stamp: 1,
        gpu_builtins: Vec::new(),
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
        ResolvedUseWire { key: ResolvedUseKeyWire::Local(def_id), kind: ResolvedUseKindWire::Call, enclosing: None },
        ResolvedUseWire {
            key: ResolvedUseKeyWire::Portable("pkg::func".to_string()),
            kind: ResolvedUseKindWire::ImportBinding,
            enclosing: None,
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
        annotations: Vec::new(),
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
            annotations: Vec::new(),
        })),
        span: radix_lexer::Span::default(),
    };
    let item1 = radix_hir::HirItem {
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
            annotations: Vec::new(),
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
            annotations: Vec::new(),
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
    use radix_hir_fhir::{SourceIdentity, SCHEMA_VERSION};
    HirArtifact {
        schema_version: SCHEMA_VERSION,
        source_identity: SourceIdentity {
            content_hash: "test_hash".to_string(),
            relative_path: "test.faber".to_string(),
        },
        hir: unit.hir.clone(),
        presentation: unit.presentation.clone(),
        cli_program: None,
        types: unit.types.snapshot(),
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
    }
}

// ── U2.4: Full round-trip acceptance (5 HIR-backed targets) ────────

#[test]
fn hir_artifact_round_trip_acceptance() {
    // Source matching the hello-world fixture.
    let source = "incipit {\n    nota \"Salve, Munde!\"\n}\n";

    let session = crate::driver::Session::new(crate::driver::Config::default());
    let unit = crate::driver::analyze_source(&session, "test.faber", source, AnalyzeRequest::default())
        .expect("analyze hello-world fixture");

    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);

    use crate::codegen::{generate_from_analyzed, Target};

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

/// Extract the code string from a codegen Output.
fn output_code(output: &crate::Output) -> &str {
    match output {
        crate::Output::Rust(o) => &o.code,
        crate::Output::TypeScript(o) => &o.code,
        crate::Output::Go(o) => &o.code,
        crate::Output::Swift(o) => &o.code,
        crate::Output::Faber(o) => &o.code,
        _ => unreachable!("unexpected output variant in round-trip test"),
    }
}

#[test]
fn conversio_assign_fhir_artifact_round_trip_preserves_operator() {
    // CDA-U7 — FHIR validation walks `↤` (target/source/recovery): the
    // artifact round-trip (serialize → load_fhir, which runs referential-
    // integrity validation) must carry the conversion-directed assignment
    // through unchanged, and every claimed HIR backend must re-emit it.
    let source = "incipit {\n  varia numerus count ← 0\n  count ↤ \"42\" ⇥ 0\n  nota count\n}\n";

    let session = crate::driver::Session::new(crate::driver::Config::default());
    let unit = crate::driver::analyze_source(&session, "conversio-assign.faber", source, AnalyzeRequest::default())
        .expect("analyze ↤ fixture");

    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);

    let artifact = build_artifact_from_unit(&unit);
    let bytes = postcard::to_allocvec(&artifact).expect("serialize ↤ artifact");
    let loaded = load_fhir(&bytes).expect("load_fhir validates and loads the ↤ artifact");

    use crate::codegen::{generate_from_analyzed, Target};
    let faber =
        generate_from_analyzed(Target::HirFaber, &loaded, &surface).expect("faber re-emit from the loaded ↤ artifact");
    let crate::Output::Faber(faber) = faber else {
        panic!("expected faber output");
    };
    assert!(
        faber.code.contains("count ↤ \"42\" ⇥ 0"),
        "↤ must survive the FHIR round-trip verbatim:\n{}",
        faber.code
    );
}
