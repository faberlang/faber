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
fn decode_rejects_garbage_bytes_fail_closed() {
    let result = decode(b"\x00\x01\x02\xff\xfe\xfd");
    assert!(result.is_err(), "garbage bytes must fail closed");
}

#[test]
fn decode_rejects_truncated_payload() {
    let bytes = encode(&stub_artifact()).expect("encode artifact");
    let truncated = &bytes[..bytes.len() / 2];
    assert!(
        matches!(decode(truncated), Err(HirArtifactError::Truncated)),
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
                        def_id: imported_def,
                        ty: struct_ty,
                        fields: Vec::new(),
                    },
                )],
            },
        )],
        used_namespaces: vec![Symbol(0)],
        imported_nominal_types: vec![((NominalKind::Struct, Symbol(1)), struct_ty)],
        ambiguous_imported_nominal_types: Vec::new(),
    };
    let bytes = encode(&artifact).expect("encode artifact");
    let decoded = decode(&bytes).expect("imported nominal def in type table must load");
    assert_eq!(decoded.resolver.file_interfaces.len(), 1);
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
