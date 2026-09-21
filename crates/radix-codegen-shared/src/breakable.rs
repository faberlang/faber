//! Breakable-block brace emission for C-family backends.
//!
//! Faber `rumpe` lowers to `break` in Rust, Go, TypeScript, and Swift. Only
//! loop bodies accept `break` today, so breakable `fac` bodies wrap in a
//! one-shot loop per target: `loop { … break; }` (Rust), `for { … break; }`
//! (Go), `while (true) { … break; }` (TypeScript), `repeat { … break } while
//! true` (Swift). Failable closures must emit their success value **after**
//! the loop, not inside it before the `break`.

use crate::CodeWriter;
use radix_hir::visit::HirVisitor;
use radix_hir::{HirBlock, HirStatementKind};

/// Breakable-scope loop shape for each C-family backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakableLoopStyle {
    Rust,
    Go,
    TypeScript,
    Swift,
}

impl BreakableLoopStyle {
    fn header(self) -> &'static str {
        match self {
            Self::Rust => "loop {",
            Self::Go => "for {",
            Self::TypeScript => "while (true) {",
            Self::Swift => "repeat {",
        }
    }

    fn footer(self) -> &'static str {
        match self {
            Self::Swift => "} while true",
            _ => "}",
        }
    }

    /// Swift statements carry no terminating `;`.
    fn break_stmt(self) -> &'static str {
        match self {
            Self::Swift => "break",
            _ => "break;",
        }
    }

    /// Close the loop. Swift needs `} while true` on its own line; the other
    /// targets close with a bare `}` glued to the caller's next text.
    fn write_footer(self, writer: &mut CodeWriter) {
        if self == Self::Swift {
            writer.writeln(self.footer());
        } else {
            writer.write(self.footer());
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
    block.statements.len() == 1
        && matches!(block.statements[0].kind, HirStatementKind::Rumpe)
        && block.expr.is_none()
}

/// Whether `rumpe` may appear anywhere inside `block` (including nested `si` arms).
#[must_use]
pub fn block_contains_rumpe(block: &HirBlock) -> bool {
    struct RumpeFinder {
        found: bool,
    }

    impl HirVisitor for RumpeFinder {
        fn visit_stmt(&mut self, stmt: &radix_hir::HirStatement) {
            if self.found {
                return;
            }
            if matches!(stmt.kind, HirStatementKind::Rumpe) {
                self.found = true;
                return;
            }
            radix_hir::visit::walk_stmt(self, stmt);
        }
    }

    let mut finder = RumpeFinder { found: false };
    finder.visit_block(block);
    finder.found
}

/// Whether a breakable loop needs a fallthrough `break` after the inner body.
///
/// `redde` lowers to a return inside the handled closure, so a trailing
/// `break` there is dead code (and in Rust it breaks the typecheck when the
/// loop is the closure tail).
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
/// success/return glue after the loop. Swift has no bare block expression, so
/// its plain blocks emit inline instead of becoming unused closures.
pub fn write_breakable_scope<F>(
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
            writer.writeln(loop_style.break_stmt());
        }
        loop_style.write_footer(writer);
        true
    } else if loop_style == BreakableLoopStyle::Swift {
        emit_inner(writer);
        false
    } else {
        writer.writeln("{");
        writer.indented(emit_inner);
        loop_style.write_footer(writer);
        false
    }
}

#[cfg(test)]
#[path = "breakable_test.rs"]
mod tests;
