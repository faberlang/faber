use super::*;
use crate::test_support::stub_artifact;

#[test]
fn encode_decode_round_trip_preserves_artifact() {
    let original = stub_artifact();
    let bytes = encode(&original).expect("encode artifact");
    let decoded = decode(&bytes).expect("decode artifact");
    assert_eq!(original, decoded);
}

#[test]
fn encode_decode_round_trip_preserves_await_assignment_presentation() {
    use radix_hir::{HirBlock, HirId, HirItemKind, HirLocal, HirStatement, HirStatementKind};
    use radix_lexer::{Span, Symbol};
    use radix_types::DefId;

    let mut artifact = stub_artifact();
    let local_def = DefId(1);
    let HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub item must be a function");
    };
    function.body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(HirLocal {
                def_id: local_def,
                name: Symbol(1),
                ty: None,
                init: None,
                mutable: false,
                is_await: true,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    artifact
        .presentation
        .await_assignment_bindings
        .push(local_def);

    let bytes = encode(&artifact).expect("encode artifact");
    let decoded = decode(&bytes).expect("decode artifact");
    assert_eq!(
        decoded.presentation.await_assignment_bindings,
        vec![local_def]
    );
}

#[test]
fn decode_rejects_garbage_bytes_fail_closed() {
    let result = decode(b"\x00\x01\x02\xff\xfe\xfd");
    assert!(result.is_err(), "garbage bytes must fail closed");
}

#[test]
fn decode_rejects_truncated_payload() {
    let bytes = encode(&stub_artifact()).expect("encode artifact");
    let truncated = &bytes[..bytes.len() / 2];
    assert!(
        matches!(decode(truncated), Err(HirArtifactError::TruncatedArtifact)),
        "mid-payload truncation must be classified as Truncated"
    );
}

#[test]
fn decode_rejects_unsupported_schema_version() {
    let mut artifact = stub_artifact();
    artifact.schema_version = 99;
    let bytes = encode(&artifact).expect("encode artifact");
    match decode(&bytes) {
        Err(HirArtifactError::UnsupportedSchemaVersion {
            found: 99,
            expected: SCHEMA_VERSION,
        }) => {}
        other => panic!("expected UnsupportedSchemaVersion(99), got {other:?}"),
    }
}

#[test]
fn decode_rejects_schema_v8_after_authored_types_wire_break() {
    let mut artifact = stub_artifact();
    artifact.schema_version = 8;
    let bytes = encode(&artifact).expect("encode schema-v8 artifact");
    assert!(matches!(
        decode(&bytes),
        Err(HirArtifactError::UnsupportedSchemaVersion {
            found: 8,
            expected: SCHEMA_VERSION,
        })
    ));
}

#[test]
fn decode_rejects_schema_v11_after_await_assignment_presentation_break() {
    let mut artifact = stub_artifact();
    artifact.schema_version = 11;
    let bytes = encode(&artifact).expect("encode schema-v11 artifact");
    assert!(matches!(
        decode(&bytes),
        Err(HirArtifactError::UnsupportedSchemaVersion {
            found: 11,
            expected: SCHEMA_VERSION,
        })
    ));
}

#[test]
fn decode_accepts_imported_nominal_def_living_in_type_table() {
    // Regression (faber run --compile on triga exempla, 2026-08-03):
    // the resolver allocates imported nominal types as fresh unit-local
    // DefIds and interns `Type::Struct(def_id)` into the type arena; the
    // definition site lives only in the type table on the wire (the
    // resolver's runtime `symbols` map is excluded, CTM E.2). A resolver
    // file-interface `Struct` export referencing such a def must load,
    // not fail with a false `DanglingIndex`.
    use crate::artifact::{
        FileExportSnapshot, FileInterfaceSnapshot, NominalKind, ResolverSnapshot,
    };
    use radix_lexer::Symbol;
    use radix_types::{DefId, Type, TypeTable, USER_DEF_ID_BASE};

    let mut artifact = stub_artifact();
    let imported_def = DefId(USER_DEF_ID_BASE + 12);
    let mut types = TypeTable::new();
    let struct_ty = types.intern(Type::Struct(imported_def));
    artifact.types = types.snapshot();
    artifact.interner = vec!["data".to_owned(), "BufferGeometry".to_owned()];
    artifact.resolver = ResolverSnapshot {
        namespace_exports: vec![(Symbol(0), vec![Symbol(1)])],
        file_interfaces: vec![(
            Symbol(0),
            FileInterfaceSnapshot {
                exports: vec![(
                    Symbol(1),
                    FileExportSnapshot::Struct {
                        is_json: true,
                        implements: Vec::new(),
                        def_id: imported_def,
                        ty: struct_ty,
                        fields: Vec::new(),
                        methods: Vec::new(),
                    },
                )],
                unavailable: Vec::new(),
            },
        )],
        used_namespaces: vec![Symbol(0)],
        imported_nominal_types: vec![((NominalKind::Struct, Symbol(1)), struct_ty)],
        ambiguous_imported_nominal_types: Vec::new(),
        conversion_rows: Vec::new(),
    };
    let bytes = encode(&artifact).expect("encode artifact");
    let decoded = decode(&bytes).expect("imported nominal def in type table must load");
    assert_eq!(
        decoded.resolver.file_interfaces,
        artifact.resolver.file_interfaces
    );
}

#[test]
fn decode_rejects_imported_method_with_dangling_signature_type() {
    use crate::artifact::{
        FileExportSnapshot, FileInterfaceSnapshot, NominalKind, ResolverSnapshot,
        StructMethodSnapshot,
    };
    use radix_lexer::Symbol;
    use radix_types::{DefId, FuncSig, Type, TypeId, TypeTable, USER_DEF_ID_BASE};

    let mut artifact = stub_artifact();
    let owner_def = DefId(USER_DEF_ID_BASE + 12);
    let callback_def = DefId(USER_DEF_ID_BASE + 13);
    let mut types = TypeTable::new();
    let owner_ty = types.intern(Type::Struct(owner_def));
    artifact.types = types.snapshot();
    artifact.interner = vec![
        "demo".to_owned(),
        "Widget".to_owned(),
        "transform".to_owned(),
    ];
    artifact.resolver = ResolverSnapshot {
        namespace_exports: vec![(Symbol(0), vec![Symbol(1)])],
        file_interfaces: vec![(
            Symbol(0),
            FileInterfaceSnapshot {
                exports: vec![(
                    Symbol(1),
                    FileExportSnapshot::Struct {
                        def_id: owner_def,
                        ty: owner_ty,
                        fields: Vec::new(),
                        methods: vec![StructMethodSnapshot {
                            def_id: callback_def,
                            name: Symbol(2),
                            surface_name: Symbol(2),
                            sig: FuncSig {
                                type_params: Vec::new(),
                                params: Vec::new(),
                                ret: TypeId(999),
                                err: None,
                                is_async: false,
                                is_generator: false,
                            },
                            visibility: radix_syntax::MemberVisibility::Publica,
                            unavailable: Vec::new(),
                        }],
                        is_json: false,
                        implements: Vec::new(),
                    },
                )],
                unavailable: Vec::new(),
            },
        )],
        used_namespaces: vec![Symbol(0)],
        imported_nominal_types: vec![((NominalKind::Struct, Symbol(1)), owner_ty)],
        ambiguous_imported_nominal_types: Vec::new(),
        conversion_rows: Vec::new(),
    };

    let bytes = encode(&artifact).expect("encode malformed imported method artifact");
    assert!(matches!(
        decode(&bytes),
        Err(HirArtifactError::DanglingIndex {
            kind: "TypeId",
            index: 999,
            ..
        })
    ));
}

#[test]
fn decode_round_trips_registry_conversion_rows() {
    // CTR-05: the artifact carries the coherence registry's ordered pairs
    // and stable ordinary-function DefIds. Builtin and user rows share the
    // one wire shape; destination-union arm rows are gone.
    use crate::artifact::{ConversionRowSnapshot, ResolverSnapshot};
    use radix_lexer::Span;
    use radix_types::{DefId, Primitive, TypeTable};

    let mut artifact = stub_artifact();
    let types = TypeTable::new();
    let source = types.primitive(Primitive::Numerus);
    let destination = types.primitive(Primitive::Fractus);
    artifact.types = types.snapshot();
    artifact.resolver = ResolverSnapshot {
        namespace_exports: Vec::new(),
        file_interfaces: Vec::new(),
        used_namespaces: Vec::new(),
        imported_nominal_types: Vec::new(),
        ambiguous_imported_nominal_types: Vec::new(),
        conversion_rows: vec![
            ConversionRowSnapshot {
                source,
                destination,
                fn_def_id: Some(DefId(0)),
                span: Span::new(3, 9),
                builtin: false,
            },
            ConversionRowSnapshot {
                source: destination,
                destination: source,
                fn_def_id: None,
                span: Span::default(),
                builtin: true,
            },
        ],
    };

    let bytes = encode(&artifact).expect("encode artifact");
    let decoded = decode(&bytes).expect("registry rows must round-trip");
    let rows = &decoded.resolver.conversion_rows;
    assert_eq!(rows.len(), 2, "both registry rows survive the boundary");
    let user = &rows[0];
    assert_eq!(user.source, source);
    assert_eq!(user.destination, destination);
    assert_eq!(user.fn_def_id, Some(DefId(0)));
    assert!(!user.builtin);
    let builtin = &rows[1];
    assert_eq!(
        (builtin.source, builtin.destination, builtin.builtin),
        (destination, source, true),
        "ordered pair identity is preserved, not collapsed"
    );
}

#[test]
fn decode_rejects_registry_row_with_dangling_fn_def() {
    // Negative control: a registry row naming a DefId with no definition
    // site anywhere in the artifact must fail closed as DanglingIndex.
    use crate::artifact::{ConversionRowSnapshot, ResolverSnapshot};
    use radix_lexer::Span;
    use radix_types::{DefId, Primitive, TypeTable, USER_DEF_ID_BASE};

    let mut artifact = stub_artifact();
    let types = TypeTable::new();
    let source = types.primitive(Primitive::Numerus);
    let destination = types.primitive(Primitive::Fractus);
    artifact.types = types.snapshot();
    let phantom = DefId(USER_DEF_ID_BASE + 12);
    artifact.resolver = ResolverSnapshot {
        namespace_exports: Vec::new(),
        file_interfaces: Vec::new(),
        used_namespaces: Vec::new(),
        imported_nominal_types: Vec::new(),
        ambiguous_imported_nominal_types: Vec::new(),
        conversion_rows: vec![ConversionRowSnapshot {
            source,
            destination,
            fn_def_id: Some(phantom),
            span: Span::default(),
            builtin: false,
        }],
    };

    let bytes = encode(&artifact).expect("encode artifact");
    match decode(&bytes) {
        Err(HirArtifactError::DanglingIndex {
            kind: "DefId",
            index,
            ..
        }) => assert_eq!(index, phantom.0),
        other => panic!("expected DanglingIndex DefId, got {other:?}"),
    }
}

#[test]
fn decode_rejects_nominal_def_with_no_def_site() {
    // Negative control: a nominal DefId that appears only in the type
    // table (Type::Struct) with no export identity anywhere — not in the
    // HIR tree, not in the library registry, not in a resolver
    // file-interface export — must still fail closed with
    // `DanglingIndex`: the def-site collection must not over-accept
    // type-table-only DefIds (U2.3, S2).
    use crate::artifact::ResolverSnapshot;
    use radix_types::{DefId, Type, TypeTable, USER_DEF_ID_BASE};

    let mut artifact = stub_artifact();
    let phantom_def = DefId(USER_DEF_ID_BASE + 12);
    let mut types = TypeTable::new();
    types.intern(Type::Struct(phantom_def));
    artifact.types = types.snapshot();
    artifact.resolver = ResolverSnapshot {
        namespace_exports: Vec::new(),
        file_interfaces: Vec::new(),
        used_namespaces: Vec::new(),
        imported_nominal_types: Vec::new(),
        ambiguous_imported_nominal_types: Vec::new(),
        conversion_rows: Vec::new(),
    };
    let bytes = encode(&artifact).expect("encode artifact");
    match decode(&bytes) {
        Err(HirArtifactError::DanglingIndex {
            kind: "DefId",
            index,
            ..
        }) => assert_eq!(index, phantom_def.0),
        other => panic!("expected DanglingIndex DefId, got {other:?}"),
    }
}

/// N7E-1: an artifact whose type table names both policy-float cells encodes,
/// decodes and passes referential-integrity validation at the current schema.
#[test]
fn policy_float_cells_round_trip_through_the_artifact_codec() {
    use radix_types::{FloatPolicy, NumericWidth, Type, TypeTable};

    let mut table = TypeTable::new();
    let trapping = table.intern(Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32));
    let saturating = table.intern(Type::PolicyFloat(
        FloatPolicy::Saturating,
        NumericWidth::F64,
    ));
    let mut artifact = stub_artifact();
    artifact.types = table.snapshot();

    let bytes = encode(&artifact).expect("encode artifact");
    let decoded = decode(&bytes).expect("decode artifact with policy-float cells");
    assert_eq!(decoded.schema_version, SCHEMA_VERSION);
    assert_eq!(artifact, decoded);

    let restored = TypeTable::from_snapshot(decoded.types).expect("restore type table");
    assert_eq!(
        restored.get(trapping),
        &Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32)
    );
    assert_eq!(
        restored.get(saturating),
        &Type::PolicyFloat(FloatPolicy::Saturating, NumericWidth::F64)
    );
}
