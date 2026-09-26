use super::*;
use crate::test_support::{selective_import_artifact, stub_artifact, transfer_artifact};

#[test]
fn hir_artifact_round_trip() {
    let original = stub_artifact();
    let bytes = postcard::to_allocvec(&original).expect("serialize HirArtifact");
    let deserialized: HirArtifact = postcard::from_bytes(&bytes).expect("deserialize HirArtifact");
    assert_eq!(original.schema_version, SCHEMA_VERSION);
    assert_eq!(original, deserialized);
}

/// IL-3 FHIR inventory row: this crate owns binary postcard/MessagePack
/// artifacts, not a JSON emitter. The binary artifact path preserves an IEEE
/// non-finite carrier; any JSON-facing boundary must reject it explicitly
/// rather than inherit a `null`/`0` coercion.
#[test]
fn non_finite_float_fhir_binary_posture_is_explicit() {
    let mut original = stub_artifact();
    original.hir.entry = Some(radix_hir::HirBlock::simple(
        vec![radix_hir::HirStatement {
            id: radix_hir::HirId(1),
            kind: radix_hir::HirStatementKind::Expr(radix_hir::HirExpression {
                id: radix_hir::HirId(2),
                kind: radix_hir::HirExpressionKind::Literal(radix_hir::HirLiteral::Float(
                    f64::INFINITY,
                )),
                ty: None,
                span: radix_lexer::Span::default(),
            }),
            span: radix_lexer::Span::default(),
        }],
        None,
        radix_lexer::Span::default(),
    ));

    let bytes = postcard::to_allocvec(&original).expect("serialize non-finite binary artifact");
    let decoded: HirArtifact = postcard::from_bytes(&bytes).expect("deserialize binary artifact");
    let Some(radix_hir::HirBlock { statements, .. }) = decoded.hir.entry else {
        panic!("non-finite inventory fixture must carry an entry");
    };
    let radix_hir::HirStatementKind::Expr(expression) = &statements[0].kind else {
        panic!("non-finite inventory fixture must carry an expression");
    };
    let radix_hir::HirExpressionKind::Literal(radix_hir::HirLiteral::Float(value)) =
        expression.kind
    else {
        panic!("non-finite inventory fixture must carry a float literal");
    };
    assert!(value.is_infinite(), "binary FHIR must preserve infinity");
}

// ── EOT-2W selective-import wire contract ────────────────────────────────

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
fn selective_import_field_kept_schema_v4_wire_breaks_ratchet_version() {
    // Schema-version receipt (fail-closed policy): `HirImport.selective` is an
    // additive `#[serde(default)]` field, so the version stayed 4 at EOT-2W —
    // no wire break, no compatibility decoder. Consumers of the bool today:
    // radix-hir-faber decl emission (locale-preserving `fixum` list),
    // radix-hir-rust namespace/use analysis (locals, not namespaces),
    // radix-hir-ts decl emission (value-import form), radix-hir-go
    // fail-closed, and this crate's round-trip/validate proof.
    // EOT-3W ratchets to 5 (Transfer variant shifts serde ordinals), E3
    // ratchets to 6 for resolver conversion-arm rows, and CTR-05 ratchets to
    // 7 (registry rows replace destination-owned arm rows; the enum's
    // retired conversion-arm payload leaves the serde'd HIR payload), and
    // SCR-04 ratchets to 8 (capability-provenance summaries join the wire),
    // and ATF-5 ratchets to 9 because authored type forms join presentation,
    // and GPD-5 ratchets to 10 because resolved generic binding vectors join
    // the wire. Schema 14 (provider-import-parity repair) carries analyzed
    // provider carriage per unit — a serialized addition, so the wire breaks
    // and there is no compatibility decoder for schema-v13 artifacts.
    // Schema 15 adds the authored `iuncta` element label on
    // `HirTypeForm` and `HirPresentation::tuple_label_accesses` so authored
    // named-tuple labels and by-label reads survive re-emission — both are
    // serialized presentation additions, so schema-v14 artifacts fail closed.
    // Schema 16 (D4.1) removes the dead `HirCasuArm::guard` field — a
    // serialized HIR removal, so schema-v15 artifacts fail closed.
    // Schema 17 (D8.1) drops `HirStruct.extends` and `HirStruct.is_abstract`.
    assert_eq!(SCHEMA_VERSION, 17);
}

// ── EOT-3W transfer wire contract ────────────────────────────────────────

#[test]
fn transfer_round_trips_through_postcard() {
    // The Transfer variant rides the direct-serde HIR payload; its insertion
    // before `Error` shifts later serde ordinals, which is exactly why
    // SCHEMA_VERSION ratcheted to 5.
    let original = transfer_artifact();
    let bytes = postcard::to_allocvec(&original).expect("serialize transfer HirArtifact");
    let deserialized: HirArtifact = postcard::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, deserialized);
    let radix_hir::HirItemKind::Function(function) = &deserialized.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = function.body.as_ref().expect("body");
    let radix_hir::HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected expr statement");
    };
    let radix_hir::HirExpressionKind::Transfer { sink, payloads } = &expr.kind else {
        panic!("expected transfer expression");
    };
    assert!(
        matches!(sink.kind, radix_hir::HirExpressionKind::Path(DefId(0))),
        "sink must survive the wire"
    );
    assert_eq!(
        payloads.len(),
        2,
        "ordered payload list must survive the wire"
    );
}

#[test]
fn transfer_decode_passes_referential_integrity() {
    // decode() runs the schema-version gate plus validate_referential_integrity:
    // the sink/payload expressions must be collected (ids + defs) so a valid
    // transfer artifact loads rather than failing with DanglingIndex.
    let artifact = transfer_artifact();
    let bytes = crate::decode::encode(&artifact).expect("encode");
    let decoded = crate::decode::decode(&bytes).expect("transfer artifact must validate");
    let radix_hir::HirItemKind::Function(function) = &decoded.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = function.body.as_ref().expect("body");
    assert!(
        matches!(
            &body.statements[0].kind,
            radix_hir::HirStatementKind::Expr(radix_hir::HirExpression {
                kind: radix_hir::HirExpressionKind::Transfer { .. },
                ..
            })
        ),
        "transfer must survive the full decode/validate path"
    );
}

// ── SCR-04 capability provenance round trips ─────────────────────────────

#[test]
fn capability_provenance_round_trips_through_postcard() {
    use crate::artifact::{
        FunctionProvenanceTableWire, FunctionProvenanceWire, LastUseWire, RegionRelationshipWire,
        ReturnedBorrowWire,
    };

    let mut original = stub_artifact();
    original.capability_provenance = FunctionProvenanceTableWire {
        functions: vec![FunctionProvenanceWire {
            function: DefId(0),
            returned_borrow: ReturnedBorrowWire::Parameter(DefId(0)),
            required_regions: vec![RegionRelationshipWire {
                from: DefId(0),
                to: DefId(0),
            }],
            last_uses: vec![LastUseWire {
                borrower: DefId(0),
                site: radix_hir::HirId(0),
            }],
        }],
    };
    let bytes = postcard::to_allocvec(&original).expect("serialize provenance-carrying artifact");
    let deserialized: HirArtifact = postcard::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, deserialized);
}
