//! `HirExpressionKind::Widen` on the FHIR wire (schema 26, inf-wn).
//!
//! Typecheck inserts the node where a bounded integer lands in an `inf` slot,
//! so a real artifact carries it. The validator holds it to the same
//! referential rules as any typed node: the operand subtree validates and both
//! the `from` and `to` types must resolve in the artifact's type table.

use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_hir::{HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLiteral};
use radix_lexer::Span;
use radix_types::TypeId;

fn widen_artifact(from: TypeId, to: TypeId) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let operand = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Literal(HirLiteral::Int(7)),
        ty: Some(from),
        span: Span::default(),
    };
    let body = HirBlock::simple(
        Vec::new(),
        Some(Box::new(HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Widen {
                operand: Box::new(operand),
                from,
                to,
            },
            ty: Some(to),
            span: Span::default(),
        })),
        Span::default(),
    );
    let HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    function.body = Some(body);
    artifact
}

#[test]
fn widen_node_round_trips_with_both_types() {
    let artifact = widen_artifact(TypeId(0), TypeId(1));
    let decoded = decode(&encode(&artifact).expect("encode")).expect("decode");
    let HirItemKind::Function(function) = &decoded.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    let tail = function
        .body
        .as_ref()
        .and_then(|body| body.expr.as_ref())
        .expect("tail expression");
    assert!(
        matches!(
            tail.kind,
            HirExpressionKind::Widen {
                from: TypeId(0),
                to: TypeId(1),
                ..
            }
        ),
        "{:?}",
        tail.kind
    );
}

#[test]
fn widen_node_with_a_dangling_type_fails_closed() {
    for (from, to) in [(TypeId(0), TypeId(9_999)), (TypeId(9_999), TypeId(0))] {
        let artifact = widen_artifact(from, to);
        let err = decode(&encode(&artifact).expect("encode"))
            .expect_err("a dangling widen type must fail closed");
        assert!(
            matches!(
                err,
                crate::error::HirArtifactError::DanglingIndex {
                    kind: "TypeId",
                    index: 9_999,
                    ..
                }
            ),
            "{err:?}"
        );
    }
}
