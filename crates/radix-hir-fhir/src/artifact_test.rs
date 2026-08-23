use super::*;
use crate::test_support::stub_artifact;

#[test]
fn hir_artifact_round_trip() {
    let original = stub_artifact();
    let bytes = postcard::to_allocvec(&original).expect("serialize HirArtifact");
    let deserialized: HirArtifact = postcard::from_bytes(&bytes).expect("deserialize HirArtifact");
    assert_eq!(original.schema_version, SCHEMA_VERSION);
    assert_eq!(original, deserialized);
}

// ── EOT-2W selective-import wire contract ────────────────────────────────

/// Artifact whose HIR carries one selective import (`importa ex "…" fixum …`).
///
/// The `HirImport.selective` marker is additive with `#[serde(default)]`
/// (EOT-2M), so the wire carries it without a schema ratchet: v4 artifacts
/// written before the field decode with `selective == false` (legacy
/// namespace shape) and v4 artifacts written after carry it explicitly.
fn selective_import_artifact() -> HirArtifact {
    use radix_hir::{HirImport, HirImportItem};
    use radix_types::USER_DEF_ID_BASE;

    let mut artifact = stub_artifact();
    artifact.interner = vec![
        "main".to_string(),
        "test".to_string(),
        "norma:consolum".to_string(),
        "dic".to_string(),
        "output".to_string(),
        "funde".to_string(),
    ];
    let import = radix_hir::HirImport {
        path: Symbol(2),
        visibility: radix_syntax::Visibility::Privata,
        items: vec![
            HirImportItem {
                def_id: DefId(USER_DEF_ID_BASE + 40),
                name: Symbol(3),
                alias: Some(Symbol(4)),
            },
            HirImportItem {
                def_id: DefId(USER_DEF_ID_BASE + 41),
                name: Symbol(5),
                alias: None,
            },
        ],
        selective: true,
    };
    artifact.hir.items.push(radix_hir::HirItem {
        id: radix_hir::HirId(1),
        def_id: DefId(USER_DEF_ID_BASE + 42),
        kind: radix_hir::HirItemKind::Import(import),
        span: radix_lexer::Span::default(),
    });
    artifact
}

#[test]
fn selective_import_round_trips_through_postcard() {
    let original = selective_import_artifact();
    let bytes = postcard::to_allocvec(&original).expect("serialize selective HirArtifact");
    let deserialized: HirArtifact = postcard::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, deserialized);
    let radix_hir::HirItemKind::Import(import) = &deserialized.hir.items[1].kind else {
        panic!("expected import item");
    };
    assert!(import.selective, "selective marker must survive the wire");
    assert_eq!(
        import.items.len(),
        2,
        "ordered member list must survive the wire"
    );
    assert_eq!(import.items[0].name, Symbol(3));
    assert_eq!(import.items[0].alias, Some(Symbol(4)));
    assert_eq!(import.items[1].alias, None);
}

#[test]
fn selective_import_decode_passes_referential_integrity() {
    // decode() runs the schema-version gate plus validate_referential_integrity;
    // the selective binding def ids are registered from the import items, so a
    // valid selective artifact must load rather than fail with DanglingIndex.
    let artifact = selective_import_artifact();
    let bytes = crate::decode::encode(&artifact).expect("encode");
    let decoded = crate::decode::decode(&bytes).expect("selective artifact must validate");
    let radix_hir::HirItemKind::Import(import) = &decoded.hir.items[1].kind else {
        panic!("expected import item");
    };
    assert!(import.selective);
}

#[test]
fn selective_import_field_keeps_schema_v4_no_ratchet() {
    // Schema-version receipt (fail-closed policy): `HirImport.selective` is an
    // additive `#[serde(default)]` field, so the version stays 4 — no wire
    // break, no compatibility decoder. Consumers of the bool today:
    // radix-hir-faber decl emission (locale-preserving `fixum` list),
    // radix-hir-rust namespace/use analysis (locals, not namespaces),
    // radix-hir-ts decl emission (value-import form), radix-hir-go
    // fail-closed, and this crate's round-trip/validate proof.
    assert_eq!(SCHEMA_VERSION, 4);
}
