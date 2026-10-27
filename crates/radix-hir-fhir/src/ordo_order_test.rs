//! Need: the artifact carries the resolved `ordo` order rows.
//!
//! A `≺ ≻ ≤ ≥` comparison of two `ordo` values lowers as an integer comparison
//! over the declared discriminants, keyed by the comparison's left-operand HIR
//! id. Before schema v49 the wire carried no such rows, so a loaded unit lowered
//! the comparison as a plain operator and gave wrong results on every target.

use crate::artifact::{HirArtifact, OrdoOrderComparisonWire};
use crate::decode::{decode, encode};
use crate::error::HirArtifactError;
use crate::msgpack::{decode_msgpack, encode_msgpack};
use crate::test_support::stub_artifact;
use radix_hir::{
    HirBinOp, HirBlock, HirEnum, HirExpression, HirExpressionKind, HirId, HirItem, HirItemKind,
    HirLiteral, HirStatement, HirStatementKind, HirVariant,
};
use radix_lexer::{Interner, Span};
use radix_types::{DefId, Type, TypeTable, USER_DEF_ID_BASE};

const ENUM_DEF: DefId = DefId(USER_DEF_ID_BASE);
const PENDING: DefId = DefId(USER_DEF_ID_BASE + 1);
const ACTIVE: DefId = DefId(USER_DEF_ID_BASE + 2);
const DONE: DefId = DefId(USER_DEF_ID_BASE + 3);

/// HIR id of the comparison's left operand (the order table's key).
const LHS: HirId = HirId(3);

/// The declared discriminants of `ordo Status { pending = 10, active, done = -3 }`
/// in declaration order: a later member orders before an earlier one.
fn status_rows() -> Vec<(DefId, i64)> {
    vec![(PENDING, 10), (ACTIVE, 11), (DONE, -3)]
}

fn operand(id: u32, value: u64, ty: radix_types::TypeId) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: Some(ty),
        span: Span::default(),
    }
}

/// An artifact whose HIR declares `Status` and compares two of its values with
/// `≺`, carrying the resolved order rows for that comparison.
fn ordo_comparison_artifact() -> HirArtifact {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let status = types.intern(Type::Enum(ENUM_DEF));
    let bivalens = types.primitive(radix_types::Primitive::Bivalens);

    let hir_enum = HirEnum {
        name: interner.intern("Status"),
        type_params: Vec::new(),
        commune_fields: Vec::new(),
        variants: [("pending", PENDING), ("active", ACTIVE), ("done", DONE)]
            .into_iter()
            .map(|(name, def_id)| HirVariant {
                def_id,
                name: interner.intern(name),
                fields: Vec::new(),
                span: Span::default(),
            })
            .collect(),
    };
    let comparison = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Binary(
            HirBinOp::Lt,
            Box::new(operand(3, 0, status)),
            Box::new(operand(4, 1, status)),
        ),
        ty: Some(bivalens),
        span: Span::default(),
    };
    let entry = HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Expr(comparison),
            span: Span::default(),
        }],
        None,
        Span::default(),
    );

    let mut artifact = stub_artifact();
    artifact.hir.items = vec![HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id: ENUM_DEF,
        kind: HirItemKind::Enum(hir_enum),
        span: Span::default(),
    }];
    artifact.hir.entry = Some(entry);
    artifact.types = types.snapshot();
    artifact.interner = interner.strings().to_vec();
    artifact.tensor_edge_policies.ordo_order = vec![OrdoOrderComparisonWire {
        lhs: LHS,
        rows: status_rows(),
    }];
    artifact
}

fn rejected(artifact: &HirArtifact) -> HirArtifactError {
    let bytes = encode(artifact).expect("encode artifact");
    decode(&bytes).expect_err("a malformed ordo order table must fail closed")
}

#[test]
fn ordo_order_rows_survive_the_postcard_round_trip() {
    let original = ordo_comparison_artifact();
    let bytes = encode(&original).expect("encode ordo comparison artifact");
    let decoded = decode(&bytes).expect("an ordo comparison artifact must validate");

    assert_eq!(
        decoded.tensor_edge_policies.ordo_order,
        original.tensor_edge_policies.ordo_order
    );
    let [comparison] = decoded.tensor_edge_policies.ordo_order.as_slice() else {
        panic!("exactly one comparison site must survive");
    };
    assert_eq!(comparison.lhs, LHS);
    assert_eq!(
        comparison.rows,
        status_rows(),
        "the rows keep declaration order and their declared discriminants, negative included"
    );
    assert_eq!(decoded, original);
}

#[test]
fn ordo_order_rows_survive_the_msgpack_round_trip() {
    let original = ordo_comparison_artifact();
    let bytes = encode_msgpack(&original).expect("encode ordo comparison artifact");
    let decoded = decode_msgpack(&bytes).expect("decode ordo comparison artifact");

    assert_eq!(
        decoded.tensor_edge_policies.ordo_order,
        original.tensor_edge_policies.ordo_order
    );
}

#[test]
fn dangling_ordo_comparison_site_fails_closed() {
    let mut artifact = ordo_comparison_artifact();
    artifact.tensor_edge_policies.ordo_order = vec![OrdoOrderComparisonWire {
        lhs: HirId(9_999),
        rows: status_rows(),
    }];
    assert!(matches!(
        rejected(&artifact),
        HirArtifactError::DanglingIndex { kind: "HirId", .. }
    ));
}

#[test]
fn dangling_ordo_variant_fails_closed() {
    let mut artifact = ordo_comparison_artifact();
    artifact.tensor_edge_policies.ordo_order = vec![OrdoOrderComparisonWire {
        lhs: LHS,
        rows: vec![(PENDING, 10), (DefId(USER_DEF_ID_BASE + 999), 11)],
    }];
    assert!(matches!(
        rejected(&artifact),
        HirArtifactError::DanglingIndex { kind: "DefId", .. }
    ));
}

#[test]
fn duplicate_ordo_comparison_site_fails_closed() {
    let mut artifact = ordo_comparison_artifact();
    artifact.tensor_edge_policies.ordo_order = vec![
        OrdoOrderComparisonWire {
            lhs: LHS,
            rows: status_rows(),
        },
        OrdoOrderComparisonWire {
            lhs: LHS,
            rows: vec![(PENDING, 0)],
        },
    ];
    assert!(matches!(
        rejected(&artifact),
        HirArtifactError::MalformedSnapshot {
            kind: "tensor edge policies",
            ..
        }
    ));
}
