//! Shared traversal helpers for the Faber AST.
//!
//! Visitors separate "what to do at a node" from the recursive mechanics of
//! descending through syntax.
//!
//! This module is intentionally conservative: the walkers visit the child
//! expressions, statements, identifiers, and type syntax that generic visitors
//! need today, not every field of every node. A phase that relies on
//! exhaustive semantics must audit the AST variants it needs instead of
//! assuming these helpers encode every language rule.
//!
//! INVARIANTS
//! ==========
//! - Visitor callbacks observe AST nodes in source order where the walker covers
//!   that node family.
//! - Calling a `walk_*` function continues traversal after custom visitor logic.
//! - Traversal has no semantic side effects beyond what the visitor performs.
//! - Walkers enforce [`MAX_VISIT_DEPTH`] fail-closed: deeper nesting reports
//!   [`DepthExceeded`] through [`Visitor::on_depth_exceeded`] and stops
//!   descending instead of overflowing the stack. Depth is tracked inside the
//!   walk functions so existing `Visitor` overrides that call `walk_*` keep
//!   working without signature changes.

use std::cell::Cell;

use super::ast::{
    ArrayElement, BindingPattern, BlockStmt, CapeClause, ClassMemberKind, ClausuraBody,
    ConversioTarget, Expr, ExprKind, FacStmt, FiguraExpr, FuncDecl, GenericArgExpr,
    GenericParamDefault, Ident, IfBody, ImportKind, LiftExtent, NonNullKind, OptionalChainKind,
    Pattern, PatternBind, Program, RangeBoundExpr, SecusClause, Stmt, StmtKind, TypeExpr,
    TypeExprKind, TypeParam,
};

/// Maximum AST nesting depth before the walker refuses to descend further.
///
/// Chosen so a `visit_*` → `walk_*` pair (two frames per node) on a debug
/// build stays safely under a typical ~2 MiB thread stack.
pub const MAX_VISIT_DEPTH: usize = 1024;

/// Depth-limit diagnostic handed to [`Visitor::on_depth_exceeded`].
///
/// A walker that reports this stops descending, so the reported node's subtree
/// is never entered. A visitor whose answer feeds compilation must treat the
/// report as an incomplete walk: the missing subtree is unknown, not absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthExceeded {
    /// Depth the walker refused to enter (one past [`MAX_VISIT_DEPTH`]).
    pub depth: usize,

    /// Depth limit that stopped the walk.
    pub limit: usize,
}

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
            visitor.on_depth_exceeded(DepthExceeded {
                depth,
                limit: MAX_VISIT_DEPTH,
            });
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
    /// The walker stops descending immediately after this call — the subtree at
    /// the reported depth is never entered — so the default is a clean
    /// fail-closed stop rather than a panic, and a deep AST can never abort the
    /// process from this hook. Override to carry the failure to the caller
    /// (see [`DepthExceeded`]); a visitor on a compile path must do so, because
    /// an unreported stop reads as "nothing found" in the pruned subtree.
    fn on_depth_exceeded(&mut self, _exceeded: DepthExceeded) {}

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

    /// Identifier occurrence or binding. Default is a no-op; identifiers are
    /// leaves.
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
        StmtKind::SizeDecl(decl) => {
            visitor.visit_ident(&decl.name);
            visitor.visit_expr(&decl.init);
        }
        StmtKind::Func(decl) => walk_func_decl(visitor, decl),
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
                    ClassMemberKind::Method(method) => walk_func_decl(visitor, method),
                }
            }
        }
        StmtKind::Block(block) => visitor.visit_block(block),
        StmtKind::Expr(expr_stmt) => visitor.visit_expr(&expr_stmt.expr),
        StmtKind::IncDec(inc_dec) => visitor.visit_expr(&inc_dec.target),
        StmtKind::Si(if_stmt) => {
            visitor.visit_expr(&if_stmt.cond);
            walk_if_body(visitor, &if_stmt.then);
            if let Some(catch) = &if_stmt.catch {
                walk_catch(visitor, catch);
            }
            if let Some(else_) = &if_stmt.else_ {
                walk_secus_clause(visitor, else_);
            }
        }
        StmtKind::Dum(while_stmt) => {
            visitor.visit_expr(&while_stmt.cond);
            walk_if_body(visitor, &while_stmt.body);
            if let Some(catch) = &while_stmt.catch {
                walk_catch(visitor, catch);
            }
        }
        StmtKind::Itera(iter_stmt) => {
            for source in &iter_stmt.sources {
                visitor.visit_expr(source);
            }
            for coordinate in &iter_stmt.apud {
                visitor.visit_ident(coordinate);
            }
            if let Some(filum) = &iter_stmt.filum {
                visitor.visit_ident(filum);
            }
            for binding in &iter_stmt.bindings {
                walk_binding_pattern(visitor, binding);
            }
            walk_if_body(visitor, &iter_stmt.body);
            if let Some(catch) = &iter_stmt.catch {
                walk_catch(visitor, catch);
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
        StmtKind::ReddeFilum(ret) => {
            if let Some(value) = &ret.value {
                visitor.visit_expr(value);
            }
        }
        StmtKind::Proba(test) => visitor.visit_block(&test.body),
        StmtKind::Iace(throw) => visitor.visit_expr(&throw.value),
        StmtKind::Mori(panic) => visitor.visit_expr(&panic.value),
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
        StmtKind::Enum(decl) => visitor.visit_ident(&decl.name),
        StmtKind::Union(decl) => visitor.visit_ident(&decl.name),
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
            ImportKind::Selective { members } => {
                for member in members {
                    visitor.visit_ident(&member.name);
                    if let Some(alias) = &member.alias {
                        visitor.visit_ident(alias);
                    }
                }
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
                walk_catch(visitor, catch);
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
        StmtKind::Fac(stmt) => walk_fac_stmt(visitor, stmt),
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
        StmtKind::Reject(stmt) => {
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
        StmtKind::Reddet(stmt) => visitor.visit_expr(&stmt.future),
        StmtKind::Tacebit(stmt) => visitor.visit_expr(&stmt.future),
    }
}

#[allow(clippy::too_many_lines)]
pub fn walk_expr<V: Visitor>(visitor: &mut V, expr: &Expr) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match &expr.kind {
        ExprKind::Ident(ident) => visitor.visit_ident(ident),
        ExprKind::Binary(bin) => {
            visitor.visit_expr(&bin.lhs);
            visitor.visit_expr(&bin.rhs);
        }
        ExprKind::Unary(un) => visitor.visit_expr(&un.operand),
        ExprKind::Ternary(tern) => {
            visitor.visit_expr(&tern.cond);
            visitor.visit_expr(&tern.then);
            visitor.visit_expr(&tern.else_);
        }
        ExprKind::Call(call) => {
            visitor.visit_expr(&call.callee);
            for arg in &call.type_args {
                walk_generic_arg_expr(visitor, arg);
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
                    ArrayElement::Infer(_) => {}
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
                ClausuraBody::Fac(fac) => walk_fac_stmt(visitor, fac),
            }
        }
        ExprKind::Cede(cede) => visitor.visit_expr(&cede.expr),
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
            if let Some(recovery) = &conversio.recovery {
                visitor.visit_expr(recovery);
            }
        }
        ExprKind::Ad(ad) => {
            if let Some(opener) = &ad.opener {
                visitor.visit_expr(opener);
            }
        }
        ExprKind::Paren(inner) => visitor.visit_expr(inner),
        ExprKind::Construct(ty) => visitor.visit_type_expr(ty),
        ExprKind::Iuncta(iuncta) => {
            for ty in &iuncta.type_args {
                visitor.visit_type_expr(ty);
            }
            for expr in &iuncta.elements {
                visitor.visit_expr(expr);
            }
        }
        ExprKind::Reducta(reduction) => {
            visitor.visit_expr(&reduction.source);
            for coordinate in &reduction.apud {
                visitor.visit_ident(coordinate);
            }
            if let Some(group) = &reduction.group {
                visitor.visit_expr(group);
            }
            if let Some(filum) = &reduction.filum {
                visitor.visit_ident(filum);
            }
            if let Some(binding) = &reduction.binding {
                visitor.visit_ident(binding);
            }
            if let Some(body) = &reduction.body {
                visitor.visit_block(body);
            }
            if let Some(identity) = &reduction.identity {
                visitor.visit_expr(identity);
            }
        }
        ExprKind::Scan(scan) => {
            visitor.visit_expr(&scan.source);
            for coordinate in &scan.apud {
                visitor.visit_ident(coordinate);
            }
            if let Some(group) = &scan.group {
                visitor.visit_expr(group);
            }
            visitor.visit_ident(&scan.filum);
            visitor.visit_ident(&scan.binding);
            visitor.visit_block(&scan.body);
        }
        ExprKind::Sort(sort) => {
            visitor.visit_expr(&sort.source);
            if let Some(binding) = &sort.binding {
                visitor.visit_ident(binding);
            }
            if let Some(body) = &sort.body {
                visitor.visit_block(body);
            }
        }
        ExprKind::Vacua(_)
        | ExprKind::KeywordLiteral(_)
        | ExprKind::Literal(_)
        | ExprKind::Lege(_)
        | ExprKind::Ego(_)
        | ExprKind::Insere(_) => {}
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
        ExprKind::Praefixum(praefixum) => visitor.visit_block(&praefixum.body),
        ExprKind::TypeCheck(type_check) => {
            visitor.visit_expr(&type_check.expr);
            visitor.visit_type_expr(&type_check.ty);
        }
        ExprKind::CopyInto(copy) => {
            visitor.visit_expr(&copy.target);
            visitor.visit_expr(&copy.value);
        }
        ExprKind::SinkInto(sink) => {
            visitor.visit_expr(&sink.sink);
            visitor.visit_expr(&sink.item);
        }
        ExprKind::Trap(trap) => visitor.visit_block(&trap.body),
        ExprKind::CallDefault(call_default) => {
            visitor.visit_expr(&call_default.call);
            visitor.visit_expr(&call_default.default);
        }
        ExprKind::Format(format) => visitor.visit_expr(&format.value),
    }
}

fn walk_binding_pattern<V: Visitor>(visitor: &mut V, pattern: &BindingPattern) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match pattern {
        BindingPattern::Ident(ident) => visitor.visit_ident(ident),
        BindingPattern::Wildcard(_) => {}
        BindingPattern::Array {
            elements, is_rest, ..
        } => {
            for element in elements {
                walk_binding_pattern(visitor, element);
            }
            if let Some(rest) = is_rest {
                visitor.visit_ident(rest);
            }
        }
        BindingPattern::Object {
            fields, is_rest, ..
        } => {
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
        // All three holes are leaves, so one empty arm keeps
        // clippy::match_same_arms quiet.
        TypeExprKind::Infer | TypeExprKind::UnionHole | TypeExprKind::FieldListHole => {}
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
                walk_generic_arg_expr(visitor, param);
            }
        }
        TypeExprKind::Qualified(path, params) => {
            for segment in path {
                visitor.visit_ident(segment);
            }
            for param in params {
                walk_generic_arg_expr(visitor, param);
            }
        }
        TypeExprKind::Func(func) => {
            for param in &func.params {
                visitor.visit_type_expr(param);
            }
            visitor.visit_type_expr(&func.ret);
            if let Some(err) = &func.err {
                visitor.visit_type_expr(err);
            }
        }
        // Union and Intersection share one walk: both are flat member lists.
        TypeExprKind::Union(members) | TypeExprKind::Intersection(members) => {
            for m in members {
                visitor.visit_type_expr(m);
            }
        }
    }
}

fn walk_generic_arg_expr<V: Visitor>(visitor: &mut V, arg: &GenericArgExpr) {
    match arg {
        GenericArgExpr::Type(ty) => visitor.visit_type_expr(ty),
        GenericArgExpr::Size(figura) => walk_figura_expr(visitor, figura),
        GenericArgExpr::Name(ident) => visitor.visit_ident(ident),
        GenericArgExpr::Range(range) => {
            for bound in [&range.lo, &range.hi] {
                if let RangeBoundExpr::Name { name, .. } = bound {
                    visitor.visit_ident(name);
                }
            }
        }
    }
}

fn walk_figura_expr<V: Visitor>(visitor: &mut V, figura: &FiguraExpr) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match figura {
        FiguraExpr::Infer | FiguraExpr::Literal(_) => {}
        FiguraExpr::Var(ident) => visitor.visit_ident(ident),
        FiguraExpr::Add(value, _)
        | FiguraExpr::Subtract(value, _)
        | FiguraExpr::Divide(value, _) => walk_figura_expr(visitor, value),
        FiguraExpr::Lift(extent) => match &**extent {
            LiftExtent::Literal(_) | LiftExtent::Negative { .. } => {}
            LiftExtent::Name(ident) => visitor.visit_ident(ident),
            LiftExtent::Paren(term) => walk_figura_expr(visitor, term),
            LiftExtent::Shape(dims) => {
                for dim in dims {
                    walk_figura_expr(visitor, dim);
                }
            }
        },
        FiguraExpr::Tuple(dims) => {
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

/// Walk a function declaration's name, generic parameters, parameter types,
/// names, aliases and defaults, return and error annotations, and body. Shared
/// by `functio` statements and `genus` methods: [`ClassMemberKind::Method`]
/// stores the same [`FuncDecl`].
fn walk_func_decl<V: Visitor>(visitor: &mut V, decl: &FuncDecl) {
    visitor.visit_ident(&decl.name);
    for type_param in &decl.type_params {
        walk_type_param(visitor, type_param);
    }
    for param in &decl.params {
        visitor.visit_type_expr(&param.ty);
        visitor.visit_ident(&param.name);
        if let Some(alias) = &param.alias {
            visitor.visit_ident(alias);
        }
        if let Some(default) = &param.default {
            visitor.visit_expr(default);
        }
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

/// Walk one generic parameter's name, `=` default type, and `implet` bounds.
/// A `magnitudo` default is a bare literal, so it has no child to descend into.
fn walk_type_param<V: Visitor>(visitor: &mut V, type_param: &TypeParam) {
    visitor.visit_ident(&type_param.name);
    if let Some(GenericParamDefault::Typus(ty)) = &type_param.default {
        visitor.visit_type_expr(ty);
    }
    for bound in &type_param.bounds {
        visitor.visit_ident(&bound.name);
        for arg in &bound.args {
            visitor.visit_type_expr(arg);
        }
    }
}

/// Visit a `cape` clause's binding identifier and body block.
fn walk_catch<V: Visitor>(visitor: &mut V, catch: &CapeClause) {
    visitor.visit_ident(&catch.binding);
    visitor.visit_block(&catch.body);
}

/// Walk a `fac` block: body, optional `cape` clause, and the optional postfix
/// `dum` condition. Shared by the statement form and [`ClausuraBody::Fac`],
/// which stores the same [`FacStmt`].
fn walk_fac_stmt<V: Visitor>(visitor: &mut V, fac: &FacStmt) {
    visitor.visit_block(&fac.body);
    if let Some(catch) = &fac.catch {
        walk_catch(visitor, catch);
    }
    if let Some(while_) = &fac.while_ {
        visitor.visit_expr(while_);
    }
}

/// Walk a `secus` chain: its statements, catches, and nested `si`/`sin`
/// conditionals.
fn walk_secus_clause<V: Visitor>(visitor: &mut V, clause: &SecusClause) {
    match clause {
        SecusClause::Sin { stmt, .. } => {
            visitor.visit_expr(&stmt.cond);
            walk_if_body(visitor, &stmt.then);
            if let Some(catch) = &stmt.catch {
                walk_catch(visitor, catch);
            }
            if let Some(else_) = &stmt.else_ {
                walk_secus_clause(visitor, else_);
            }
        }
        SecusClause::Block { body, catch, .. } => {
            visitor.visit_block(body);
            if let Some(catch) = catch {
                walk_catch(visitor, catch);
            }
        }
        SecusClause::Stmt { stmt, catch, .. } => {
            visitor.visit_stmt(stmt);
            if let Some(catch) = catch {
                walk_catch(visitor, catch);
            }
        }
    }
}

/// Walk a `discerne` pattern: identifiers, type checks, and bindings.
fn walk_pattern<V: Visitor>(visitor: &mut V, pattern: &Pattern) {
    let Some(_frame) = DepthFrame::enter(visitor) else {
        return;
    };
    match pattern {
        Pattern::Wildcard(_) | Pattern::Literal(_, _) | Pattern::NegatedLiteral(_, _) => {}
        Pattern::Or(alternatives) => {
            for alternative in alternatives {
                walk_pattern(visitor, alternative);
            }
        }
        Pattern::Ident(ident, bind) => {
            visitor.visit_ident(ident);
            if let Some(bind) = bind {
                walk_pattern_bind(visitor, bind);
            }
        }
        Pattern::Type(ty, bind) => {
            visitor.visit_type_expr(ty);
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
#[path = "visit_test.rs"]
mod tests;
