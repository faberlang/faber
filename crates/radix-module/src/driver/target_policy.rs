//! Parsed-syntax target policy checks for the driver frontend.
//!
//! These checks run after parsing and before semantic lowering/codegen. They
//! keep backend-specific impossibilities visible as source diagnostics instead
//! of late generator errors, while leaving package discovery and backend
//! emission policy outside this module.
use crate::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase};
#[allow(clippy::wildcard_imports)]
// syntax is a pure re-export barrel from radix-syntax
use crate::syntax::*;

/// Go async-stream posture policy (`go_target_async_stream_unsupported`).
///
/// DEFINED POLICY (U3, codex-gap Stage 3): `fient` async streams are
/// **fail-closed** on Go. Go's type-level contract is already spelled
/// (`<-chan T` for `AsyncCursor`/`Promissum` in `radix-hir-go/src/types.rs`)
/// and sync `fiunt` generators materialize to `[]T`, but producing `fient`
/// streams (channel emission in generator bodies, `async_main`/`await` entry
/// lowering, provider shims for `norma:solum`/`norma:tempus` routes) is not
/// yet implemented in the Go emitter — generator bodies live in `decl.rs`
/// (`write_generator_block`) and are out of scope for the provider/async
/// carrier unit. Every `fient` declaration surface (free function, class
/// method, interface method) rejects with this diagnostic so callers never
/// see late Go compile failures. The `itera/cursor-iteratio.fab` compile-fail
/// row and the `ad/async-*` provider rows stay gated under this policy until
/// the channel carrier lands; the retention decision is recorded in the
/// codex-gap Stage 3 delivery (U3).
const GO_ASYNC_STREAM_UNSUPPORTED_MESSAGE: &str = "fient async streams are not supported for Go targets yet";
const GO_ASYNC_STREAM_UNSUPPORTED_ISSUE: &str = "go_target_async_stream_unsupported";

fn reject_go_async_stream_posture(file: &str, span: crate::lexer::Span, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(target_exception_diagnostic(
        file,
        span,
        GO_ASYNC_STREAM_UNSUPPORTED_MESSAGE,
        GO_ASYNC_STREAM_UNSUPPORTED_ISSUE,
    ));
}

#[must_use]
pub(super) fn collect_rust_unsupported_errors(program: &Program, file: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for stmt in &program.statements {
        scan_rust_stmt(stmt, file, &mut diagnostics);
    }
    diagnostics
}

#[must_use]
pub(super) fn collect_go_unsupported_errors(
    program: &Program,
    file: &str,
    _interner: &crate::lexer::Interner,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for stmt in &program.statements {
        scan_go_stmt(stmt, file, &mut diagnostics);
    }

    diagnostics
}

fn scan_go_stmt(stmt: &Stmt, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match &stmt.kind {
        StmtKind::Block(block) => scan_go_block(block, file, diagnostics),
        StmtKind::Func(func) => {
            if func.posture == CallablePosture::AsyncStream {
                reject_go_async_stream_posture(file, func.name.span, diagnostics);
            }
            if let Some(body) = &func.body {
                scan_go_block(body, file, diagnostics);
            }
        }
        StmtKind::Class(class) => {
            for member in &class.members {
                if let ClassMemberKind::Method(method) = &member.kind {
                    if method.posture == CallablePosture::AsyncStream {
                        reject_go_async_stream_posture(file, member.span, diagnostics);
                    }
                    if let Some(body) = &method.body {
                        scan_go_block(body, file, diagnostics);
                    }
                }
            }
        }
        StmtKind::Interface(interface) => {
            for method in &interface.methods {
                if method.posture == CallablePosture::AsyncStream {
                    reject_go_async_stream_posture(file, method.span, diagnostics);
                }
            }
        }
        StmtKind::Probandum(test) => scan_go_probandum(test, file, diagnostics),
        StmtKind::Proba(test) => {
            scan_go_block(&test.body, file, diagnostics);
        }
        StmtKind::Si(if_stmt) => {
            scan_go_expr(&if_stmt.cond, file, diagnostics);
            scan_go_if_body(&if_stmt.then, file, diagnostics);
            if let Some(catch) = &if_stmt.catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
            if let Some(else_) = &if_stmt.else_ {
                scan_go_else(else_, file, diagnostics);
            }
        }
        StmtKind::Dum(while_stmt) => {
            scan_go_expr(&while_stmt.cond, file, diagnostics);
            scan_go_if_body(&while_stmt.body, file, diagnostics);
            if let Some(catch) = &while_stmt.catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
        }
        StmtKind::Itera(iter_stmt) => {
            for source in &iter_stmt.sources {
                scan_go_expr(source, file, diagnostics);
            }
            scan_go_if_body(&iter_stmt.body, file, diagnostics);
            if let Some(catch) = &iter_stmt.catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
        }
        StmtKind::Elige(switch_stmt) => {
            scan_go_expr(&switch_stmt.expr, file, diagnostics);
            for case in &switch_stmt.cases {
                scan_go_expr(&case.value, file, diagnostics);
                scan_go_if_body(&case.body, file, diagnostics);
            }
            if let Some(default) = &switch_stmt.default {
                scan_go_if_body(&default.body, file, diagnostics);
            }
            if let Some(catch) = &switch_stmt.catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
        }
        StmtKind::Discerne(match_stmt) => {
            for subject in &match_stmt.subjects {
                scan_go_expr(subject, file, diagnostics);
            }
            for arm in &match_stmt.arms {
                scan_go_if_body(&arm.body, file, diagnostics);
            }
            if let Some(default) = &match_stmt.default {
                scan_go_if_body(&default.body, file, diagnostics);
            }
        }
        StmtKind::Custodi(guard_stmt) => {
            for clause in &guard_stmt.clauses {
                scan_go_expr(&clause.cond, file, diagnostics);
                scan_go_if_body(&clause.body, file, diagnostics);
            }
        }
        StmtKind::Fac(fac_stmt) => {
            scan_go_block(&fac_stmt.body, file, diagnostics);
            if let Some(catch) = &fac_stmt.catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
            if let Some(cond) = &fac_stmt.while_ {
                scan_go_expr(cond, file, diagnostics);
            }
        }
        StmtKind::Redde(ret) => {
            if let Some(value) = &ret.value {
                scan_go_expr(value, file, diagnostics);
            }
        }
        StmtKind::ReddeFilum(ret) => {
            if let Some(value) = &ret.value {
                scan_go_expr(value, file, diagnostics);
            }
        }
        StmtKind::Iace(stmt) => {
            scan_go_expr(&stmt.value, file, diagnostics);
        }
        StmtKind::Mori(stmt) => {
            scan_go_expr(&stmt.value, file, diagnostics);
        }
        StmtKind::Adfirma(stmt) => {
            scan_go_expr(&stmt.cond, file, diagnostics);
            if let Some(message) = &stmt.message {
                scan_go_expr(message, file, diagnostics);
            }
        }
        StmtKind::Require(stmt) => {
            scan_go_expr(&stmt.cond, file, diagnostics);
            scan_go_expr(&stmt.error, file, diagnostics);
        }
        StmtKind::Reject(stmt) => {
            scan_go_expr(&stmt.cond, file, diagnostics);
            scan_go_expr(&stmt.error, file, diagnostics);
        }
        StmtKind::Scribe(stmt) => {
            for arg in &stmt.args {
                scan_go_expr(arg, file, diagnostics);
            }
        }
        StmtKind::Incipit(entry) => {
            if let Some(exitus) = &entry.exitus {
                scan_go_expr(exitus, file, diagnostics);
            }
            scan_go_if_body(&entry.body, file, diagnostics);
        }
        StmtKind::Expr(expr) => scan_go_expr(&expr.expr, file, diagnostics),
        StmtKind::IncDec(stmt) => {
            scan_go_expr(&stmt.target, file, diagnostics);
        }
        StmtKind::Var(decl) => {
            if let Some(init) = &decl.init {
                scan_go_expr(init, file, diagnostics);
            }
        }
        StmtKind::SizeDecl(decl) => scan_go_expr(&decl.init, file, diagnostics),
        StmtKind::Ex(stmt) => scan_go_expr(&stmt.source, file, diagnostics),
        StmtKind::Reddet(stmt) => scan_go_expr(&stmt.future, file, diagnostics),
        StmtKind::Tacebit(stmt) => scan_go_expr(&stmt.future, file, diagnostics),
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Union(_)
        | StmtKind::Schema(_)
        | StmtKind::Rumpe(_)
        | StmtKind::Perge(_)
        | StmtKind::Tacet(_) => {}
    }
}

fn scan_go_probandum(test: &ProbandumDecl, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    for setup in &test.body.setup {
        scan_go_block(&setup.body, file, diagnostics);
    }
    for case in &test.body.tests {
        scan_go_block(&case.body, file, diagnostics);
    }
    for nested in &test.body.nested {
        scan_go_probandum(nested, file, diagnostics);
    }
}

fn scan_go_block(block: &BlockStmt, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    for stmt in &block.statements {
        scan_go_stmt(stmt, file, diagnostics);
    }
}

fn scan_go_if_body(body: &IfBody, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match body {
        IfBody::Block(block) => scan_go_block(block, file, diagnostics),
        IfBody::Ergo(stmt) => scan_go_stmt(stmt, file, diagnostics),
    }
}

fn scan_go_else(clause: &SecusClause, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match clause {
        SecusClause::Sin { stmt, .. } => {
            scan_go_expr(&stmt.cond, file, diagnostics);
            scan_go_if_body(&stmt.then, file, diagnostics);
            if let Some(catch) = &stmt.catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
            if let Some(else_) = &stmt.else_ {
                scan_go_else(else_, file, diagnostics);
            }
        }
        SecusClause::Block { body, catch, .. } => {
            scan_go_block(body, file, diagnostics);
            if let Some(catch) = catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
        }
        SecusClause::Stmt { stmt, catch, .. } => {
            scan_go_stmt(stmt, file, diagnostics);
            if let Some(catch) = catch {
                scan_go_block(&catch.body, file, diagnostics);
            }
        }
    }
}

fn scan_go_expr(expr: &Expr, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.kind {
        ExprKind::Binary(binary) => {
            scan_go_expr(&binary.lhs, file, diagnostics);
            scan_go_expr(&binary.rhs, file, diagnostics);
        }
        ExprKind::Unary(unary) => scan_go_expr(&unary.operand, file, diagnostics),
        ExprKind::Call(call) => {
            scan_go_expr(&call.callee, file, diagnostics);
            for arg in &call.args {
                scan_go_expr(&arg.value, file, diagnostics);
            }
        }
        ExprKind::Member(member) => {
            scan_go_expr(&member.object, file, diagnostics);
        }
        ExprKind::Index(index) => {
            scan_go_expr(&index.object, file, diagnostics);
            scan_go_expr(&index.index, file, diagnostics);
        }
        ExprKind::Gradient(gradient) => {
            scan_go_expr(&gradient.call, file, diagnostics);
            for selector in &gradient.selectors {
                scan_go_expr(selector, file, diagnostics);
            }
        }
        ExprKind::OptionalChain(chain) => {
            scan_go_expr(&chain.object, file, diagnostics);
            match &chain.chain {
                OptionalChainKind::Member(_) => {}
                OptionalChainKind::Index(index) => scan_go_expr(index, file, diagnostics),
                OptionalChainKind::Call(args) => {
                    for arg in args {
                        scan_go_expr(&arg.value, file, diagnostics);
                    }
                }
            }
        }
        ExprKind::NonNull(chain) => {
            scan_go_expr(&chain.object, file, diagnostics);
            match &chain.chain {
                NonNullKind::Member(_) => {}
                NonNullKind::Index(index) => scan_go_expr(index, file, diagnostics),
                NonNullKind::Call(args) => {
                    for arg in args {
                        scan_go_expr(&arg.value, file, diagnostics);
                    }
                }
            }
        }
        ExprKind::Assign(assign) => {
            scan_go_expr(&assign.target, file, diagnostics);
            scan_go_expr(&assign.value, file, diagnostics);
            // `↤` inline recovery (`⊥ fallback`) is part of the assignment's
            // expression surface; it must not escape the target-policy scan.
            if let Some(recovery) = &assign.recovery {
                scan_go_expr(recovery, file, diagnostics);
            }
        }
        ExprKind::Ternary(ternary) => {
            scan_go_expr(&ternary.cond, file, diagnostics);
            scan_go_expr(&ternary.then, file, diagnostics);
            scan_go_expr(&ternary.else_, file, diagnostics);
        }
        ExprKind::Array(items) => {
            for item in &items.elements {
                match item {
                    ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                        scan_go_expr(expr, file, diagnostics);
                    }
                    ArrayElement::Infer(_) => {}
                }
            }
        }
        ExprKind::Object(fields) => {
            for field in &fields.fields {
                match &field.key {
                    ObjectKey::Computed(expr) | ObjectKey::Spread(expr) => {
                        scan_go_expr(expr, file, diagnostics);
                    }
                    ObjectKey::Ident(_) | ObjectKey::String(_) => {}
                }
                if let Some(value) = &field.value {
                    scan_go_expr(value, file, diagnostics);
                }
            }
        }
        ExprKind::Scriptum(scriptum) => {
            for arg in &scriptum.args {
                scan_go_expr(arg, file, diagnostics);
            }
        }
        ExprKind::Clausura(clausura) => match &clausura.body {
            ClausuraBody::Expr(expr) => scan_go_expr(expr, file, diagnostics),
            ClausuraBody::Block(block) => scan_go_block(block, file, diagnostics),
            ClausuraBody::Fac(stmt) => {
                scan_go_block(&stmt.body, file, diagnostics);
                if let Some(catch) = &stmt.catch {
                    scan_go_block(&catch.body, file, diagnostics);
                }
                if let Some(while_) = &stmt.while_ {
                    scan_go_expr(while_, file, diagnostics);
                }
            }
        },
        ExprKind::Conversio(conversio) => {
            scan_go_expr(&conversio.expr, file, diagnostics);
            if let Some(recovery) = &conversio.recovery {
                scan_go_expr(recovery, file, diagnostics);
            }
        }
        ExprKind::Cede(cede) => scan_go_expr(&cede.expr, file, diagnostics),
        ExprKind::Verte(verte) => scan_go_expr(&verte.expr, file, diagnostics),
        ExprKind::TypeCheck(type_check) => {
            scan_go_expr(&type_check.expr, file, diagnostics);
        }
        ExprKind::Intervallum(range) => {
            scan_go_expr(&range.start, file, diagnostics);
            scan_go_expr(&range.end, file, diagnostics);
            if let Some(step) = &range.step {
                scan_go_expr(step, file, diagnostics);
            }
        }
        ExprKind::Finge(finge) => {
            for field in &finge.fields {
                scan_go_expr(&field.value, file, diagnostics);
            }
        }
        ExprKind::Iuncta(iuncta) => {
            for expr in &iuncta.elements {
                scan_go_expr(expr, file, diagnostics);
            }
        }
        ExprKind::Praefixum(praefixum) => scan_go_block(&praefixum.body, file, diagnostics),
        // UT: the capture boundary's body is ordinary scanned source.
        ExprKind::Trap(trap) => scan_go_block(&trap.body, file, diagnostics),
        ExprKind::CallDefault(call_default) => {
            scan_go_expr(&call_default.call, file, diagnostics);
            scan_go_expr(&call_default.default, file, diagnostics);
        }
        ExprKind::Reducta(reduction) => {
            scan_go_expr(&reduction.source, file, diagnostics);
            if let Some(group) = &reduction.group {
                scan_go_expr(group, file, diagnostics);
            }
            if let Some(body) = &reduction.body {
                scan_go_block(body, file, diagnostics);
            }
            if let Some(identity) = &reduction.identity {
                scan_go_expr(identity, file, diagnostics);
            }
        }
        ExprKind::Scan(scan) => {
            scan_go_expr(&scan.source, file, diagnostics);
            if let Some(group) = &scan.group {
                scan_go_expr(group, file, diagnostics);
            }
            scan_go_block(&scan.body, file, diagnostics);
        }
        ExprKind::Sort(sort) => {
            scan_go_expr(&sort.source, file, diagnostics);
            if let Some(body) = &sort.body {
                scan_go_block(body, file, diagnostics);
            }
        }
        // `⇇` copy-into: walk the subexpressions only; per-target support is
        // decided by the emitter.
        ExprKind::CopyInto(copy) => {
            scan_go_expr(&copy.target, file, diagnostics);
            scan_go_expr(&copy.value, file, diagnostics);
        }
        // `⇜` sink-into: walk the subexpressions only; semantic lowering fails
        // closed until the sinking lowering lands.
        ExprKind::SinkInto(sink) => {
            scan_go_expr(&sink.sink, file, diagnostics);
            scan_go_expr(&sink.item, file, diagnostics);
        }
        ExprKind::Paren(expr) => scan_go_expr(expr, file, diagnostics),
        ExprKind::Format(format) => scan_go_expr(&format.value, file, diagnostics),
        ExprKind::Literal(_)
        | ExprKind::Vacua(_)
        | ExprKind::KeywordLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::Lege(_)
        | ExprKind::Ego(_)
        | ExprKind::Insere(_)
        | ExprKind::Ad(_)
        | ExprKind::Construct(_) => {}
    }
}

fn scan_rust_stmt(stmt: &Stmt, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match &stmt.kind {
        StmtKind::Block(block) => scan_rust_block(block, file, diagnostics),
        StmtKind::Func(func) => {
            if let Some(body) = &func.body {
                scan_rust_block(body, file, diagnostics);
            }
        }
        StmtKind::Class(class) => {
            for member in &class.members {
                if let ClassMemberKind::Method(method) = &member.kind
                    && let Some(body) = &method.body
                {
                    scan_rust_block(body, file, diagnostics);
                }
            }
        }
        StmtKind::Probandum(test) => scan_rust_probandum(test, file, diagnostics),
        StmtKind::Proba(test) => scan_rust_block(&test.body, file, diagnostics),
        StmtKind::Si(if_stmt) => {
            scan_rust_expr(&if_stmt.cond, file, diagnostics);
            scan_rust_if_body(&if_stmt.then, file, diagnostics);
            if let Some(catch) = &if_stmt.catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
            if let Some(else_) = &if_stmt.else_ {
                scan_rust_else(else_, file, diagnostics);
            }
        }
        StmtKind::Dum(while_stmt) => {
            scan_rust_expr(&while_stmt.cond, file, diagnostics);
            scan_rust_if_body(&while_stmt.body, file, diagnostics);
            if let Some(catch) = &while_stmt.catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
        }
        StmtKind::Itera(iter_stmt) => {
            for source in &iter_stmt.sources {
                scan_rust_expr(source, file, diagnostics);
            }
            scan_rust_if_body(&iter_stmt.body, file, diagnostics);
            if let Some(catch) = &iter_stmt.catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
        }
        StmtKind::Elige(switch_stmt) => {
            scan_rust_expr(&switch_stmt.expr, file, diagnostics);
            for case in &switch_stmt.cases {
                scan_rust_expr(&case.value, file, diagnostics);
                scan_rust_if_body(&case.body, file, diagnostics);
            }
            if let Some(default) = &switch_stmt.default {
                scan_rust_if_body(&default.body, file, diagnostics);
            }
            if let Some(catch) = &switch_stmt.catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
        }
        StmtKind::Discerne(match_stmt) => {
            for subject in &match_stmt.subjects {
                scan_rust_expr(subject, file, diagnostics);
            }
            for arm in &match_stmt.arms {
                scan_rust_if_body(&arm.body, file, diagnostics);
            }
            if let Some(default) = &match_stmt.default {
                scan_rust_if_body(&default.body, file, diagnostics);
            }
        }
        StmtKind::Custodi(guard_stmt) => {
            for clause in &guard_stmt.clauses {
                scan_rust_expr(&clause.cond, file, diagnostics);
                scan_rust_if_body(&clause.body, file, diagnostics);
            }
        }
        StmtKind::Fac(fac_stmt) => {
            scan_rust_block(&fac_stmt.body, file, diagnostics);
            if let Some(catch) = &fac_stmt.catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
            if let Some(cond) = &fac_stmt.while_ {
                scan_rust_expr(cond, file, diagnostics);
            }
        }
        StmtKind::Redde(ret) => {
            if let Some(value) = &ret.value {
                scan_rust_expr(value, file, diagnostics);
            }
        }
        StmtKind::ReddeFilum(ret) => {
            if let Some(value) = &ret.value {
                scan_rust_expr(value, file, diagnostics);
            }
        }
        StmtKind::Iace(stmt) => {
            scan_rust_expr(&stmt.value, file, diagnostics);
        }
        StmtKind::Mori(stmt) => scan_rust_expr(&stmt.value, file, diagnostics),
        StmtKind::Adfirma(stmt) => {
            scan_rust_expr(&stmt.cond, file, diagnostics);
            if let Some(message) = &stmt.message {
                scan_rust_expr(message, file, diagnostics);
            }
        }
        StmtKind::Require(stmt) => {
            scan_rust_expr(&stmt.cond, file, diagnostics);
            scan_rust_expr(&stmt.error, file, diagnostics);
        }
        StmtKind::Reject(stmt) => {
            scan_rust_expr(&stmt.cond, file, diagnostics);
            scan_rust_expr(&stmt.error, file, diagnostics);
        }
        StmtKind::Scribe(stmt) => {
            for arg in &stmt.args {
                scan_rust_expr(arg, file, diagnostics);
            }
        }
        StmtKind::Incipit(entry) => {
            if let Some(exitus) = &entry.exitus {
                scan_rust_expr(exitus, file, diagnostics);
            }
            scan_rust_if_body(&entry.body, file, diagnostics);
        }
        StmtKind::Expr(expr) => scan_rust_expr(&expr.expr, file, diagnostics),
        StmtKind::IncDec(stmt) => scan_rust_expr(&stmt.target, file, diagnostics),
        StmtKind::Var(decl) => {
            if let Some(init) = &decl.init {
                scan_rust_expr(init, file, diagnostics);
            }
        }
        StmtKind::Interface(_)
        | StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Union(_)
        | StmtKind::Schema(_)
        | StmtKind::Rumpe(_)
        | StmtKind::Perge(_)
        | StmtKind::Tacet(_) => {}
        StmtKind::Reddet(stmt) => scan_rust_expr(&stmt.future, file, diagnostics),
        StmtKind::SizeDecl(decl) => scan_rust_expr(&decl.init, file, diagnostics),
        StmtKind::Tacebit(stmt) => scan_rust_expr(&stmt.future, file, diagnostics),
        StmtKind::Ex(stmt) => scan_rust_expr(&stmt.source, file, diagnostics),
    }
}

fn scan_rust_probandum(test: &ProbandumDecl, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    for setup in &test.body.setup {
        scan_rust_block(&setup.body, file, diagnostics);
    }
    for case in &test.body.tests {
        scan_rust_block(&case.body, file, diagnostics);
    }
    for nested in &test.body.nested {
        scan_rust_probandum(nested, file, diagnostics);
    }
}

fn scan_rust_block(block: &BlockStmt, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    for stmt in &block.statements {
        scan_rust_stmt(stmt, file, diagnostics);
    }
}

fn scan_rust_if_body(body: &IfBody, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match body {
        IfBody::Block(block) => scan_rust_block(block, file, diagnostics),
        IfBody::Ergo(stmt) => scan_rust_stmt(stmt, file, diagnostics),
    }
}

fn scan_rust_else(clause: &SecusClause, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match clause {
        SecusClause::Sin { stmt, .. } => {
            scan_rust_expr(&stmt.cond, file, diagnostics);
            scan_rust_if_body(&stmt.then, file, diagnostics);
            if let Some(catch) = &stmt.catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
            if let Some(else_) = &stmt.else_ {
                scan_rust_else(else_, file, diagnostics);
            }
        }
        SecusClause::Block { body, catch, .. } => {
            scan_rust_block(body, file, diagnostics);
            if let Some(catch) = catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
        }
        SecusClause::Stmt { stmt, catch, .. } => {
            scan_rust_stmt(stmt, file, diagnostics);
            if let Some(catch) = catch {
                scan_rust_block(&catch.body, file, diagnostics);
            }
        }
    }
}

fn scan_rust_expr(expr: &Expr, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.kind {
        ExprKind::Binary(binary) => {
            scan_rust_expr(&binary.lhs, file, diagnostics);
            scan_rust_expr(&binary.rhs, file, diagnostics);
        }
        ExprKind::Unary(unary) => scan_rust_expr(&unary.operand, file, diagnostics),
        ExprKind::Call(call) => {
            scan_rust_expr(&call.callee, file, diagnostics);
            for arg in &call.args {
                scan_rust_expr(&arg.value, file, diagnostics);
            }
        }
        ExprKind::Member(member) => scan_rust_expr(&member.object, file, diagnostics),
        ExprKind::Index(index) => {
            scan_rust_expr(&index.object, file, diagnostics);
            scan_rust_expr(&index.index, file, diagnostics);
        }
        ExprKind::Gradient(gradient) => {
            scan_rust_expr(&gradient.call, file, diagnostics);
            for selector in &gradient.selectors {
                scan_rust_expr(selector, file, diagnostics);
            }
        }
        ExprKind::OptionalChain(chain) => {
            scan_rust_expr(&chain.object, file, diagnostics);
            match &chain.chain {
                OptionalChainKind::Member(_) => {}
                OptionalChainKind::Index(index) => scan_rust_expr(index, file, diagnostics),
                OptionalChainKind::Call(args) => {
                    for arg in args {
                        scan_rust_expr(&arg.value, file, diagnostics);
                    }
                }
            }
        }
        ExprKind::NonNull(chain) => {
            scan_rust_expr(&chain.object, file, diagnostics);
            match &chain.chain {
                NonNullKind::Member(_) => {}
                NonNullKind::Index(index) => scan_rust_expr(index, file, diagnostics),
                NonNullKind::Call(args) => {
                    for arg in args {
                        scan_rust_expr(&arg.value, file, diagnostics);
                    }
                }
            }
        }
        ExprKind::Assign(assign) => {
            scan_rust_expr(&assign.target, file, diagnostics);
            scan_rust_expr(&assign.value, file, diagnostics);
            // `↤` inline recovery (`⊥ fallback`) is part of the assignment's
            // expression surface; it must not escape the target-policy scan.
            if let Some(recovery) = &assign.recovery {
                scan_rust_expr(recovery, file, diagnostics);
            }
        }
        ExprKind::Ternary(ternary) => {
            scan_rust_expr(&ternary.cond, file, diagnostics);
            scan_rust_expr(&ternary.then, file, diagnostics);
            scan_rust_expr(&ternary.else_, file, diagnostics);
        }
        ExprKind::Array(items) => {
            for item in &items.elements {
                match item {
                    ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                        scan_rust_expr(expr, file, diagnostics);
                    }
                    ArrayElement::Infer(_) => {}
                }
            }
        }
        ExprKind::Object(fields) => {
            for field in &fields.fields {
                match &field.key {
                    ObjectKey::Computed(expr) | ObjectKey::Spread(expr) => {
                        scan_rust_expr(expr, file, diagnostics);
                    }
                    ObjectKey::Ident(_) | ObjectKey::String(_) => {}
                }
                if let Some(value) = &field.value {
                    scan_rust_expr(value, file, diagnostics);
                }
            }
        }
        ExprKind::Scriptum(scriptum) => {
            for arg in &scriptum.args {
                scan_rust_expr(arg, file, diagnostics);
            }
        }
        ExprKind::Clausura(clausura) => match &clausura.body {
            ClausuraBody::Expr(expr) => scan_rust_expr(expr, file, diagnostics),
            ClausuraBody::Block(block) => scan_rust_block(block, file, diagnostics),
            ClausuraBody::Fac(stmt) => {
                scan_rust_block(&stmt.body, file, diagnostics);
                if let Some(catch) = &stmt.catch {
                    scan_rust_block(&catch.body, file, diagnostics);
                }
                if let Some(while_) = &stmt.while_ {
                    scan_rust_expr(while_, file, diagnostics);
                }
            }
        },
        ExprKind::Conversio(conversio) => {
            scan_rust_expr(&conversio.expr, file, diagnostics);
            if let Some(recovery) = &conversio.recovery {
                scan_rust_expr(recovery, file, diagnostics);
            }
        }
        ExprKind::Cede(cede) => scan_rust_expr(&cede.expr, file, diagnostics),
        ExprKind::Verte(verte) => scan_rust_expr(&verte.expr, file, diagnostics),
        ExprKind::TypeCheck(type_check) => {
            scan_rust_expr(&type_check.expr, file, diagnostics);
        }
        ExprKind::Intervallum(range) => {
            scan_rust_expr(&range.start, file, diagnostics);
            scan_rust_expr(&range.end, file, diagnostics);
            if let Some(step) = &range.step {
                scan_rust_expr(step, file, diagnostics);
            }
        }
        ExprKind::Finge(finge) => {
            for field in &finge.fields {
                scan_rust_expr(&field.value, file, diagnostics);
            }
        }
        ExprKind::Iuncta(iuncta) => {
            for expr in &iuncta.elements {
                scan_rust_expr(expr, file, diagnostics);
            }
        }
        ExprKind::Praefixum(praefixum) => scan_rust_block(&praefixum.body, file, diagnostics),
        // UT: the capture boundary's body is ordinary scanned source.
        ExprKind::Trap(trap) => scan_rust_block(&trap.body, file, diagnostics),
        ExprKind::CallDefault(call_default) => {
            scan_rust_expr(&call_default.call, file, diagnostics);
            scan_rust_expr(&call_default.default, file, diagnostics);
        }
        ExprKind::Reducta(reduction) => {
            scan_rust_expr(&reduction.source, file, diagnostics);
            if let Some(group) = &reduction.group {
                scan_rust_expr(group, file, diagnostics);
            }
            if let Some(body) = &reduction.body {
                scan_rust_block(body, file, diagnostics);
            }
            if let Some(identity) = &reduction.identity {
                scan_rust_expr(identity, file, diagnostics);
            }
        }
        ExprKind::Scan(scan) => {
            scan_rust_expr(&scan.source, file, diagnostics);
            if let Some(group) = &scan.group {
                scan_rust_expr(group, file, diagnostics);
            }
            scan_rust_block(&scan.body, file, diagnostics);
        }
        ExprKind::Sort(sort) => {
            scan_rust_expr(&sort.source, file, diagnostics);
            if let Some(body) = &sort.body {
                scan_rust_block(body, file, diagnostics);
            }
        }
        // `⇇` copy-into: same contract as the Go scanner — walk
        // subexpressions only.
        ExprKind::CopyInto(copy) => {
            scan_rust_expr(&copy.target, file, diagnostics);
            scan_rust_expr(&copy.value, file, diagnostics);
        }
        // `⇜` sink-into: walk the subexpressions only; semantic lowering fails
        // closed until the sinking lowering lands.
        ExprKind::SinkInto(sink) => {
            scan_rust_expr(&sink.sink, file, diagnostics);
            scan_rust_expr(&sink.item, file, diagnostics);
        }
        ExprKind::Paren(expr) => scan_rust_expr(expr, file, diagnostics),
        ExprKind::Format(format) => scan_rust_expr(&format.value, file, diagnostics),
        ExprKind::Literal(_)
        | ExprKind::Vacua(_)
        | ExprKind::KeywordLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::Lege(_)
        | ExprKind::Ego(_)
        | ExprKind::Insere(_)
        | ExprKind::Ad(_)
        | ExprKind::Construct(_) => {}
    }
}

fn target_exception_diagnostic(file: &str, span: crate::lexer::Span, message: &str, issue: &str) -> Diagnostic {
    Diagnostic::error(message)
        .with_phase(DiagnosticPhase::Analysis)
        .with_code("TARGET001")
        .with_file(file)
        .with_span(span)
        .with_args(vec![DiagnosticArg::new("issue", issue)])
}

#[cfg(test)]
#[path = "target_policy_test.rs"]
mod tests;
