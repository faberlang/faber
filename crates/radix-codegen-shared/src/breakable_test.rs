//! Breakable-scope predicates and per-target emission pins.

use super::*;
use crate::CodeWriter;
use radix_hir::{
    HirBlock, HirBreakableKind, HirExpression, HirExpressionKind, HirId, HirStatement,
    HirStatementKind,
};
use radix_lexer::Span;

fn breakable_block(statements: Vec<HirStatement>) -> HirBlock {
    HirBlock {
        statements,
        expr: None,
        span: Span::new(0, 0),
        breakable: Some(HirBreakableKind::Fac),
        end: None,
    }
}

fn plain_block(statements: Vec<HirStatement>) -> HirBlock {
    HirBlock {
        statements,
        expr: None,
        span: Span::new(0, 0),
        breakable: None,
        end: None,
    }
}

fn rumpe_stmt() -> HirStatement {
    HirStatement {
        id: HirId(1),
        kind: HirStatementKind::Rumpe,
        span: Span::new(0, 0),
    }
}

fn perge_stmt() -> HirStatement {
    HirStatement {
        id: HirId(2),
        kind: HirStatementKind::Perge,
        span: Span::new(0, 0),
    }
}

/// A breakable block whose only statement is `rumpe` (break)
/// should not need a fallthrough break — the rumpe exits the block.
#[test]
fn break_from_loop_needs_no_fallthrough() {
    let block = breakable_block(vec![rumpe_stmt()]);
    assert!(is_breakable(&block));
    assert!(block_is_immediate_rumpe(&block));
    assert!(block_contains_rumpe(&block));
    assert!(!block_needs_fallthrough_break(&block));
}

/// A breakable block containing `perge` (continue) needs a
/// fallthrough break because the loop body can complete normally.
#[test]
fn continue_in_loop_needs_fallthrough() {
    let block = breakable_block(vec![perge_stmt()]);
    assert!(is_breakable(&block));
    assert!(!block_is_immediate_rumpe(&block)); // Perge, not Rumpe
    assert!(!block_contains_rumpe(&block)); // No Rumpe anywhere
    // No explicit return, no rumpe → fallthrough break needed
    assert!(block_needs_fallthrough_break(&block));
}

/// An empty breakable block needs fallthrough break (no rumpe).
#[test]
fn empty_breakable_block_needs_fallthrough() {
    let block = breakable_block(vec![]);
    assert!(is_breakable(&block));
    assert!(!block_is_immediate_rumpe(&block));
    assert!(!block_contains_rumpe(&block));
    assert!(block_needs_fallthrough_break(&block));
}

/// A non-breakable block (breakable is None) should not be breakable.
#[test]
fn non_breakable_block_is_not_breakable() {
    let block = plain_block(vec![]);
    assert!(!is_breakable(&block));
    assert!(!block_is_immediate_rumpe(&block));
    assert!(!block_contains_rumpe(&block));
    assert!(block_needs_fallthrough_break(&block));
}

/// A breakable block with `redde` (return) does not need fallthrough break.
#[test]
fn return_in_breakable_block_no_fallthrough() {
    let return_stmt = HirStatement {
        id: HirId(3),
        kind: HirStatementKind::Redde(Some(HirExpression {
            id: HirId(4),
            kind: HirExpressionKind::Literal(radix_hir::HirLiteral::Int(0)),
            ty: None,
            span: Span::new(0, 0),
        })),
        span: Span::new(0, 0),
    };
    let block = breakable_block(vec![return_stmt]);
    assert!(is_breakable(&block));
    assert!(!block_is_immediate_rumpe(&block));
    assert!(!block_contains_rumpe(&block));
    assert!(!block_needs_fallthrough_break(&block));
}

/// Each target gets its own loop shape; an immediate rumpe body emits no
/// fallthrough break. The fallthrough break lands at the enclosing indent
/// (it is written after the indented body closure) — behavior preserved
/// from the deleted per-backend copies.
#[test]
fn breakable_scope_emits_target_loop() {
    for (style, expected) in [
        (BreakableLoopStyle::Rust, "loop {\n    body\n}"),
        (BreakableLoopStyle::Go, "for {\n    body\n}"),
        (
            BreakableLoopStyle::TypeScript,
            "while (true) {\n    body\n}",
        ),
        (
            BreakableLoopStyle::Swift,
            "repeat {\n    body\n} while true\n",
        ),
    ] {
        let block = breakable_block(vec![rumpe_stmt()]);
        let mut writer = CodeWriter::new();
        let wrapped = write_breakable_scope(&block, &mut writer, style, |w| {
            w.writeln("body");
        });
        assert!(wrapped, "{style:?}: breakable block must wrap in a loop");
        assert_eq!(writer.finish(), expected, "{style:?}");
    }
}

/// A body that can complete normally gets the target's fallthrough break.
#[test]
fn breakable_scope_emits_fallthrough_break() {
    let block = breakable_block(vec![perge_stmt()]);
    let mut writer = CodeWriter::new();
    let wrapped = write_breakable_scope(&block, &mut writer, BreakableLoopStyle::Go, |w| {
        w.writeln("body");
    });
    assert!(wrapped);
    assert_eq!(writer.finish(), "for {\n    body\nbreak;\n}");
}

/// Plain blocks wrap in braces except Swift, which emits inline.
#[test]
fn plain_block_wraps_except_swift() {
    let block = plain_block(vec![]);
    let mut writer = CodeWriter::new();
    let wrapped = write_breakable_scope(&block, &mut writer, BreakableLoopStyle::Rust, |w| {
        w.writeln("stmt");
    });
    assert!(!wrapped);
    assert_eq!(writer.finish(), "{\n    stmt\n}");

    let mut writer = CodeWriter::new();
    let wrapped = write_breakable_scope(&block, &mut writer, BreakableLoopStyle::Swift, |w| {
        w.writeln("stmt");
    });
    assert!(!wrapped);
    assert_eq!(writer.finish(), "stmt\n");
}

fn expr_stmt(id: u32, kind: HirExpressionKind) -> HirStatement {
    HirStatement {
        id: HirId(id),
        kind: HirStatementKind::Expr(HirExpression {
            id: HirId(id + 1000),
            kind,
            ty: None,
            span: Span::new(0, 0),
        }),
        span: Span::new(0, 0),
    }
}

/// Ruling R3: a `perge` directly in a `fac` body passes through it.
#[test]
fn perge_in_fac_body_is_a_passthrough_site() {
    let body = breakable_block(vec![perge_stmt()]);
    assert_eq!(passthrough_perge_sites(&body), vec![HirId(2)]);
    assert!(block_may_exit_early(&body));
}

/// A `perge` inside a loop nested in the `fac` targets that loop.
#[test]
fn perge_in_nested_loop_is_not_a_passthrough_site() {
    let body = breakable_block(vec![expr_stmt(
        10,
        HirExpressionKind::Loop(plain_block(vec![perge_stmt()])),
    )]);
    assert!(passthrough_perge_sites(&body).is_empty());
    assert!(!block_may_exit_early(&body));
}

/// A nested `fac` with a pass-through `perge` is a re-issue site of the
/// outer body; its own `perge` belongs to the nested route.
#[test]
fn nested_fac_with_passthrough_perge_is_a_reissue_site() {
    let inner = breakable_block(vec![perge_stmt()]);
    let outer = breakable_block(vec![expr_stmt(20, HirExpressionKind::Block(inner))]);
    assert_eq!(passthrough_perge_sites(&outer), vec![HirId(20)]);
}

/// Opening a route registers its sites and wrapper; closing releases them.
#[test]
fn perge_routes_open_route_and_close() {
    let stmt = expr_stmt(
        30,
        HirExpressionKind::Block(breakable_block(vec![perge_stmt()])),
    );
    let routes = PergeRoutes::default();
    let route = routes
        .open(&stmt)
        .expect("fac with pass-through perge opens");
    assert_eq!(routes.route(HirId(2)), Some(route));
    let body = fac_statement_body(&stmt).expect("fac body");
    assert_eq!(routes.wrapper(body), Some(route));
    assert_eq!(routes.open(&stmt), None, "re-entrant open is a no-op");
    routes.close(route);
    assert_eq!(routes.route(HirId(2)), None);
    assert_eq!(routes.wrapper(body), None);

    let plain = expr_stmt(
        40,
        HirExpressionKind::Block(breakable_block(vec![rumpe_stmt()])),
    );
    assert_eq!(routes.open(&plain), None, "no pass-through perge, no route");
}

/// A routed wrapper carries its label in each target's syntax.
#[test]
fn labeled_breakable_scope_prefixes_the_wrapper() {
    for (style, expected) in [
        (BreakableLoopStyle::Rust, "'k0: loop {\n    body\n}"),
        (BreakableLoopStyle::Go, "k0: for {\n    body\n}"),
        (
            BreakableLoopStyle::TypeScript,
            "k0: while (true) {\n    body\n}",
        ),
    ] {
        let block = breakable_block(vec![rumpe_stmt()]);
        let mut writer = CodeWriter::new();
        write_labeled_breakable_scope(&block, &mut writer, style, Some("k0"), |w| {
            w.writeln("body");
        });
        assert_eq!(writer.finish(), expected, "{style:?}");
    }
}
