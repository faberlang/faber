//! Shared traversal helpers for the Faber AST.
//!
//! Visitors separate "what to do at a node" from the recursive mechanics of
//! descending through syntax. That is useful for light analyses, diagnostics,
//! indexing, and tooling that need source-shaped traversal without owning a
//! full semantic pass.
//!
//! This module is intentionally conservative: the walkers visit the high-value
//! child expressions, statements, identifiers, and type syntax currently needed
//! by generic visitors. A compiler phase that relies on exhaustive semantics
//! should audit the relevant AST variants instead of assuming these helpers
//! encode every language rule.
//!
//! INVARIANTS
//! ==========
//! - Visitor callbacks observe AST nodes in source order where the walker covers
//!   that node family.
//! - Calling a `walk_*` function continues traversal after custom visitor logic.
//! - Traversal has no semantic side effects beyond what the visitor performs.
//! - Walkers enforce [`MAX_VISIT_DEPTH`] fail-closed: deeper nesting calls
//!   [`Visitor::on_depth_exceeded`] (default: panic) instead of overflowing the
//!   stack. Depth is tracked inside the walk functions so existing `Visitor`
//!   overrides that call `walk_*` keep working without signature changes.

use std::cell::Cell;

use super::ast::{
    ArrayElement, BindingPattern, BlockStmt, ClassMemberKind, ClausuraBody, ConversioTarget, Expr,
    ExprKind, Ident, IfBody, ImportKind, NonNullKind, OptionalChainKind, Pattern, PatternBind,
    PraefixumBody, Program, SecusClause, Stmt, StmtKind, TypeExpr, TypeExprKind,
};

/// Maximum AST nesting depth before the walker refuses to descend further.
///
/// Chosen so a `visit_*` → `walk_*` pair (two frames per node) on a debug
/// build stays safely under a typical ~2 MiB thread stack. Realistic Faber
/// ASTs are far shallower.
pub const MAX_VISIT_DEPTH: usize = 1024;

thread_local! {
    /// Nesting depth of the active `walk_*` chain on this thread.
    ///
    /// WHY thread-local: depth must propagate through default `visit_*` methods
    /// that call `walk_*` without an extra parameter, keeping existing
    /// `Visitor` impls source-compatible.
    static VISIT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// RAII frame that increments visit depth on construction and restores on drop.
struct DepthFrame;

impl DepthFrame {
    /// Enter one nesting level, or invoke the depth-exceeded hook and return
    /// `None` without pushing when the limit is already past.
    fn enter<V: Visitor>(visitor: &mut V) -> Option<Self> {
        let depth = VISIT_DEPTH.with(Cell::get);
        if depth > MAX_VISIT_DEPTH {
            visitor.on_depth_exceeded(depth);
            return None;
        }
        VISIT_DEPTH.with(|cell| cell.set(depth + 1));
        Some(Self)
    }
}

impl Drop for DepthFrame {
    fn drop(&mut self) {
        VISIT_DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
    }
}

// ---------------------------------------------------------------------------
// Visitor
// ---------------------------------------------------------------------------

/// Callback surface for source-shaped AST traversal.
///
/// Implementers override the callbacks they care about and usually call the
/// matching `walk_*` helper to keep descending. Omitting the helper is a valid
/// way to prune traversal for a subtree.
pub trait Visitor: Sized {
    /// Called when a walker would nest deeper than [`MAX_VISIT_DEPTH`].
    ///
    /// Default: panic with a diagnostic. Override to fail closed without
    /// unwinding (e.g. record a diagnostic and stop descending).
    fn on_depth_exceeded(&mut self, depth: usize) {
        panic!("AST visitor exceeded maximum depth of {MAX_VISIT_DEPTH} at node depth {depth}");
    }

    fn visit_program(&mut self, program: &Program) {
        walk_program(self, program);
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr);
    }

    fn visit_type_expr(&mut self, ty: &TypeExpr) {
        walk_type_expr(self, ty);
    }

    /// Identifier occurrence or binding selected by a walker. Default is a no-op
    /// because identifiers are leaves.
    fn visit_ident(&mut self, _ident: &Ident) {}

    fn visit_block(&mut self, block: &BlockStmt) {
        walk_block(self, block);
    }
}

// ---------------------------------------------------------------------------
// Walk helpers
// ---------------------------------------------------------------------------
// Public so a visitor can run custom logic at a node, then resume this policy.

pub fn walk_program<V: Visitor>(visitor: &mut V, program: &Program) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    for stmt in &program.statements {
        visitor.visit_stmt(stmt);
    }
}

#[allow(clippy::too_many_lines)]
pub fn walk_stmt<V: Visitor>(visitor: &mut V, stmt: &Stmt) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match &stmt.kind {
        StmtKind::Var(decl) => {
            if let Some(ty) = &decl.ty {
                visitor.visit_type_expr(ty);
            }
            walk_binding_pattern(visitor, &decl.binding);
            if let Some(init) = &decl.init {
                visitor.visit_expr(init);
            }
        }
        StmtKind::Func(decl) => {
            visitor.visit_ident(&decl.name);
            for param in &decl.params {
                visitor.visit_type_expr(&param.ty);
                visitor.visit_ident(&param.name);
            }
            if let Some(ret) = &decl.ret {
                visitor.visit_type_expr(ret);
            }
            if let Some(err) = &decl.err {
                visitor.visit_type_expr(err);
            }
            if let Some(body) = &decl.body {
                visitor.visit_block(body);
            }
        }
        StmtKind::Class(decl) => {
            visitor.visit_ident(&decl.name);
            for member in &decl.members {
                match &member.kind {
                    ClassMemberKind::Field(field) => {
                        visitor.visit_type_expr(&field.ty);
                        visitor.visit_ident(&field.name);
                        if let Some(init) = &field.init {
                            visitor.visit_expr(init);
                        }
                    }
                    ClassMemberKind::Method(method) => {
                        visitor.visit_ident(&method.name);
                        for param in &method.params {
                            visitor.visit_type_expr(&param.ty);
                            visitor.visit_ident(&param.name);
                        }
                        if let Some(ret) = &method.ret {
                            visitor.visit_type_expr(ret);
                        }
                        if let Some(err) = &method.err {
                            visitor.visit_type_expr(err);
                        }
                        if let Some(body) = &method.body {
                            visitor.visit_block(body);
                        }
                    }
                }
            }
        }
        StmtKind::Block(block) => {
            visitor.visit_block(block);
        }
        StmtKind::Expr(expr_stmt) => {
            visitor.visit_expr(&expr_stmt.expr);
        }
        StmtKind::IncDec(inc_dec) => {
            visitor.visit_expr(&inc_dec.target);
        }
        StmtKind::Si(if_stmt) => {
            visitor.visit_expr(&if_stmt.cond);
            walk_if_body(visitor, &if_stmt.then);
            if let Some(catch) = &if_stmt.catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
            if let Some(else_) = &if_stmt.else_ {
                walk_secus_clause(visitor, else_);
            }
        }
        StmtKind::Dum(while_stmt) => {
            visitor.visit_expr(&while_stmt.cond);
            walk_if_body(visitor, &while_stmt.body);
            if let Some(catch) = &while_stmt.catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
        }
        StmtKind::Itera(iter_stmt) => {
            visitor.visit_expr(&iter_stmt.iterable);
            walk_binding_pattern(visitor, &iter_stmt.binding);
            walk_if_body(visitor, &iter_stmt.body);
            if let Some(catch) = &iter_stmt.catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
        }
        StmtKind::Ex(extract_stmt) => {
            visitor.visit_expr(&extract_stmt.source);
            for field in &extract_stmt.fields {
                visitor.visit_ident(&field.name);
                if let Some(alias) = &field.alias {
                    visitor.visit_ident(alias);
                }
            }
            if let Some(rest) = &extract_stmt.is_rest {
                visitor.visit_ident(rest);
            }
        }
        StmtKind::Redde(ret) => {
            if let Some(value) = &ret.value {
                visitor.visit_expr(value);
            }
        }
        StmtKind::Proba(test) => {
            visitor.visit_block(&test.body);
        }
        StmtKind::Iace(throw) => {
            visitor.visit_expr(&throw.value);
        }
        StmtKind::Mori(panic) => {
            visitor.visit_expr(&panic.value);
        }
        StmtKind::Interface(decl) => {
            visitor.visit_ident(&decl.name);
            for method in &decl.methods {
                visitor.visit_ident(&method.name);
                for param in &method.params {
                    visitor.visit_type_expr(&param.ty);
                    visitor.visit_ident(&param.name);
                }
                if let Some(ret) = &method.ret {
                    visitor.visit_type_expr(ret);
                }
                if let Some(err) = &method.err {
                    visitor.visit_type_expr(err);
                }
            }
        }
        StmtKind::TypeAlias(decl) => {
            visitor.visit_ident(&decl.name);
            visitor.visit_type_expr(&decl.ty);
        }
        StmtKind::Schema(decl) => {
            visitor.visit_ident(&decl.name);
            for column in &decl.columns {
                visitor.visit_type_expr(&column.ty);
                visitor.visit_ident(&column.name);
                if let Some(alias) = &column.source_alias {
                    visitor.visit_ident(alias);
                }
            }
        }
        StmtKind::Enum(decl) => {
            visitor.visit_ident(&decl.name);
        }
        StmtKind::Union(decl) => {
            visitor.visit_ident(&decl.name);
        }
        StmtKind::Import(decl) => match &decl.kind {
            ImportKind::Named { name, alias, .. } => {
                visitor.visit_ident(name);
                if let Some(alias) = alias {
                    visitor.visit_ident(alias);
                }
            }
            ImportKind::Wildcard { alias } => {
                visitor.visit_ident(alias);
            }
        },
        StmtKind::Probandum(decl) => {
            for setup in &decl.body.setup {
                visitor.visit_block(&setup.body);
            }
            for test in &decl.body.tests {
                visitor.visit_block(&test.body);
            }
            for nested in &decl.body.nested {
                for test in &nested.body.tests {
                    visitor.visit_block(&test.body);
                }
            }
        }
        StmtKind::Elige(stmt) => {
            visitor.visit_expr(&stmt.expr);
            for case in &stmt.cases {
                visitor.visit_expr(&case.value);
                walk_if_body(visitor, &case.body);
            }
            if let Some(default) = &stmt.default {
                walk_if_body(visitor, &default.body);
            }
            if let Some(catch) = &stmt.catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
        }
        StmtKind::Discerne(stmt) => {
            for subject in &stmt.subjects {
                visitor.visit_expr(subject);
            }
            for arm in &stmt.arms {
                for pattern in &arm.patterns {
                    walk_pattern(visitor, pattern);
                }
                walk_if_body(visitor, &arm.body);
            }
            if let Some(default) = &stmt.default {
                walk_if_body(visitor, &default.body);
            }
        }
        StmtKind::Custodi(stmt) => {
            for clause in &stmt.clauses {
                visitor.visit_expr(&clause.cond);
                walk_if_body(visitor, &clause.body);
            }
        }
        StmtKind::Fac(stmt) => {
            visitor.visit_block(&stmt.body);
            if let Some(catch) = &stmt.catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
            if let Some(while_) = &stmt.while_ {
                visitor.visit_expr(while_);
            }
        }
        StmtKind::Rumpe(_) | StmtKind::Perge(_) | StmtKind::Tacet(_) => {}
        StmtKind::Adfirma(stmt) => {
            visitor.visit_expr(&stmt.cond);
            if let Some(msg) = &stmt.message {
                visitor.visit_expr(msg);
            }
        }
        StmtKind::Require(stmt) => {
            visitor.visit_expr(&stmt.cond);
            visitor.visit_expr(&stmt.error);
        }
        StmtKind::Scribe(stmt) => {
            for arg in &stmt.args {
                visitor.visit_expr(arg);
            }
        }
        StmtKind::Incipit(stmt) => {
            walk_if_body(visitor, &stmt.body);
            if let Some(args) = &stmt.args {
                visitor.visit_ident(args);
            }
            if let Some(exitus) = &stmt.exitus {
                visitor.visit_expr(exitus);
            }
        }
        StmtKind::Cura(stmt) => {
            visitor.visit_type_expr(&stmt.ty);
            visitor.visit_ident(&stmt.binding);
            visitor.visit_block(&stmt.body);
            if let Some(catch) = &stmt.catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
        }
        StmtKind::Reddet(stmt) => {
            visitor.visit_expr(&stmt.future);
        }
        StmtKind::Tacebit(stmt) => {
            visitor.visit_expr(&stmt.future);
        }
    }
}

#[allow(clippy::too_many_lines)]
pub fn walk_expr<V: Visitor>(visitor: &mut V, expr: &Expr) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match &expr.kind {
        ExprKind::Ident(ident) => {
            visitor.visit_ident(ident);
        }
        ExprKind::Binary(bin) => {
            visitor.visit_expr(&bin.lhs);
            visitor.visit_expr(&bin.rhs);
        }
        ExprKind::Unary(un) => {
            visitor.visit_expr(&un.operand);
        }
        ExprKind::Ternary(tern) => {
            visitor.visit_expr(&tern.cond);
            visitor.visit_expr(&tern.then);
            visitor.visit_expr(&tern.else_);
        }
        ExprKind::Call(call) => {
            visitor.visit_expr(&call.callee);
            for ty in &call.type_args {
                visitor.visit_type_expr(ty);
            }
            for arg in &call.args {
                visitor.visit_expr(&arg.value);
            }
        }
        ExprKind::Member(member) => {
            visitor.visit_expr(&member.object);
            visitor.visit_ident(&member.member);
        }
        ExprKind::Index(index) => {
            visitor.visit_expr(&index.object);
            visitor.visit_expr(&index.index);
        }
        ExprKind::Gradient(gradient) => {
            visitor.visit_expr(&gradient.call);
            for selector in &gradient.selectors {
                visitor.visit_expr(selector);
            }
        }
        ExprKind::Assign(assign) => {
            visitor.visit_expr(&assign.target);
            visitor.visit_expr(&assign.value);
        }
        ExprKind::Array(arr) => {
            for elem in &arr.elements {
                match elem {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        visitor.visit_expr(e);
                    }
                }
            }
        }
        ExprKind::Object(obj) => {
            for field in &obj.fields {
                if let Some(value) = &field.value {
                    visitor.visit_expr(value);
                }
            }
        }
        ExprKind::Clausura(closure) => {
            for param in &closure.params {
                visitor.visit_type_expr(&param.ty);
            }
            if let Some(ret) = &closure.ret {
                visitor.visit_type_expr(ret);
            }
            if let Some(err) = &closure.err {
                visitor.visit_type_expr(err);
            }
            match &closure.body {
                ClausuraBody::Expr(e) => visitor.visit_expr(e),
                ClausuraBody::Block(b) => visitor.visit_block(b),
                ClausuraBody::Fac(f) => {
                    visitor.visit_block(&f.body);
                    if let Some(catch) = &f.catch {
                        visitor.visit_block(&catch.body);
                    }
                    if let Some(while_) = &f.while_ {
                        visitor.visit_expr(while_);
                    }
                }
            }
        }
        ExprKind::Cede(cede) => {
            visitor.visit_expr(&cede.expr);
        }
        ExprKind::Verte(verte) => {
            visitor.visit_expr(&verte.expr);
            visitor.visit_type_expr(&verte.ty);
        }
        ExprKind::Conversio(conversio) => {
            visitor.visit_expr(&conversio.expr);
            match &conversio.target {
                ConversioTarget::Explicit(ty) => visitor.visit_type_expr(ty),
                ConversioTarget::Intervallum(operand) => visitor.visit_expr(operand),
            }
            for ty in &conversio.type_params {
                visitor.visit_type_expr(ty);
            }
            if let Some(recovery) = &conversio.recovery {
                visitor.visit_expr(recovery);
            }
        }
        ExprKind::Ad(ad) => {
            if let Some(opener) = &ad.opener {
                visitor.visit_expr(opener);
            }
        }
        ExprKind::Paren(inner) => {
            visitor.visit_expr(inner);
        }
        ExprKind::Iuncta(iuncta) => {
            for ty in &iuncta.type_args {
                visitor.visit_type_expr(ty);
            }
            for expr in &iuncta.elements {
                visitor.visit_expr(expr);
            }
        }
        ExprKind::Vacua(_) | ExprKind::Literal(_) | ExprKind::Lege(_) | ExprKind::Ego(_) => {}
        ExprKind::OptionalChain(opt) => {
            visitor.visit_expr(&opt.object);
            match &opt.chain {
                OptionalChainKind::Member(ident) => visitor.visit_ident(ident),
                OptionalChainKind::Index(index) => visitor.visit_expr(index),
                OptionalChainKind::Call(args) => {
                    for arg in args {
                        visitor.visit_expr(&arg.value);
                    }
                }
            }
        }
        ExprKind::NonNull(non_null) => {
            visitor.visit_expr(&non_null.object);
            match &non_null.chain {
                NonNullKind::Member(ident) => visitor.visit_ident(ident),
                NonNullKind::Index(index) => visitor.visit_expr(index),
                NonNullKind::Call(args) => {
                    for arg in args {
                        visitor.visit_expr(&arg.value);
                    }
                }
            }
        }
        ExprKind::Finge(finge) => {
            for segment in &finge.variant {
                visitor.visit_ident(segment);
            }
            for field in &finge.fields {
                visitor.visit_expr(&field.value);
            }
            if let Some(cast) = &finge.cast {
                visitor.visit_type_expr(cast);
            }
        }
        ExprKind::Intervallum(intervallum) => {
            visitor.visit_expr(&intervallum.start);
            visitor.visit_expr(&intervallum.end);
            if let Some(step) = &intervallum.step {
                visitor.visit_expr(step);
            }
        }
        ExprKind::Scriptum(scriptum) => {
            for arg in &scriptum.args {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::Praefixum(praefixum) => match &praefixum.body {
            PraefixumBody::Block(b) => visitor.visit_block(b),
            PraefixumBody::Expr(e) => visitor.visit_expr(e),
        },
        ExprKind::TypeCheck(type_check) => {
            visitor.visit_expr(&type_check.expr);
            visitor.visit_type_expr(&type_check.ty);
        }
    }
}

fn walk_binding_pattern<V: Visitor>(visitor: &mut V, pattern: &BindingPattern) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match pattern {
        BindingPattern::Ident(ident) => visitor.visit_ident(ident),
        BindingPattern::Wildcard(_) => {}
        BindingPattern::Array { elements, is_rest, .. } => {
            for element in elements {
                walk_binding_pattern(visitor, element);
            }
            if let Some(rest) = is_rest {
                visitor.visit_ident(rest);
            }
        }
        BindingPattern::Object { fields, is_rest, .. } => {
            for field in fields {
                visitor.visit_ident(field.alias.as_ref().unwrap_or(&field.name));
            }
            if let Some(rest) = is_rest {
                visitor.visit_ident(rest);
            }
        }
    }
}

pub fn walk_type_expr<V: Visitor>(visitor: &mut V, ty: &TypeExpr) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match &ty.kind {
        // Both holes are leaves with no nested type syntax to walk. Kept in
        // one arm: the shared empty body is intentional (clippy::match_same_arms).
        TypeExprKind::Infer | TypeExprKind::UnionHole => {}
        TypeExprKind::Tensor(elem, figura)
        | TypeExprKind::Sparsa(elem, figura)
        | TypeExprKind::Vector(elem, figura)
        | TypeExprKind::Matrix(elem, figura) => {
            visitor.visit_type_expr(elem);
            walk_figura_expr(visitor, figura);
        }
        TypeExprKind::Atomic(elem) => visitor.visit_type_expr(elem),
        TypeExprKind::PromissumFailable(success, alternate) => {
            visitor.visit_type_expr(success);
            visitor.visit_type_expr(alternate);
        }
        TypeExprKind::Named(name, params) => {
            visitor.visit_ident(name);
            for param in params {
                visitor.visit_type_expr(param);
            }
        }
        TypeExprKind::Qualified(path, params) => {
            for segment in path {
                visitor.visit_ident(segment);
            }
            for param in params {
                visitor.visit_type_expr(param);
            }
        }
        TypeExprKind::Func(func) => {
            for param in &func.params {
                visitor.visit_type_expr(param);
            }
            visitor.visit_type_expr(&func.ret);
        }
        TypeExprKind::Union(members) => {
            for m in members {
                visitor.visit_type_expr(m);
            }
        }
    }
}

fn walk_figura_expr<V: Visitor>(visitor: &mut V, figura: &crate::FiguraExpr) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match figura {
        crate::FiguraExpr::Infer | crate::FiguraExpr::Literal(_) => {}
        crate::FiguraExpr::Var(ident) => visitor.visit_ident(ident),
        crate::FiguraExpr::Tuple(dims) => {
            for dim in dims {
                walk_figura_expr(visitor, dim);
            }
        }
    }
}

pub fn walk_block<V: Visitor>(visitor: &mut V, block: &BlockStmt) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    for stmt in &block.statements {
        visitor.visit_stmt(stmt);
    }
}

/// `IfBody::Ergo` is the `si cond ergo stmt` shorthand; dispatch both shapes.
fn walk_if_body<V: Visitor>(visitor: &mut V, body: &IfBody) {
    match body {
        IfBody::Block(block) => visitor.visit_block(block),
        IfBody::Ergo(stmt) => visitor.visit_stmt(stmt),
    }
}

/// Walk a `secus` chain (an `else` subtree) so walker consumers do not miss
/// its statements, catches, and nested `si`/`sin` conditionals.
fn walk_secus_clause<V: Visitor>(visitor: &mut V, clause: &SecusClause) {
    match clause {
        SecusClause::Sin { stmt, .. } => {
            visitor.visit_expr(&stmt.cond);
            walk_if_body(visitor, &stmt.then);
            if let Some(catch) = &stmt.catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
            if let Some(else_) = &stmt.else_ {
                walk_secus_clause(visitor, else_);
            }
        }
        SecusClause::Block { body, catch, .. } => {
            visitor.visit_block(body);
            if let Some(catch) = catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
        }
        SecusClause::Stmt { stmt, catch, .. } => {
            visitor.visit_stmt(stmt);
            if let Some(catch) = catch {
                visitor.visit_ident(&catch.binding);
                visitor.visit_block(&catch.body);
            }
        }
    }
}

/// Visit identifiers in `discerne` bindings so visitors can register binding sites.
fn walk_pattern<V: Visitor>(visitor: &mut V, pattern: &Pattern) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match pattern {
        Pattern::Wildcard(_) | Pattern::Literal(_, _) => {}
        Pattern::Ident(ident, bind) => {
            visitor.visit_ident(ident);
            if let Some(bind) = bind {
                walk_pattern_bind(visitor, bind);
            }
        }
        Pattern::Path(path) => {
            for segment in &path.segments {
                visitor.visit_ident(segment);
            }
            if let Some(bind) = &path.bind {
                walk_pattern_bind(visitor, bind);
            }
        }
    }
}

fn walk_pattern_bind<V: Visitor>(visitor: &mut V, bind: &PatternBind) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match bind {
        PatternBind::Alias(ident) => visitor.visit_ident(ident),
        PatternBind::Bindings { bindings, .. } => {
            for field in bindings {
                visitor.visit_ident(&field.name);
                if let Some(alias) = &field.alias {
                    visitor.visit_ident(alias);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{walk_expr, walk_stmt, walk_type_expr, Visitor, MAX_VISIT_DEPTH};
    use crate::{AstMutability, ExField, PathPattern};
    use crate::{
        BinOp, BinaryExpr, BlockStmt, CasuArm, DiscerneStmt, Expr, ExprKind, FuncDecl, Ident,
        IfBody, Literal, Pattern, PatternBind, ProbandumBody, ProbandumDecl, Stmt, StmtKind,
        TypeExpr, TypeExprKind,
    };
    use crate::{PraeparaBlock, PraeparaKind};
    use radix_lexer::{Span, Symbol};

    /// Visitor that records depth-limit hits instead of panicking.
    struct DepthProbe {
        exceeded_at: Option<usize>,
        expr_visits: usize,
        type_visits: usize,
    }

    impl DepthProbe {
        fn new() -> Self {
            Self {
                exceeded_at: None,
                expr_visits: 0,
                type_visits: 0,
            }
        }
    }

    impl Visitor for DepthProbe {
        fn on_depth_exceeded(&mut self, depth: usize) {
            self.exceeded_at = Some(depth);
        }

        fn visit_expr(&mut self, expr: &Expr) {
            if self.exceeded_at.is_some() {
                return;
            }
            self.expr_visits += 1;
            walk_expr(self, expr);
        }

        fn visit_type_expr(&mut self, ty: &TypeExpr) {
            if self.exceeded_at.is_some() {
                return;
            }
            self.type_visits += 1;
            walk_type_expr(self, ty);
        }
    }

    fn lit_expr(id: u32) -> Expr {
        Expr {
            id,
            kind: ExprKind::Literal(Literal::Int(1)),
            span: Span::new(0, 1),
        }
    }

    /// Left-nested binary chain: depth `n` means `n` Binary nodes above a leaf.
    /// Walk depth along the left spine reaches `n` at the deepest leaf entry.
    fn deep_binary_chain(depth: usize) -> Expr {
        let mut expr = lit_expr(0);
        for i in 0..depth {
            let id = u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX);
            let rhs_id = u32::try_from(i.saturating_add(10_000)).unwrap_or(u32::MAX);
            expr = Expr {
                id,
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinOp::Add,
                    lhs: Box::new(expr),
                    rhs: Box::new(lit_expr(rhs_id)),
                }),
                span: Span::new(0, 1),
            };
        }
        expr
    }

    fn infer_type() -> TypeExpr {
        TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Infer,
            span: Span::new(0, 1),
            iuncta_label: None,
        }
    }

    /// Nested `atomic<...atomic<_>...>` chain of the given depth.
    fn deep_atomic_chain(depth: usize) -> TypeExpr {
        let mut ty = infer_type();
        for _ in 0..depth {
            ty = TypeExpr {
                nullable: false,
                mode: None,
                kind: TypeExprKind::Atomic(Box::new(ty)),
                span: Span::new(0, 1),
                iuncta_label: None,
            };
        }
        ty
    }

    #[test]
    fn shallow_expr_walk_succeeds() {
        let expr = deep_binary_chain(3);
        let mut probe = DepthProbe::new();
        probe.visit_expr(&expr);
        assert!(probe.exceeded_at.is_none());
        // 3 Binary + 1 leftmost leaf + 3 rhs leaves = 7 expression nodes
        assert_eq!(probe.expr_visits, 7);
    }

    #[test]
    fn expr_at_max_depth_succeeds() {
        // Left spine depths 0..=MAX: root Binary at 0, leaf entry at MAX.
        let expr = deep_binary_chain(MAX_VISIT_DEPTH);
        let mut probe = DepthProbe::new();
        probe.visit_expr(&expr);
        assert!(
            probe.exceeded_at.is_none(),
            "depth {MAX_VISIT_DEPTH} must be allowed, got {:?}",
            probe.exceeded_at
        );
        assert!(probe.expr_visits > MAX_VISIT_DEPTH);
    }

    #[test]
    fn expr_beyond_max_depth_fails_closed() {
        let expr = deep_binary_chain(MAX_VISIT_DEPTH + 1);
        let mut probe = DepthProbe::new();
        probe.visit_expr(&expr);
        assert_eq!(
            probe.exceeded_at,
            Some(MAX_VISIT_DEPTH + 1),
            "walker must fail closed past MAX_VISIT_DEPTH"
        );
    }

    #[test]
    fn type_expr_beyond_max_depth_fails_closed() {
        let ty = deep_atomic_chain(MAX_VISIT_DEPTH + 1);
        let mut probe = DepthProbe::new();
        probe.visit_type_expr(&ty);
        assert_eq!(probe.exceeded_at, Some(MAX_VISIT_DEPTH + 1));
    }

    #[test]
    fn type_expr_at_max_depth_succeeds() {
        let ty = deep_atomic_chain(MAX_VISIT_DEPTH);
        let mut probe = DepthProbe::new();
        probe.visit_type_expr(&ty);
        assert!(probe.exceeded_at.is_none());
        // MAX Atomic wrappers + 1 Infer leaf
        assert_eq!(probe.type_visits, MAX_VISIT_DEPTH + 1);
    }

    #[test]
    fn default_on_depth_exceeded_panics() {
        struct PanicVisitor;
        impl Visitor for PanicVisitor {}

        let expr = deep_binary_chain(MAX_VISIT_DEPTH + 1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut v = PanicVisitor;
            v.visit_expr(&expr);
        }));
        assert!(result.is_err(), "default on_depth_exceeded must panic");
    }

    fn test_ident(sym: u32) -> Ident {
        Ident {
            name: Symbol(sym),
            span: Span::new(0, 1),
        }
    }

    fn test_block() -> BlockStmt {
        BlockStmt {
            statements: vec![],
            end_trivia: Vec::new(),
            span: Span::new(0, 1),
        }
    }

    struct CountVisitor {
        idents: usize,
        types: usize,
        blocks: usize,
    }

    impl CountVisitor {
        fn new() -> Self {
            Self {
                idents: 0,
                types: 0,
                blocks: 0,
            }
        }
    }

    impl Visitor for CountVisitor {
        fn visit_ident(&mut self, _ident: &Ident) {
            self.idents += 1;
        }
        fn visit_type_expr(&mut self, ty: &TypeExpr) {
            self.types += 1;
            walk_type_expr(self, ty);
        }
        fn visit_block(&mut self, block: &BlockStmt) {
            self.blocks += 1;
            for stmt in &block.statements {
                walk_stmt(self, stmt);
            }
        }
    }

    #[test]
    fn walk_func_decl_err() {
        let func = FuncDecl {
            name: test_ident(1),
            type_params: vec![],
            params: vec![],
            modifiers: vec![],
            posture: crate::CallablePosture::SyncFinite,
            ret: None,
            err: Some(infer_type()),
            body: None,
            annotations: vec![],
            leading_trivia: vec![],
        };
        let stmt = Stmt {
            id: 0,
            kind: StmtKind::Func(func),
            span: Span::new(0, 1),
            annotations: vec![],
            leading_trivia: vec![],
        };

        let mut visitor = CountVisitor::new();
        walk_stmt(&mut visitor, &stmt);

        assert_eq!(visitor.idents, 1, "must visit func name");
        assert_eq!(visitor.types, 1, "must visit decl.err");
    }

    #[test]
    fn walk_probandum_setup() {
        let probandum = ProbandumDecl {
            name: Symbol(1),
            modifiers: vec![],
            body: ProbandumBody {
                setup: vec![PraeparaBlock {
                    kind: PraeparaKind::Praepara,
                    all: false,
                    body: test_block(),
                    span: Span::new(0, 1),
                }],
                tests: vec![],
                nested: vec![],
            },
            span: Span::new(0, 1),
        };
        let stmt = Stmt {
            id: 0,
            kind: StmtKind::Probandum(probandum),
            span: Span::new(0, 1),
            annotations: vec![],
            leading_trivia: vec![],
        };

        let mut visitor = CountVisitor::new();
        walk_stmt(&mut visitor, &stmt);

        assert_eq!(visitor.blocks, 1, "must visit setup block");
    }

    #[test]
    fn walk_discern_patterns() {
        // Ident pattern with alias bind + Path pattern with destructure bindings.
        let ident_pattern = Pattern::Ident(test_ident(1), Some(PatternBind::Alias(test_ident(2))));
        let path_pattern = Pattern::Path(PathPattern {
            segments: vec![test_ident(3), test_ident(4)],
            bind: Some(PatternBind::Bindings {
                mutability: AstMutability::Immutable,
                bindings: vec![
                    ExField {
                        name: test_ident(5),
                        alias: Some(test_ident(6)),
                    },
                    ExField {
                        name: test_ident(7),
                        alias: None,
                    },
                ],
            }),
            span: Span::new(0, 1),
        });

        let discerne = DiscerneStmt {
            exhaustive: false,
            subjects: vec![],
            arms: vec![CasuArm {
                patterns: vec![ident_pattern, path_pattern],
                body: IfBody::Block(test_block()),
                span: Span::new(0, 1),
            }],
            default: None,
        };
        let stmt = Stmt {
            id: 0,
            kind: StmtKind::Discerne(discerne),
            span: Span::new(0, 1),
            annotations: vec![],
            leading_trivia: vec![],
        };

        let mut visitor = CountVisitor::new();
        walk_stmt(&mut visitor, &stmt);

        // Expected ident visits:
        //   - Ident pattern: name=1 + alias=2           = 2
        //   - Path segments: 3 + 4                      = 2
        //   - Bindings: name=5 + alias=6 + name=7       = 3
        // Total: 7
        assert_eq!(visitor.idents, 7, "must visit all pattern idents");
    }
}
