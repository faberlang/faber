//! Breakable-block brace emission for C-family backends.
//!
//! Faber `rumpe` lowers to `break` in Rust, Go, and TypeScript. Only loop bodies
//! accept `break` today, so breakable `fac` bodies use ``loop { … break; }``.
//! Failable closures must emit `Ok(…)` **after** the loop, not inside it before
//! `break;`.

use crate::hir::visit::HirVisitor;
use crate::hir::{HirBlock, HirStatementKind};

use super::CodeWriter;

/// Breakable-scope loop header for each C-family backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakableLoopStyle {
    Rust,
    Go,
    TypeScript,
}

impl BreakableLoopStyle {
    fn header(self) -> &'static str {
        match self {
            Self::Rust => "loop {",
            Self::Go => "for {",
            Self::TypeScript => "while (true) {",
        }
    }
}

/// Whether [`HirBlock::breakable`] is set.
#[must_use]
pub fn is_breakable(block: &HirBlock) -> bool {
    block.breakable.is_some()
}

/// Whether the block is exactly one top-level `rumpe` (fallthrough `break` would be unreachable).
#[must_use]
pub fn block_is_immediate_rumpe(block: &HirBlock) -> bool {
    block.statements.len() == 1 && matches!(block.statements[0].kind, HirStatementKind::Rumpe) && block.expr.is_none()
}

/// Whether `rumpe` may appear anywhere inside `block` (including nested `si` arms).
#[must_use]
pub fn block_contains_rumpe(block: &HirBlock) -> bool {
    struct RumpeFinder {
        found: bool,
    }

    impl HirVisitor for RumpeFinder {
        fn visit_stmt(&mut self, stmt: &crate::hir::HirStatement) {
            if self.found {
                return;
            }
            if matches!(stmt.kind, HirStatementKind::Rumpe) {
                self.found = true;
                return;
            }
            crate::hir::visit::walk_stmt(self, stmt);
        }
    }

    let mut finder = RumpeFinder { found: false };
    finder.visit_block(block);
    finder.found
}

/// Whether a breakable loop needs a fallthrough `break` after the inner body.
///
/// `redde` lowers to `return Ok(…)` inside the handled closure; a trailing `break`
/// is dead code and breaks Rust typecheck when the loop is the closure tail.
#[must_use]
pub fn block_needs_fallthrough_break(block: &HirBlock) -> bool {
    if block_is_immediate_rumpe(block) {
        return false;
    }
    if block_contains_rumpe(block) {
        return true;
    }
    if block
        .statements
        .iter()
        .any(|stmt| matches!(stmt.kind, HirStatementKind::Redde(Some(_))))
    {
        return false;
    }
    true
}

/// Emit a breakable loop or plain block around `emit_inner`.
///
/// Returns `true` when a breakable loop was emitted so callers can append
/// success/return glue after the loop.
pub fn emit_breakable_scope<F>(
    block: &HirBlock,
    writer: &mut CodeWriter,
    loop_style: BreakableLoopStyle,
    emit_inner: F,
) -> bool
where
    F: FnOnce(&mut CodeWriter),
{
    if block.breakable.is_some() {
        writer.writeln(loop_style.header());
        writer.indented(emit_inner);
        if block_needs_fallthrough_break(block) {
            writer.writeln("break;");
        }
        writer.write("}");
        true
    } else {
        writer.writeln("{");
        writer.indented(emit_inner);
        writer.write("}");
        false
    }
}
