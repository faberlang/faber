//! `HirExpressionKind::Insere` on the FHIR wire (schema 25, st1 U4).
//!
//! The embed folds to a literal right after typecheck, so a successful
//! analysis never serializes one; the variant is still a legal HIR shape and
//! the validator holds it to the same referential rules as any leaf: its path
//! symbol must resolve in the artifact's interner table.

use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_hir::{HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind};
use radix_lexer::{Span, Symbol};

fn insere_artifact(path: Symbol) -> crate::artifact::HirArtifact {
    let mut artifact = stub_artifact();
    let body = HirBlock::simple(
        Vec::new(),
        Some(Box::new(HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Insere(path),
            ty: None,
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
fn insere_node_round_trips_with_its_path() {
    let artifact = insere_artifact(Symbol(0));
    let decoded = decode(&encode(&artifact).expect("encode")).expect("decode");
    let HirItemKind::Function(function) = &decoded.hir.items[0].kind else {
        panic!("stub artifact item must be a function");
    };
    let body = function.body.as_ref().expect("function body");
    let tail = body.expr.as_ref().expect("tail expression");
    assert!(
        matches!(tail.kind, HirExpressionKind::Insere(Symbol(0))),
        "{:?}",
        tail.kind
    );
}

#[test]
fn insere_node_with_a_dangling_path_symbol_fails_closed() {
    let artifact = insere_artifact(Symbol(97));
    let err = decode(&encode(&artifact).expect("encode"))
        .expect_err("dangling embed path symbol must fail closed");
    assert!(
        matches!(
            err,
            crate::error::HirArtifactError::DanglingIndex {
                kind: "Symbol",
                index,
                ..
            } if index == 97
        ),
        "{err:?}"
    );
}
