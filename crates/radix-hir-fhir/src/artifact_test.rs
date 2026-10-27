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

#[test]
fn reduction_node_round_trips_through_fhir_validation() {
    use radix_hir::{
        HirBlock, HirExpression, HirExpressionKind, HirId, HirLiteral, HirReductionOp,
        HirStatement, HirStatementKind,
    };
    use radix_types::{Primitive, TypeTable};

    let mut artifact = stub_artifact();
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    artifact.types = types.snapshot();

    let expression = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Reduction {
            op: HirReductionOp::Sum,
            source: Box::new(HirExpression {
                id: HirId(3),
                kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                ty: Some(numerus),
                span: Span::default(),
            }),
            coordinates: Vec::new(),
            group: None,
            filum: None,
            binding: None,
            body: None,
            identity: None,
            mutable: false,
        },
        ty: Some(numerus),
        span: Span::default(),
    };
    let body = HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Expr(expression),
            span: Span::default(),
        }],
        None,
        Span::default(),
    );
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact must contain a function");
    };
    function.body = Some(body);

    let bytes = crate::decode::encode(&artifact).expect("encode Reduction artifact");
    let decoded = crate::decode::decode(&bytes).expect("validate Reduction artifact");
    let radix_hir::HirItemKind::Function(function) = &decoded.hir.items[0].kind else {
        panic!("decoded artifact must contain a function");
    };
    let HirStatementKind::Expr(expression) = &function.body.as_ref().unwrap().statements[0].kind
    else {
        panic!("decoded body must contain an expression");
    };
    assert!(matches!(
        expression.kind,
        HirExpressionKind::Reduction {
            op: HirReductionOp::Sum,
            ..
        }
    ));
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
    let radix_hir::HirExpressionKind::CopyInto { target, value } = &expr.kind else {
        panic!("expected copy-into expression");
    };
    assert!(
        matches!(target.kind, radix_hir::HirExpressionKind::Path(DefId(0))),
        "target must survive the wire"
    );
    assert!(
        matches!(value.kind, radix_hir::HirExpressionKind::Literal(_)),
        "value must survive the wire"
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
                kind: radix_hir::HirExpressionKind::CopyInto { .. },
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
