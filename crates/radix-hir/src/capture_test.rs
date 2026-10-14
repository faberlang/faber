//! Leaf-owned proofs for the shared closure-capture analysis.

use crate::capture::CaptureFacts;
use crate::nodes::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirLocal, HirStatement, HirStatementKind,
};
use radix_lexer::{Span, Symbol};
use radix_types::DefId;

fn path(def_id: DefId) -> HirExpression {
    HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(def_id),
        ty: None,
        span: Span::default(),
    }
}

fn expression(id: u32, kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind,
        ty: None,
        span: Span::default(),
    }
}

/// A block declaring mutable locals `locals` and then a closure whose body is
/// `body`: the smallest shape that observes capture facts for those locals.
fn block_with_closure_body(locals: &[DefId], body: HirExpression) -> HirBlock {
    let mut statements = Vec::new();
    for (ordinal, def_id) in locals.iter().enumerate() {
        let local = HirLocal {
            def_id: *def_id,
            name: Symbol(ordinal as u32),
            ty: None,
            init: None,
            mutable: true,
            is_await: false,
            runtime_provided: false,
            await_store: None,
            size_decl: false,
            size_static: false,
        };
        statements.push(HirStatement {
            id: HirId(10 + ordinal as u32),
            kind: HirStatementKind::Local(local),
            span: Span::default(),
        });
    }
    let closure = expression(
        2,
        HirExpressionKind::Clausura(Vec::new(), None, None, Box::new(body)),
    );
    statements.push(HirStatement {
        id: HirId(4),
        kind: HirStatementKind::Expr(closure),
        span: Span::default(),
    });
    HirBlock::simple(statements, None, Span::default())
}

#[test]
fn closure_copy_into_marks_the_target_content_mutated() {
    let target = DefId(1);
    let source = DefId(2);
    let body = expression(
        5,
        HirExpressionKind::CopyInto {
            target: Box::new(path(target)),
            value: Box::new(path(source)),
        },
    );
    let facts = CaptureFacts::for_block(&block_with_closure_body(&[target], body));
    assert!(
        facts.captured.contains(&target),
        "the copy-into target is used inside the closure, so it is captured"
    );
    assert!(
        facts.content_mutated.contains(&target),
        "`target ⇇ source` mutates the target's content in place"
    );
}

#[test]
fn closure_copy_into_operand_is_walked() {
    let target = DefId(1);
    let source = DefId(2);
    let body = expression(
        5,
        HirExpressionKind::CopyInto {
            target: Box::new(path(target)),
            value: Box::new(path(source)),
        },
    );
    let facts = CaptureFacts::for_block(&block_with_closure_body(&[target, source], body));
    assert!(
        facts.captured.contains(&source),
        "the copy-into value operand is read inside the closure, so it is captured"
    );
    assert!(
        !facts.content_mutated.contains(&source),
        "the copied-from operand is read, never mutated in place"
    );
}

#[test]
fn closure_read_of_the_local_does_not_mark_content_mutated() {
    let target = DefId(1);
    let body = path(target);
    let facts = CaptureFacts::for_block(&block_with_closure_body(&[target], body));
    assert!(
        facts.captured.contains(&target),
        "the read still captures the local"
    );
    assert!(
        !facts.content_mutated.contains(&target),
        "a plain read is not an in-place mutation"
    );
}
