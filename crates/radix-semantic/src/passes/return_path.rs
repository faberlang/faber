//! Return-path exhaustiveness for value-returning functions (DEFER-011).
//!
//! A function that declares a normal result type (`→ T` where `T` is not
//! `vacuum`) must return on every control-flow path. `redde` is the only
//! statement that satisfies the normal return channel; a path that reaches the
//! end of the body without returning leaves the result uninitialized, which is
//! a soundness gap rather than a style issue. The primary backend (Rust)
//! rejects such functions at link time, so Faber's `check` must reject them
//! earlier with a Faber-side diagnostic instead.
//!
//! This pass owns exactly that contract. It runs after typecheck, when
//! expression/statement types and function result types are settled, and after
//! [`exhaustive`], reusing that pass's coverage helper to decide whether a
//! `discerne` can fall out the bottom for an uncovered value.
//!
//! TERMINATION MODEL
//! =================
//! Each construct is reduced to one of two outcomes:
//!
//! - `Terminates`: control cannot reach the end of the construct. Either a
//!   `redde` returns, or the construct diverges (`iace`, `mori`/panic, or an
//!   infinite `loop` with no reachable `rumpe`).
//! - `FallsThrough`: control may reach the end and continue, so the enclosing
//!   path has not provably returned.
//!
//! A `→ T` function body is valid iff it `Terminates`. Loops (`dum`, `itera`)
//! always `FallsThrough` because their bodies may execute zero times; a bare
//! `loop` `Terminates` only when no `rumpe` can escape it. The do-while
//! spelling `fac { … } dum verum` lowers to the same `loop` shape and is
//! accepted by the same rule: its synthesized `si ¬verum { rumpe }` tail is
//! unreachable, so only a user-written `rumpe` counts as an escape.
//! `rumpe`/`perge` transfer control to an enclosing breakable scope, so at
//! function level they do not satisfy the return contract and are treated as
//! `FallsThrough`.
//!
//! The error channel (`iace`, `⇥ E`) does not satisfy the normal `→ T`
//! channel: a function that only throws on every path still owes a value on
//! the path where it does not throw. `iace` counts as `Terminates` only
//! because it diverges the current path, never because it satisfies the
//! result type.
//!
//! LIMITATIONS
//! ===========
//! - `Handled` (`fac { … } cape err { … }`) requires both body and catch to
//!   `Terminate`. This is sound but slightly strict: a body that always
//!   returns via `redde` makes a dead catch, yet the catch is still required
//!   to terminate. Rare in practice; revisited when effect-error CFG lands.
//! - Statement-bodied closures with an explicit `→ T` are checked; expression-
//!   bodied closures still infer their result from the tail expression.
//! - `loop { … }` divergence is decided by a syntactic `rumpe` search of the
//!   body subtree, which conservatively treats a `rumpe` in a nested
//!   breakable scope as escaping the outer loop.

use crate::passes::exhaustive::discerne_definitely_covered;
use crate::{SemanticError, SemanticErrorKind, TypeTable};
use handroll::FxHashMap;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItem, HirItemKind, HirModule,
    HirStatement, HirStatementKind,
    visit::{HirVisitor, walk_expr, walk_program},
};

/// Validate that every value-returning function body returns on all paths.
///
/// `enum_variants` is the closed variant list of every enum in scope (see
/// [`crate::passes::exhaustive::collect_all_enum_variants`]).
///
/// # Errors
///
/// Returns missing-return and control-flow errors as a list of
/// [`SemanticError`]s.
pub fn analyze(
    hir: &HirModule,
    types: &TypeTable,
    enum_variants: &FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = ReturnPathChecker {
        types,
        enum_variants,
        errors: Vec::new(),
    };
    checker.check_program(hir);

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

struct ReturnPathChecker<'a> {
    types: &'a TypeTable,
    enum_variants: &'a FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
    errors: Vec<SemanticError>,
}

impl ReturnPathChecker<'_> {
    fn check_program(&mut self, hir: &HirModule) {
        for item in &hir.items {
            self.check_item(item);
        }
        walk_program(
            &mut ClosureReturnPathVisitor {
                types: self.types,
                enum_variants: self.enum_variants,
                errors: &mut self.errors,
            },
            hir,
        );
    }

    fn check_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(func) => self.check_function(func, item.span),
            // A method owes the same normal-return contract as a free
            // function; its body is no less able to fall off the end.
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    self.check_function(&method.func, method.span);
                }
            }
            _ => {}
        }
    }

    fn check_function(&mut self, func: &HirFunction, span: radix_lexer::Span) {
        // Generators (`@ cursor`) are excluded: their declared `→ T` is the
        // yielded type produced by `cede`, not a normal return, so they owe
        // no `redde`. Async (`@ futura`) functions still return a value through
        // `redde` (wrapped in a future), so they are checked.
        if func.is_generator {
            return;
        }
        let Some(ret_ty) = func.ret_ty else { return };
        if is_unit_return(ret_ty, self.types) {
            return;
        }
        let Some(body) = &func.body else { return };
        if !block_terminates(body, self.types, self.enum_variants) {
            push_missing_return(&mut self.errors, span);
        }
    }
}

struct ClosureReturnPathVisitor<'a> {
    types: &'a TypeTable,
    enum_variants: &'a FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
    errors: &'a mut Vec<SemanticError>,
}

impl HirVisitor for ClosureReturnPathVisitor<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Clausura(_, ret, _, body) = &expr.kind {
            let Some(ret_ty) = ret else {
                walk_expr(self, expr);
                return;
            };
            if !is_unit_return(*ret_ty, self.types)
                && is_statement_bodied_closure(body)
                && !expr_terminates(body, self.types, self.enum_variants)
            {
                push_missing_return(self.errors, expr.span);
            }
        }
        walk_expr(self, expr);
    }
}

fn push_missing_return(errors: &mut Vec<SemanticError>, span: radix_lexer::Span) {
    errors.push(
        SemanticError::structured(SemanticErrorKind::ReturnAbsent, span)
            .with_arg("issue", "control_flow_fallthrough"),
    );
}

fn is_statement_bodied_closure(body: &HirExpression) -> bool {
    matches!(
        body.kind,
        HirExpressionKind::Block(_)
            | HirExpressionKind::Handled { .. }
            | HirExpressionKind::Loop(_)
    )
}

/// Decide whether a block definitely terminates (returns or diverges).
fn block_terminates(
    block: &HirBlock,
    types: &TypeTable,
    enum_variants: &FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
) -> bool {
    // The first statement that terminates makes the rest unreachable. If no
    // statement terminates, the block's tail expression decides it: lowering
    // wraps bodies (`fac`, `si` arms) in `Block` expressions, so a terminating
    // construct can live in the tail rather than in a statement slot.
    if block
        .statements
        .iter()
        .any(|stmt| stmt_terminates(stmt, types, enum_variants))
    {
        return true;
    }
    block
        .expr
        .as_ref()
        .is_some_and(|expr| expr_terminates(expr, types, enum_variants))
}

fn stmt_terminates(
    stmt: &HirStatement,
    types: &TypeTable,
    enum_variants: &FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
) -> bool {
    match &stmt.kind {
        // `redde` is the only statement that satisfies the normal channel.
        HirStatementKind::Redde(_) => true,
        HirStatementKind::ReddeFilum(_) => false,
        // Control-flow expressions embedded as statements may terminate; other
        // expression statements (calls, assignments, projections) fall through.
        HirStatementKind::Expr(expr) => expr_terminates(expr, types, enum_variants),
        // Local bindings, capability calls, frame emits, loop jumps, and the
        // explicit no-op all fall through to the following statement. `rumpe`
        // and `perge` transfer to an enclosing breakable scope, not the
        // function, so they do not discharge the return contract here.
        _ => false,
    }
}

fn expr_terminates(
    expr: &HirExpression,
    types: &TypeTable,
    enum_variants: &FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
) -> bool {
    match &expr.kind {
        // Recoverable throw and unrecoverable panic diverge the current path.
        HirExpressionKind::Throw(_) | HirExpressionKind::Panic(_) => true,
        // A conditional terminates only when both branches are present and both
        // terminate; the absence of an `else` leaves a falling-through path.
        HirExpressionKind::Si {
            then_block,
            else_block,
            ..
        } => else_block.as_ref().is_some_and(|els| {
            block_terminates(then_block, types, enum_variants)
                && block_terminates(els, types, enum_variants)
        }),
        // A dispatch terminates iff coverage is total and every reachable arm
        // body terminates. Arms after a catchall are unreachable.
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            discerne_definitely_covered(scrutinees, arms, types, enum_variants)
                && reachable_arms_all_terminate(arms, types, enum_variants)
        }
        // A bare infinite loop diverges only when no `rumpe` can escape it.
        // `dum`/`itera` always fall through because their bodies may run zero
        // times, so they are excluded here — except `dum verum`, whose constant
        // true condition makes the loop provably infinite when no `rumpe`
        // escapes it. The do-while spelling `fac { … } dum cond` lowers to a
        // `Loop` block, and mirrors the `Dum` arm through that desugaring: for
        // a literal-true test the synthesized `si ¬cond { rumpe }` tail is
        // unreachable, so it must not count as an escape. A non-literal-true
        // condition falls through exactly like `Dum`.
        HirExpressionKind::Loop(body) => match body.fac_dum_parts() {
            Some((stmts, _test, cond)) => {
                is_literal_true(cond)
                    && stmts.iter().all(|stmt| !contains_rumpe_stmt(stmt))
                    && body
                        .expr
                        .as_ref()
                        .is_none_or(|expr| !contains_rumpe_expr(expr))
            }
            None => !contains_rumpe_block(body),
        },
        HirExpressionKind::Dum(cond, body) => is_literal_true(cond) && !contains_rumpe_block(body),
        // `Handled` (`fac { … } cape err { … }`) terminates when both the body
        // and the catch terminate: the body may throw into the catch, and the
        // catch's value becomes the expression's value when it does.
        HirExpressionKind::Handled { body, catch } => {
            block_terminates(body, types, enum_variants)
                && block_terminates(&catch.body, types, enum_variants)
        }
        // A nested block expression terminates iff its inner block does.
        HirExpressionKind::Block(inner) => block_terminates(inner, types, enum_variants),
        _ => false,
    }
}

/// Whether every arm up to and including the first catchall terminates.
///
/// Arms after a catchall are unreachable and ignored.
fn reachable_arms_all_terminate(
    arms: &[radix_hir::HirCasuArm],
    types: &TypeTable,
    enum_variants: &FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
) -> bool {
    for arm in arms {
        if !expr_terminates(&arm.body, types, enum_variants) {
            return false;
        }
        // A catchall shadows the arms after it; stop once reached.
        if arm.patterns.iter().any(is_catchall_pattern) {
            break;
        }
    }
    true
}

fn is_catchall_pattern(pattern: &radix_hir::HirPattern) -> bool {
    use radix_hir::HirPattern;
    match pattern {
        HirPattern::Wildcard | HirPattern::Binding(_, _) => true,
        HirPattern::Alias(_, _, inner) => is_catchall_pattern(inner),
        HirPattern::Or(alternatives) => alternatives.iter().any(is_catchall_pattern),
        HirPattern::Variant(_, _)
        | HirPattern::TypeTest { .. }
        | HirPattern::Literal(_)
        | HirPattern::NegatedLiteral(_) => false,
    }
}

/// Syntactic search for a `rumpe` anywhere in a block's subtree.
///
/// Conservative: a `rumpe` bound to a nested breakable scope (an inner `dum`
/// or `itera`) is still counted as escaping the outer `loop`, so the loop is
/// treated as fall-through. This is sound — it only refuses to prove
/// divergence — at the cost of precision for nested breakable blocks.
fn contains_rumpe_block(block: &HirBlock) -> bool {
    block.statements.iter().any(contains_rumpe_stmt)
        || block.expr.as_ref().is_some_and(|e| contains_rumpe_expr(e))
}

fn contains_rumpe_stmt(stmt: &HirStatement) -> bool {
    match &stmt.kind {
        HirStatementKind::Rumpe => true,
        HirStatementKind::Perge
        | HirStatementKind::Tacet
        | HirStatementKind::Redde(_)
        | HirStatementKind::ReddeFilum(_) => false,
        HirStatementKind::Expr(expr) => contains_rumpe_expr(expr),
        HirStatementKind::Local(local) => local.init.as_ref().is_some_and(contains_rumpe_expr),
        HirStatementKind::IncDec(inc) => contains_rumpe_expr(&inc.target),
        HirStatementKind::Custodi(custodi) => custodi
            .clauses
            .iter()
            .any(|clause| contains_rumpe_expr(&clause.cond) || contains_rumpe_block(&clause.body)),
    }
}

fn contains_rumpe_expr(expr: &HirExpression) -> bool {
    use radix_hir::HirExpressionKind;
    // `rumpe` only appears as a statement, so an expression subtree contains one
    // only via nested blocks (`si`/`discerne`/`loop`/`handled`/`block` arms and
    // bodies). This walks those structural children without descending into
    // unrelated subexpressions that cannot legally host a statement.
    match &expr.kind {
        HirExpressionKind::Si {
            then_block,
            then_catch,
            else_block,
            ..
        } => {
            contains_rumpe_block(then_block)
                || else_block.as_ref().is_some_and(contains_rumpe_block)
                || then_catch
                    .as_ref()
                    .is_some_and(|c| contains_rumpe_block(&c.body))
        }
        HirExpressionKind::Discerne { arms, .. } => {
            arms.iter().any(|arm| contains_rumpe_expr(&arm.body))
        }
        HirExpressionKind::Loop(body)
        | HirExpressionKind::Dum(_, body)
        | HirExpressionKind::Itera { body, .. } => contains_rumpe_block(body),
        HirExpressionKind::Handled { body, catch } => {
            contains_rumpe_block(body) || contains_rumpe_block(&catch.body)
        }
        HirExpressionKind::Block(inner) => contains_rumpe_block(inner),
        // F-078: closures can host statements (including `rumpe`) in their body.
        HirExpressionKind::Clausura(_, _, _, body) => contains_rumpe_expr(body),
        _ => false,
    }
}

/// Whether a declared return type is effect-only and owes no value.
///
/// `vacuum` is the explicit unit type. `nihil` (the null value) is also
/// treated as a unit return by the primary backend: a `→ nihil` function
/// lowers to `fn … -> ()` and is the Faber idiom for a side-effect function
/// whose result is implicitly null, so it owes no `redde`. Any other type
/// means the function declares a value return and every path must `redde`.
fn is_unit_return(ty: crate::TypeId, types: &TypeTable) -> bool {
    matches!(
        types.get(ty),
        crate::Type::Primitive(crate::Primitive::Vacuum | crate::Primitive::Nihil)
    )
}

/// Whether an expression is the literal boolean `verum` (true), used to prove
/// that a `dum verum { … }` loop never exits through its condition.
fn is_literal_true(expr: &HirExpression) -> bool {
    matches!(
        &expr.kind,
        HirExpressionKind::Literal(radix_hir::HirLiteral::Bool(true))
    )
}
