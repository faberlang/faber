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
