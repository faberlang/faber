//! Breakable-block brace emission for C-family backends.
//!
//! Faber `rumpe` lowers to `break` in Rust, Go, TypeScript, and Swift. Only
//! loop bodies accept `break` today, so breakable `fac` bodies wrap in a
//! one-shot loop per target: `loop { … break; }` (Rust), `for { … break; }`
//! (Go), `while (true) { … break; }` (TypeScript), `repeat { … break } while
//! true` (Swift). Failable closures must emit their success value **after**
//! the loop, not inside it before the `break`.
//!
//! PERGE THROUGH A FAC (ruling R3)
//! ===============================
//! `perge` targets the nearest loop; a plain `fac` block is not a loop, so
//! `perge` inside a `fac` body continues the enclosing loop. The one-shot
//! wrapper loop (and, on Go/TypeScript, the handled-body closure) would
//! capture a bare `continue`, so such a `fac` statement is emitted as:
//! declare a flag, label the wrapper, and lower each pass-through `perge` to
//! "set flag; break wrapper". After the statement, `if flag { perge }`
//! re-issues the `perge` in the enclosing context (which may itself be an
//! outer `fac` wrapper). [`PergeRoutes`] carries that bookkeeping for one
//! backend; only `fac` bodies with a pass-through `perge` change shape.

use crate::CodeWriter;
use radix_hir::visit::HirVisitor;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirStatement, HirStatementKind,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::{Cell, RefCell};

/// Breakable-scope loop shape for each C-family backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakableLoopStyle {
    Rust,
    Go,
    TypeScript,
    Swift,
}

impl BreakableLoopStyle {
    /// Label prefix for a labeled wrapper loop (`'k0: loop {`, `k0: for {`).
    fn label_prefix(self, label: &str) -> String {
        match self {
            Self::Rust => format!("'{label}: "),
            _ => format!("{label}: "),
        }
    }

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

    /// Whether this backend drops the one-shot loop from a breakable block
    /// that cannot leave early (see [`block_may_exit_early`]).
    fn drops_dead_shell(self) -> bool {
        matches!(self, Self::TypeScript | Self::Go)
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
    block
        .breakable
        .is_some_and(radix_hir::HirBreakableKind::is_scope)
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
        fn visit_stmt(&mut self, stmt: &HirStatement) {
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

/// Whether control may leave `block`'s wrapper loop early: a `rumpe`, or a
/// `perge` that passes through this `fac` body (ruling R3). Either path
/// resumes after the wrapper, so post-wrapper glue must stay reachable.
#[must_use]
pub fn block_may_exit_early(block: &HirBlock) -> bool {
    block_contains_rumpe(block) || !passthrough_perge_sites(block).is_empty()
}

/// Body of a plain `fac` expression (with or without `cape`): a breakable
/// block, possibly wrapped as the effective body of a handled expression.
#[must_use]
pub fn fac_expr_body(expr: &HirExpression) -> Option<&HirBlock> {
    match &expr.kind {
        HirExpressionKind::Block(block) if is_breakable(block) => Some(block),
        HirExpressionKind::Handled { body, .. } => {
            if is_breakable(body) {
                return Some(body);
            }
            if body.statements.is_empty()
                && let Some(inner) = body.expr.as_deref()
                && let HirExpressionKind::Block(block) = &inner.kind
                && is_breakable(block)
            {
                return Some(block);
            }
            None
        }
        _ => None,
    }
}

/// [`fac_expr_body`] for an expression statement.
#[must_use]
pub fn fac_statement_body(stmt: &HirStatement) -> Option<&HirBlock> {
    match &stmt.kind {
        HirStatementKind::Expr(expr) => fac_expr_body(expr),
        _ => None,
    }
}

/// Statements inside a `fac` body whose `perge` passes through that body to
/// an enclosing loop (ruling R3).
///
/// Returns the ids of `perge` statements, plus nested `fac` statements that
/// re-issue a pass-through `perge` after their own wrapper. The search stops
/// at loops (they own their `perge`) and closures (`perge` never crosses one).
#[must_use]
pub fn passthrough_perge_sites(body: &HirBlock) -> Vec<HirId> {
    perge_sites(&body.statements, body.expr.as_deref())
}

/// Whether a `fac … dum` body (the statements before its synthesized test,
/// see [`HirBlock::fac_dum_parts`]) has a `perge` that continues this loop,
/// directly or through nested `fac` blocks (ruling R3).
///
/// Such a body needs a loop shape whose `continue` evaluates the condition;
/// the C-family `loop { body; if !c { break } }` form would restart the body.
#[must_use]
pub fn fac_dum_body_has_perge(body: &[HirStatement]) -> bool {
    !perge_sites(body, None).is_empty()
}

fn perge_sites(statements: &[HirStatement], tail: Option<&HirExpression>) -> Vec<HirId> {
    struct PergeSites {
        sites: Vec<HirId>,
    }

    impl HirVisitor for PergeSites {
        fn visit_stmt(&mut self, stmt: &HirStatement) {
            if matches!(stmt.kind, HirStatementKind::Perge) {
                self.sites.push(stmt.id);
                return;
            }
            if let Some(inner) = fac_statement_body(stmt)
                && !passthrough_perge_sites(inner).is_empty()
            {
                // The nested `fac` routes its own body; its re-issue and its
                // `cape` arm run in this body.
                self.sites.push(stmt.id);
                if let HirStatementKind::Expr(expr) = &stmt.kind
                    && let HirExpressionKind::Handled { catch, .. } = &expr.kind
                {
                    self.visit_block(&catch.body);
                }
                return;
            }
            radix_hir::visit::walk_stmt(self, stmt);
        }

        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(
                expr.kind,
                HirExpressionKind::Loop(_)
                    | HirExpressionKind::Dum(..)
                    | HirExpressionKind::Itera { .. }
                    | HirExpressionKind::IteraLockstep { .. }
                    | HirExpressionKind::IteraRange { .. }
                    | HirExpressionKind::Reduction { .. }
                    | HirExpressionKind::Scan { .. }
                    | HirExpressionKind::FirstMatch { .. }
                    | HirExpressionKind::Ad { .. }
                    | HirExpressionKind::Clausura(..)
            ) {
                return;
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }

    let mut finder = PergeSites { sites: Vec::new() };
    for stmt in statements {
        finder.visit_stmt(stmt);
    }
    if let Some(expr) = tail {
        finder.visit_expr(expr);
    }
    finder.sites
}

/// Per-backend routing state for `perge` through `fac` bodies (ruling R3).
///
/// [`PergeRoutes::open`] claims a `fac` statement with a pass-through
/// `perge` and returns its route index; the backend then emits the flag
/// declaration, the statement (its wrapper labeled via
/// [`PergeRoutes::wrapper`], each routed `perge` lowered as "set flag; break
/// wrapper"), and the re-issue, and finally calls [`PergeRoutes::close`].
#[derive(Debug, Default)]
pub struct PergeRoutes {
    next: Cell<u32>,
    sites: RefCell<FxHashMap<HirId, u32>>,
    wrappers: RefCell<FxHashMap<usize, u32>>,
    opened: RefCell<FxHashSet<HirId>>,
    owned: RefCell<FxHashMap<u32, OpenRoute>>,
}

/// What one open route claimed: the `fac` statement, its wrapper body key,
/// and its pass-through sites.
#[derive(Debug)]
struct OpenRoute {
    stmt_id: HirId,
    wrapper_key: usize,
    sites: Vec<HirId>,
}

impl PergeRoutes {
    /// Claim `stmt` when it is a `fac` statement whose body a `perge`
    /// passes through. Returns `None` for every other statement, and for a
    /// statement already open (the backend's re-entrant emission call).
    pub fn open(&self, stmt: &HirStatement) -> Option<u32> {
        if self.opened.borrow().contains(&stmt.id) {
            return None;
        }
        let body = fac_statement_body(stmt)?;
        let sites = passthrough_perge_sites(body);
        if sites.is_empty() {
            return None;
        }
        let route = self.next.get();
        self.next.set(route + 1);
        let key = std::ptr::from_ref(body) as usize;
        self.opened.borrow_mut().insert(stmt.id);
        self.wrappers.borrow_mut().insert(key, route);
        {
            let mut map = self.sites.borrow_mut();
            for site in &sites {
                map.insert(*site, route);
            }
        }
        self.owned.borrow_mut().insert(
            route,
            OpenRoute {
                stmt_id: stmt.id,
                wrapper_key: key,
                sites,
            },
        );
        Some(route)
    }

    /// Claim a fresh hygiene index for a `fac … dum` condition flag. It
    /// shares the route counter, so the flag name never collides with a
    /// route flag or label ([`crate::hygiene::perge_route`]).
    pub fn fac_dum_flag(&self) -> u32 {
        let index = self.next.get();
        self.next.set(index + 1);
        index
    }

    /// Release a route opened by [`PergeRoutes::open`].
    pub fn close(&self, route: u32) {
        let Some(OpenRoute {
            stmt_id,
            wrapper_key: key,
            sites,
        }) = self.owned.borrow_mut().remove(&route)
        else {
            return;
        };
        self.opened.borrow_mut().remove(&stmt_id);
        self.wrappers.borrow_mut().remove(&key);
        let mut map = self.sites.borrow_mut();
        for site in sites {
            if map.get(&site) == Some(&route) {
                map.remove(&site);
            }
        }
    }

    /// Route for a `perge` (or a nested `fac` re-issue): `Some(route)` when
    /// it exits an open `fac` wrapper, `None` when a plain `continue` works.
    #[must_use]
    pub fn route(&self, stmt_id: HirId) -> Option<u32> {
        self.sites.borrow().get(&stmt_id).copied()
    }

    /// Route whose wrapper loop is `block`, when that wrapper needs a label.
    #[must_use]
    pub fn wrapper(&self, block: &HirBlock) -> Option<u32> {
        self.wrappers
            .borrow()
            .get(&(std::ptr::from_ref(block) as usize))
            .copied()
    }
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
    if block_may_exit_early(block) {
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
/// Returns `true` when a breakable scope was emitted (a loop, or on a backend
/// that drops the dead shell, the bare block standing in for it) so callers
/// can append success/return glue after it. Swift has no bare block expression, so
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
    write_labeled_breakable_scope(block, writer, loop_style, None, emit_inner)
}

/// [`write_breakable_scope`] with an optional wrapper-loop label.
///
/// The label is set only for a `fac` body with a pass-through `perge` (see
/// [`PergeRoutes::wrapper`]); every other body keeps the unlabeled shape.
pub fn write_labeled_breakable_scope<F>(
    block: &HirBlock,
    writer: &mut CodeWriter,
    loop_style: BreakableLoopStyle,
    label: Option<&str>,
    emit_inner: F,
) -> bool
where
    F: FnOnce(&mut CodeWriter),
{
    if is_breakable(block) {
        if loop_style.drops_dead_shell() && !block_may_exit_early(block) {
            // No `rumpe` and no routed `perge`: the loop would run once and
            // `break` at its end. Keep the braces (they scope the body's
            // locals) and the caller's after-the-scope glue; drop the loop.
            writer.writeln("{");
            writer.indented(emit_inner);
            writer.ensure_line_start();
            loop_style.write_footer(writer);
            return true;
        }
        if let Some(label) = label {
            writer.write(&loop_style.label_prefix(label));
        }
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
